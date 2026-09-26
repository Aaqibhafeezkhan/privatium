// This file is part of Privatium
// crates/xtask/src/icons.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-09-26
// Summary: `cargo xtask icons-verify`. Every icon name the shell, the reference apps, the skills and
//          docs/icons.md's vocabulary table refer to must exist in the vendored Bootstrap
//          Icons set, and the vendored VERSION must be the one docs/icons.md pins
//          (docs/icons.md, PV503). Fails on a name the set lacks rather than falling back.
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

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Where the set lives.
const ICONS_DIR: &str = "crates/privatium-core/assets/icons/";

/// The document that pins the version and lists the vocabulary.
const ICONS_DOC: &str = "docs/icons.md";

/// One icon reference found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// 1-based line.
    pub line: usize,
    /// The name as written.
    pub name: String,
}

/// Check the repository. Returns `false` when something is missing.
pub fn check(root: &Path) -> Result<bool> {
    let files = crate::repo::files(root)?;
    let set: BTreeSet<String> = files
        .iter()
        .filter_map(|path| path.strip_prefix(ICONS_DIR))
        .filter_map(|name| name.strip_suffix(".svg"))
        .map(str::to_owned)
        .collect();
    if set.is_empty() {
        bail!("no icons under {ICONS_DIR}; the set must be vendored first (docs/icons.md)");
    }

    let mut findings = Vec::new();

    // The pin.
    let doc = crate::repo::read_normalized(&root.join(ICONS_DOC))?;
    let pinned = pinned_version(&doc)
        .with_context(|| format!("{ICONS_DOC} does not say which release is pinned"))?;
    let version = crate::repo::read_normalized(&root.join(ICONS_DIR).join("VERSION"))?;
    if version.trim() != pinned {
        findings.push(format!(
            "{ICONS_DIR}VERSION is {:?} but {ICONS_DOC} pins {pinned:?}",
            version.trim()
        ));
    }
    for required in ["LICENSE", "VENDOR.md"] {
        if !root.join(ICONS_DIR).join(required).is_file() {
            findings.push(format!("{ICONS_DIR}{required} is missing"));
        }
    }

    // The references.
    let mut checked = 0usize;
    for path in &files {
        if !scanned(path) {
            continue;
        }
        let contents = crate::repo::read_normalized(&root.join(path))?;
        let mut references = references_in(&contents);
        if path == ICONS_DOC {
            references.extend(vocabulary_in(&contents));
        }
        if path.ends_with("app.toml") {
            references.extend(manifest_icons_in(&contents));
        }
        checked += references.len();
        for finding in verify(&set, path, &references) {
            findings.push(finding);
        }
    }

    if findings.is_empty() {
        println!(
            "icons-verify: {} icons vendored ({pinned}), {checked} references, all present",
            set.len()
        );
        return Ok(true);
    }
    for finding in &findings {
        eprintln!("icons-verify: {finding}");
    }
    eprintln!(
        "\nicons-verify: {} problems. The set is Bootstrap Icons {pinned}; an app or the shell \
         may only name an icon it ships (docs/icons.md).",
        findings.len()
    );
    Ok(false)
}

/// Which files are read for `icon(...)` calls.
fn scanned(path: &str) -> bool {
    let in_tree = path.starts_with("apps/")
        || path.starts_with("skills/")
        || path.starts_with("app-skills/")
        || path.starts_with("docs/")
        || path.starts_with("crates/privatium-core/src/");
    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or_default();
    in_tree
        && matches!(
            extension,
            "lsp" | "lua" | "html" | "js" | "md" | "rs" | "toml"
        )
        && !path.contains("/vendor/")
        && !path.ends_with(".min.js")
        // The linter's corpus names a missing icon on purpose (`fail/PV503`), and the
        // generated anti-patterns reference quotes it.
        && !path.starts_with("apps/_lint/")
        && !path.contains("/reference/anti-patterns.md")
}

/// Every name that the set lacks, as a finding naming the file and line.
#[must_use]
pub fn verify(set: &BTreeSet<String>, path: &str, references: &[Reference]) -> Vec<String> {
    references
        .iter()
        .filter(|reference| !set.contains(&reference.name))
        .map(|reference| {
            format!(
                "{path}:{}: icon {:?} is not in the vendored set",
                reference.line, reference.name
            )
        })
        .collect()
}

