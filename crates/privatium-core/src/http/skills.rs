// This file is part of Privatium
// crates/privatium-core/src/http/skills.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-09-26
// Summary: /skills/<name>.md and /skills/bundle.zip (spec/cli.md §6, docs/skills.md §6): the app-skills/
//          tree of this build, embedded so an owner gets the contract matching the version
//          they are running. The bundle is a stored (uncompressed) zip written by hand — a
//          hundred kilobytes of Markdown does not justify a compression crate.
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

use std::sync::LazyLock;

use include_dir::{Dir, include_dir};

/// `app-skills/` at the repository root, as of this build: the skills shipped to app
/// authors, kept apart from `skills/`, which holds the skills for working on this
/// repository and ships nowhere.
static SKILLS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../app-skills");

/// The file each skill folder is served as.
const SKILL_FILE: &str = "SKILL.md";

/// Every skill folder name, sorted.
#[must_use]
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = SKILLS
        .dirs()
        .filter(|dir| dir.get_file(dir.path().join(SKILL_FILE)).is_some())
        .filter_map(|dir| dir.path().file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `app-skills/<name>/SKILL.md`, if `name` is a skill.
#[must_use]
pub fn skill(name: &str) -> Option<&'static str> {
    if name.contains(['/', '\\', '.']) {
        return None;
    }
    SKILLS
        .get_file(format!("{name}/{SKILL_FILE}"))
        .and_then(|file| file.contents_utf8())
}

/// Every file under `app-skills/` at its path relative to that folder, slash-separated, sorted —
/// what `privatium skill export` writes to disk (`spec/cli.md §6`) and what the bundle
/// below holds.
#[must_use]
pub fn files() -> Vec<(String, &'static [u8])> {
    let mut entries: Vec<(String, &[u8])> = Vec::new();
    collect(&SKILLS, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// `/skills/bundle.zip`: every file under `app-skills/` — `README.md`, each skill's `SKILL.md`
/// and its `reference/` — at its path relative to that folder, so extracting the archive
/// into a `skills/` folder reproduces the tree `privatium skill export` writes
/// (`spec/cli.md §6`).
///
/// Built once per process; the same bytes every time, since the entries carry a fixed
/// timestamp rather than the moment of the request.
#[must_use]
pub fn bundle() -> &'static [u8] {
    static BUNDLE: LazyLock<Vec<u8>> = LazyLock::new(|| zip::stored(&files()));
    BUNDLE.as_slice()
}

fn collect<'a>(dir: &'a Dir<'a>, into: &mut Vec<(String, &'a [u8])>) {
    for file in dir.files() {
        let name = file.path().to_string_lossy().replace('\\', "/");
        into.push((name, file.contents()));
    }
    for sub in dir.dirs() {
        collect(sub, into);
    }
}

/// A stored-only zip writer (PKWARE APPNOTE 6.3.x): local file headers, a central
/// directory, and the end-of-central-directory record. Method 0, no data descriptors, no
/// zip64, so the format is the 1989 one every extractor reads.
pub mod zip {
    /// One entry's fixed DOS date-time: 2026-01-01 00:00:00. Reproducible output matters
    /// more than a real mtime, and the files have none the binary could know.
    const DOS_TIME: u16 = 0;
    const DOS_DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 1;

