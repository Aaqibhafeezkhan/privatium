// This file is part of Privatium
// crates/privatium-core/src/app/manifest.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-02
// Last Modified: 2026-10-04
// Summary: app.toml (spec/app-contract.md §3) — the manifest as a type, its validation against §3.1,
//          protocol §1.1's reserved slugs and §12's api ceiling, and the [permissions] table
//          of §5.4 with the plain-language widenings it implies.
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

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Mode;

/// The manifest's file name. An app is a folder containing one (`spec/app-contract.md`).
pub const MANIFEST_FILE: &str = "app.toml";

/// The app contract this build implements (`spec/app-contract.md`'s `api = 1`).
///
/// `spec/protocol.md §12`: a node MUST refuse an app declaring a higher `api`. A build
/// that qualifies its `--version` as partial (`spec/cli.md §1`) is making a claim about
/// `§13` conformance, not about which contract apps are written against — `pv/1` speaks
/// `api = 1`, partial or not.
pub const SUPPORTED_API: u32 = 1;

/// `spec/protocol.md §1.1`, in the order written there. All ten are refused.
pub const RESERVED_SLUGS: [&str; 10] = [
    "_sys",
    "api",
    "a",
    "ws",
    "static",
    "health",
    "pair",
    "well-known",
    "settings",
    "skills",
];

/// `title` is at most this many characters (`§3`).
pub const MAX_TITLE_CHARS: usize = 40;

/// The longest slug DNS-SD can carry as a subtype label (`spec/protocol.md §6.1`).
pub const MAX_ADVERTISED_SLUG: usize = 15;

/// `[app] tier` (`spec/app-contract.md §1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Lua 5.4 and LSP templates, rendered on the node.
    Lua,
    /// The app's own `web/`, served as-is.
    Web,
    /// Linked at compile time; index entry only (`§8`).
    Rust,
}

impl Tier {
    /// The value as `app.toml` and `sys_app.tier` spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lua => "lua",
            Self::Web => "web",
            Self::Rust => "rust",
        }
    }

    /// The one file `§8`'s tier check requires, relative to the app folder.
    #[must_use]
    pub fn required_file(self) -> Option<&'static str> {
        match self {
            Self::Lua => Some("app.lua"),
            Self::Web => Some("web/index.html"),
            Self::Rust => None,
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `app.toml`, parsed.
///
/// Unknown keys and unknown tables are refused rather than ignored, as `config.rs` does for
/// `config.toml`: `§3` names every key, `api` is what gates a manifest written for a later
/// contract, and a mistyped `inline_scrpt = true` that silently granted nothing would be a
/// worse failure than one that names the key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[app]`.
    pub app: AppTable,
    /// `[nav]`.
    #[serde(default)]
    pub nav: Nav,
    /// `[permissions]` (`§5.4`).
    #[serde(default)]
    pub permissions: Permissions,
}

/// `[app]`. The five REQUIRED keys have no default; the rest are optional.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppTable {
    /// `^[a-z][a-z0-9-]{1,30}$`, not reserved, equal to the folder name.
    pub slug: String,
    /// ≤ 40 characters.
    pub title: String,
    /// Semver.
    pub version: String,
    /// The framework API targeted; ≤ [`SUPPORTED_API`].
    pub api: u32,
    /// `lua | web | rust`.
    pub tier: Tier,
    /// Free text.
    #[serde(default)]
    pub description: Option<String>,
    /// A Bootstrap Icons file name (`docs/icons.md`). Checked against the vendored set
    /// where the shell renders it; here it is only carried.
    #[serde(default)]
    pub icon: Option<String>,
    /// Authors.
    #[serde(default)]
    pub authors: Vec<String>,
    /// SPDX expression.
    #[serde(default)]
    pub license: Option<String>,
}

/// `[nav]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Nav {
    /// `sys_app.nav_order`. Absent is NULL, which sorts last.
    pub order: Option<i32>,
    /// Advertise a DNS-SD subtype (`spec/protocol.md §6.1`). Defaults to on, as the
    /// example manifest shows it.
    pub advertise: bool,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            order: None,
            advertise: true,
        }
    }
}

