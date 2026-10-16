// This file is part of Privatium
// crates/privatium-core/src/zip.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-10-08
// Last Modified: 2026-10-16
// Summary: A stored-only zip writer (PKWARE APPNOTE 6.3.x) and a minimal zip reader for
//          the same format. Writer: local file headers, a central directory, and the
//          end-of-central-directory record — method 0, no data descriptors, no zip64.
//          Reader: walks local file headers, validates CRC-32, refuses path traversal.
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

// ── Reader ─────────────────────────────────────────────────────────────────────

/// Errors from [`entries`].
#[derive(Debug)]
pub enum ZipError {
    /// The byte slice does not contain a valid local file header signature at `offset`.
    BadSignature { offset: usize },
    /// A local file header field points past the end of the slice.
    Truncated,
    /// The stored CRC-32 does not match the data (`entry_name` names the entry).
    Crc { name: String },
    /// The compression method is not "stored" (0). `privatium install` only needs the
    /// app bundles this binary's own `stored()` produces, so deflate is not worth adding.
    Compressed { name: String, method: u16 },
    /// An entry name is not valid UTF-8.
    BadName,
    /// An entry name contains `..` or starts with `/` or `\`, which would write outside
    /// the target folder — refused before any file is written.
    PathTraversal { name: String },
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipError::BadSignature { offset } => {
                write!(f, "zip: bad signature at byte {offset}")
            }
            ZipError::Truncated => write!(f, "zip: file is truncated"),
            ZipError::Crc { name } => write!(f, "zip: CRC mismatch in {name:?}"),
            ZipError::Compressed { name, method } => write!(
                f,
                "zip: {name:?} uses compression method {method}; only stored (0) is supported"
            ),
            ZipError::BadName => write!(f, "zip: entry name is not valid UTF-8"),
            ZipError::PathTraversal { name } => {
                write!(f, "zip: {name:?} would write outside the target folder")
            }
        }
    }
}

impl std::error::Error for ZipError {}

/// Walk every local file header in `data` and return `(name, file_data)` pairs in order.
///
/// Only method 0 (stored) is accepted. CRC-32 is verified for every entry. Entry names
/// that contain `..` or start with `/` or `\` are refused so the caller can write
/// directly into a target folder without a second path-traversal check.
///
/// Directory entries (names ending with `/`) are silently skipped; callers need files.
pub fn entries(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, ZipError> {
    let mut pos = 0usize;
    let mut out = Vec::new();

    while pos + 4 <= data.len() {
        // Central directory and end-of-central-directory start with different signatures;
        // stop when we leave the local-file-header section.
        let sig = u32::from_le_bytes(
            data[pos..pos + 4]
                .try_into()
                .map_err(|_| ZipError::Truncated)?,
        );
        if sig == 0x0201_4b50 || sig == 0x0605_4b50 {
            // Central directory or EOCD record — we have seen every local header.
            break;
        }
        if sig != 0x0403_4b50 {
            return Err(ZipError::BadSignature { offset: pos });
        }

        // Local file header: 30 bytes fixed, then name and extra field.
        if pos + 30 > data.len() {
            return Err(ZipError::Truncated);
        }
        let method = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
        let stored_crc = u32::from_le_bytes([
            data[pos + 14],
            data[pos + 15],
            data[pos + 16],
            data[pos + 17],
        ]);
        let comp_size = u32::from_le_bytes([
            data[pos + 18],
            data[pos + 19],
            data[pos + 20],
            data[pos + 21],
        ]) as usize;
        let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;

        let name_start = pos + 30;
        let data_start = name_start + name_len + extra_len;
        let data_end = data_start + comp_size;

        if data_end > data.len() {
            return Err(ZipError::Truncated);
        }

        let name = std::str::from_utf8(&data[name_start..name_start + name_len])
            .map_err(|_| ZipError::BadName)?
            .to_owned();

        // Skip directory entries — they have no payload we need.
        if name.ends_with('/') {
            pos = data_end;
            continue;
        }

        // Refuse names that would write outside the target folder (path traversal).
        if name.starts_with('/') || name.starts_with('\\') {
            return Err(ZipError::PathTraversal { name });
        }
        for component in name.split('/').chain(name.split('\\')) {
            if component == ".." {
                return Err(ZipError::PathTraversal { name });
            }
        }

        // Only method 0 (stored) is supported. Deflate needs an inflate implementation
        // and none of the app bundles this binary produces need it.
        if method != 0 {
            return Err(ZipError::Compressed { name, method });
        }

        let file_data = data[data_start..data_end].to_vec();

        // Verify the stored CRC-32, so a truncated download is caught before writing.
        let actual_crc = crc32(&file_data);
        if actual_crc != stored_crc {
            return Err(ZipError::Crc { name });
        }

        out.push((name, file_data));
        pos = data_end;
    }

    Ok(out)
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip: write with `stored`, read back with `entries`.
    #[test]
    fn round_trip() {
        let hello = b"hello world".as_slice();
        let empty = b"".as_slice();
        let zip = stored(&[("a/b.txt".to_owned(), hello), ("a/c.txt".to_owned(), empty)]);
        let got = entries(&zip).expect("parse");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "a/b.txt");
        assert_eq!(got[0].1, hello);
        assert_eq!(got[1].0, "a/c.txt");
        assert_eq!(got[1].1, empty);
    }

    /// A bad signature at offset 0 is rejected.
    #[test]
    fn bad_signature() {
        let mut zip = stored(&[("x.txt".to_owned(), b"x".as_slice())]);
        zip[0] = 0xFF;
        assert!(matches!(
            entries(&zip),
            Err(ZipError::BadSignature { offset: 0 })
        ));
    }

    /// CRC mismatch in payload is caught.
    #[test]
    fn crc_mismatch() {
        let mut zip = stored(&[("x.txt".to_owned(), b"x".as_slice())]);
        // Flip a byte in the payload (past the 30-byte header + 5-char name).
        let payload_pos = 30 + 5;
        zip[payload_pos] ^= 0xFF;
        assert!(matches!(entries(&zip), Err(ZipError::Crc { .. })));
    }

    /// Path traversal with `..` is refused.
    #[test]
    fn path_traversal_dotdot() {
        let zip = stored(&[("../evil.txt".to_owned(), b"bad".as_slice())]);
        assert!(matches!(entries(&zip), Err(ZipError::PathTraversal { .. })));
    }

    /// Absolute path is refused.
    #[test]
    fn path_traversal_absolute() {
        let zip = stored(&[("/etc/passwd".to_owned(), b"bad".as_slice())]);
        assert!(matches!(entries(&zip), Err(ZipError::PathTraversal { .. })));
    }
}
