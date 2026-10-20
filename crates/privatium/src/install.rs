// This file is part of Privatium
// crates/privatium/src/install.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-10-15
// Last Modified: 2026-10-16
// Summary: `privatium install <zip> [--yes]` (spec/cli.md §4.1): read an app zip, show what
//          will be written, surface security and privacy warnings, ask for confirmation
//          (unless --yes), then extract into <data-dir>/apps/<slug>/.
//          No file is written before the user confirms. The zip reader in
//          privatium_core::zip refuses path-traversal names before this function runs.
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
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use privatium_core::app::manifest::{Manifest, Tier};
use privatium_core::config::Mode;
use privatium_core::zip;

use crate::cli::Global;
use crate::node;

/// `privatium install <zip> [--yes]` (`spec/cli.md §4.1`).
///
/// Reads the zip at `zip_path`, locates `app.toml` (at the root of the archive or inside
/// a single top-level folder), validates the slug and refuses if the destination folder
/// already exists, prints everything that will be written, surfaces security and privacy
/// warnings, asks for confirmation (unless `--yes`), then extracts the app into
/// `<data-dir>/apps/<slug>/`.
pub fn install(global: &Global, zip_path: &Path, yes: bool) -> Result<u8> {
    // ### Read the zip ###
    let bytes = fs::read(zip_path).with_context(|| format!("reading {}", zip_path.display()))?;
    let all = zip::entries(&bytes).map_err(|e| anyhow::anyhow!("{e}"))?;

    // ### Locate app.toml and determine strip prefix ###
    let (app_toml_bytes, strip) = locate_manifest(&all)?;

    // ### Parse and validate the manifest ###
    let manifest_text =
        std::str::from_utf8(app_toml_bytes).context("app.toml is not valid UTF-8")?;
    let manifest = Manifest::parse(manifest_text).context("parsing app.toml")?;
    let slug = &manifest.app.slug;

    manifest
        .validate(slug, Mode::Host)
        .map_err(|e| anyhow::anyhow!("invalid manifest: {e}"))?;

    // ### Check the destination ###
    let paths = node::paths(global)?;
    let apps_dir = paths.apps_dir();
    let target = apps_dir.join(slug);
    if target.exists() {
        bail!(
            "{}: already exists; `privatium install` does not overwrite - delete it first \
             or choose another slug",
            target.display()
        );
    }

    // ### Collect files that will be written ###
    let files: Vec<(PathBuf, &[u8])> = all
        .iter()
        .filter_map(|(name, data)| {
            let rel = strip_prefix(name, strip)?;
            Some((PathBuf::from(rel), data.as_slice()))
        })
        .collect();

    if files.is_empty() {
        bail!("install: zip contains no files to extract");
    }

    // Check that the required file for the tier exists
    if let Some(required) = manifest.app.tier.required_file() {
        let req_path = Path::new(required);
        if !files.iter().any(|(rel, _)| rel == req_path) {
            bail!(
                "install: app declares tier {:?} but zip is missing required file {:?}",
                manifest.app.tier.as_str(),
                required
            );
        }
    }

    // ### Show what will be installed and display warnings ###
    eprintln!(
        "privatium install: {} ({}), from {}",
        manifest.app.title,
        slug,
        zip_path.display()
    );
    eprintln!(
        "  Will write {} file(s) to {}:",
        files.len(),
        target.display()
    );
    for (rel, _) in &files {
        eprintln!("    {}", rel.display());
    }

    // Security warning (`spec/app-contract.md §9`)
    eprintln!("\nSecurity warning:");
    match manifest.app.tier {
        Tier::Lua => {
            eprintln!(
                "  Treat installing a third-party app the way you would treat running a \
                 script someone emailed you: this app's Lua runs on your node against your data."
            );
        }
        Tier::Web => {
            eprintln!(
                "  Treat installing a third-party app the way you would treat running a \
                 script someone emailed you: this app's JavaScript runs in your browser on your session's origin."
            );
        }
        Tier::Rust => {
            eprintln!(
                "  Treat installing a third-party app the way you would treat running a \
                 script someone emailed you."
            );
        }
    }

    // Privacy warning for non-default permissions (`spec/app-contract.md §5.4`)
    let widenings = manifest.permissions.widenings();
    if !widenings.is_empty() {
        eprintln!(
            "\nPrivacy warning{}:",
            if widenings.len() == 1 { "" } else { "s" }
        );
        for w in &widenings {
            eprintln!("  - {w}");
        }
    }

    // Confirmation prompt (skipped if --yes)
    if !yes {
        eprint!("\nInstall {slug:?}? [y/N] ");
        io::stderr().flush().ok();

        let mut answer = String::new();
        let bytes_read = io::stdin()
            .read_line(&mut answer)
            .context("reading confirmation")?;
        if bytes_read == 0 || !answer.trim().eq_ignore_ascii_case("y") {
            eprintln!("privatium install: cancelled");
            return Ok(1);
        }
    }

    // ### Write the files ###
    // Refuse before writing anything if any destination file already exists, then write
    // all at once - consistent with `new` (spec/cli.md §4).
    for (rel, _) in &files {
        let on_disk = target.join(rel);
        if on_disk.exists() {
            bail!(
                "{}: exists; `privatium install` never overwrites",
                on_disk.display()
            );
        }
    }
    fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;
    for (rel, data) in &files {
        let on_disk = target.join(rel);
        if let Some(parent) = on_disk.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&on_disk, data).with_context(|| format!("writing {}", on_disk.display()))?;
        println!("{}", relative(&on_disk, &apps_dir));
    }
    eprintln!(
        "privatium install: {slug} installed to {} - `privatium dev --app {slug}` runs it",
        target.display()
    );
    Ok(0)
}

