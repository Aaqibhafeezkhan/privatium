// This file is part of Privatium
// crates/privatium-core/src/http/assets.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: /static/* (spec/protocol.md §9.1): the shell's own assets, embedded from assets/shell/ —
//          stylesheet, htmx, pv.js, and browser session modules.
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

use base64::{Engine as _, engine::general_purpose::STANDARD};
use include_dir::{Dir, include_dir};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The brand logo, served from outside the shell directory.
const LOGO: &str = "privatium-logo-light.svg";
const LOGO_BYTES: &[u8] = include_bytes!("../../../../assets/branding/privatium-logo-light.svg");

/// The square brand mark the header shows on a narrow screen (`docs/branding.md`).
const MARK: &str = "privatium-mark-white.svg";
const MARK_BYTES: &[u8] = include_bytes!("../../../../assets/branding/privatium-mark-white.svg");

/// The branding files served beside the shell's own, by name.
const BRANDING: [(&str, &[u8]); 2] = [(LOGO, LOGO_BYTES), (MARK, MARK_BYTES)];

/// Every served asset's path and SHA-256, computed once per process: the shell directory's
/// files and the logo. Walked in path order, so the build ID below is stable.
static HASHES: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    fn walk(dir: &Dir<'static>, into: &mut BTreeMap<String, String>) {
        for file in dir.files() {
            into.insert(
                file.path().to_string_lossy().replace('\\', "/"),
                format!(
                    "sha256-{}",
                    STANDARD.encode(Sha256::digest(file.contents()))
                ),
            );
        }
        for child in dir.dirs() {
            walk(child, into);
        }
    }
    let mut hashes = BTreeMap::new();
    walk(&SHELL, &mut hashes);
    for (name, bytes) in BRANDING {
        hashes.insert(name.to_owned(), integrity_of(bytes));
    }
    hashes
});

/// The `integrity` value for `bytes`: `sha256-` and the digest in base64, as a `<script>`
/// or `<link>` spells it. The embedded assets and an app's declared `ui.scripts` and
/// `ui.styles` are hashed the same way.
#[must_use]
pub fn integrity_of(bytes: &[u8]) -> String {
    format!("sha256-{}", STANDARD.encode(Sha256::digest(bytes)))
}

/// Each asset's strong `ETag`: its integrity hash in quotes, as the header is spelled.
static ETAGS: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    HASHES
        .iter()
        .map(|(path, hash)| (path.clone(), format!("\"{hash}\"")))
        .collect()
});

/// SHA-256 metadata for an embedded asset (§8.3), as a `<script>` or `<link>` `integrity`
/// value. Empty for a name the shell does not ship.
#[must_use]
pub fn integrity(name: &str) -> &str {
    HASHES.get(name).map_or("", String::as_str)
}

/// The identifier of this build's asset set: the first sixteen hex digits of the SHA-256
/// over every served asset's path and hash. It changes whenever any asset changes, which is
/// what lets the framework's own pages name assets under `/static/<build>/` and let a
/// browser keep them for a day (`spec/protocol.md §9.3`): a new build is a new path, so
/// a cached copy can never be served against a bootstrap whose integrity hash it fails.
#[must_use]
pub fn build_id() -> &'static str {
    static ID: LazyLock<String> = LazyLock::new(|| {
        let mut hasher = Sha256::new();
        for (path, hash) in HASHES.iter() {
            hasher.update(path.as_bytes());
            hasher.update(b"\n");
            hasher.update(hash.as_bytes());
            hasher.update(b"\n");
        }
        let digest = hasher.finalize();
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    });
    ID.as_str()
}

/// The content-addressed URL of an embedded asset, for the pages the framework itself
/// renders. Apps keep the fixed `/static/<name>` paths the specification gives them.
#[must_use]
pub fn versioned(name: &str) -> String {
    format!("/static/{}/{name}", build_id())
}

/// Shell scripts, stylesheets and the Noble import closure. Provenance is not served.
static SHELL: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/shell");

/// One embedded asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// The bytes, for the life of the process.
    pub bytes: &'static [u8],
    /// The `Content-Type` to send it with.
    pub content_type: &'static str,
    /// The strong `ETag` to send it with: the integrity hash in quotes, so a browser can
    /// revalidate the fixed path with `If-None-Match` and be answered 304.
    pub etag: &'static str,
    /// Whether the request named this build's `/static/<build>/` prefix. Such a path is
    /// safe to cache for a day, because a different build is a different path.
    pub addressed: bool,
}

