// This file is part of Privatium
// crates/privatium/src/skill.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-09-26
// Summary: `privatium skill list|export` (spec/cli.md §6, docs/skills.md §6): the skills embedded in
//          this build — the same files /skills/<name>.md and /skills/bundle.zip serve — named,
//          and written to disk at their paths under app-skills/ so an owner hands their
//          assistant the contract of the version they are running.
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

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result};
use privatium_core::http::skills;

use crate::cli::HELP;

/// Where `export` writes without `--out`: a `skills/` folder in the working directory,
/// which is where a repository built from the template keeps the skills its assistants read.
const DEFAULT_OUT: &str = "skills";

/// `skill list`: one line per skill — the folder name, then the `description:` of its
/// front matter, which is what an assistant's own index shows.
pub fn list() -> Result<u8> {
    for name in skills::names() {
        let description = skills::skill(&name)
            .and_then(front_matter_description)
            .unwrap_or_default();
        if description.is_empty() {
            println!("{name}");
        } else {
            println!("{name}\n    {description}");
        }
    }
    Ok(0)
}

/// `skill export [<name>...] [--out <dir>]`: every named skill's folder — all of them, and
/// `README.md`, when none is named — written under `--out`, files overwritten so a
/// re-export after an upgrade is the new version.
pub fn export(names: &[String], out: Option<&Path>) -> Result<u8> {
    let known = skills::names();
    let unknown: Vec<&String> = names.iter().filter(|n| !known.contains(n)).collect();
    if !unknown.is_empty() {
        eprintln!(
            "privatium: skill export: no skill named {}; this build ships {}\n\n{HELP}",
            unknown
                .iter()
                .map(|n| format!("{n:?}"))
                .collect::<Vec<_>>()
                .join(", "),
            known.join(", ")
        );
        return Ok(2);
    }

    let out = out.unwrap_or_else(|| Path::new(DEFAULT_OUT));
    let mut written = 0usize;
    for (path, bytes) in skills::files() {
        let wanted = if names.is_empty() {
            true
        } else {
            names
                .iter()
                .any(|name| path.starts_with(&format!("{name}/")))
        };
        if !wanted {
            continue;
        }
        let target = out.join(&path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&target, bytes).with_context(|| format!("writing {}", target.display()))?;
        written += 1;
    }
    eprintln!(
        "privatium: {written} file(s) written under {} — the skills of {} ({})",
        out.display(),
        env!("CARGO_PKG_VERSION"),
        crate::protocol_claim()
    );
    Ok(0)
}

/// The `description:` line of a `SKILL.md` front matter block.
fn front_matter_description(text: &str) -> Option<String> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    body[..end]
        .lines()
        .find_map(|line| line.strip_prefix("description:"))
        .map(|d| d.trim().trim_matches('"').to_owned())
}
