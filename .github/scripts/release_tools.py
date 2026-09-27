# Project:  Privatium™  |  File: .github/scripts/release_tools.py
# Authors:  Gabriel Mongefranco (@gabrielmongefranco)
# Created:  2026-09-05  |  Modified: 2026-09-27
# Summary:  Package a single executable — and for Windows a portable zip holding the
#           privatium-data folder with the example apps — name every archive a release
#           carries, and require successful CI for a release commit, waiting for a
#           run still in progress.
#           See main README.md for full license information.

import argparse
import json
import os
import re
import subprocess
import tarfile
import time
import zipfile
from pathlib import Path

# The CI runs that count for a release: a push to main, or a run started by hand for a
# commit that never had one. A pull request run tested a merge preview, not the commit.
RELEASE_CI_EVENTS = ("push", "workflow_dispatch")

# The example apps of apps/README.md, the folders a portable zip carries under
# privatium-data/apps/ so its launcher is never empty (spec/cli.md §1, §2).
EXAMPLE_APPS = ("hello", "animals", "sketch")

# One single-binary archive per platform, keyed as GitHub names the runner's operating
# system, and the executable each holds.
ARCHIVES = {"Linux": ("privatium-linux.tar.gz", "privatium"),
            "macOS": ("privatium-mac.zip", "privatium"),
            "Windows": ("privatium-windows.zip", "privatium.exe")}

# The Windows portable zip (spec/cli.md §1).
PORTABLE_ARCHIVE = "privatium-windows-portable.zip"

# Every archive a release carries, in the order they are attached. The release
# workflow reads this list, so an archive built here is never left off a release.
ASSETS = tuple(name for name, _ in ARCHIVES.values()) + (PORTABLE_ARCHIVE,)

PORTABLE_README = """Privatium, portable.

Keep privatium.exe and the privatium-data folder together: while that folder sits
beside the program, everything Privatium knows lives in it and nothing is written
anywhere else on this computer (spec/cli.md section 1).

  privatium-data\\apps      the apps; three examples to start with
  privatium-data\\data      your information, as plain text files
  privatium-data\\identity  this node's keys; keep private

Back up by copying this whole folder. Move it, and the program follows.
Delete privatium-data (after copying it) to use the platform data directory instead.
"""


def package(platform, source, destination):
    """Archive only the platform binary, preserving an executable Unix mode."""
    if platform not in ARCHIVES:
        raise ValueError("Cannot package this platform; use Linux, macOS or Windows.")
    archive, binary = ARCHIVES[platform]
    path = source / binary
    if not path.is_file() or path.is_symlink():
        raise FileNotFoundError("Cannot package the binary; build the release executable first.")
    destination.mkdir(parents=True, exist_ok=True)
    result = destination / archive
    if platform == "Linux":
        with tarfile.open(result, "w:gz") as bundle, path.open("rb") as executable:
            entry = bundle.gettarinfo(str(path), arcname=binary)
            entry.mode = 0o755
            entry.uid = entry.gid = 0
            entry.uname = entry.gname = ""
            bundle.addfile(entry, executable)
    else:
        with zipfile.ZipFile(result, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            entry = zipfile.ZipInfo(binary)
            entry.create_system = 3
            entry.external_attr = 0o100755 << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(entry, path.read_bytes())
    return result


def package_portable(source, destination, apps):
    """Zip the Windows binary with a privatium-data folder holding the example apps."""
    binary = "privatium.exe"
    path = source / binary
    if not path.is_file() or path.is_symlink():
        raise FileNotFoundError("Cannot package the binary; build the release executable first.")
    for slug in EXAMPLE_APPS:
        if not (apps / slug / "app.toml").is_file():
            raise FileNotFoundError(f"Cannot package the portable zip; apps/{slug} is not an app folder.")
    destination.mkdir(parents=True, exist_ok=True)
    result = destination / PORTABLE_ARCHIVE
    with zipfile.ZipFile(result, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        entry = zipfile.ZipInfo(binary)
        entry.create_system = 3
        entry.external_attr = 0o100755 << 16
        entry.compress_type = zipfile.ZIP_DEFLATED
        bundle.writestr(entry, path.read_bytes())
        bundle.writestr("README.txt", PORTABLE_README)
        for slug in EXAMPLE_APPS:
            folder = apps / slug
            for file in sorted(p for p in folder.rglob("*") if p.is_file()):
                arcname = "privatium-data/apps/" + file.relative_to(apps).as_posix()
                bundle.write(file, arcname)
    return result


# How long a release build waits for push CI on its commit, and how often it looks. A
# release published minutes after its merge finds that CI still running; the full matrix
# takes well under this, and the job's timeout-minutes in release.yml sits above it.
CI_WAIT_SECONDS = 25 * 60
CI_POLL_SECONDS = 30


class PendingCI(Exception):
    """The latest push CI run for the release commit has not finished yet."""


def require_ci(runs, sha):
    """Refuse unless the latest push or manually started CI run for this exact commit
    completed successfully; a run still queued or in progress is pending rather than
    refused."""
    matching = [run for run in runs if run.get("head_sha") == sha and run.get("event") in RELEASE_CI_EVENTS]
    latest = max(matching, key=lambda run: run["id"], default={})
    if latest and latest.get("status") != "completed":
        raise PendingCI("Push CI for the release commit is still running.")
    if latest.get("conclusion") != "success":
        raise ValueError("Release build refused: let CI pass for the release commit (a push to main, or "
                         "the CI workflow started by hand on its branch), then rerun this workflow.")


def wait_for_ci(fetch_runs, sha, sleep=time.sleep, now=time.monotonic, timeout=CI_WAIT_SECONDS):
    """Poll `fetch_runs` until require_ci passes, refuses, or the wait runs out."""
    deadline = now() + timeout
    while True:
        try:
            require_ci(fetch_runs(), sha)
            return
        except PendingCI as pending:
            if now() >= deadline:
                raise ValueError(f"Release build refused: {pending} Wait for it, then rerun this workflow.")
            print(f"{pending} Checking again in {CI_POLL_SECONDS} seconds.", flush=True)
            sleep(CI_POLL_SECONDS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["package", "check-ci", "assets"])
    parser.add_argument("value")
    args = parser.parse_args()
    if args.command == "assets":
        # `assets list`: the archive names a release carries, one per line, for the
        # workflow step that attaches them.
        if args.value != "list":
            parser.error("Expected `assets list`.")
        print("\n".join(ASSETS))
    elif args.command == "package":
        result = package(args.value, Path("target/release"), Path("dist"))
        # A second output rather than a second line: with `archive: false` the upload
        # action takes exactly one file per step, so the portable zip has its own step.
        portable = package_portable(Path("target/release"), Path("dist"), Path("apps")) if args.value == "Windows" else None
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"path={result.as_posix()}\n")
            output.write(f"portable={portable.as_posix() if portable else ''}\n")
    else:
        if not re.fullmatch(r"[0-9a-f]{40}", args.value):
            parser.error("Expected the release commit SHA.")
        repo = os.environ["GITHUB_REPOSITORY"]

        def fetch_runs():
            response = subprocess.check_output([
                "gh", "api", "--method", "GET", "--paginate", "--slurp",
                f"repos/{repo}/actions/workflows/ci.yml/runs",
                "-f", f"head_sha={args.value}", "-f", "per_page=100",
            ], text=True)
            return [run for page in json.loads(response) for run in page["workflow_runs"]]

        wait_for_ci(fetch_runs, args.value)
        print("Latest CI run passed for the release commit.")


if __name__ == "__main__":
    main()
