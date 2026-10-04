// This file is part of Privatium
// crates/privatium/src/node.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-10-04
// Summary: What every subcommand that touches a node shares: opening it from the two global flags of
//          spec/cli.md §1, the app roots it loads (the owner's apps/ and, in a checkout, the
//          repository's example apps as bundled), what a first run is, the load report printed
//          the same way everywhere, and the browser opener `--open` uses.
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

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result};
use privatium_core::{AppRoot, DataLock, LoadReport, Node, Paths};

use crate::cli::Global;

/// The node the global flags name, opened — or created on first run.
pub fn open(global: &Global) -> Result<Node> {
    Node::open_with(global.data_dir.as_deref(), global.config.as_deref()).with_context(|| {
        match &global.data_dir {
            Some(dir) => format!("opening the node at {}", dir.display()),
            None => "opening the node in the platform data directory".to_owned(),
        }
    })
}

/// The paths the global flags name, without opening anything — for `new`, which writes
/// into `apps/` and needs no node, and for `restore`, which copies before opening.
pub fn paths(global: &Global) -> Result<Paths> {
    Paths::resolve(global.data_dir.as_deref(), global.config.as_deref())
        .context("resolving the data directory")
}

/// The repository's `apps/`, when this binary runs from a checkout.
///
/// A development start is the one situation in which the example apps are on disk
/// beside the binary's source, mounted as `bundled` so an edit to the repository's copy
/// is what runs. A bare binary — a CI artefact, a release download — has no such folder:
/// it carries the same apps embedded (`privatium_core::app::examples`) and writes them
/// into the owner's `apps/` while that folder holds no app ([`apps_dir_is_empty`],
/// `spec/cli.md §2`). A package that ships the folder at a path of its own is not
/// supported here (`spec/data-dictionary.md §3.4`, `source = bundled`), which is what a
/// distribution build would add. The path is fixed at compile time and simply absent
/// anywhere else; the test suite, which always runs from a checkout, sets
/// `PRIVATIUM_TEST_NO_CHECKOUT` to exercise the release binary's path.
#[must_use]
pub fn checkout_apps() -> Option<PathBuf> {
    if std::env::var_os("PRIVATIUM_TEST_NO_CHECKOUT").is_some() {
        return None;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .join("apps");
    dir.is_dir().then_some(dir)
}

/// Whether the owner's `apps/` holds no app folder at all (`spec/cli.md §2`): it does not
/// exist, or contains no directory. Files do not count; a single folder, the owner's or
/// an example's, does. The rest of the data directory is not consulted, so a root that
/// was used before the binary carried the examples still gets them.
#[must_use]
pub fn apps_dir_is_empty(paths: &Paths) -> bool {
    match std::fs::read_dir(paths.apps_dir()) {
        Ok(entries) => !entries
            .flatten()
            .any(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Every root in one call, or "folder missing" is wrong (`Node::load_apps`).
#[must_use]
pub fn roots(node: &Node) -> Vec<AppRoot> {
    let mut roots = vec![AppRoot::local(node.paths().apps_dir())];
    if let Some(checkout) = checkout_apps() {
        roots.push(AppRoot::bundled(checkout));
    }
    roots
}

/// Open the node and load its apps, printing the report.
pub fn open_loaded(global: &Global) -> Result<(Node, LoadReport)> {
    let mut node = open(global)?;
    let report = node.load_apps(&roots(&node))?;
    print_report(&report, global.verbose);
    Ok((node, report))
}

/// [`open_loaded`] for a caller that already holds the root's lock — `restore`, which
/// takes it before it copies anything in (`spec/cli.md §7`).
pub fn open_loaded_holding(global: &Global, lock: DataLock) -> Result<(Node, LoadReport)> {
    let mut node = Node::open_holding(lock).context("opening the node")?;
    let report = node.load_apps(&roots(&node))?;
    print_report(&report, global.verbose);
    Ok((node, report))
}

/// What `load_apps` found, to standard error: failures and warnings always, the loaded
/// list under `--verbose`.
pub fn print_report(report: &LoadReport, verbose: bool) {
    if verbose {
        for slug in &report.loaded {
            eprintln!("privatium: loaded {slug}");
        }
        for slug in &report.disabled {
            eprintln!("privatium: {slug} is disabled");
        }
        for slug in &report.missing {
            eprintln!("privatium: {slug}: folder missing");
        }
    }
    for failure in &report.failed {
        eprintln!("privatium: not loaded — {failure}");
    }
    for warning in &report.warnings {
        eprintln!(
            "privatium: {} — {warning}",
            warning.label().to_ascii_lowercase()
        );
    }
}

/// Why `slug` did not load, from the report, for a message.
#[must_use]
pub fn failure_of(report: &LoadReport, slug: &str) -> Option<String> {
    report
        .failed
        .iter()
        .find(|failure| failure.folder == slug)
        .map(ToString::to_string)
}

/// Open `url` in the owner's browser (`spec/cli.md §2`, `--open`), through the platform's
/// opener. Best effort: a failure is a line on standard error, never a reason to stop
/// the node. The test suite sets `PRIVATIUM_TEST_NO_BROWSER` so a test of `--open` never
/// launches one; then the URL is reported and nothing is opened.
pub fn open_browser(url: &str) {
    if std::env::var_os("PRIVATIUM_TEST_NO_BROWSER").is_some() {
        eprintln!("privatium: would open {url}");
        return;
    }
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    let result = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match result {
        Ok(_) => eprintln!("privatium: opening {url}"),
        Err(error) => eprintln!("privatium: could not open a browser: {error}"),
    }
}