/// Every `icon('name'` / `icon("name"` / `icon_labeled("name"` in a text, by line. A call
/// whose first argument is not a string literal is a variable and is not a reference.
#[must_use]
pub fn references_in(text: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for opener in ["icon_labeled(", "icon("] {
            let mut from = 0;
            while let Some(at) = line[from..].find(opener) {
                let start = from + at;
                // `icon(` inside `icon_labeled(` or inside another identifier is not a call.
                let preceded_by_ident = start > 0
                    && line[..start]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                from = start + opener.len();
                if preceded_by_ident {
                    continue;
                }
                let rest = line[from..].trim_start();
                let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') else {
                    continue;
                };
                let inner = &rest[1..];
                let Some(end) = inner.find(quote) else {
                    continue;
                };
                let name = &inner[..end];
                if !name.is_empty() {
                    out.push(Reference {
                        line: index + 1,
                        name: name.to_owned(),
                    });
                }
            }
        }
    }
    out
}

/// The `| Meaning | Icon |` rows of `docs/icons.md`'s vocabulary table: the last cell,
/// unquoted.
#[must_use]
pub fn vocabulary_in(doc: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut in_table = false;
    for (index, line) in doc.lines().enumerate() {
        if line.starts_with("## ") {
            in_table = line.contains("vocabulary");
            continue;
        }
        if !in_table || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if let [_, icon] = cells.as_slice()
            && let Some(name) = icon.strip_prefix('`').and_then(|s| s.strip_suffix('`'))
        {
            out.push(Reference {
                line: index + 1,
                name: name.to_owned(),
            });
        }
    }
    out
}

/// `icon = "name"` in an `app.toml` (`docs/icons.md`, "Naming in app.toml").
#[must_use]
pub fn manifest_icons_in(toml: &str) -> Vec<Reference> {
    toml.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let (key, value) = line.split_once('=')?;
            if key.trim() != "icon" {
                return None;
            }
            let value = value.split('#').next()?.trim();
            let name = value.strip_prefix('"')?.split('"').next()?;
            Some(Reference {
                line: index + 1,
                name: name.to_owned(),
            })
        })
        .collect()
}

/// `The pinned release is **vX.Y.Z**` in `docs/icons.md`.
#[must_use]
pub fn pinned_version(doc: &str) -> Option<String> {
    let marker = "pinned release is **";
    let at = doc.find(marker)? + marker.len();
    let rest = &doc[at..];
    let end = rest.find("**")?;
    Some(rest[..end].trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn calls_are_found_in_every_syntax() {
        let text = "<?= icon('trash') ?> <?= icon('pencil', { label = 'x' }) ?>\n\
                    icon(\"gear\") icon_labeled(\"x-lg\", \"Close\") icon(name) my_icon('nope')\n";
        let references = references_in(text);
        let names: Vec<&str> = references.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["trash", "pencil", "x-lg", "gear"]);
        assert_eq!(references[3].line, 2);
    }

    /// A name the vendored set lacks is a failure, not a silent fallback glyph.
    #[test]
    fn a_missing_name_is_a_finding() {
        let set = set(&["gear", "trash"]);
        let references = references_in("icon('gear') icon('not-an-icon')");
        let findings = verify(&set, "apps/x/views/a.lsp", &references);
        assert_eq!(
            findings,
            ["apps/x/views/a.lsp:1: icon \"not-an-icon\" is not in the vendored set"]
        );
        assert!(verify(&set, "a", &references_in("icon('trash')")).is_empty());
    }

    #[test]
    fn the_vocabulary_table_and_manifests_are_read() {
        let doc = "## Framework icon vocabulary\n\n| Meaning | Icon |\n|---|---|\n\
                   | Settings | `gear` |\n| Devices | `phone` |\n\n## Attribution\n\
                   | not | `read` |\n";
        let names: Vec<String> = vocabulary_in(doc).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["gear", "phone"]);
        let manifest = "[app]\nicon = \"diagram-3\"    # the file name\nicon2 = \"x\"\n";
        assert_eq!(manifest_icons_in(manifest)[0].name, "diagram-3");
        assert_eq!(manifest_icons_in(manifest).len(), 1);
        assert_eq!(
            pinned_version("The pinned release is **v1.13.1**; re-verify").as_deref(),
            Some("v1.13.1")
        );
    }
}
