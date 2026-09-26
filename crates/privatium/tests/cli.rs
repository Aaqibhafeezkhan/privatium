// This file is part of Privatium
// crates/privatium/tests/cli.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-09-26
// Summary: spec/cli.md against the real binary, section by section: the qualified --version (§1) and
//          the exit codes; the flags, which are exactly the spec's synopsis lines (§1–§9, both
//          directions); a node on loopback with --port, --solo and --no-discovery (§2); dev
//          naming the app (§3); new for each tier and from hello (§4); skill list and export
//          (§6); snapshot, --verify, and restore from a backup with its tier reported and a
//          diverged log refused (§7); `pair` against a running node and without one (§8);
//          `--open`'s QR code and the first-run window (§2); firewall parsing and refusing
//          (§9); the commands §10 keeps absent.
// Notes: See README file for documentation and full license information.
//
// Copyright © 2026 Gabriel Mongefranco
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

// AGENTS.md, Style: unwrap() is permitted in tests, and a test that hides a failure
// behind `?` is worse than one that panics with a line number.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_privatium");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Run the binary to completion: exit code, stdout, stderr.
fn privatium(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(BIN)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A node started in the background, killed on drop.
struct Running {
    child: Child,
    port: u16,
    stdout: Vec<String>,
}

impl Running {
    /// Start with `args`, wait for the announce line, and keep the port.
    fn start(args: &[&str]) -> Self {
        Self::start_with_env(args, &[])
    }

    /// [`start`](Self::start) with environment variables set for the child.
    fn start_with_env(args: &[&str], envs: &[(&str, &str)]) -> Self {
        let mut child = Command::new(BIN)
            .args(args)
            .envs(envs.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut stdout = Vec::new();
        let port = loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).unwrap();
            assert_ne!(
                read, 0,
                "the node exited before announcing; stdout so far: {stdout:?}"
            );
            let line = line.trim_end().to_owned();
            stdout.push(line.clone());
            if let Some(rest) = line.strip_prefix("privatium: listening on http://") {
                break rest
                    .rsplit(':')
                    .next()
                    .unwrap()
                    .trim_end_matches('/')
                    .parse::<u16>()
                    .unwrap();
            }
        };
        // Drain the rest on a thread so the child never blocks on a full pipe.
        let mut running = Self {
            child,
            port,
            stdout,
        };
        // The two lines that follow the announce: the loopback notice, and under `dev`
        // the app's URL.
        for _ in 0..2 {
            let mut line = String::new();
            let started = Instant::now();
            // The dev line is printed right after; the loopback line always. Read what is
            // there without blocking forever on a bare run.
            if reader.read_line(&mut line).unwrap_or(0) > 0 {
                running.stdout.push(line.trim_end().to_owned());
            }
            if started.elapsed() > Duration::from_secs(5) {
                break;
            }
            if running
                .stdout
                .iter()
                .any(|l| l.contains(" at http://") && !l.contains("local browser at"))
            {
                break;
            }
            if !running
                .stdout
                .last()
                .is_some_and(|l| l.contains("local browser at"))
            {
                break;
            }
            if !args.contains(&"dev") {
                break;
            }
        }
        std::thread::spawn(move || {
            let mut sink = String::new();
            let _ = reader.read_to_string(&mut sink);
        });
        running
    }

    /// One request over a fresh connection: status and body.
    fn get(&self, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            self.port
        )
        .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, body.to_owned())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn data_dir(root: &tempfile::TempDir) -> String {
    root.path().join("node").to_string_lossy().into_owned()
}

/// Every `--flag` on the `privatium …` synopsis lines of a text, keyed by the command word
/// (`""` for the bare command).
fn synopsis_flags(text: &str) -> BTreeSet<(String, String)> {
    let mut flags = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("privatium ") else {
            continue;
        };
        let words: Vec<&str> = rest.split_whitespace().collect();
        let command = match words.first() {
            Some(word) if !word.starts_with('[') && !word.starts_with('-') => {
                // `skill list` and `skill export` are two commands.
                match (*word, words.get(1)) {
                    ("skill", Some(sub)) => format!("skill {sub}"),
                    _ => (*word).to_owned(),
                }
            }
            _ => String::new(),
        };
        for word in &words {
            let word = word.trim_matches(|c| c == '[' || c == ']');
            if let Some(flag) = word.strip_prefix("--") {
                let name = flag.split(['=', ' ', '|']).next().unwrap();
                flags.insert((command.clone(), format!("--{name}")));
            }
        }
    }
    flags
}

/// `§1` — `--version` prints the build version and a qualified protocol string, since a
/// build without sync does not satisfy `spec/protocol.md §13`, and beneath it the
/// project's own facts. Those are asserted as the literal text a person reads, so a
/// change to the manifest or to `build.rs` that moves what `--version` says shows up in
/// this diff rather than only in a release.
#[test]
fn test_spec_cli_1_version_qualifies_protocol() {
    let root = tempfile::tempdir().unwrap();
    let (code, out, _) = privatium(root.path(), &["--version"]);
    assert_eq!(code, 0);
    let mut lines = out.trim().lines();
    assert_eq!(
        lines.next().unwrap(),
        format!(
            "privatium {} pv/1 (partial: phase 2)",
            env!("CARGO_PKG_VERSION")
        )
    );
    let rest: Vec<&str> = lines.collect();
    for expected in [
        "Product:       Privatium",
        "Author:        Gabriel Mongefranco (@gabrielmongefranco)",
        "Copyright:     © 2026 Gabriel Mongefranco",
        "Licence:       GPL-3.0-or-later — see main README.md for full license information.",
        "Project:       https://github.com/gabrielmongefranco/privatium",
        "Author's site: https://gabriel.mongefranco.com",
    ] {
        assert!(
            rest.contains(&expected),
            "{expected}
{out}"
        );
    }
    // Terminal wherever it stands.
    let (code, out2, _) = privatium(root.path(), &["dev", "--version"]);
    assert_eq!(code, 0);
    assert_eq!(out2, out);
}

/// The Windows binary carries the version resource `build.rs` writes, so the Details tab
/// of the file properties dialog is filled in. The strings live in the PE's `.rsrc`
/// section as UTF-16, which is what this looks for; the fields themselves are the ones
/// `--version` prints. Nothing user-facing lands in a Mach-O or an ELF binary, so there is
/// no counterpart on the other two platforms (`spec/cli.md §1`).
#[test]
#[cfg(windows)]
fn test_windows_binary_carries_version_information() {
    let bytes = std::fs::read(BIN).unwrap();
    let utf16 =
        |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    let holds = |text: &str| {
        let needle = utf16(text);
        bytes.windows(needle.len()).any(|w| w == needle)
    };
    for expected in [
        "StringFileInfo",
        "ProductName",
        "Privatium",
        "privatium.exe",
        "Copyright (c) 2026 Gabriel Mongefranco. Licensed under GPL-3.0-or-later.",
        "Gabriel Mongefranco (@gabrielmongefranco)",
        "https://gabriel.mongefranco.com",
    ] {
        assert!(holds(expected), "the version resource has no {expected:?}");
    }
}

