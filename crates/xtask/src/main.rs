// This file is part of Privatium
// crates/xtask/src/main.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-09-26
// Summary: Command dispatch for the repository's own checks. Deliberately not clap: spec/cli.md governs
//          the flags of `privatium`, and nothing here should ever be mistaken for part of that
//          surface.
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

use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};

mod header;
mod icons;
mod repo;
mod skill_reference;
mod spec_refs;

const USAGE: &str = "\
cargo xtask <command>

  header-check          every source file carries the standard header block
                        (AGENTS.md, Style)
  icons-verify          every icon name the shell, the apps, the skills and docs/icons.md
                        refer to exists in the vendored Bootstrap Icons set (docs/icons.md)
  gen-skill-reference [--check]
                        write app-skills/*/reference/ from the crate and the spec
                        (docs/skills.md §7); --check fails naming what drifted
  lint-spec-refs        every lint rule cites a document and section this checkout has
                        (spec/cli.md §5.2)
";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(true)` when the check passed, `Ok(false)` when it found something. An `Err` means
/// the check could not run at all, which is a different failure and reads differently.
fn run() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();

    match command.as_deref() {
        Some("header-check") => header::check(&repo::root()?),
        Some("icons-verify") => icons::check(&repo::root()?),
        Some("gen-skill-reference") => {
            let check = rest.iter().any(|a| a == "--check");
            skill_reference::run(&repo::root()?, check)
        }
        Some("lint-spec-refs") => spec_refs::check(&repo::root()?),
        Some("--help" | "-h") | None => {
            print!("{USAGE}");
            Ok(true)
        }
        Some(other) => bail!("unknown command {other:?}\n\n{USAGE}"),
    }
}

/// Run `git` in the repository and return its stdout, trimmed.
fn git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("could not run `git` — the repository checks need it on PATH")?;

    if !output.status.success() {
        bail!(
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8(output.stdout)
        .context("git printed something that was not UTF-8")?
        .trim_end()
        .to_owned())
}