    /// Write `entries` as `(name, bytes)`, in the order given.
    #[must_use]
    pub fn stored(entries: &[(String, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let name = name.as_bytes();
            let crc = crc32(data);
            let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
            let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
            let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);

            // Local file header.
            put32(&mut out, 0x0403_4b50);
            put16(&mut out, 20); // version needed: 2.0
            put16(&mut out, 0x0800); // flags: UTF-8 names
            put16(&mut out, 0); // method: stored
            put16(&mut out, DOS_TIME);
            put16(&mut out, DOS_DATE);
            put32(&mut out, crc);
            put32(&mut out, size);
            put32(&mut out, size);
            put16(&mut out, name_len);
            put16(&mut out, 0); // extra
            out.extend_from_slice(name);
            out.extend_from_slice(data);

            // Central directory entry.
            put32(&mut central, 0x0201_4b50);
            put16(&mut central, 20); // version made by
            put16(&mut central, 20); // version needed
            put16(&mut central, 0x0800);
            put16(&mut central, 0);
            put16(&mut central, DOS_TIME);
            put16(&mut central, DOS_DATE);
            put32(&mut central, crc);
            put32(&mut central, size);
            put32(&mut central, size);
            put16(&mut central, name_len);
            put16(&mut central, 0); // extra
            put16(&mut central, 0); // comment
            put16(&mut central, 0); // disk
            put16(&mut central, 0); // internal attributes
            put32(&mut central, 0); // external attributes
            put32(&mut central, offset);
            central.extend_from_slice(name);
        }

        let central_offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let central_size = u32::try_from(central.len()).unwrap_or(u32::MAX);
        let count = u16::try_from(entries.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&central);

        // End of central directory.
        put32(&mut out, 0x0605_4b50);
        put16(&mut out, 0); // this disk
        put16(&mut out, 0); // central directory disk
        put16(&mut out, count);
        put16(&mut out, count);
        put32(&mut out, central_size);
        put32(&mut out, central_offset);
        put16(&mut out, 0); // comment
        out
    }

    /// CRC-32 (IEEE 802.3, reflected, polynomial `0xEDB88320`), as zip requires.
    #[must_use]
    pub fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    fn put16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn put32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seven_skills_are_embedded() {
        let names = names();
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
        assert!(
            skill("privatium-overview")
                .unwrap()
                .contains("privatium lint")
        );
        assert!(skill("../README").is_none());
        assert!(skill("nope").is_none());
    }

    /// The check value from the CRC-32 specification.
    #[test]
    fn crc32_check_value() {
        assert_eq!(zip::crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(zip::crc32(b""), 0);
    }

    /// Walk the archive by hand: every local header is where the central directory says,
    /// every name is the path it was registered under, and the record count matches.
    #[test]
    fn the_bundle_is_a_well_formed_stored_zip_of_the_skills_tree() {
        let bytes = bundle();
        let eocd = bytes.len() - 22;
        assert_eq!(&bytes[eocd..eocd + 4], &0x0605_4b50u32.to_le_bytes());
        let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
        let central_size =
            u32::from_le_bytes(bytes[eocd + 12..eocd + 16].try_into().unwrap()) as usize;
        let central_offset =
            u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(central_offset + central_size, eocd);

        let mut names = Vec::new();
        let mut at = central_offset;
        for _ in 0..count {
            assert_eq!(&bytes[at..at + 4], &0x0201_4b50u32.to_le_bytes());
            let method = u16::from_le_bytes([bytes[at + 10], bytes[at + 11]]);
            assert_eq!(method, 0, "stored only");
            let crc = u32::from_le_bytes(bytes[at + 16..at + 20].try_into().unwrap());
            let size = u32::from_le_bytes(bytes[at + 24..at + 28].try_into().unwrap()) as usize;
            let name_len = u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]) as usize;
            let offset = u32::from_le_bytes(bytes[at + 42..at + 46].try_into().unwrap()) as usize;
            let name = std::str::from_utf8(&bytes[at + 46..at + 46 + name_len]).unwrap();
            // The local header it points at, and the data behind it.
            assert_eq!(&bytes[offset..offset + 4], &0x0403_4b50u32.to_le_bytes());
            let data_at = offset + 30 + name_len;
            assert_eq!(zip::crc32(&bytes[data_at..data_at + size]), crc);
            names.push(name.to_owned());
            at += 46 + name_len;
        }
        assert!(names.contains(&"README.md".to_owned()), "{names:?}");
        assert!(names.contains(&"privatium-overview/SKILL.md".to_owned()));
        assert!(names.contains(&"privatium-tier1-lua/reference/README.md".to_owned()));
        assert!(
            names
                .iter()
                .all(|n| !n.starts_with('/') && !n.contains(".."))
        );
        assert_eq!(names.len(), count);
        // Deterministic.
        assert_eq!(bundle(), bytes);
    }
}