/// The asset at `/static/<rest>`, if the shell ships one, with or without this build's
/// `<build>/` prefix in front. Only the named brand files, stylesheets and scripts are
/// served; nested paths are confined to the vendored Noble module directory. A prefix
/// that is not this build's is not stripped, so it is refused like any other nesting.
#[must_use]
pub fn get(rest: &str) -> Option<Asset> {
    let (rest, addressed) = match rest.strip_prefix(build_id()) {
        Some(stripped) if stripped.starts_with('/') => (&stripped[1..], true),
        _ => (rest, false),
    };
    let etag = |name: &str| ETAGS.get(name).map(String::as_str);
    if let Some((name, bytes)) = BRANDING.iter().find(|(name, _)| *name == rest) {
        return Some(Asset {
            bytes,
            content_type: "image/svg+xml",
            etag: etag(name)?,
            addressed,
        });
    }
    if rest.contains('\\')
        || rest.split('/').any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
        || (rest.contains('/') && !rest.starts_with("vendor/noble/"))
    {
        return None;
    }
    let content_type = match rest.rsplit_once('.').map(|(_, ext)| ext)? {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        _ => return None,
    };
    let file = SHELL.get_file(rest)?;
    Some(Asset {
        bytes: file.contents(),
        content_type,
        etag: etag(rest)?,
        addressed,
    })
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_ships_its_stylesheets_scripts_and_brand_files_and_nothing_else() {
        for sheet in ["shell.css", "chrome.css"] {
            assert_eq!(
                get(sheet).unwrap().content_type,
                "text/css; charset=utf-8",
                "{sheet}"
            );
        }
        let htmx = get("htmx.min.js").unwrap();
        assert_eq!(htmx.content_type, "text/javascript; charset=utf-8");
        assert!(htmx.bytes.starts_with(b"var htmx="));
        let pv = get("pv.js").unwrap();
        assert_eq!(pv.content_type, "text/javascript; charset=utf-8");
        assert!(
            std::str::from_utf8(pv.bytes)
                .unwrap()
                .contains("export const pv")
        );
        let chrome = get("chrome.js").unwrap();
        assert_eq!(chrome.content_type, "text/javascript; charset=utf-8");
        assert!(
            std::str::from_utf8(chrome.bytes)
                .unwrap()
                .contains("export function install")
        );
        for brand in ["privatium-logo-light.svg", "privatium-mark-white.svg"] {
            assert_eq!(get(brand).unwrap().content_type, "image/svg+xml", "{brand}");
            assert!(!get(brand).unwrap().etag.is_empty(), "{brand}");
        }
        assert_eq!(integrity_of(b"").len(), "sha256-".len() + 44);
        assert!(get("../privatium-logo-light.svg").is_none());
        assert!(get("privatium-mark.svg").is_none());
        assert!(get("unknown.svg").is_none());
        assert!(get("VENDOR.md").is_none());
        assert!(get("../icons/LICENSE").is_none());
    }

    #[test]
    fn test_spec_9_3_assets_are_addressable_by_build_and_carry_an_etag() {
        let id = build_id();
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(versioned("client.js"), format!("/static/{id}/client.js"));

        let fixed = get("client.js").unwrap();
        let addressed = get(&format!("{id}/client.js")).unwrap();
        assert!(!fixed.addressed);
        assert!(addressed.addressed);
        assert_eq!(fixed.bytes, addressed.bytes);
        assert_eq!(fixed.etag, addressed.etag);
        assert_eq!(fixed.etag, format!("\"{}\"", integrity("client.js")));
        assert!(
            get(&format!("{id}/vendor/noble/hashes/sha2.js"))
                .unwrap()
                .addressed
        );
        assert!(
            get(&format!("{id}/privatium-logo-light.svg"))
                .unwrap()
                .addressed
        );
        assert!(!get("privatium-logo-light.svg").unwrap().etag.is_empty());

        // Another build's prefix, a partial prefix and a doubled prefix are all refused.
        assert!(get("0123456789abcdef/client.js").is_none());
        assert!(get(&format!("{}/client.js", &id[..15])).is_none());
        assert!(get(&format!("{id}client.js")).is_none());
        assert!(get(&format!("{id}/{id}/client.js")).is_none());
        assert!(get(&format!("{id}/../client.js")).is_none());
    }

    #[test]
    fn test_spec_8_browser_crypto_modules_are_served_without_path_traversal() {
        for path in [
            "session.js",
            "pair.js",
            "vendor/noble/curves/ed25519.js",
            "vendor/noble/hashes/sha2.js",
            "vendor/noble/ciphers/chacha.js",
            "vendor/noble/curves/abstract/edwards.js",
        ] {
            assert_eq!(
                get(path).unwrap().content_type,
                "text/javascript; charset=utf-8"
            );
        }
        for path in [
            "vendor/noble/../session.js",
            "vendor/noble//curves/ed25519.js",
            "vendor/noble/curves/./ed25519.js",
            "vendor/noble/curves\\ed25519.js",
            "vendor/noble/curves/LICENSE",
            "vendor/noble/VENDOR.md",
            "vendor/noble/curves/%2e%2e/ed25519.js",
            "other/pv.js",
            "/session.js",
        ] {
            assert!(get(path).is_none(), "{path}");
        }
    }
}