/// `[permissions]` (`spec/app-contract.md §5.4`). Every field defaults to the closed
/// position; the manifest only ever opens one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Permissions {
    /// `'unsafe-inline'` on `script-src`. Avoid; use an external file.
    pub inline_script: bool,
    /// `'wasm-unsafe-eval'` on `script-src`.
    pub wasm: bool,
    /// `'unsafe-eval'` on `script-src`; some older WASM loaders need it.
    pub eval: bool,
    /// Additional origins for `script-src`, `img-src` and `connect-src`: the online
    /// services the app uses, named to the owner as a privacy warning.
    pub remote: Vec<String>,
    /// Ad-hoc read-only SQL through `pv.sql()` (`spec/data-api.md §1`). Not a CSP matter.
    pub sql: bool,
    /// COOP/COEP for `SharedArrayBuffer` (`docs/frameworks.md §5.4`). Solo mode only:
    /// the headers are document-level on one origin and would break every other app.
    pub cross_origin_isolated: bool,
}

impl Permissions {
    /// The non-default permissions as JSON text — `sys_app.permissions`
    /// (`spec/data-dictionary.md §3.4`). `{}` when nothing was widened, so the column is
    /// always JSON and `permissions <> '{}'` finds every app that asked for something.
    #[must_use]
    pub fn non_default_json(&self) -> String {
        let mut out: BTreeMap<&str, serde_json::Value> = BTreeMap::new();
        if self.inline_script {
            out.insert("inline_script", true.into());
        }
        if self.wasm {
            out.insert("wasm", true.into());
        }
        if self.eval {
            out.insert("eval", true.into());
        }
        if !self.remote.is_empty() {
            out.insert("remote", self.remote.clone().into());
        }
        if self.sql {
            out.insert("sql", true.into());
        }
        if self.cross_origin_isolated {
            out.insert("cross_origin_isolated", true.into());
        }
        serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Every non-default permission, as `§5.4`'s "shown to the owner in plain language".
    #[must_use]
    pub fn widenings(&self) -> Vec<Widening> {
        let mut out = Vec::new();
        if self.inline_script {
            out.push(Widening::InlineScript);
        }
        if self.wasm {
            out.push(Widening::Wasm);
        }
        if self.eval {
            out.push(Widening::Eval);
        }
        if !self.remote.is_empty() {
            out.push(Widening::Remote(self.remote.clone()));
        }
        if self.sql {
            out.push(Widening::Sql);
        }
        if self.cross_origin_isolated {
            out.push(Widening::CrossOriginIsolated);
        }
        out
    }
}

/// One non-default permission, surfaced to the owner (`§5.4`: "every non-default
/// permission is shown to the owner at install time in plain language").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widening {
    /// `inline_script = true`.
    InlineScript,
    /// `wasm = true`.
    Wasm,
    /// `eval = true`.
    Eval,
    /// `remote = [...]`.
    Remote(Vec<String>),
    /// `sql = true`.
    Sql,
    /// `cross_origin_isolated = true`.
    CrossOriginIsolated,
}

impl fmt::Display for Widening {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InlineScript => f.write_str(
                "inline scripts run ('unsafe-inline'): any string injected into a page can execute",
            ),
            Self::Wasm => {
                f.write_str("WebAssembly may be compiled in the page ('wasm-unsafe-eval')")
            }
            Self::Eval => f.write_str(
                "eval() and the Function constructor run ('unsafe-eval'): any injected string \
                 gets a JavaScript engine",
            ),
            Self::Remote(origins) => write!(
                f,
                "this app uses the following online services: {}",
                origins.join(", ")
            ),
            Self::Sql => f.write_str("ad-hoc read-only SQL over the app's tables (pv.sql)"),
            Self::CrossOriginIsolated => {
                f.write_str("cross-origin isolation headers (COOP/COEP) — solo mode only")
            }
        }
    }
}

