// This file is part of Privatium
// crates/privatium-core/src/lint/manifest.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: The rules that read app.toml and the sample data: PV101–PV105 through the loader's own
//          Manifest type and validation, PV205 (a widened permission needs a comment beside
//          it), PV208 (nothing that looks like a secret in the manifest, the schema or
//          sample/seed.jsonl), PV110 (every [ui] reference resolves and reaches a framed
//          view), PV501 (the DNS-SD label limit) and PV502 (cross_origin_isolated is the
//          solo app's alone).
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

use crate::app::manifest::{
    MANIFEST_FILE, MAX_ADVERTISED_SLUG, Manifest, ManifestError, Navigation, SUPPORTED_API, Tier,
    is_reserved, is_valid_slug,
};
use crate::config::Mode;
use crate::lint::{Ctx, RuleId, line_of};

const SEED_FILE: &str = "sample/seed.jsonl";

/// `PV101`–`PV105`, `PV110`, `PV205`, `PV208`, `PV501`, `PV502`.
pub(crate) fn check(ctx: &mut Ctx<'_>) {
    let Some(text) = ctx.read(MANIFEST_FILE) else {
        ctx.push(
            RuleId::PV101,
            MANIFEST_FILE,
            0,
            "missing — app.toml is the one required file",
        )
        .fix = Some("write [app] with slug, title, version, api and tier".into());
        return;
    };
    check_secrets(ctx, MANIFEST_FILE, &text);
    let manifest = match Manifest::parse(&text) {
        Ok(manifest) => manifest,
        Err(ManifestError::Toml(error)) => {
            let line = error.span().map_or(0, |span| line_of(&text, span.start));
            let message = error.message().to_owned();
            ctx.push(RuleId::PV101, MANIFEST_FILE, line, message);
            return;
        }
        Err(other) => {
            ctx.push(RuleId::PV101, MANIFEST_FILE, 0, other.to_string());
            return;
        }
    };

    // PV102 and PV104 by hand, so each names its own rule; the loader's validate() would
    // stop at the first.
    let slug = manifest.app.slug.clone();
    let slug_line = line_of_key(&text, "slug");
    if is_reserved(&slug) {
        ctx.push(
            RuleId::PV102,
            MANIFEST_FILE,
            slug_line,
            format!("slug {slug:?} is reserved (spec/protocol.md §1.1)"),
        )
        .fix = Some("choose another slug".into());
    } else if !is_valid_slug(&slug) {
        ctx.push(
            RuleId::PV102,
            MANIFEST_FILE,
            slug_line,
            format!("slug {slug:?} does not match ^[a-z][a-z0-9-]{{1,30}}$"),
        )
        .fix = Some(
            "lowercase letters, digits and hyphens, 2 to 31 characters, starting with a letter"
                .into(),
        );
    }
    if manifest.app.api == 0 || manifest.app.api > SUPPORTED_API {
        ctx.push(
            RuleId::PV103,
            MANIFEST_FILE,
            line_of_key(&text, "api"),
            format!(
                "api = {} — this framework implements api = {SUPPORTED_API}",
                manifest.app.api
            ),
        )
        .fix = Some(format!("api = {SUPPORTED_API}"));
    }
    if slug != ctx.folder {
        let folder = ctx.folder.clone();
        ctx.push(
            RuleId::PV104,
            MANIFEST_FILE,
            slug_line,
            format!("slug {slug:?} but the folder is {folder:?}"),
        )
        .fix = Some(format!(
            "rename the folder to {slug} or the slug to {folder}"
        ));
    }
    // The rest of the loader's validation — title length, semver, remote origins — is
    // PV101's: the manifest does not carry what §3 requires. The `[ui]` table's own
    // shape is PV110's, below, so it is left out here.
    if let Err(error) = manifest.validate(&slug, Mode::Solo)
        && !matches!(
            error,
            ManifestError::ReservedSlug { .. }
                | ManifestError::MalformedSlug { .. }
                | ManifestError::FolderMismatch { .. }
                | ManifestError::ApiTooHigh { .. }
                | ManifestError::ApiZero
                | ManifestError::MenuLabel { .. }
                | ManifestError::MenuPath { .. }
                | ManifestError::UiAssetPath { .. }
                | ManifestError::UiAssetMissing { .. }
                | ManifestError::SwapNavigationOnWeb
        )
    {
        ctx.push(RuleId::PV101, MANIFEST_FILE, 0, error.to_string());
    }
    check_ui(ctx, &text, &manifest);
    if let Some(file) = manifest.app.tier.required_file()
        && !ctx.dir.join(file).is_file()
    {
        ctx.push(
            RuleId::PV105,
            MANIFEST_FILE,
            line_of_key(&text, "tier"),
            format!("tier = \"{}\" requires {file}", manifest.app.tier),
        )
        .fix = Some(format!("create {file}"));
    }
    if manifest.nav.advertise && slug.len() > MAX_ADVERTISED_SLUG {
        ctx.push(
            RuleId::PV501,
            MANIFEST_FILE,
            slug_line,
            format!("slug {slug:?} is {} characters; a DNS-SD subtype label holds {MAX_ADVERTISED_SLUG}", slug.len()),
        )
        .fix = Some("shorten the slug, or set [nav] advertise = false".into());
    }
    let solo =
        ctx.options.mode == Mode::Solo && ctx.options.solo_app.as_deref() == Some(slug.as_str());
    if manifest.permissions.cross_origin_isolated && !solo {
        ctx.push(
            RuleId::PV502,
            MANIFEST_FILE,
            line_of_key(&text, "cross_origin_isolated"),
            "permissions.cross_origin_isolated is honoured for the solo app alone; in host mode the COOP/COEP headers would break every other app and the loader refuses it (docs/frameworks.md §5.4)",
        )
        .fix = Some("run the node in solo mode with [node] app naming this app, or drop the permission and export single-threaded".into());
    }
    if let Some(icon) = &manifest.app.icon
        && !crate::icons::exists(icon)
    {
        ctx.push(
            RuleId::PV503,
            MANIFEST_FILE,
            line_of_key(&text, "icon"),
            format!("icon {icon:?} is not in the vendored Bootstrap Icons set"),
        )
        .fix = Some("name a file of assets/icons/ without .svg".into());
    }
    check_permission_comments(ctx, &text, &manifest);
    ctx.manifest = Some(manifest);
}

