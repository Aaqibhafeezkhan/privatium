// This file is part of Privatium
// crates/privatium-core/src/http/shell.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: The framework's own pages — launcher, settings, errors — as server-rendered HTML with HTMX
//          and inlined Bootstrap Icons (docs/architecture.md §2.5, docs/icons.md). No client
//          framework, no bundler, no inline script or style: every page renders under the
//          default CSP of spec/protocol.md §9.3 exactly as written, and every page is held to
//          the PV4xx rules of spec/cli.md §5 by tests/reference.rs.
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

use std::fmt::Write as _;

use axum::http::StatusCode;

use crate::app::{LoadReport, Warning};
use crate::config::Mode;
use crate::http::csrf::Csrf;
use crate::icons::{escape, icon};
use crate::lua::SourceContext;
use crate::wire::router::{SettingsPage, url};
use crate::{Node, Result, StoreError, sys};

/// What a page needs to render.
pub struct Context<'a> {
    /// The node, under the handler's lock.
    pub node: &'a Node,
    /// What `load_apps` reported at startup.
    pub report: &'a LoadReport,
    /// The token issuer for the page's forms.
    pub csrf: &'a Csrf,
    /// Whether the request has the node's own standing (`spec/protocol.md §8.4`, `§9.2`)
    /// — a loopback request or an in-process call — rather than a paired session. The
    /// forms that open pairing, name the node, label or revoke a device render for the
    /// owner alone; a session sees the pages without them.
    pub owner: bool,
}

/// A one-line message shown at the top of a settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// `ok`, `warn`, or `alert`, which is also the icon: status never by colour alone.
    pub kind: NoticeKind,
    /// The text.
    pub text: String,
}

/// The three tones a notice takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Something worked.
    Ok,
    /// Something needs the owner's attention.
    Warn,
    /// `§3.10`'s `alert`: MUST surface in the UI.
    Alert,
}

impl NoticeKind {
    fn class(self) -> &'static str {
        match self {
            Self::Ok => "pv-notice-info",
            Self::Warn => "pv-notice-warn",
            Self::Alert => "pv-notice-alert",
        }
    }

    fn icon(self) -> String {
        match self {
            Self::Ok => icon("check-lg"),
            Self::Warn => icon("exclamation-triangle"),
            Self::Alert => icon("shield-exclamation"),
        }
    }
}

/// Which header link is the current page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Active {
    Launcher,
    Settings,
    None,
}

/// The shell's own page frame. `solo` drops the launcher link: there is no launcher to
/// link to.
fn layout(title: &str, active: Active, solo: bool, body: &str) -> String {
    page(title, active, solo, true, "", body, None)
}

/// The frame a Tier 1 view renders inside when it calls no `layout()`
/// (`spec/lua-api.md §4.1`): the same head and header as the shell's, titled by the app,
/// with the brand demoted to a paragraph so the view keeps the page's one `<h1>`, and
/// `hx-headers` on the body so every htmx request beneath the mount carries the CSRF
/// token — which is what lets an `hx-delete` button, with no form, pass the host's check.
#[must_use]
pub fn app_frame(
    title: &str,
    solo: bool,
    csrf_token: &str,
    body: &str,
    node_label: &str,
) -> String {
    let attrs = format!(
        " hx-headers='{{\"X-CSRF-Token\":\"{}\"}}'",
        escape(csrf_token)
    );
    page(
        title,
        Active::None,
        solo,
        false,
        &attrs,
        body,
        Some(node_label),
    )
}