/// Why a manifest was refused. The `Display` text is `sys_app.last_error`.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The file is not the TOML `§3` describes — including a missing required key or a key
    /// `§3` does not name.
    #[error("app.toml: {0}")]
    Toml(Box<toml::de::Error>),

    /// `spec/protocol.md §1.1`.
    #[error("slug {slug:?} is reserved (spec/protocol.md §1.1)")]
    ReservedSlug {
        /// The slug.
        slug: String,
    },

    /// `^[a-z][a-z0-9-]{1,30}$`.
    #[error("slug {slug:?} does not match ^[a-z][a-z0-9-]{{1,30}}$ (spec/app-contract.md §3)")]
    MalformedSlug {
        /// The slug.
        slug: String,
    },

    /// The slug is the row key and the folder name (`spec/app-contract.md §3.1`,
    /// `spec/cli.md` PV104).
    #[error("slug {slug:?} does not match its folder {folder:?} (spec/app-contract.md §3.1)")]
    FolderMismatch {
        /// The slug.
        slug: String,
        /// The folder.
        folder: String,
    },

    /// `spec/protocol.md §12`.
    #[error(
        "api {api} exceeds api {supported}, which is what this framework implements \
         (spec/protocol.md §12)"
    )]
    ApiTooHigh {
        /// Declared.
        api: u32,
        /// Implemented.
        supported: u32,
    },

    /// `api = 0` names no contract.
    #[error("api must be a positive integer (spec/protocol.md §12)")]
    ApiZero,

    /// `title` is REQUIRED and ≤ 40 characters.
    #[error(
        "title must be 1 to {MAX_TITLE_CHARS} characters, found {found} \
         (spec/app-contract.md §3)"
    )]
    TitleLength {
        /// Characters found.
        found: usize,
    },

    /// `version` is REQUIRED semver.
    #[error("version {version:?} is not semver (spec/app-contract.md §3)")]
    Version {
        /// What was written.
        version: String,
    },

    /// A `remote` entry that is not an origin. A path, a wildcard, or a `;` here would be
    /// written into a CSP header verbatim, so the shape is enforced at load.
    #[error(
        "permissions.remote entry {entry:?} is not an origin — expected http(s)://host[:port] \
         (spec/app-contract.md §5.4)"
    )]
    RemoteNotOrigin {
        /// The entry.
        entry: String,
    },

    /// `docs/frameworks.md §5.4`: MUST fail to load in host mode.
    #[error(
        "permissions.cross_origin_isolated is allowed in solo mode only \
         (spec/app-contract.md §5.4, docs/frameworks.md §5.4)"
    )]
    CrossOriginIsolatedInHostMode,

    /// `§8`'s tier check.
    #[error("tier {tier} requires {file} (spec/app-contract.md §8)")]
    TierFileMissing {
        /// The tier.
        tier: Tier,
        /// The file, relative to the folder.
        file: &'static str,
    },
}

impl Manifest {
    /// Parse `app.toml`. Validation is separate, because it needs the folder name and the
    /// node's mode.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        toml::from_str(text).map_err(|error| ManifestError::Toml(Box::new(error)))
    }

    /// `spec/app-contract.md §3.1` and `§3`, `spec/protocol.md §1.1` and `§12`, and
    /// `docs/frameworks.md §5.4`, in that order — the first failure is the answer.
    ///
    /// The reserved list is checked before the regex so that `_sys` and `a`, which fail
    /// both, are refused for the reason `§1.1` gives.
    pub fn validate(&self, folder: &str, mode: Mode) -> Result<(), ManifestError> {
        let slug = &self.app.slug;
        if is_reserved(slug) {
            return Err(ManifestError::ReservedSlug { slug: slug.clone() });
        }
        if !is_valid_slug(slug) {
            return Err(ManifestError::MalformedSlug { slug: slug.clone() });
        }
        if slug != folder {
            return Err(ManifestError::FolderMismatch {
                slug: slug.clone(),
                folder: folder.to_owned(),
            });
        }
        if self.app.api == 0 {
            return Err(ManifestError::ApiZero);
        }
        if self.app.api > SUPPORTED_API {
            return Err(ManifestError::ApiTooHigh {
                api: self.app.api,
                supported: SUPPORTED_API,
            });
        }
        let title_chars = self.app.title.chars().count();
        if title_chars == 0 || title_chars > MAX_TITLE_CHARS {
            return Err(ManifestError::TitleLength { found: title_chars });
        }
        if !is_semver(&self.app.version) {
            return Err(ManifestError::Version {
                version: self.app.version.clone(),
            });
        }
        for entry in &self.permissions.remote {
            if !is_origin(entry) {
                return Err(ManifestError::RemoteNotOrigin {
                    entry: entry.clone(),
                });
            }
        }
        if self.permissions.cross_origin_isolated && mode == Mode::Host {
            return Err(ManifestError::CrossOriginIsolatedInHostMode);
        }
        Ok(())
    }
}

