// This file is part of Privatium
// crates/xtask/src/header.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-09-26
// Summary: `cargo xtask header-check`. AGENTS.md requires a header block on every source file; this is
//          what turns that from a habit into a gate.
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

use std::path::Path;

use anyhow::Result;

/// How many lines from the top of a file the header block may occupy. The block carries a
/// full licence notice now, so the window is larger than the six-field form it replaced.
const HEADER_WINDOW: usize = 40;

/// The first line of every header, naming the project.
const PROJECT_LINE: &str = "This file is part of Privatium";

/// The notes line that points at the licence notices the README carries in full, so no
/// file has to repeat them. Kept whole on one line, so this check and a reader can both
/// find it.
const NOTES_SENTENCE: &str = "See README file for documentation and full license information.";

/// The copyright line's stable prefix. The year varies by file.
const COPYRIGHT: &str = "Copyright \u{a9} ";

/// Extensions that must carry a header, and the comment openers each accepts.
///
/// `.md` is here but is further narrowed to `spec/` and `docs/` by [`in_scope`]: the
/// root-level documents, every `apps/**` README and SKILL.md, and everything under
/// `skills/` and `app-skills/` are exempt by design — they are prose for humans and assistants, and a
/// provenance block at the top of `README.md` would be noise.
///
/// `.html` and `.toml` are deliberately absent. Manifests in this repository carry a
/// header by convention and should keep doing so, but the convention is not enforced.
const CHECKED: &[(&str, &[&str])] = &[
    ("rs", &["//", "/*"]),
    ("lua", &["--"]),
    ("sql", &["--"]),
    ("js", &["//", "/*"]),
    ("css", &["//", "/*"]),
    ("lsp", &["<?--"]),
    ("md", &["<!--"]),
];

/// What a header must contain. `.lsp` files carry a reduced form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// The project line, the file's own path, `Author(s):`, `Created:`, `Last Modified:`,
    /// `Summary:`, `Notes:`, and the licence notice its file type takes.
    Full,
    /// The project line, the file's own path, `Summary:`, `Notes:`. Templates are read
    /// alongside the app they belong to, and repeating authorship and a full licence
    /// notice on every partial helps nobody; the notes line still points at the README.
    Template,
}

/// Check every in-scope file. Returns `false` when at least one is missing a header.
pub fn check(root: &Path) -> Result<bool> {
    let mut checked = 0usize;
    let mut findings = Vec::new();

    for path in crate::repo::files(root)? {
        let Some(shape) = in_scope(root, &path) else {
            continue;
        };
        checked += 1;

        let contents = crate::repo::read_normalized(&root.join(&path))?;
        for problem in problems(&path, &contents, shape) {
            findings.push(format!("{path}: {problem}"));
        }
    }

    if findings.is_empty() {
        println!("header-check: {checked} files, all carry a header block");
        return Ok(true);
    }

    for finding in &findings {
        eprintln!("header-check: {finding}");
    }
    eprintln!(
        "\nheader-check: {} problems across {checked} files. \
         The format is in AGENTS.md, section 3.",
        findings.len()
    );
    Ok(false)
}

/// The shape required of `path`, or `None` if the file is not checked.
fn in_scope(root: &Path, path: &str) -> Option<Shape> {
    let extension = path.rsplit_once('.').map(|(_, ext)| ext)?;
    if !CHECKED.iter().any(|(checked, _)| *checked == extension) {
        return None;
    }

    if extension == "md" && !(path.starts_with("spec/") || path.starts_with("docs/")) {
        return None;
    }
    if is_vendored(root, path) {
        return None;
    }

    Some(if extension == "lsp" {
        Shape::Template
    } else {
        Shape::Full
    })
}

/// Third-party code carries its own provenance and must not be given ours.
///
/// The marker is a `VENDOR.md` beside the file or above it, which this repository already
/// requires of anything vendored (`apps/animals/static/VENDOR.md` is the worked example).
/// Minified bundles are excluded too, because a minifier discards comments anyway.
fn is_vendored(root: &Path, path: &str) -> bool {
    if path.ends_with(".min.js") || path.ends_with(".min.css") {
        return true;
    }
    if path.starts_with("vendor/") || path.contains("/vendor/") {
        return true;
    }

    let mut directory = Path::new(path).parent();
    while let Some(current) = directory {
        if root.join(current).join("VENDOR.md").is_file() {
            return true;
        }
        directory = current.parent();
    }
    false
}