/// `§1` — `0` success, `1` runtime error, `2` usage error. (`3` is lint findings, held
/// by the linter's own tests.)
#[test]
fn test_cli_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let usage: &[&[&str]] = &[
        &["--nope"],
        &["new"],
        &["new", "_sys"],
        &["new", "Not-A-Slug"],
        &["new", "a", "b"],
        &["new", "a", "--tier", "perl"],
        &["restore"],
        &["skill"],
        &["skill", "export", "no-such-skill"],
        &["--port", "x"],
        &["--data-dir"],
        &["snapshot", "extra"],
    ];
    for args in usage {
        let mut full = vec!["--data-dir", dir.as_str()];
        full.extend_from_slice(args);
        let (code, _, err) = privatium(root.path(), &full);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(
            err.contains("privatium ["),
            "{args:?}: the help follows a usage error: {err}"
        );
    }
    let runtime: &[&[&str]] = &[
        &["snapshot", "--app", "no-such-app"],
        &["restore", "--from", "no-such-dir"],
        &["dev", "--app", "no-such-app"],
        &["new", "xy", "--from", "no-such-app"],
        &["pair"],
        &["firewall"],
    ];
    for args in runtime {
        let mut full = vec!["--data-dir", dir.as_str()];
        full.extend_from_slice(args);
        let (code, _, err) = privatium(root.path(), &full);
        assert_eq!(code, 1, "{args:?}: {err}");
        assert!(err.starts_with("privatium"), "{args:?}: {err}");
    }
    for args in [&["--version"][..], &["--help"], &["-h"], &["skill", "list"]] {
        let (code, _, _) = privatium(root.path(), args);
        assert_eq!(code, 0, "{args:?}");
    }
}

/// The flags the binary accepts are the flags `spec/cli.md` names, per command, in both
/// directions: nothing undocumented, nothing missing.
#[test]
fn test_no_undocumented_flags() {
    let spec = fs::read_to_string(repo().join("spec").join("cli.md")).unwrap();
    let (code, help, _) = privatium(&repo(), &["--help"]);
    assert_eq!(code, 0);
    let documented = synopsis_flags(&spec);
    let implemented = synopsis_flags(&help);
    assert!(!documented.is_empty());
    let undocumented: Vec<_> = implemented.difference(&documented).collect();
    let missing: Vec<_> = documented.difference(&implemented).collect();
    assert!(
        undocumented.is_empty(),
        "flags not in spec/cli.md: {undocumented:?}"
    );
    assert!(
        missing.is_empty(),
        "spec/cli.md flags the binary lacks: {missing:?}"
    );
    // And the parser really refuses what the help does not list.
    for (command, flag) in [
        ("", "--bind"),
        ("dev", "--port"),
        ("new", "--open"),
        ("snapshot", "--from"),
    ] {
        let mut args = vec![];
        if !command.is_empty() {
            args.push(command);
        }
        args.push(flag);
        args.push("x");
        let (code, _, err) = privatium(&repo(), &args);
        assert_eq!(code, 2, "{command} {flag}: {err}");
    }
}

/// `§2` — bare `privatium` runs a node: `--port` and `--solo` hold for one run, and the
/// local browser URL is announced beside the LAN one.
#[test]
fn test_spec_cli_2_runs_a_node_on_loopback() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);

    let node = Running::start(&["--data-dir", &dir, "--port", "0"]);
    assert!(
        node.stdout
            .iter()
            .any(|l| l.contains("local browser at http://127.0.0.1:")),
        "{:?}",
        node.stdout
    );
    let (status, body) = node.get("/api/v1/health");
    assert_eq!(status, 200, "{body}");
    let health: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["v"], 1);
    assert!(health["id"].is_string());
    let (status, body) = node.get("/");
    assert_eq!(status, 200);
    assert!(
        body.contains("Hello"),
        "the launcher lists the reference apps: {body}"
    );
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200);
    assert!(body.contains("We haven't met yet."), "{body}");
    drop(node);

    // The state file was written on the way down and the port never touched config.toml.
    assert!(Path::new(&dir).join("local").join("state.jsonl").is_file());
    assert!(!Path::new(&dir).join("config.toml").exists());

    // --solo: one app at `/`, no launcher, for this run only.
    let solo = Running::start(&[
        "--data-dir",
        &dir,
        "--port",
        "0",
        "--solo",
        "hello",
        "--no-discovery",
    ]);
    let (status, body) = solo.get("/");
    assert_eq!(status, 200);
    assert!(body.contains("We haven't met yet."), "{body}");
    let (status, _) = solo.get("/a/hello/");
    assert_eq!(status, 404);
    drop(solo);
}

/// Start a node, read its standard error until the local-browser line, kill it, and hand
/// back everything it wrote to standard error.
fn stderr_of_a_short_run(args: &[&str]) -> String {
    stderr_of_a_short_run_of(Path::new(BIN), args, &[])
}