/// `PV110`: every `[ui]` reference resolves — the shape rules of `Ui::validate`, the
/// files of `Ui::check_files` — and `scripts` and `styles` reach a page that loads them,
/// which a Tier 2 app, owning its document, has none of (`spec/app-contract.md §3`).
fn check_ui(ctx: &mut Ctx<'_>, text: &str, manifest: &Manifest) {
    let ui_line = line_of_table(text, "[ui]").max(line_of_table(text, "[[ui.menu]]"));
    if let Err(error) = manifest.ui.validate() {
        ctx.push(RuleId::PV110, MANIFEST_FILE, ui_line, error.to_string())
            .fix = Some("give every menu item a label and a path beginning with /, and spell every asset static/<name>".into());
    } else if let Err(error) = manifest.ui.check_files(ctx.dir) {
        ctx.push(RuleId::PV110, MANIFEST_FILE, ui_line, error.to_string())
            .fix = Some("create the file under static/, or drop the entry".into());
    }
    if manifest.app.tier == Tier::Web
        && (!manifest.ui.scripts.is_empty() || !manifest.ui.styles.is_empty())
    {
        ctx.push(
            RuleId::PV110,
            MANIFEST_FILE,
            ui_line,
            "ui.scripts and ui.styles load in the page frame's head, and a Tier 2 app owns its document, so nothing here is ever loaded",
        )
        .fix = Some("link the stylesheet and script from web/index.html and drop the entries".into());
    }
    if manifest.app.tier == Tier::Web && manifest.ui.navigation == Navigation::Swap {
        ctx.push(
            RuleId::PV110,
            MANIFEST_FILE,
            line_of_key(text, "navigation"),
            ManifestError::SwapNavigationOnWeb.to_string(),
        )
        .fix = Some(
            "drop the navigation key; the node refuses to load the app while it is there".into(),
        );
    }
}

/// The 1-based line on which `table` — `[ui]`, `[[ui.menu]]` — opens, or 0.
fn line_of_table(text: &str, table: &str) -> u32 {
    text.lines()
        .position(|line| line.trim_start() == table)
        .map_or(0, |index| index as u32 + 1)
}

/// The 1-based line of the first `key =` at the start of a line, or 0.
fn line_of_key(text: &str, key: &str) -> u32 {
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key)
            && rest.trim_start().starts_with('=')
        {
            return index as u32 + 1;
        }
    }
    0
}