/// The document around a body: head, header, main, footer. `brand_heading` makes the
/// brand the page's `<h1>` (the shell's pages) rather than a paragraph (an app's).
fn page(
    title: &str,
    active: Active,
    solo: bool,
    brand_heading: bool,
    body_attrs: &str,
    body: &str,
    node_label: Option<&str>,
) -> String {
    let mut out = String::with_capacity(body.len() + 2048);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    // htmx never evaluates anything: the shell has no `hx-on` and no `js:` values, and the
    // config says so, which keeps the default CSP's `script-src 'self'` honest (AGENTS.md).
    // It also injects a `<style>` for its request indicators unless told not to, which the
    // default CSP (no `style-src`, so `default-src 'self'`) refuses with a console error on
    // every page; the shell's stylesheet is the only style there is.
    out.push_str(
        "<meta name=\"htmx-config\" content='{\"allowEval\":false,\"allowScriptTags\":false,\
         \"selfRequestsOnly\":true,\"includeIndicatorStyles\":false}'>\n",
    );
    let _ = writeln!(out, "<title>{} — Privatium</title>", escape(title));
    let _ = writeln!(
        out,
        "<link rel=\"stylesheet\" href=\"/static/shell.css\" integrity=\"{}\">",
        crate::http::assets::integrity("shell.css")
    );
    let _ = writeln!(
        out,
        "<script src=\"/static/htmx.min.js\" integrity=\"{}\" defer></script>",
        crate::http::assets::integrity("htmx.min.js")
    );
    let _ = writeln!(
        out,
        "</head>\n<body{body_attrs}>\n<a class=\"pv-skip\" href=\"#main\">Skip to content</a>"
    );
    let (brand_open, brand_close) = if brand_heading {
        ("<h1>", "</h1>")
    } else {
        ("<p class=\"pv-brand\">", "</p>")
    };
    let _ = write!(
        out,
        "<header class=\"pv-header\">\n{brand_open}<a href=\"/\"><img class=\"pv-brand-logo\" src=\"/static/privatium-logo-light.svg\" alt=\"\" width=\"160\" height=\"34\"><span class=\"pv-visually-hidden\">Privatium</span></a>{brand_close}\n\
         <nav aria-label=\"Framework\">\n"
    );
    if !solo {
        let _ = writeln!(
            out,
            "<a href=\"/\"{}>{} Apps</a>",
            current(active == Active::Launcher),
            icon("grid-3x3-gap")
        );
    }
    let _ = write!(
        out,
        "<details class=\"pv-menu\"><summary aria-label=\"Menu\" title=\"Menu\">{}</summary><nav aria-label=\"All pages\">",
        icon("list")
    );
    if !solo {
        out.push_str("<a href=\"/\">Apps</a>");
    }
    out.push_str("<a href=\"/settings\">Space settings</a><a href=\"/settings/apps\">App settings</a><a href=\"/settings/data\">Data settings</a><a href=\"/settings/devices\">Devices</a></nav></details>");
    out.push_str("</nav>\n</header>\n<main id=\"main\">\n");
    out.push_str(body);
    let _ = write!(
        out,
        "\n</main>\n<footer class=\"pv-footer\"><a href=\"https://github.com/gabrielmongefranco/privatium\">Privatium</a>\n<div class=\"pv-footer-node\"><span>{}</span><a class=\"pv-join\" href=\"/settings/devices\" aria-label=\"Connect a device — opens Devices\" title=\"Connect a device\">{}</a></div></footer>\n</body>\n</html>\n",
        escape(node_label.unwrap_or("")),
        icon("qr-code")
    );
    out
}

fn node_layout(
    cx: &Context<'_>,
    title: &str,
    active: Active,
    solo: bool,
    body: &str,
) -> Result<String> {
    let label = crate::http::api::display_name(cx.node)?
        .unwrap_or_else(|| cx.node.id().as_str().to_owned());
    Ok(page(title, active, solo, true, "", body, Some(&label)))
}

fn current(active: bool) -> &'static str {
    if active { " aria-current=\"page\"" } else { "" }
}