/// [`stderr_of_a_short_run`] for a copy of the binary somewhere else.
fn stderr_of_a_short_run_of(bin: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let mut child = Command::new(bin)
        .args(args)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        if out.read_line(&mut line).unwrap() == 0 || line.contains("local browser at") {
            break;
        }
    }
    // Discovery starts after the announce; give it a moment to report.
    std::thread::sleep(Duration::from_millis(1500));
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `§2`, `spec/protocol.md §6` — `--no-discovery` starts neither mDNS nor the UDP
/// responder and says so; without it a node reports each mechanism under `--verbose`
/// and writes a `discovery.method` audit row either way, naming what started.
#[test]
fn test_cli_no_discovery_starts_nothing() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);

    let err = stderr_of_a_short_run(&["--data-dir", &dir, "--port", "0", "--no-discovery"]);
    assert!(
        err.contains("--no-discovery: not advertising on this network"),
        "{err}"
    );
    assert!(!err.contains("discovery: mDNS"), "{err}");
    assert!(!err.contains("discovery: UDP"), "{err}");
    let audit = fs::read_to_string(
        walk(&Path::new(&dir).join("data").join("_sys").join("log"))
            .into_iter()
            .find(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .unwrap(),
    )
    .unwrap();
    assert!(!audit.contains("discovery.method"), "{audit}");

    let err = stderr_of_a_short_run(&["--data-dir", &dir, "--port", "0", "--verbose"]);
    assert!(
        err.contains("discovery: mDNS started") || err.contains("discovery: mDNS not started"),
        "{err}"
    );
    assert!(
        err.contains("discovery: UDP 52525 started")
            || err.contains("discovery: UDP 52525 not started"),
        "{err}"
    );
    let audit = fs::read_to_string(
        walk(&Path::new(&dir).join("data").join("_sys").join("log"))
            .into_iter()
            .find(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .unwrap(),
    )
    .unwrap();
    assert!(audit.contains("discovery.method"), "{audit}");
}

/// `§3` — `dev --app <slug>` runs the node and names the app: where its files are, and
/// its URL. An app that did not load is a runtime error naming why.
#[test]
fn test_spec_cli_3_dev_names_the_app() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(Path::new(&dir).join("config.toml"), "[node]\nport = 0\n").unwrap();

    let dev = Running::start(&["--data-dir", &dir, "dev", "--app", "hello"]);
    assert!(
        dev.stdout
            .iter()
            .any(|l| l.starts_with("privatium: hello at http://127.0.0.1:")
                && l.ends_with("/a/hello/")),
        "{:?}",
        dev.stdout
    );
    let (status, body) = dev.get("/a/hello/edit");
    assert_eq!(status, 200);
    assert!(body.contains("What should I call you?"), "{body}");
    drop(dev);

    let (code, _, err) = privatium(root.path(), &["--data-dir", &dir, "dev", "--app", "nope"]);
    assert_eq!(code, 1);
    assert!(err.contains("dev --app nope"), "{err}");
}

/// `§4` — `new` writes an app under `<data-dir>/apps/<slug>/` for each tier, refuses a
/// second time, and `--scaffold` adds screens to a folder that already has a schema.
#[test]
fn test_spec_cli_4_new_each_tier_loads() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let apps = Path::new(&dir).join("apps");

    let (code, out, err) = privatium(root.path(), &["--data-dir", &dir, "new", "my-app"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("my-app/app.toml"), "{out}");
    assert!(apps.join("my-app").join("app.lua").is_file());
    assert!(
        apps.join("my-app")
            .join("views")
            .join("index.lsp")
            .is_file()
    );
    assert!(err.contains("privatium dev --app my-app"), "{err}");
    let manifest = fs::read_to_string(apps.join("my-app").join("app.toml")).unwrap();
    assert!(manifest.contains("slug        = \"my-app\""), "{manifest}");
    assert!(manifest.contains("title       = \"My App\""), "{manifest}");

    let (code, _, _) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "my-web", "--tier", "web"],
    );
    assert_eq!(code, 0);
    assert!(apps.join("my-web").join("web").join("index.html").is_file());
    let (code, _, _) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "my-rust", "--tier=rust"],
    );
    assert_eq!(code, 0);
    assert!(apps.join("my-rust").join("README.md").is_file());
    assert!(!apps.join("my-rust").join("app.lua").exists());

    // Never overwrites.
    let (code, _, err) = privatium(root.path(), &["--data-dir", &dir, "new", "my-app"]);
    assert_eq!(code, 1);
    assert!(err.contains("already exists"), "{err}");

    // --scaffold against an existing folder's schema.sql, refusing to replace app.lua.
    fs::write(
        apps.join("my-app").join("schema.sql"),
        "CREATE TABLE note (id VARCHAR PRIMARY KEY, text VARCHAR NOT NULL);",
    )
    .unwrap();
    let (code, _, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "my-app", "--scaffold", "note"],
    );
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("app.lua") && err.contains("never overwrites"),
        "{err}"
    );
    fs::remove_file(apps.join("my-app").join("app.lua")).unwrap();
    let (code, out, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "my-app", "--scaffold", "note"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("my-app/views/note_form.lsp"), "{out}");
    let (code, _, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "my-app", "--scaffold", "nope"],
    );
    assert_eq!(code, 1);
    assert!(err.contains("declares no table \"nope\""), "{err}");

    // Every one of them loads on the node.
    let node = Running::start(&["--data-dir", &dir, "--port", "0"]);
    for (path, needle) in [
        ("/a/my-app/", "<h1>Note</h1>"),
        ("/a/my-web/", "<h1>My Web</h1>"),
        ("/", "My Rust"),
    ] {
        let (status, body) = node.get(path);
        assert_eq!(status, 200, "{path}");
        assert!(body.contains(needle), "{path}: {body}");
    }
}

/// `§4` — `--from hello` copies the reference app and rewrites its slug and title, and the
/// copy runs beside the original.
#[test]
fn test_new_from_hello_rewrites_slug_and_title() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let app = Path::new(&dir).join("apps").join("greeter");

    let (code, out, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "greeter", "--from", "hello"],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("greeter/views/edit.lsp"), "{out}");
    let manifest = fs::read_to_string(app.join("app.toml")).unwrap();
    assert!(manifest.contains("slug        = \"greeter\""), "{manifest}");
    assert!(manifest.contains("title       = \"Greeter\""), "{manifest}");
    assert!(manifest.contains("apps/greeter/app.toml"), "{manifest}");
    let lua = fs::read_to_string(app.join("app.lua")).unwrap();
    assert!(lua.contains("File: apps/greeter/app.lua"), "{lua}");
    assert!(!lua.contains("apps/hello/"), "{lua}");
    let skill = fs::read_to_string(app.join("SKILL.md")).unwrap();
    assert!(skill.contains("name: privatium-app-greeter"), "{skill}");

    // A tier that disagrees with the copy is a usage error; a copy over a folder is refused.
    let (code, _, err) = privatium(
        root.path(),
        &[
            "--data-dir",
            &dir,
            "new",
            "other",
            "--from",
            "hello",
            "--tier",
            "web",
        ],
    );
    assert_eq!(code, 2, "{err}");
    let (code, _, _) = privatium(
        root.path(),
        &["--data-dir", &dir, "new", "greeter", "--from", "hello"],
    );
    assert_eq!(code, 1);

    // Copy plus scaffold: hello's schema, the scaffold's screens.
    let (code, out, err) = privatium(
        root.path(),
        &[
            "--data-dir",
            &dir,
            "new",
            "profiles",
            "--from",
            "hello",
            "--scaffold",
            "profile",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("profiles/views/profile_index.lsp"), "{out}");
    let lua = fs::read_to_string(Path::new(&dir).join("apps/profiles/app.lua")).unwrap();
    assert!(lua.contains("--scaffold profile"), "{lua}");

    let node = Running::start(&["--data-dir", &dir, "--port", "0"]);
    let (status, body) = node.get("/a/greeter/");
    assert_eq!(status, 200);
    assert!(body.contains("We haven't met yet."), "{body}");
    assert!(body.contains("<title>Greeter"), "{body}");
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200, "{body}");
    let (status, body) = node.get("/a/profiles/");
    assert_eq!(status, 200);
    assert!(body.contains("<h1>Profile</h1>"), "{body}");
}