/// Find `app.toml` among the zip entries.
///
/// Returns `(bytes_of_app_toml, strip_components)` where `strip_components` is the number
/// of leading path components to remove when writing each entry. Two layouts are accepted:
///
/// - **Flat**: `app.toml` is at the root of the archive. Strip 0 components.
/// - **Wrapped**: every entry starts with the same single top-level folder, e.g.
///   `hello/app.toml`. Strip that one component.
///
/// Any other layout is a usage error.
fn locate_manifest(all: &[(String, Vec<u8>)]) -> Result<(&[u8], usize)> {
    // Flat layout: app.toml at the top.
    if let Some((_, data)) = all.iter().find(|(n, _)| n == "app.toml") {
        return Ok((data, 0));
    }

    // Wrapped layout: a single top-level folder contains app.toml.
    let prefix = all
        .iter()
        .find_map(|(n, _)| {
            let (folder, rest) = n.split_once('/')?;
            (rest == "app.toml").then_some(folder)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "install: zip does not contain app.toml at the top level or inside a \
                 single folder - is this a valid Privatium app bundle?"
            )
        })?;

    // Verify every entry sits under that same folder.
    for (name, _) in all {
        if !name.starts_with(&format!("{prefix}/")) {
            bail!(
                "install: zip mixes entries inside and outside {prefix:?}; expected a \
                 single wrapping folder or a flat archive"
            );
        }
    }

    let data = all
        .iter()
        .find_map(|(n, d)| (n == &format!("{prefix}/app.toml")).then_some(d.as_slice()))
        .ok_or_else(|| anyhow::anyhow!("install: {prefix}/app.toml not found"))?;

    Ok((data, 1))
}

/// Remove `strip` leading slash-separated components from `name`.
///
/// Returns `None` if `name` has fewer components than `strip` after splitting, which
/// means the entry represents the folder itself and has no file payload to write.
fn strip_prefix(name: &str, strip: usize) -> Option<&str> {
    if strip == 0 {
        return Some(name);
    }
    let mut rest = name;
    for _ in 0..strip {
        rest = rest.split_once('/')?.1;
    }
    if rest.is_empty() { None } else { Some(rest) }
}

fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_manifest(slug: &str, tier: &str) -> String {
        format!(
            r#"[app]
slug = "{slug}"
title = "Sample App"
version = "0.1.0"
api = 1
tier = "{tier}"
"#
        )
    }

    #[test]
    fn locate_manifest_flat() {
        let manifest = sample_manifest("testapp", "web");
        let entries = vec![
            ("app.toml".to_string(), manifest.into_bytes()),
            ("web/index.html".to_string(), b"<h1>Hello</h1>".to_vec()),
        ];
        let (data, strip) = locate_manifest(&entries).unwrap();
        assert_eq!(strip, 0);
        assert!(std::str::from_utf8(data).unwrap().contains("testapp"));
    }

    #[test]
    fn locate_manifest_wrapped() {
        let manifest = sample_manifest("testapp", "web");
        let entries = vec![
            ("testapp/app.toml".to_string(), manifest.into_bytes()),
            (
                "testapp/web/index.html".to_string(),
                b"<h1>Hello</h1>".to_vec(),
            ),
        ];
        let (data, strip) = locate_manifest(&entries).unwrap();
        assert_eq!(strip, 1);
        assert!(std::str::from_utf8(data).unwrap().contains("testapp"));
    }

    #[test]
    fn locate_manifest_missing() {
        let entries = vec![("other.txt".to_string(), b"hello".to_vec())];
        assert!(locate_manifest(&entries).is_err());
    }

    #[test]
    fn locate_manifest_mixed_folders_refused() {
        let manifest = sample_manifest("testapp", "web");
        let entries = vec![
            ("folder1/app.toml".to_string(), manifest.into_bytes()),
            ("folder2/index.html".to_string(), b"hello".to_vec()),
        ];
        assert!(locate_manifest(&entries).is_err());
    }

    #[test]
    fn install_extracts_files() {
        let dir = TempDir::new().unwrap();
        let global = Global {
            data_dir: Some(dir.path().to_path_buf()),
            config: None,
            verbose: false,
        };

        let manifest = sample_manifest("myweb", "web");
        let zip_bytes = zip::stored(&[
            ("myweb/app.toml".to_string(), manifest.as_bytes()),
            (
                "myweb/web/index.html".to_string(),
                b"<h1>Test</h1>".as_slice(),
            ),
        ]);

        let zip_path = dir.path().join("myweb.zip");
        fs::write(&zip_path, zip_bytes).unwrap();

        let res = install(&global, &zip_path, true).unwrap();
        assert_eq!(res, 0);

        let app_dir = dir.path().join("apps").join("myweb");
        assert!(app_dir.join("app.toml").exists());
        assert!(app_dir.join("web").join("index.html").exists());
        assert_eq!(
            fs::read_to_string(app_dir.join("web").join("index.html")).unwrap(),
            "<h1>Test</h1>"
        );

        // Second install of same app is refused
        let err = install(&global, &zip_path, true).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn install_refuses_missing_required_file() {
        let dir = TempDir::new().unwrap();
        let global = Global {
            data_dir: Some(dir.path().to_path_buf()),
            config: None,
            verbose: false,
        };

        let manifest = sample_manifest("mylua", "lua");
        let zip_bytes = zip::stored(&[("mylua/app.toml".to_string(), manifest.as_bytes())]);

        let zip_path = dir.path().join("mylua.zip");
        fs::write(&zip_path, zip_bytes).unwrap();

        let err = install(&global, &zip_path, true).unwrap_err();
        assert!(err.to_string().contains("missing required file"));
    }
}