/// `PV205`: every non-default permission has a `#` comment on its own line or on the
/// line above it.
fn check_permission_comments(ctx: &mut Ctx<'_>, text: &str, manifest: &Manifest) {
    let p = &manifest.permissions;
    let widened: Vec<&str> = [
        ("inline_script", p.inline_script),
        ("wasm", p.wasm),
        ("eval", p.eval),
        ("remote", !p.remote.is_empty()),
        ("sql", p.sql),
        ("cross_origin_isolated", p.cross_origin_isolated),
    ]
    .into_iter()
    .filter_map(|(key, on)| on.then_some(key))
    .collect();
    if widened.is_empty() {
        return;
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut in_permissions = false;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_permissions = trimmed == "[permissions]";
            continue;
        }
        if !in_permissions {
            continue;
        }
        let Some(key) = widened.iter().find(|key| {
            trimmed
                .strip_prefix(*key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        }) else {
            continue;
        };
        let commented_beside = comment_outside_strings(trimmed);
        let commented_above = index > 0 && lines[index - 1].trim_start().starts_with('#');
        if !commented_beside && !commented_above {
            ctx.push(
                RuleId::PV205,
                MANIFEST_FILE,
                index as u32 + 1,
                format!("permissions.{key} is widened with no comment saying why; the owner is shown it at install"),
            )
            .fix = Some("add `# why` on the line, or a comment line above it".into());
        }
    }
}

/// Whether a `#` appears outside a TOML string on the line.
fn comment_outside_strings(line: &str) -> bool {
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (None, '#') => return true,
            (None, '"' | '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    false
}

/// `sample/seed.jsonl`: `PV208` over each line's `d`.
pub(crate) fn check_seed(ctx: &mut Ctx<'_>) {
    let Some(text) = ctx.read(SEED_FILE) else {
        return;
    };
    check_secrets(ctx, SEED_FILE, &text);
}

/// Key names that mean "this value is a credential".
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "access_token",
    "auth_token",
    "private_key",
    "privatekey",
    "client_secret",
    "bearer",
    "pairing_code",
    "session_key",
];

/// Prefixes of well-known credential formats.
const SECRET_PREFIXES: &[&str] = &[
    "-----BEGIN ",
    "AKIA",
    "sk_live_",
    "sk_test_",
    "ghp_",
    "gho_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AIza",
    "ya29.",
    "eyJhbGciOi",
];

/// `PV208`: a key named like a credential with a non-empty value, or a value in a
/// well-known credential format, anywhere in `text`. Scanned line by line, so the finding
/// names the line and never repeats the value.
pub(crate) fn check_secrets(ctx: &mut Ctx<'_>, rel: &str, text: &str) {
    for (index, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let keyed = SECRET_KEYS.iter().find(|key| {
            lower.find(*key).is_some_and(|at| {
                let after = lower[at + key.len()..]
                    .trim_start_matches(['"', '\'', ' '])
                    .trim_start();
                let value = after
                    .trim_start_matches([':', '='])
                    .trim_start()
                    .trim_start_matches(['"', '\'']);
                (after.starts_with(':') || after.starts_with('='))
                    && value
                        .chars()
                        .next()
                        .is_some_and(|c| !matches!(c, '"' | '\'' | ',' | '}' | '\n' | ' '))
            })
        });
        let formatted = SECRET_PREFIXES.iter().find(|prefix| line.contains(*prefix));
        let what = match (keyed, formatted) {
            (Some(key), _) => format!("`{key}` carries a value"),
            (None, Some(prefix)) => format!("a value in a credential format ({})", prefix.trim()),
            (None, None) => continue,
        };
        ctx.push(
            RuleId::PV208,
            rel,
            index as u32 + 1,
            format!("{what} — a secret in an app folder or its data ends up in plain-text logs, snapshots and backups"),
        )
        .fix = Some("keep credentials in the OS keyring or identity/, never under data/ or in an app folder".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_comments_are_located() {
        let text =
            "[app]\nslug = \"x\"\n[permissions]\n# why\nsql = true\nremote = [\"https://a.b\"]\n";
        assert_eq!(line_of_key(text, "slug"), 2);
        assert_eq!(line_of_key(text, "sql"), 5);
        assert_eq!(line_of_key(text, "nope"), 0);
        assert!(comment_outside_strings("sql = true # yes"));
        assert!(!comment_outside_strings("x = \"#no\""));
    }
}