/// `§5` — `lint` exits 0 on a clean app and 3 on findings (`§1`); `--format json` is one
/// object per finding with the seven fields of `§5.2`; `--severity error` hides a
/// warn-only fixture; `--fix` rewrites a mount path and reports what it touched (`§5.3`);
/// with no path it lints the installed apps, the reference apps included.
#[test]
fn test_spec_cli_5_lint_exit_codes_and_formats() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let repo = repo();

    let (code, out, err) = privatium(&repo, &["--data-dir", &dir, "lint", "apps/hello"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("0 finding(s)"), "{err}");

    let (code, out, err) = privatium(
        &repo,
        &["--data-dir", &dir, "lint", "apps/_lint/fail/PV301"],
    );
    assert_eq!(code, 3, "{out}{err}");
    assert!(out.contains("PV301 error:"), "{out}");
    assert!(
        out.contains("apps/_lint/fail/PV301/pv301bad/app.lua:"),
        "{out}"
    );
    assert!(out.contains("spec/app-contract.md §2.2"), "{out}");

    let (code, out, _) = privatium(
        &repo,
        &[
            "--data-dir",
            &dir,
            "lint",
            "apps/_lint/fail/PV301",
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 3);
    for line in out.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        for key in ["id", "severity", "file", "line", "message", "fix", "spec"] {
            assert!(value.get(key).is_some(), "{key}: {line}");
        }
        assert_eq!(value["id"], "PV301");
    }

    let (code, out, _) = privatium(
        &repo,
        &["--data-dir", &dir, "lint", "apps/_lint/fail/PV202"],
    );
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("PV202 warn:"), "{out}");
    let (code, out, _) = privatium(
        &repo,
        &[
            "--data-dir",
            &dir,
            "lint",
            "apps/_lint/fail/PV202",
            "--severity",
            "error",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.is_empty(), "{out}");

    // --fix on a copy: the mount path becomes url(), and the app is clean afterwards.
    let scratch = root.path().join("fix");
    copy_dir(
        &repo.join("apps/_lint/fail/PV301/pv301bad"),
        &scratch.join("pv301bad"),
    );
    let scratch_text = scratch.to_string_lossy().into_owned();
    let (code, _, err) = privatium(&repo, &["--data-dir", &dir, "lint", &scratch_text, "--fix"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("fixed 2 file(s)"), "{err}");
    let lua = fs::read_to_string(scratch.join("pv301bad").join("app.lua")).unwrap();
    assert!(lua.contains("pv.redirect(url('/'))"), "{lua}");

    // The solo-mode fixture needs the node configuration it was written for.
    let (code, out, _) = privatium(
        &repo,
        &["--data-dir", &dir, "lint", "apps/_lint/pass/PV502"],
    );
    assert_eq!(code, 3, "{out}");
    let (code, out, _) = privatium(
        &repo,
        &[
            "--data-dir",
            &dir,
            "--config",
            "apps/_lint/pass/PV502/config.toml",
            "lint",
            "apps/_lint/pass/PV502",
        ],
    );
    assert_eq!(code, 0, "{out}");

    // No path: every installed app plus the checkout's reference apps.
    let (code, out, err) = privatium(&repo, &["--data-dir", &dir, "lint"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(err.contains("4 app(s)"), "{err}");

    // A path that is not an app.
    let (code, out, _) = privatium(&repo, &["--data-dir", &dir, "lint", "docs"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("PV101"), "{out}");
}

/// `§6` — `skill list` names every skill this build ships and `skill export` writes the
/// `app-skills/` tree the repository holds, byte for byte, matching the running version.
#[test]
fn test_spec_cli_6_skill_list_and_export() {
    let root = tempfile::tempdir().unwrap();
    let (code, out, _) = privatium(root.path(), &["skill", "list"]);
    assert_eq!(code, 0);
    let names: Vec<&str> = out.lines().filter(|l| !l.starts_with(' ')).collect();
    assert_eq!(
        names,
        [
            "privatium-accessibility",
            "privatium-games",
            "privatium-overview",
            "privatium-security",
            "privatium-tier1-lua",
            "privatium-tier2-web",
            "privatium-tier3-rust",
        ]
    );
    assert!(out.contains("    Start here"), "descriptions follow: {out}");

    // Everything, into the default `skills/` under the working directory.
    let (code, _, err) = privatium(root.path(), &["skill", "export"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("pv/1 (partial: phase 2)"), "{err}");
    let exported = root.path().join("skills");
    let source = repo().join("app-skills");
    let mut count = 0;
    for entry in walk(&source) {
        let relative = entry.strip_prefix(&source).unwrap();
        let copy = exported.join(relative);
        assert!(copy.is_file(), "{}", relative.display());
        assert_eq!(
            fs::read(&entry).unwrap(),
            fs::read(&copy).unwrap(),
            "{}",
            relative.display()
        );
        count += 1;
    }
    assert_eq!(walk(&exported).len(), count);

    // One skill, into --out.
    let out_dir = root.path().join("one");
    let (code, _, _) = privatium(
        root.path(),
        &[
            "skill",
            "export",
            "privatium-tier1-lua",
            "--out",
            out_dir.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0);
    assert!(
        out_dir
            .join("privatium-tier1-lua")
            .join("SKILL.md")
            .is_file()
    );
    assert!(
        out_dir
            .join("privatium-tier1-lua")
            .join("reference")
            .join("README.md")
            .is_file()
    );
    assert!(!out_dir.join("README.md").exists());
    assert!(!out_dir.join("privatium-overview").exists());
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// `§7` — `snapshot` writes the set of `spec/protocol.md §5`; `--verify` recomputes the
/// checksums and exits non-zero on a mismatch, writing nothing.
/// `spec/cli.md §1`, `spec/protocol.md §3.1` — a data directory is one process's at a
/// time: while a node runs, `snapshot` and `restore` on the same directory are refused
/// naming the lock, `lint` — which opens no node — is not, and once the node stops the
/// same command succeeds.
#[test]
fn test_spec_cli_1_a_running_node_refuses_a_second_command() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let node = Running::start(&["--data-dir", &dir, "--port", "0"]);

    let (code, _, err) = privatium(root.path(), &["--data-dir", &dir, "snapshot"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("another privatium process"), "{err}");
    assert!(err.contains("local"), "{err}");

    let (code, _, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "restore", "--from", &dir, "--dry-run"],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("another privatium process"), "{err}");

    let hello = repo().join("apps").join("hello");
    let (code, _, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "lint", hello.to_str().unwrap()],
    );
    assert_eq!(code, 0, "lint takes no lock: {err}");

    drop(node);
    let (code, _, err) = privatium(root.path(), &["--data-dir", &dir, "snapshot"]);
    assert_eq!(code, 0, "{err}");
}

#[test]
fn test_spec_cli_7_snapshot_and_verify() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let (code, out, err) = privatium(root.path(), &["--data-dir", &dir, "snapshot"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("_sys: 20"), "{out}");
    assert!(out.contains("hello: 20"), "every loaded app: {out}");
    let snap = Path::new(&dir).join("data").join("_sys").join("snap");
    let ids: Vec<PathBuf> = fs::read_dir(&snap)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(ids.len(), 1);
    assert!(ids[0].join("MANIFEST.json").is_file());
    assert!(ids[0].join("sys_device.csv").is_file());

    let (code, out, _) = privatium(root.path(), &["--data-dir", &dir, "snapshot", "--verify"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("_sys: ") && l.ends_with(": ok")),
        "{out}"
    );
    assert_eq!(
        fs::read_dir(&snap).unwrap().count(),
        1,
        "--verify writes no snapshot"
    );

    // One app only, and a flipped byte.
    let csv = ids[0].join("sys_device.csv");
    let mut bytes = fs::read(&csv).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    fs::write(&csv, bytes).unwrap();
    let (code, out, err) = privatium(
        root.path(),
        &["--data-dir", &dir, "snapshot", "--verify", "--app", "_sys"],
    );
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("sys_device: csv mismatch"), "{out}");
    assert!(out.contains("MISMATCH"), "{out}");
    assert!(err.contains("do not match MANIFEST.json"), "{err}");
    assert!(!out.contains("hello:"), "--app narrows: {out}");
}

/// `§7` — `restore --from <backup>` brings a `data/` folder in, rebuilds each app by the
/// three tiers and reports the tier; `--dry-run` reports and copies nothing.
#[test]
fn test_spec_cli_7_restore_from_backup_reports_tier() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a").to_string_lossy().into_owned();
    let b = root.path().join("b").to_string_lossy().into_owned();

    // Node A: a hello event through the app, then a snapshot of everything.
    let node = Running::start(&["--data-dir", &a, "--port", "0"]);
    drop(node);
    let a_log_dir = Path::new(&a).join("data").join("hello").join("log");
    let a_log = fs::read_dir(&a_log_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let dev = a_log.file_stem().unwrap().to_string_lossy().into_owned();
    fs::write(
        &a_log,
        format!(
            "{{\"seq\":1,\"lam\":1,\"ts\":\"2026-09-04T00:00:00.000Z\",\"dev\":\"{dev}\",\"app\":\"hello\",\"op\":\"put\",\"tbl\":\"profile\",\"id\":\"01K4B0000000000000000000AA\",\"d\":{{\"display_name\":\"Backed Up\"}}}}\n"
        ),
    )
    .unwrap();
    let (code, _, err) = privatium(
        root.path(),
        &["--data-dir", &a, "snapshot", "--app", "hello"],
    );
    assert_eq!(code, 0, "{err}");
    let backup = root.path().join("backup");
    copy_dir(&Path::new(&a).join("data"), &backup.join("data"));

    // Node B, fresh: dry run first.
    let (code, out, err) = privatium(
        root.path(),
        &[
            "--data-dir",
            &b,
            "restore",
            "--from",
            backup.to_str().unwrap(),
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains(&format!("copy  hello/log/{dev}.jsonl (absent here)")),
        "{out}"
    );
    assert!(out.contains("copy  hello/snap/"), "{out}");
    assert!(out.contains("dry run: nothing copied"), "{out}");
    assert!(
        !Path::new(&b)
            .join("data")
            .join("hello")
            .join("log")
            .join(format!("{dev}.jsonl"))
            .exists()
    );

    // The real thing: hello's cache comes from the snapshot, tier 1.
    let (code, out, err) = privatium(
        root.path(),
        &[
            "--data-dir",
            &b,
            "restore",
            "--from",
            backup.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("hello: used tier 1 (sqlite) from 20"), "{out}");
    assert!(
        Path::new(&b)
            .join("data")
            .join("hello")
            .join("log")
            .join(format!("{dev}.jsonl"))
            .is_file()
    );
    let node = Running::start(&["--data-dir", &b, "--port", "0"]);
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200);
    assert!(body.contains("Backed Up"), "{body}");
    drop(node);

    // The same backup again is all "keep"; one narrowed to an app that is not in it is nothing.
    let (code, out, _) = privatium(
        root.path(),
        &[
            "--data-dir",
            &b,
            "restore",
            "--from",
            backup.to_str().unwrap(),
            "--app",
            "hello",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("keep  hello/log/{dev}.jsonl (identical)")),
        "{out}"
    );
    assert!(!out.contains("copy  "), "{out}");
    let (code, out, _) = privatium(
        root.path(),
        &[
            "--data-dir",
            &b,
            "restore",
            "--from",
            backup.to_str().unwrap(),
            "--app",
            "nope",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("nothing for nope"), "{out}");
}

/// `§7`, `spec/protocol.md §3.1` — a backup whose copy of a log has gone a different way
/// from this node's is refused whole, before anything is written.
#[test]
fn test_spec_cli_7_restore_refuses_a_diverged_log() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let node = Running::start(&["--data-dir", &dir, "--port", "0"]);
    drop(node);
    let backup = root.path().join("backup");
    copy_dir(&Path::new(&dir).join("data"), &backup);
    // Alter the backup's copy of this node's own _sys log, and add a snapshot dir to it,
    // so there is something the plan would otherwise copy.
    let log_dir = backup.join("_sys").join("log");
    let log = fs::read_dir(&log_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut text = fs::read_to_string(&log).unwrap();
    text.replace_range(text.len() - 3.., "X\n");
    fs::write(&log, text).unwrap();
    fs::create_dir_all(backup.join("_sys").join("snap").join("2026-W36-zzzz-1")).unwrap();
    fs::write(backup.join("_sys/snap/2026-W36-zzzz-1/MANIFEST.json"), "{}").unwrap();

    let (code, out, err) = privatium(
        root.path(),
        &[
            "--data-dir",
            &dir,
            "restore",
            "--from",
            backup.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(out.contains("DIVERGED  _sys/log/"), "{out}");
    assert!(err.contains("nothing was written"), "{err}");
    assert!(
        !Path::new(&dir)
            .join("data/_sys/snap/2026-W36-zzzz-1")
            .exists()
    );
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// `§9` — `firewall` parses its flags and says it is not in this build, so the help
/// text is the spec's without pretending to a phase that is not here.
#[test]
fn test_spec_cli_9_firewall_parses_and_refuses() {
    let root = tempfile::tempdir().unwrap();
    let (code, _, err) = privatium(root.path(), &["firewall", "--apply"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("privatium firewall: not in this build"),
        "{err}"
    );
    // A wrong flag is still a usage error, not a "not in this build".
    let (code, _, _) = privatium(root.path(), &["firewall", "--now"]);
    assert_eq!(code, 2);
}

/// `§10` — what is deliberately absent stays absent: neither a command nor a help entry.
#[test]
fn test_spec_cli_10_absent_commands() {
    let root = tempfile::tempdir().unwrap();
    let (_, help, _) = privatium(root.path(), &["--help"]);
    for absent in [
        "doctor", "diagnose", "serve", "migrate", "install", "login", "account",
    ] {
        let (code, _, err) = privatium(root.path(), &[absent]);
        assert_eq!(code, 2, "{absent}");
        assert!(err.contains("spec/cli.md §10"), "{absent}: {err}");
        assert!(
            !help.contains(&format!("privatium {absent}")),
            "{absent} in help"
        );
    }
}

/// `§2` — a run whose `apps/` holds no app folder writes the example apps there before
/// loading, so a release download's launcher is never empty: on a data directory that
/// does not exist, and on one used before the binary carried the examples; the second
/// run writes nothing, and an `apps/` with any folder in it gets nothing. The test-only
/// `PRIVATIUM_TEST_NO_CHECKOUT` stands in for a binary with no checkout beside it;
/// without it the checkout's `apps/` is what serves and nothing is written.
#[test]
fn test_spec_cli_2_first_run_writes_the_example_apps() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let apps = Path::new(&dir).join("apps");

    let node = Running::start_with_env(
        &["--data-dir", &dir, "--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    for slug in ["hello", "animals", "sketch", "pantry"] {
        assert!(apps.join(slug).join("app.toml").is_file(), "{slug}");
    }
    assert!(
        apps.join("animals")
            .join("static")
            .join("alpine-csp.min.js")
            .is_file()
    );
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("We haven't met yet."), "{body}");
    let (status, body) = node.get("/a/sketch/");
    assert_eq!(status, 200, "{body}");
    drop(node);

    // The second run is not a first run: an edit to a written copy survives it.
    let marker = apps.join("hello").join("README.md");
    fs::write(&marker, "edited by the owner\n").unwrap();
    let again = Running::start_with_env(
        &["--data-dir", &dir, "--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    drop(again);
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        "edited by the owner\n"
    );

    // A data directory used before, from a checkout, has an empty apps/ and index rows
    // for the examples: the release binary writes them rather than reporting every app
    // as folder missing.
    let used = root.path().join("used");
    let used_dir = used.to_string_lossy().into_owned();
    let node = Running::start(&["--data-dir", &used_dir, "--port", "0", "--no-discovery"]);
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200, "{body}");
    drop(node);
    assert!(fs::read_dir(used.join("apps")).unwrap().next().is_none());
    let node = Running::start_with_env(
        &["--data-dir", &used_dir, "--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    let (status, body) = node.get("/a/hello/");
    assert_eq!(status, 200, "{body}");
    let (status, body) = node.get("/settings/apps");
    assert_eq!(status, 200);
    assert!(!body.contains("folder missing"), "{body}");
    drop(node);
    assert!(used.join("apps").join("sketch").join("app.toml").is_file());

    // An apps/ with any folder in it — the owner's own — gets nothing.
    let mine = root.path().join("mine");
    let mine_apps = mine.join("apps");
    fs::create_dir_all(mine_apps.join("myapp")).unwrap();
    fs::write(
        mine_apps.join("myapp").join("app.toml"),
        "[app]\nslug = \"myapp\"\ntitle = \"Mine\"\nversion = \"1.0.0\"\napi = 1\ntier = \"web\"\n",
    )
    .unwrap();
    fs::create_dir_all(mine_apps.join("myapp").join("web")).unwrap();
    fs::write(
        mine_apps.join("myapp").join("web").join("index.html"),
        "<h1>Mine</h1>",
    )
    .unwrap();
    let mine_dir = mine.to_string_lossy().into_owned();
    let node = Running::start_with_env(
        &["--data-dir", &mine_dir, "--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    let (status, body) = node.get("/a/myapp/");
    assert_eq!(status, 200, "{body}");
    drop(node);
    let folders: Vec<String> = fs::read_dir(&mine_apps)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(folders, ["myapp"]);

    // In a checkout the repository's apps/ serves them and nothing is written.
    let checkout = root.path().join("checkout");
    let checkout_dir = checkout.to_string_lossy().into_owned();
    let node = Running::start(&["--data-dir", &checkout_dir, "--port", "0", "--no-discovery"]);
    let (status, body) = node.get("/");
    assert_eq!(status, 200);
    assert!(body.contains("Hello"), "{body}");
    drop(node);
    assert!(
        fs::read_dir(checkout.join("apps"))
            .unwrap()
            .next()
            .is_none()
    );
}

/// `§4` — `new --examples` writes every example app under its own slug, prints each file,
/// refuses to combine with a slug or another flag, and never overwrites: a second call is
/// a runtime error naming the folder, before anything is written.
#[test]
fn test_spec_cli_4_new_examples_writes_all_three_and_never_overwrites() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let apps = Path::new(&dir).join("apps");

    let (code, out, err) = privatium(root.path(), &["--data-dir", &dir, "new", "--examples"]);
    assert_eq!(code, 0, "{err}");
    for path in [
        "hello/app.lua",
        "animals/lib/tree.lua",
        "sketch/web/app.js",
        "pantry/schema.sql",
    ] {
        assert!(out.contains(path), "{path}: {out}");
        assert!(apps.join(path).is_file(), "{path}");
    }
    assert!(err.contains("hello, animals, sketch, pantry"), "{err}");
    let manifest = fs::read_to_string(apps.join("hello").join("app.toml")).unwrap();
    assert!(manifest.contains("slug        = \"hello\""), "{manifest}");

    // Never overwrites, and nothing else is touched by the refusal.
    fs::remove_dir_all(apps.join("sketch")).unwrap();
    let (code, _, err) = privatium(root.path(), &["--data-dir", &dir, "new", "--examples"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("hello") && err.contains("never overwrites"),
        "{err}"
    );
    assert!(!apps.join("sketch").exists(), "the refusal wrote nothing");

    for args in [
        &["new", "myapp", "--examples"][..],
        &["new", "--examples", "--tier", "web"],
        &["new", "--examples", "--from", "hello"],
    ] {
        let mut full = vec!["--data-dir", dir.as_str()];
        full.extend_from_slice(args);
        let (code, _, err) = privatium(root.path(), &full);
        assert_eq!(code, 2, "{args:?}: {err}");
    }

    // Every example loads on the node from the owner's apps/ alone.
    let node = Running::start_with_env(
        &["--data-dir", &dir, "--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    let (status, body) = node.get("/a/animals/");
    assert_eq!(status, 200, "{body}");
}

/// `§4` — `--from hello` copies the embedded example when no checkout and no installed
/// app of that slug is beside the binary, which is every release download.
#[test]
fn test_new_from_hello_works_without_a_checkout() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let output = Command::new(BIN)
        .args(["--data-dir", &dir, "new", "greeter", "--from", "hello"])
        .env("PRIVATIUM_TEST_NO_CHECKOUT", "1")
        .current_dir(root.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert!(err.contains("the example app hello"), "{err}");
    let manifest = fs::read_to_string(Path::new(&dir).join("apps/greeter/app.toml")).unwrap();
    assert!(manifest.contains("slug        = \"greeter\""), "{manifest}");
    assert!(manifest.contains("title       = \"Greeter\""), "{manifest}");

    let output = Command::new(BIN)
        .args(["--data-dir", &dir, "new", "xy", "--from", "no-such-app"])
        .env("PRIVATIUM_TEST_NO_CHECKOUT", "1")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("hello, animals, sketch, pantry"), "{err}");
}

/// `§1` — the data root is `--data-dir` when given, else a `privatium-data` folder beside
/// the executable when one exists, and every start says which; the platform directory
/// is never touched here, since a test must not write into the owner's own.
#[test]
fn test_spec_cli_1_portable_folder_beside_the_binary_is_the_data_root() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("portable");
    fs::create_dir_all(home.join("privatium-data")).unwrap();
    let copy = home.join(Path::new(BIN).file_name().unwrap());
    fs::copy(BIN, &copy).unwrap();

    let err = stderr_of_a_short_run_of(
        &copy,
        &["--port", "0", "--no-discovery"],
        &[("PRIVATIUM_TEST_NO_CHECKOUT", "1")],
    );
    let expected = format!(
        "privatium: data in {} (the privatium-data folder beside the program)",
        home.join("privatium-data").display()
    );
    assert!(err.contains(&expected), "{err}");
    assert!(
        home.join("privatium-data")
            .join("identity")
            .join("node.key")
            .is_file()
    );
    assert!(
        home.join("privatium-data")
            .join("apps")
            .join("hello")
            .join("app.toml")
            .is_file(),
        "the examples are written into the portable folder like any other empty apps/"
    );

    // The flag wins over the folder.
    let flagged = root.path().join("flagged");
    let flagged_dir = flagged.to_string_lossy().into_owned();
    let err = stderr_of_a_short_run_of(
        &copy,
        &["--data-dir", &flagged_dir, "--port", "0", "--no-discovery"],
        &[],
    );
    assert!(
        err.contains(&format!(
            "privatium: data in {} (named on the command line)",
            flagged.display()
        )),
        "{err}"
    );
    assert!(flagged.join("identity").join("node.key").is_file());

    // The data page names the same folder and the same reason.
    let node = Running::start_with_env(
        &["--data-dir", &flagged_dir, "--port", "0", "--no-discovery"],
        &[],
    );
    let (status, body) = node.get("/settings/data");
    assert_eq!(status, 200);
    assert!(body.contains("named on the command line"), "{body}");
}

// ---------------------------------------------------------------------------------------
// §2 `--open`, §8 `privatium pair`
// ---------------------------------------------------------------------------------------

/// A loopback port nobody listens on right now, for a `config.toml` a node and `pair`
/// will both read.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A data directory whose `config.toml` names `port`.
fn data_dir_on(root: &tempfile::TempDir, port: u16) -> String {
    let dir = data_dir(root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        Path::new(&dir).join("config.toml"),
        format!("[node]\nport = {port}\n"),
    )
    .unwrap();
    dir
}

/// One HTTP/1.1 exchange with a node on loopback, by hand — the same shape
/// `privatium pair` uses.
fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status: u16 = head
        .lines()
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (
        status,
        String::from_utf8_lossy(&raw[split + 4..]).into_owned(),
    )
}

/// Pair a synthetic browser with the node on `port` using the words of its open window,
/// over a real `/ws/pair` socket; the device's ID.
fn pair_with(port: u16) -> String {
    use futures_util::{SinkExt as _, StreamExt as _};
    use privatium_core::pair::{Code, handshake::Client};
    use tokio_tungstenite::tungstenite::Message;
    let (status, body) = http(port, "GET", "/api/v1/pair", "");
    assert_eq!(status, 200, "{body}");
    let window: serde_json::Value = serde_json::from_str(&body).unwrap();
    let words: Vec<&str> = window["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    let code = Code::parse(&words.join(" ")).unwrap();
    type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;
    async fn next(socket: &mut Ws) -> Message {
        tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws/pair"))
                .await
                .unwrap();
        let hello = next(&mut socket).await.into_text().unwrap();
        let (mut client, start) = Client::start(&hello, code, "browser").unwrap();
        socket.send(Message::Text(start.into())).await.unwrap();
        let reply = next(&mut socket).await.into_text().unwrap();
        let confirm = client.reply(&reply).unwrap();
        socket.send(Message::Text(confirm.into())).await.unwrap();
        let sealed = next(&mut socket).await.into_data();
        let (paired, finish) = client
            .finish(
                &sealed,
                Some("Synthetic phone"),
                None,
                jiff::Timestamp::now(),
            )
            .unwrap();
        socket.send(Message::Binary(finish.into())).await.unwrap();
        paired.device
    })
}

/// A node run in the background with both of its output streams captured, killed on
/// drop. `settle` is how long to keep reading after the announce line.
struct Captured {
    child: Child,
    port: u16,
    stdout: std::sync::Arc<std::sync::Mutex<String>>,
    stderr: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Drop for Captured {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Captured {
    fn start(args: &[&str], envs: &[(&str, &str)], settle: Duration) -> Self {
        let mut child = Command::new(BIN)
            .args(args)
            .envs(envs.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stderr = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (out_pipe, err_pipe) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let (out_sink, err_sink) = (stdout.clone(), stderr.clone());
        std::thread::spawn(move || {
            let mut reader = BufReader::new(err_pipe);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                err_sink.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(out_pipe);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                out_sink.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
        let started = Instant::now();
        let port = loop {
            let text = stdout.lock().unwrap().clone();
            if let Some(line) = text
                .lines()
                .find_map(|l| l.strip_prefix("privatium: listening on http://"))
            {
                break line
                    .rsplit(':')
                    .next()
                    .unwrap()
                    .trim_end_matches('/')
                    .parse::<u16>()
                    .unwrap();
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "no announce line; stderr: {}",
                stderr.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        std::thread::sleep(settle);
        Self {
            child,
            port,
            stdout,
            stderr,
        }
    }

    fn out(&self) -> String {
        self.stdout.lock().unwrap().clone()
    }

    fn err(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

/// `§2` — `--open` prints a QR code of the LAN URL with the URL in text beside it, and
/// opens the browser on the loopback URL; with the test variable set nothing is
/// launched and the URL is reported instead.
#[test]
fn test_spec_cli_2_open_prints_a_qr_and_the_lan_url() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir(&root);
    let node = Captured::start(
        &[
            "--data-dir",
            &dir,
            "--port",
            "0",
            "--no-discovery",
            "--open",
        ],
        &[("PRIVATIUM_TEST_NO_BROWSER", "1")],
        Duration::from_millis(1500),
    );
    let out = node.out();
    assert!(out.contains("scan this code or open http://"), "{out}");
    assert!(out.contains('█'), "the QR code in block characters:\n{out}");
    assert!(
        out.contains(&format!(":{}", node.port)),
        "the URL names the bound port:\n{out}"
    );
    let err = node.err();
    assert!(
        err.contains(&format!("would open http://127.0.0.1:{}/", node.port)),
        "{err}"
    );
}

/// `§2`, `spec/protocol.md §7.1` — on a node whose `sys_device` holds no row but its
/// own, `--open` opens one pairing window as the node starts and prints the code
/// beneath the QR code; a phone pairs through it; once any device has paired, `--open`
/// prints the QR code and opens no window.
#[test]
fn test_spec_cli_2_open_on_an_unpaired_node_opens_one_window() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port();
    let dir = data_dir_on(&root, port);
    let env = [("PRIVATIUM_TEST_NO_BROWSER", "1")];
    let device = {
        let node = Captured::start(
            &["--data-dir", &dir, "--no-discovery", "--open"],
            &env,
            Duration::from_millis(1500),
        );
        assert_eq!(node.port, port);
        let out = node.out();
        assert!(out.contains("pairing is open for 120 seconds"), "{out}");
        assert!(out.contains("or type these two words: "), "{out}");
        let (status, body) = http(port, "GET", "/api/v1/pair", "");
        assert_eq!(status, 200);
        let window: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(!window.is_null(), "the first-run window is open");
        let words = window["words"].as_array().unwrap();
        assert!(
            out.contains(&format!(
                "or type these two words: {} {}",
                words[0].as_str().unwrap(),
                words[1].as_str().unwrap()
            )),
            "{out}"
        );
        let device = pair_with(port);
        let (_, body) = http(port, "GET", "/api/v1/pair", "");
        let window: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(window["consumed_by"], device);
        device
    };
    let node = Captured::start(
        &["--data-dir", &dir, "--no-discovery", "--open"],
        &env,
        Duration::from_millis(1500),
    );
    let out = node.out();
    assert!(out.contains('█'), "{out}");
    assert!(!out.contains("pairing is open"), "{out}");
    let (_, body) = http(port, "GET", "/api/v1/pair", "");
    assert_eq!(body.trim(), "null", "no window once {device} has paired");
}

/// `§8` — `privatium pair` opens a window on the running node, prints the four emoji
/// with their labels, the two words, the QR code and the URL, follows the window and
/// exits 0 naming the device that paired.
#[test]
fn test_spec_cli_8_pair_prints_the_code_and_exits_on_success() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port();
    let dir = data_dir_on(&root, port);
    let _node = Captured::start(
        &["--data-dir", &dir, "--no-discovery"],
        &[],
        Duration::from_millis(200),
    );
    let pair = Command::new(BIN)
        .args(["--data-dir", &dir, "pair", "--timeout", "60"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The window opens on the node's first answer; wait for it before pairing.
    let started = Instant::now();
    loop {
        let (status, body) = http(port, "GET", "/api/v1/pair", "");
        assert_eq!(status, 200);
        if body.trim() != "null" {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "pair never opened a window"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let device = pair_with(port);
    let output = pair.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{out}\n{err}");
    assert!(
        out.contains(&format!("privatium pair: paired {device}")),
        "{out}"
    );
    assert!(out.contains("tap these four emoji"), "{out}");
    assert!(out.contains("or type these two words: "), "{out}");
    assert!(out.contains('█'), "{out}");
    assert!(out.contains("http://"), "{out}");
    // A glyph, two spaces, its label — the QR code's lines are indented too, but hold
    // block characters and spaces alone.
    let labels = out
        .lines()
        .filter(|line| {
            line.starts_with("    ")
                && line
                    .trim_start()
                    .split("  ")
                    .nth(1)
                    .is_some_and(|label| label.chars().next().is_some_and(char::is_uppercase))
        })
        .count();
    assert_eq!(labels, 4, "one glyph with its label per line:\n{out}");
}

/// `§8` — with no node running, `pair` is a runtime error that says to start one, and
/// opens no node of its own: the data directory stays untouched.
#[test]
fn test_spec_cli_8_pair_without_a_node_is_a_runtime_error() {
    let root = tempfile::tempdir().unwrap();
    let dir = data_dir_on(&root, free_port());
    let (code, out, err) = privatium(root.path(), &["--data-dir", &dir, "pair"]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(err.contains("no node is running"), "{err}");
    assert!(err.contains("start one with `privatium`"), "{err}");
    assert!(
        !Path::new(&dir).join("identity").exists(),
        "pair opened no node"
    );
    // A wrong flag is still a usage error.
    let (code, _, _) = privatium(root.path(), &["pair", "--qr"]);
    assert_eq!(code, 2);
}