/// `/` in host mode: every enabled app in the index, mounted ones as links and the rest as
/// unavailable with the reason — an app whose folder is missing is shown, not hidden
/// (`spec/data-dictionary.md §3.4`).
pub fn launcher(cx: &Context<'_>) -> Result<String> {
    let mut body = String::from("<h2>Apps</h2>\n");
    let rows = query(
        cx.node,
        "SELECT id, title, icon, last_error FROM v_app_nav",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    if rows.is_empty() {
        body.push_str(
            "<p class=\"pv-muted\">No apps yet. Copy an app folder into <code>apps/</code> under \
             the data directory (see <a href=\"/settings/data\">Data and backup</a>) and restart.</p>",
        );
        return node_layout(cx, "Apps", Active::Launcher, false, &body);
    }
    body.push_str("<ul class=\"pv-launcher\">\n");
    for (slug, title, glyph, last_error) in rows {
        let title = title.unwrap_or_else(|| slug.clone());
        let glyph = glyph.as_deref().unwrap_or(crate::icons::FALLBACK);
        match cx.node.app(&slug).and_then(|app| app.mount()) {
            Some(mount) => {
                let description = cx
                    .node
                    .app(&slug)
                    .and_then(|app| app.manifest().app.description.as_deref())
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or(&slug);
                let _ = writeln!(
                    body,
                    "<li><a href=\"{}\">{}<span>{}<small>{}</small></span></a></li>",
                    escape(&url(mount, "")),
                    icon(glyph),
                    escape(&title),
                    escape(description)
                );
            }
            None => {
                let reason = last_error.unwrap_or_else(|| "not loaded".to_owned());
                let _ = writeln!(
                    body,
                    "<li><div class=\"pv-unavailable\">{}<span>{}<small>{} — unavailable: {}</small>\
                     </span></div></li>",
                    icon(glyph),
                    escape(&title),
                    escape(&slug),
                    escape(&reason)
                );
            }
        }
    }
    body.push_str("</ul>\n");
    node_layout(cx, "Apps", Active::Launcher, false, &body)
}

/// One of the four settings pages.
pub fn settings(cx: &Context<'_>, page: SettingsPage, notice: Option<&Notice>) -> Result<String> {
    let mut body = String::new();
    match page {
        SettingsPage::Node => node_page(cx, &mut body)?,
        SettingsPage::Apps => apps_page(cx, &mut body)?,
        SettingsPage::Data => data_page(cx, &mut body),
        SettingsPage::Devices => crate::http::devices::page(cx, &mut body)?,
    }
    settings_frame(cx, page, page.title(), notice, &body)
}

/// The settings document around `content`: the heading, the settings navigation with
/// `active` marked, an optional notice, then the content. The four pages and the code
/// page of `spec/protocol.md §7.2` (`/settings/devices/pairing`) share it.
pub fn settings_frame(
    cx: &Context<'_>,
    active: SettingsPage,
    title: &str,
    notice: Option<&Notice>,
    content: &str,
) -> Result<String> {
    let solo = cx.node.config().node.mode == Mode::Solo;
    let mut body = String::from(
        "<div class=\"pv-settings\"><h2>Settings</h2>\n<nav aria-label=\"Settings\">\n<ul class=\"pv-subnav\">\n",
    );
    for item in SettingsPage::ALL {
        let _ = writeln!(
            body,
            "<li><a href=\"{}\"{}>{}</a></li>",
            item.path(),
            current(item == active),
            item.title()
        );
    }
    body.push_str("</ul>\n</nav>\n");
    if let Some(notice) = notice {
        let _ = writeln!(
            body,
            "<div class=\"pv-notice {}\" role=\"status\">{}<div>{}</div></div>",
            notice.kind.class(),
            notice.kind.icon(),
            escape(&notice.text)
        );
    }
    body.push_str(content);
    body.push_str("</div>\n");
    node_layout(cx, title, Active::Settings, solo, &body)
}

fn node_page(cx: &Context<'_>, body: &mut String) -> Result<()> {
    let node = cx.node;
    let config = &node.config().node;
    let row = query(
        node,
        &format!(
            "SELECT display_name, pubkey, created_at, protocol, build FROM {}",
            sys::NODE
        ),
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    )?;
    let (display_name, pubkey, created_at, protocol, build) =
        row.into_iter().next().unwrap_or_default();

    body.push_str("<div class=\"pv-card\"><h3>");
    body.push_str(&icon("info-circle"));
    body.push_str(" This space</h3>\n<dl>\n");
    dl(body, "Space ID", &code(node.id().as_str()));
    dl(
        body,
        "Display name",
        &crate::http::devices::display_name_cell(cx, display_name.as_deref()),
    );
    dl(body, "Public key", &code(pubkey.as_deref().unwrap_or("")));
    dl(
        body,
        "Protocol",
        &code(protocol.as_deref().unwrap_or(crate::PROTOCOL)),
    );
    dl(body, "Build", &escape(build.as_deref().unwrap_or("")));
    dl(
        body,
        "Created",
        &escape(created_at.as_deref().unwrap_or("")),
    );
    dl(
        body,
        "Mode",
        &match (config.mode, config.app.as_deref()) {
            (Mode::Solo, Some(app)) => {
                format!("solo — <code>{}</code> at <code>/</code>", escape(app))
            }
            (Mode::Solo, None) => "solo".to_owned(),
            (Mode::Host, _) => "host".to_owned(),
        },
    );
    crate::http::devices::listening_rows(cx, body);
    body.push_str("</dl></div>\n");
    crate::http::devices::node_section(cx, body);

    // §3.10: alerts MUST surface in the UI, not only in the log.
    let alerts = query(
        node,
        "SELECT \"at\", kind, subject, detail FROM v_audit_recent WHERE severity = 'alert'",
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    body.push_str("<h3>Alerts</h3>\n");
    if alerts.is_empty() {
        body.push_str("<p class=\"pv-muted\">No alerts in the last 200 audit rows.</p>\n");
    } else {
        body.push_str(
            "<table class=\"pv-records\" role=\"table\" aria-label=\"Recent alerts\"><thead role=\"rowgroup\"><tr role=\"row\"><th scope=\"col\">When</th><th scope=\"col\">Kind</th>\
             <th scope=\"col\">Subject</th><th scope=\"col\">Detail</th></tr></thead><tbody role=\"rowgroup\">\n",
        );
        for (at, kind, subject, detail) in alerts {
            let _ = writeln!(
                body,
                "<tr role=\"row\"><td role=\"cell\"><span class=\"pv-cell-label\" aria-hidden=\"true\">When</span>{}</td>\
                 <td role=\"cell\"><span class=\"pv-cell-label\" aria-hidden=\"true\">Kind</span><span class=\"pv-badge pv-badge-alert\">{} {}</span></td>\
                 <td role=\"cell\"><span class=\"pv-cell-label\" aria-hidden=\"true\">Subject</span>{}</td>\
                 <td role=\"cell\"><span class=\"pv-cell-label\" aria-hidden=\"true\">Detail</span><code>{}</code></td></tr>",
                escape(at.as_deref().unwrap_or("")),
                icon("shield-exclamation"),
                escape(match kind.as_deref() {
                    Some("node") => "space",
                    other => other.unwrap_or(""),
                }),
                escape(subject.as_deref().unwrap_or("")),
                escape(detail.as_deref().unwrap_or(""))
            );
        }
        body.push_str("</tbody></table>\n");
    }
    Ok(())
}

/// One `sys_app` row as the apps page shows it.
struct AppIndexRow {
    slug: String,
    title: Option<String>,
    version: Option<String>,
    tier: Option<String>,
    source: Option<String>,
    enabled: bool,
    icon: Option<String>,
    installed_at: Option<String>,
    last_error: Option<String>,
}

fn apps_page(cx: &Context<'_>, body: &mut String) -> Result<()> {
    let node = cx.node;
    let rows = query(
        node,
        &format!(
            "SELECT id, title, version, tier, source, enabled, icon, installed_at, last_error \
             FROM {} ORDER BY nav_order NULLS LAST, title, id",
            sys::APP
        ),
        |row| {
            Ok(AppIndexRow {
                slug: row.get(0)?,
                title: row.get(1)?,
                version: row.get(2)?,
                tier: row.get(3)?,
                source: row.get(4)?,
                enabled: row.get::<_, Option<bool>>(5)?.unwrap_or(true),
                icon: row.get(6)?,
                installed_at: row.get(7)?,
                last_error: row.get(8)?,
            })
        },
    )?;

    if rows.is_empty() {
        body.push_str("<p class=\"pv-muted\">No app folders were found.</p>\n");
    }
    for row in rows {
        let loaded = node.app(&row.slug);
        let glyph = row.icon.as_deref().unwrap_or(crate::icons::FALLBACK);
        let _ = writeln!(
            body,
            "<section class=\"pv-card\" id=\"app-{slug}\" aria-labelledby=\"app-{slug}-title\">\n\
             <h3 id=\"app-{slug}-title\">{} {} <span class=\"pv-muted\">{slug}</span> {}</h3>\n<dl>",
            icon(glyph),
            escape(row.title.as_deref().unwrap_or(&row.slug)),
            status_badge(&row, loaded.is_some()),
            slug = escape(&row.slug),
        );
        dl(
            body,
            "Version",
            &escape(row.version.as_deref().unwrap_or("")),
        );
        dl(body, "App type", &escape(&app_type(row.tier.as_deref())));
        dl(body, "Source", &escape(row.source.as_deref().unwrap_or("")));
        dl(
            body,
            "Installed",
            &escape(
                row.installed_at
                    .as_deref()
                    .unwrap_or("never loaded cleanly"),
            ),
        );
        if let Some(error) = &row.last_error {
            dl(
                body,
                "Last error",
                &format!(
                    "<span class=\"pv-badge pv-badge-warn\">{} {}</span>",
                    icon("exclamation-triangle"),
                    escape(error)
                ),
            );
        }
        if let Some(app) = loaded {
            dl(
                body,
                "Mounted at",
                &app.mount().map_or_else(
                    || "<span class=\"pv-muted\">not mounted in this mode</span>".to_owned(),
                    |mount| {
                        format!(
                            "<a href=\"{0}\"><code>{0}</code></a>",
                            escape(&url(mount, ""))
                        )
                    },
                ),
            );
            let mut tables = String::new();
            let tier2 = app.manifest().app.tier == crate::app::Tier::Web;
            for table in &app.store().schema().tables {
                let count = table_count(app, &table.name);
                let _ = write!(
                    tables,
                    "<code>{}</code>: {} row{} ",
                    escape(&table.name),
                    count,
                    if count == 1 { "" } else { "s" }
                );
            }
            if tables.is_empty() {
                tables.push_str(if tier2 {
                    "<span class=\"pv-muted\">no schema.sql — the event log is the store</span>"
                } else {
                    "<span class=\"pv-muted\">no tables declared</span>"
                });
            }
            dl(body, "Tables", &tables);
            dl(body, "Events (this space)", &app.log().seq().to_string());
        }
        body.push_str("</dl>\n");

        // Permission widenings read as a privacy warning (`spec/app-contract.md §5.4`);
        // warnings about the app's shape keep their own box, so the two are not confused.
        for label in ["Privacy warning", "Load warning"] {
            let warnings: Vec<&Warning> = cx
                .report
                .warnings
                .iter()
                .filter(|w| w.slug() == row.slug && w.label() == label)
                .collect();
            if warnings.is_empty() {
                continue;
            }
            body.push_str("<div class=\"pv-notice pv-notice-warn\">");
            body.push_str(&icon("exclamation-triangle"));
            let _ = write!(
                body,
                "<div>{label}{}<ul>",
                if warnings.len() == 1 { "" } else { "s" }
            );
            for warning in warnings {
                let _ = write!(body, "<li>{}</li>", escape(&warning.detail()));
            }
            body.push_str("</ul></div></div>\n");
        }

        // The seed offer (spec/app-contract.md §9): shown only for a loaded app whose log is
        // empty and whose folder ships a seed; loading is a POST, never a GET, never automatic.
        if let Some(app) = loaded
            && app.seed_path().is_some()
            && app.log().seq() == 0
            && app.log().heads().is_empty()
        {
            let action = format!("/settings/apps/{}/seed", row.slug);
            let _ = writeln!(
                body,
                "<form class=\"pv-inline\" method=\"post\" action=\"{action}\" hx-post=\"{action}\" \
                 hx-target=\"body\" hx-push-url=\"true\">{}<button type=\"submit\" class=\"pv-btn pv-btn-primary\">\
                 {} Load sample data</button> <span class=\"pv-muted\">synthetic events from \
                 <code>sample/seed.jsonl</code>, appended as this space's; only offered while the \
                 app has no events</span></form>",
                cx.csrf.field(&action),
                icon("plus-lg"),
                action = escape(&action)
            );
        }
        body.push_str("</section>\n");
    }

    if !cx.report.failed.is_empty() || !cx.report.missing.is_empty() {
        body.push_str("<h3>Not loaded at startup</h3>\n<ul>\n");
        for failure in &cx.report.failed {
            let _ = writeln!(
                body,
                "<li>{} <code>{}</code> ({}, at <em>{}</em>): {}</li>",
                icon("exclamation-triangle"),
                escape(&failure.folder),
                failure.source,
                failure.stage,
                escape(&failure.reason)
            );
        }
        for slug in &cx.report.missing {
            let _ = writeln!(
                body,
                "<li>{} <code>{}</code>: folder missing — the index row and the data are kept</li>",
                icon("exclamation-triangle"),
                escape(slug)
            );
        }
        body.push_str("</ul>\n");
    }
    Ok(())
}

fn status_badge(row: &AppIndexRow, loaded: bool) -> String {
    if !row.enabled {
        format!(
            "<span class=\"pv-badge pv-badge-muted\">{} disabled</span>",
            icon("x-lg")
        )
    } else if loaded {
        format!(
            "<span class=\"pv-badge pv-badge-ok\">{} loaded</span>",
            icon("check-lg")
        )
    } else {
        format!(
            "<span class=\"pv-badge pv-badge-warn\">{} unavailable</span>",
            icon("exclamation-triangle")
        )
    }
}

/// How the index's `tier` reads on the page: the value `app.toml` carries
/// (`spec/app-contract.md §2`) and, beside it, what that kind of app is in words. A tier
/// this build does not know is shown as it was stored rather than guessed at.
fn app_type(tier: Option<&str>) -> String {
    let words = match tier {
        Some("lua") => "lua app",
        Some("web") => "web app",
        Some("rust") => "binary app",
        _ => return tier.unwrap_or("").to_owned(),
    };
    format!("{} ({words})", tier.unwrap_or(""))
}

/// Rows in one of an app's tables, through the sandboxed connection the data API also
/// uses, taken and dropped inside the handler's lock so it never outlives a privileged
/// window — an `app_conn()` held across `refresh_app` would read a replaced cache.
fn table_count(app: &crate::App, table: &str) -> i64 {
    let Ok(conn) = app.store().app_conn() else {
        return 0;
    };
    conn.query_row(
        &format!(
            "SELECT count(*) FROM {}",
            crate::store::materialize::quote_ident(table)
        ),
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn data_page(cx: &Context<'_>, body: &mut String) {
    let paths = cx.node.paths();
    body.push_str("<div class=\"pv-card\"><h3>");
    body.push_str(&icon("archive"));
    body.push_str(" Where your data is</h3>\n<dl>\n");
    dl(
        body,
        "App Directory",
        &code(&paths.root().display().to_string()),
    );
    dl(
        body,
        "Your data",
        &code(&paths.data_dir().display().to_string()),
    );
    dl(
        body,
        "Your space's key",
        &code(&paths.identity_dir().display().to_string()),
    );
    dl(
        body,
        "App folders",
        &code(&paths.apps_dir().display().to_string()),
    );
    dl(
        body,
        "Disposable",
        &format!(
            "{} and {}",
            code(&paths.cache_dir().display().to_string()),
            code(&paths.local_dir().display().to_string())
        ),
    );
    // `spec/cli.md §1`: the page names the root and which of the three rules chose it,
    // the same as the line every run prints. It reads as a sentence under the table rather
    // than as a note appended to a path.
    let _ = write!(
        body,
        "</dl>\n<p class=\"pv-help\">This space keeps its files there because that folder is \
         {}.</p>\n</div>\n",
        escape(paths.source().describe())
    );
    let _ = write!(
        body,
        "<h3>Backup</h3>\n\
         <p><strong>To backup your data, simply copy the {} folder.</strong></p>\n\
         <p>Point Syncthing, Dropbox, OneDrive, or a monthly USB stick at the folder above.</p>\n\
         <p>Snapshots under <code>data/&lt;app&gt;/snap/</code> and the SQLite files under \
         <code>cache/</code> are caches. Deleting every one of them loses no data.</p>\n\
         <p>Backups are plain text by design.</p>\n\
         <p class=\"pv-muted\">For more information, see \
         <a href=\"{}/blob/main/docs/backup-and-restore.md\">Backup and Restore</a>.</p>\n",
        folder_link(&paths.data_dir(), "data"),
        PROJECT_URL
    );
}

/// The repository the shell links documentation to. Documentation is not shipped beside
/// the binary, so a page an owner reads under stress points at the copy that is always
/// there rather than at a path that may not exist on this machine.
const PROJECT_URL: &str = "https://github.com/gabrielmongefranco/privatium";

/// A folder as a link the owner's file manager can open, with `text` as the link's words.
/// A `file:` URL is built from the path's components rather than from its display form, so
/// a Windows backslash and a space both arrive intact; a path that cannot be encoded is
/// rendered as plain code instead of a link that would go nowhere.
fn folder_link(path: &std::path::Path, text: &str) -> String {
    let Some(url) = file_url(path) else {
        return format!("<code>{}</code>", escape(text));
    };
    format!(
        "<a href=\"{}\"><code>{}</code></a>",
        escape(&url),
        escape(text)
    )
}

/// `file:///…` for an absolute path, percent-encoding everything outside the unreserved
/// set of RFC 3986 §2.3 so no character in a folder name can end the URL early.
fn file_url(path: &std::path::Path) -> Option<String> {
    let text = path.to_str()?;
    let mut out = String::from("file:///");
    for byte in text.replace('\\', "/").trim_start_matches('/').bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(char::from(byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    Some(out)
}

/// The 404 page.
#[must_use]
pub fn not_found(path: &str, solo: bool) -> String {
    let body = format!(
        "<h2>Not found</h2>\n<p class=\"pv-error\">{} Nothing is served at <code>{}</code>.</p>\n",
        icon("exclamation-triangle"),
        escape(path)
    );
    layout("Not found", Active::None, solo, &body)
}

/// A Tier 1 route in a build without the Lua host — a clear 503, not a 404 that looks like
/// a routing bug.
#[must_use]
pub fn no_handler(slug: &str, solo: bool) -> String {
    let body = format!(
        "<h2>No handler in this build</h2>\n<p class=\"pv-error\">{} <code>{}</code> is a Tier 1 \
         (Lua) app and is mounted, but this build has no Lua host yet; its routes answer once it \
         does.</p>\n",
        icon("info-circle"),
        escape(slug)
    );
    layout("No handler in this build", Active::None, solo, &body)
}

/// The error page beneath a Tier 1 mount (`spec/cli.md §3`): the first line of the error
/// as the summary, the whole text with its traceback, and — when the traceback names one
/// of the app's files — that file's lines around the offending one, which is marked by a
/// glyph and `aria-current`, never by colour alone. The owner is the only reader.
#[must_use]
pub fn lua_error(
    status: StatusCode,
    detail: &str,
    at: Option<&SourceContext>,
    solo: bool,
) -> String {
    let reason = status.canonical_reason().unwrap_or("Error");
    let summary = detail.lines().next().unwrap_or(detail);
    let mut body = format!(
        "<h2>{}</h2>\n<p class=\"pv-error\">{} {}</p>\n<pre class=\"pv-trace\">{}</pre>\n",
        escape(reason),
        icon("exclamation-triangle"),
        escape(summary),
        escape(detail)
    );
    if let Some(at) = at {
        let _ = writeln!(
            body,
            "<h3><code>{}</code>, line {}</h3>\n<pre class=\"pv-source\">",
            escape(&at.file),
            at.line
        );
        for (number, text) in &at.lines {
            if *number == at.line {
                let _ = writeln!(
                    body,
                    "<mark aria-current=\"true\">&gt; {number:>4}  {}</mark>",
                    escape(text)
                );
            } else {
                let _ = writeln!(body, "  {number:>4}  {}", escape(text));
            }
        }
        body.push_str("</pre>\n");
    }
    layout(reason, Active::None, solo, &body)
}

/// A failure page. `detail` is the error's own text; the owner is the only reader.
#[must_use]
pub fn error(status: StatusCode, detail: &str, solo: bool) -> String {
    let body = format!(
        "<h2>{}</h2>\n<p class=\"pv-error\">{} {}</p>\n",
        escape(status.canonical_reason().unwrap_or("Error")),
        icon("exclamation-triangle"),
        escape(detail)
    );
    layout(
        status.canonical_reason().unwrap_or("Error"),
        Active::None,
        solo,
        &body,
    )
}

/// One `<dt>`/`<dd>` pair; `definition` is markup the caller already escaped.
pub(crate) fn dl(out: &mut String, term: &str, definition: &str) {
    let _ = writeln!(out, "<dt>{}</dt><dd>{definition}</dd>", escape(term));
}

/// `text` escaped inside `<code>`.
pub(crate) fn code(text: &str) -> String {
    format!("<code>{}</code>", escape(text))
}

/// Run a read on the `_sys` store's privileged connection.
pub(crate) fn query<T>(
    node: &Node,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let duck = |error| crate::Error::Store(Box::new(StoreError::Sql(error)));
    let conn = node.store().conn();
    let mut statement = conn.prepare(sql).map_err(duck)?;
    let rows = statement.query_map([], map).map_err(duck)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(duck)?);
    }
    Ok(out)
}