/// Everything wrong with one file's header. Empty means it is fine.
fn problems(path: &str, contents: &str, shape: Shape) -> Vec<String> {
    let mut problems = Vec::new();

    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or_default();
    let openers = CHECKED
        .iter()
        .find(|(checked, _)| *checked == extension)
        .map(|(_, openers)| *openers)
        .unwrap_or_default();

    let first = contents
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    if !openers
        .iter()
        .any(|opener| first.trim_start().starts_with(opener))
    {
        problems.push(format!(
            "does not open with a comment ({}); found {:?}",
            openers.join(" or "),
            first.trim().chars().take(40).collect::<String>()
        ));
    }

    // Scanning a bounded window rather than parsing each language's comment grammar. The
    // job is to notice a missing header, and a file whose first line is a comment and
    // whose next twenty carry all six fields has one.
    let window: String = contents
        .lines()
        .take(HEADER_WINDOW)
        .collect::<Vec<_>>()
        .join("\n");

    if !window.contains(PROJECT_LINE) {
        problems.push(format!("no `{PROJECT_LINE}` line"));
    }
    if !window.contains(path) {
        problems.push(format!("header does not name its own path `{path}`"));
    }
    if !window.contains("Summary:") {
        problems.push("no `Summary:` field".to_owned());
    }
    if !window.contains(NOTES_SENTENCE) {
        // The sentence is kept whole on one line so this check, and a reader, can find it.
        problems.push(format!("no notes line reading `{NOTES_SENTENCE}`"));
    }

    if shape == Shape::Full {
        if !window.contains("Author(s):") {
            problems.push("no `Author(s):` field".to_owned());
        }
        for field in ["Created:", "Last Modified:"] {
            match window.split_once(field) {
                None => problems.push(format!("no `{field}` field")),
                // Shape only. A mechanical check that `Last Modified:` matches the last
                // commit would either be wrong or fight every commit that touches the file.
                Some((_, after)) if !starts_with_iso_date(after) => {
                    problems.push(format!("`{field}` is not followed by a YYYY-MM-DD date"));
                }
                Some(_) => {}
            }
        }

        match window.split_once(COPYRIGHT) {
            None => problems.push("no `Copyright \u{a9}` line".to_owned()),
            Some((_, after)) if !starts_with_year(after) => {
                problems.push("`Copyright \u{a9}` is not followed by a four-digit year".to_owned());
            }
            Some(_) => {}
        }

        if !has_license_notice(&window, extension) {
            problems.push(license_expectation(extension).to_owned());
        }
    }

    problems
}

/// True when the header carries the licence notice its file type takes. Documents under
/// `spec/` and `docs/` are FDL; everything else is GPL. Matching one distinctive line of
/// each keeps this readable without pinning every word of the notice.
fn has_license_notice(window: &str, extension: &str) -> bool {
    if extension == "md" {
        window.contains("GNU Free Documentation License")
    } else {
        window.contains("GNU General Public License")
    }
}

fn license_expectation(extension: &str) -> &'static str {
    if extension == "md" {
        "no GNU Free Documentation License notice"
    } else {
        "no GNU General Public License notice"
    }
}

/// True when `text`, after leading spaces, begins with four digits.
fn starts_with_year(text: &str) -> bool {
    let candidate: Vec<char> = text.trim_start().chars().take(4).collect();
    candidate.len() == 4 && candidate.iter().all(char::is_ascii_digit)
}