/// `spec/protocol.md §1.1`.
#[must_use]
pub fn is_reserved(slug: &str) -> bool {
    RESERVED_SLUGS.contains(&slug)
}

/// `^[a-z][a-z0-9-]{1,30}$` (`spec/protocol.md §1`), by hand — no regex crate in the
/// workspace.
#[must_use]
pub fn is_valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (2..=31).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `MAJOR.MINOR.PATCH[-pre][+build]`, the shape and no more: numeric core without leading
/// zeros, dot-separated non-empty alphanumeric identifiers after `-` and `+`.
#[must_use]
pub fn is_semver(version: &str) -> bool {
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let numeric = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
    };
    let identifiers = |s: &str| {
        !s.is_empty()
            && s.split('.').all(|id| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| numeric(p))
        && pre.is_none_or(identifiers)
        && build.is_none_or(identifiers)
}

/// `http(s)://host[:port]` and nothing after it — the only shape a `remote` entry may take.
#[must_use]
pub fn is_origin(entry: &str) -> bool {
    let Some(rest) = entry
        .strip_prefix("https://")
        .or_else(|| entry.strip_prefix("http://"))
    else {
        return false;
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (rest, None),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port.is_none_or(|p| {
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok()
    });
    host_ok && port_ok
}

/// SHA-256 of `app.toml`, lowercase hex — `sys_app.manifest_hash`
/// (`spec/data-dictionary.md §3.4`). The same digest `Schema::hash` is.
#[must_use]
pub fn manifest_hash(text: &str) -> String {
    crate::store::schema::hash_of(text)
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = "[app]\nslug = \"hello\"\ntitle = \"Hello\"\nversion = \"1.0.0\"\napi = 1\ntier = \"lua\"\n";

    #[test]
    fn the_example_manifest_parses_with_defaults() {
        let m = Manifest::parse(HELLO).unwrap();
        assert_eq!(m.app.tier, Tier::Lua);
        assert_eq!(m.nav, Nav::default());
        assert!(m.nav.advertise);
        assert_eq!(m.permissions, Permissions::default());
        m.validate("hello", Mode::Host).unwrap();
    }

    #[test]
    fn a_missing_required_key_names_itself() {
        let error = Manifest::parse("[app]\nslug = \"hello\"\n").unwrap_err();
        assert!(error.to_string().contains("title"), "{error}");
    }

    #[test]
    fn an_unknown_key_or_table_is_refused() {
        assert!(Manifest::parse(&format!("{HELLO}homepage = \"x\"\n")).is_err());
        assert!(Manifest::parse(&format!("{HELLO}[lua]\nmax_seconds = 1\n")).is_err());
        assert!(Manifest::parse(&format!("{HELLO}[permissions]\ninline_scrpt = true\n")).is_err());
    }

    #[test]
    fn every_reserved_slug_is_refused_as_reserved() {
        for slug in RESERVED_SLUGS {
            let m = Manifest::parse(&HELLO.replace("\"hello\"", &format!("{slug:?}"))).unwrap();
            let error = m.validate(slug, Mode::Host).unwrap_err();
            assert!(
                matches!(error, ManifestError::ReservedSlug { .. }),
                "{slug}: {error}"
            );
        }
    }

    #[test]
    fn the_slug_regex_by_hand() {
        for ok in ["ab", "hello", "med-tracker-2", "a1"] {
            assert!(is_valid_slug(ok), "{ok}");
        }
        for bad in [
            "",
            "a",
            "Hello",
            "1abc",
            "-abc",
            "a_b",
            "a.b",
            "a b",
            &"a".repeat(32),
        ] {
            assert!(!is_valid_slug(bad), "{bad:?}");
        }
        assert!(is_valid_slug(&"a".repeat(31)));
    }

    #[test]
    fn semver_shape() {
        for ok in [
            "1.0.0",
            "0.1.2",
            "10.20.30",
            "1.0.0-rc.1",
            "1.0.0+build.5",
            "1.0.0-a-b+c",
        ] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in [
            "1.0",
            "1",
            "01.0.0",
            "1.0.0-",
            "1.0.0+",
            "1.0.0-a..b",
            "v1.0.0",
            "",
        ] {
            assert!(!is_semver(bad), "{bad:?}");
        }
    }

    #[test]
    fn origins_are_scheme_host_port_and_nothing_else() {
        for ok in [
            "https://example.com",
            "http://192.168.1.5:8420",
            "https://a.b-c.d:443",
        ] {
            assert!(is_origin(ok), "{ok}");
        }
        for bad in [
            "example.com",
            "https://example.com/",
            "https://example.com/x",
            "*",
            "https://*.example.com",
            "https://example.com:99999",
            "https://x; script-src 'unsafe-inline'",
            "ftp://x",
            "https://",
        ] {
            assert!(!is_origin(bad), "{bad:?}");
        }
    }

    #[test]
    fn validation_order_and_the_api_ceiling() {
        let bad_api = HELLO.replace("api = 1", "api = 2");
        let error = Manifest::parse(&bad_api)
            .unwrap()
            .validate("hello", Mode::Host)
            .unwrap_err();
        assert!(
            matches!(
                error,
                ManifestError::ApiTooHigh {
                    api: 2,
                    supported: 1
                }
            ),
            "{error}"
        );
        let zero = HELLO.replace("api = 1", "api = 0");
        assert!(matches!(
            Manifest::parse(&zero)
                .unwrap()
                .validate("hello", Mode::Host)
                .unwrap_err(),
            ManifestError::ApiZero
        ));
        let m = Manifest::parse(HELLO).unwrap();
        assert!(matches!(
            m.validate("other", Mode::Host).unwrap_err(),
            ManifestError::FolderMismatch { .. }
        ));
        let long = HELLO.replace("\"Hello\"", &format!("{:?}", "x".repeat(41)));
        assert!(matches!(
            Manifest::parse(&long)
                .unwrap()
                .validate("hello", Mode::Host)
                .unwrap_err(),
            ManifestError::TitleLength { found: 41 }
        ));
        let coi = format!("{HELLO}[permissions]\ncross_origin_isolated = true\n");
        let m = Manifest::parse(&coi).unwrap();
        assert!(matches!(
            m.validate("hello", Mode::Host).unwrap_err(),
            ManifestError::CrossOriginIsolatedInHostMode
        ));
        m.validate("hello", Mode::Solo).unwrap();
    }

    #[test]
    fn non_default_permissions_are_json_and_widenings_are_named() {
        let p = Permissions::default();
        assert_eq!(p.non_default_json(), "{}");
        assert!(p.widenings().is_empty());

        let p = Permissions {
            remote: vec!["https://x".into()],
            sql: true,
            ..Permissions::default()
        };
        assert_eq!(
            p.non_default_json(),
            r#"{"remote":["https://x"],"sql":true}"#
        );
        let w = p.widenings();
        assert_eq!(w.len(), 2);
        assert!(
            w[0].to_string() == "this app uses the following online services: https://x",
            "{}",
            w[0]
        );
        assert_eq!(w[1], Widening::Sql);
    }

    #[test]
    fn the_manifest_hash_is_sha256_of_the_text() {
        assert_eq!(manifest_hash(""), crate::store::Schema::empty().hash);
        assert_eq!(manifest_hash("x").len(), 64);
        assert_ne!(manifest_hash("x"), manifest_hash("y"));
    }
}