/// True when `text`, after leading spaces, begins with `YYYY-MM-DD`.
fn starts_with_iso_date(text: &str) -> bool {
    let candidate: Vec<char> = text.trim_start().chars().take(10).collect();
    if candidate.len() != 10 {
        return false;
    }
    candidate
        .iter()
        .enumerate()
        .all(|(index, character)| match index {
            4 | 7 => *character == '-',
            _ => character.is_ascii_digit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
// This file is part of Privatium
// crates/privatium-core/src/lib.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-08-31
// Summary: Crate root.
// Notes: See README file for documentation and full license information.
//
// Copyright \u{a9} 2026 Gabriel Mongefranco
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

pub fn nothing() {}
";

    const GOOD_DOC: &str = "\
<!--
This file is part of Privatium
spec/protocol.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-08-28
Summary: NORMATIVE. Wire formats.
Notes: See README file for documentation and full license information.

Copyright \u{a9} 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Protocol
";

    #[test]
    fn a_complete_header_passes() {
        let found = problems("crates/privatium-core/src/lib.rs", GOOD, Shape::Full);
        assert!(found.is_empty(), "{found:?}");
    }

    /// A document takes the documentation licence, and the code licence does not satisfy it.
    #[test]
    fn a_document_carries_the_documentation_licence() {
        let found = problems("spec/protocol.md", GOOD_DOC, Shape::Full);
        assert!(found.is_empty(), "{found:?}");

        let wrong = GOOD_DOC.replace("GNU Free Documentation License", "GNU General Public License");
        let found = problems("spec/protocol.md", &wrong, Shape::Full);
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("Free Documentation")),
            "{found:?}"
        );
    }

    #[test]
    fn a_header_without_the_notes_sentence_fails() {
        let bare = GOOD.replace(NOTES_SENTENCE, "");
        let found = problems("crates/privatium-core/src/lib.rs", &bare, Shape::Full);
        assert!(
            found.iter().any(|problem| problem.contains("notes line")),
            "{found:?}"
        );

        // Split across two lines it is not found either: the sentence stays whole.
        let split = GOOD.replace(
            NOTES_SENTENCE,
            "See README file for documentation\n// and full license information.",
        );
        let found = problems("crates/privatium-core/src/lib.rs", &split, Shape::Full);
        assert!(!found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_header_without_a_licence_notice_fails() {
        let bare = GOOD.replace("GNU General Public License", "a licence of some sort");
        let found = problems("crates/privatium-core/src/lib.rs", &bare, Shape::Full);
        assert!(
            found.iter().any(|problem| problem.contains("General Public")),
            "{found:?}"
        );
    }

    #[test]
    fn a_header_without_a_copyright_year_fails() {
        let undated = GOOD.replace("Copyright \u{a9} 2026", "Copyright \u{a9} this year");
        let found = problems("crates/privatium-core/src/lib.rs", &undated, Shape::Full);
        assert!(
            found.iter().any(|problem| problem.contains("four-digit")),
            "{found:?}"
        );
    }

    /// Delete the header, and the check fails - the whole point of it.
    #[test]
    fn a_deleted_header_fails() {
        let stripped = "pub fn nothing() {}\n";
        let found = problems("crates/privatium-core/src/lib.rs", stripped, Shape::Full);
        assert!(
            !found.is_empty(),
            "a file with no header at all was accepted"
        );
    }

    #[test]
    fn a_header_naming_the_wrong_file_fails() {
        let found = problems("crates/privatium/src/main.rs", GOOD, Shape::Full);
        assert!(
            found.iter().any(|problem| problem.contains("its own path")),
            "{found:?}"
        );
    }

    #[test]
    fn a_missing_date_fails() {
        let undated = GOOD.replace("Created: 2026-08-31", "Created: last Tuesday");
        let found = problems("crates/privatium-core/src/lib.rs", &undated, Shape::Full);
        assert!(
            found.iter().any(|problem| problem.contains("YYYY-MM-DD")),
            "{found:?}"
        );
    }

    #[test]
    fn a_template_needs_less() {
        let template = "\
<?--
This file is part of Privatium
apps/hello/views/index.lsp
Summary: Greeting.
Notes: See README file for documentation and full license information.
--?>
<h1>Hello</h1>
";
        let found = problems("apps/hello/views/index.lsp", template, Shape::Template);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_template_still_needs_a_summary() {
        let template = "\
<?--
This file is part of Privatium
apps/hello/views/index.lsp
Notes: See README file for documentation and full license information.
--?>
";
        let found = problems("apps/hello/views/index.lsp", template, Shape::Template);
        assert!(
            found.iter().any(|problem| problem.contains("Summary")),
            "{found:?}"
        );
    }
}
