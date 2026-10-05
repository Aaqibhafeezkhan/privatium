// This file is part of Privatium
// crates/privatium-core/src/http/shell.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: The framework's own pages — launcher, settings, errors — and the frame a Tier 1 view
//          renders inside, as server-rendered HTML with HTMX and inlined Bootstrap Icons
//          (docs/architecture.md §2.5, docs/icons.md). The frame is the standard chrome of
//          spec/lua-api.md §4.1: a three-zone bar, one menu with the app's items first, and
//          a footer with a status slot; the same bar and footer are rendered as the pieces the
//          node inserts into a document an app owns (spec/app-contract.md §5). No client
//          framework, no bundler, no inline script or
//          style: every page renders under the default CSP of spec/protocol.md §9.3 exactly
//          as written, and every page is held to the PV4xx rules of spec/cli.md §5 by
//          tests/reference.rs.
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

/// What the page frame carries for one app (`spec/lua-api.md §4.1`, `spec/app-contract.md
/// §3`): the title zone, the menu's app items, and the stylesheets and scripts the head
/// loads on every page. Built from the manifest under the node lock; the asset hashes are
/// filled in afterwards, off the lock, by [`Frame::hash_assets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// `app.title`, the window title and the centre of the bar.
    pub title: String,
    /// `app.icon`, drawn before the title when the manifest declares one.
    pub icon: Option<String>,
    /// The mount the title links to: `/a/<slug>/` or `/`.
    pub mount: String,
    /// `[[ui.menu]]`, resolved: the app-wide items, in manifest order.
    pub menu: Vec<MenuLink>,
    /// `ui.styles`, as the head loads them.
    pub styles: Vec<FrameAsset>,
    /// `ui.scripts`, as the head loads them.
    pub scripts: Vec<FrameAsset>,
    /// The app's `static/` folder, where the assets above are hashed from.
    pub static_dir: Option<std::path::PathBuf>,
    /// `ui.chrome`: whether a document the app owns receives the bar and footer
    /// (`spec/app-contract.md §5`).
    pub chrome: crate::app::manifest::Chrome,
    /// `ui.navigation`: whether the frame's main region swaps the app's next page in
    /// place (`spec/lua-api.md §4.1`, `spec/protocol.md §8.3.1`).
    pub navigation: crate::app::manifest::Navigation,
}

/// The three pieces the node inserts into a document an app owns (`spec/app-contract.md
/// §5`): what goes before `</head>`, what goes after the opening `<body>` tag, and what
/// goes before the closing `</body>`. Rendered by [`chrome_pieces`]; placed by
/// `http::apps::insert_chrome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePieces {
    /// The chrome stylesheet and script, at their addressed paths with their hashes.
    pub head: String,
    /// The skip link and the three-zone header.
    pub body_start: String,
    /// The footer with the status slot.
    pub body_end: String,
}

/// One item of the menu's app section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLink {
    /// The text.
    pub label: String,
    /// Already resolved through `url()`.
    pub href: String,
    /// A vendored icon name, when the item has one.
    pub icon: Option<String>,
}

/// One stylesheet or script the frame's head loads for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameAsset {
    /// The file beneath `static/`, as the manifest spells it: `static/forms.js`.
    pub entry: String,
    /// Already resolved through `url()`.
    pub href: String,
    /// `sha256-…` of the file as it is on disk, or `None` when it could not be read.
    pub integrity: Option<String>,
}

impl Frame {
    /// The frame for an app at `mount`, from its manifest. Asset hashes are not read here:
    /// call [`Frame::hash_assets`] once off the lock.
    #[must_use]
    pub fn for_app(
        manifest: &crate::app::Manifest,
        mount: &str,
        dir: Option<&std::path::Path>,
    ) -> Self {
        let asset = |entry: &String| FrameAsset {
            entry: entry.clone(),
            href: url(mount, entry),
            integrity: None,
        };
        Self {
            title: manifest.app.title.clone(),
            icon: manifest.app.icon.clone(),
            mount: mount.to_owned(),
            menu: manifest
                .ui
                .menu
                .iter()
                .map(|item| MenuLink {
                    label: item.label.trim().to_owned(),
                    href: url(mount, &item.path),
                    icon: item.icon.clone(),
                })
                .collect(),
            styles: manifest.ui.styles.iter().map(asset).collect(),
            scripts: manifest.ui.scripts.iter().map(asset).collect(),
            static_dir: dir.map(std::path::Path::to_path_buf),
            chrome: manifest.ui.chrome,
            navigation: manifest.ui.navigation,
        }
    }

    /// Read each declared asset from the folder and record its `integrity` hash, so the
    /// page names the bytes it expects and a stale cache is refused. Read on every render
    /// rather than once at load, because `static/` is edited and served live. A file that
    /// cannot be read keeps no hash; the browser then reports the missing file itself.
    pub fn hash_assets(&mut self) {
        let Some(dir) = self.static_dir.clone() else {
            return;
        };
        let app_dir = dir.parent().map(std::path::Path::to_path_buf);
        for asset in self.styles.iter_mut().chain(self.scripts.iter_mut()) {
            let path = app_dir.as_ref().map(|app| app.join(&asset.entry));
            asset.integrity = path
                .and_then(|path| std::fs::read(path).ok())
                .map(|bytes| crate::http::assets::integrity_of(&bytes));
        }
    }
}

/// Everything `page` needs besides the body.
struct PageSpec<'a> {
    /// The window title, before ` — Privatium`.
    title: &'a str,
    active: Active,
    /// No launcher to link to: no Apps link in the bar.
    solo: bool,
    /// The brand is the page's `<h1>` (the shell's pages) rather than a paragraph (an
    /// app's frame, where the view supplies the heading).
    brand_heading: bool,
    body_attrs: &'a str,
    node_label: Option<&'a str>,
    /// The app whose frame this is, with the page's own menu items appended; `None` for
    /// the shell's pages.
    app: Option<(&'a Frame, &'a [MenuLink])>,
    /// Swap navigation: the main region is boosted and the menu's app list is swapped out
    /// of band with every page, so it follows the page.
    swap: bool,
}

/// The shell's own page frame. `solo` drops the launcher link: there is no launcher to
/// link to.
fn layout(title: &str, active: Active, solo: bool, body: &str) -> String {
    page(
        &PageSpec {
            title,
            active,
            solo,
            brand_heading: true,
            body_attrs: "",
            node_label: None,
            app: None,
            swap: false,
        },
        body,
    )
}

/// The frame a Tier 1 view renders inside when it calls no `layout()`
/// (`spec/lua-api.md §4.1`): the same head and header as the shell's, titled by the app,
/// with the brand demoted to a paragraph so the view keeps the page's one `<h1>`, and
/// `hx-headers` on the body so every htmx request beneath the mount carries the CSRF
/// token — which is what lets an `hx-delete` button, with no form, pass the host's check.
/// `page_menu` is what the view added with `menu()`, listed after the manifest's items.
/// Under `navigation = "swap"` the main region carries `hx-boost`, so a link or form inside
/// it fetches the next page and `chrome.js` swaps that page's main region into this one.
#[must_use]
pub fn app_frame(
    frame: &Frame,
    page_menu: &[MenuLink],
    solo: bool,
    csrf_token: &str,
    body: &str,
    node_label: &str,
) -> String {
    let attrs = format!(
        " data-pv-mount=\"{}\" hx-headers='{{\"X-CSRF-Token\":\"{}\"}}'",
        escape(&frame.mount),
        escape(csrf_token)
    );
    page(
        &PageSpec {
            title: &frame.title,
            active: Active::None,
            solo,
            brand_heading: false,
            body_attrs: &attrs,
            node_label: Some(node_label),
            app: Some((frame, page_menu)),
            swap: frame.navigation == crate::app::manifest::Navigation::Swap,
        },
        body,
    )
}

/// The framework's own pages, in the order the menu lists them.
const SYSTEM_PAGES: [(&str, &str); 4] = [
    ("/settings", "Space settings"),
    ("/settings/apps", "App settings"),
    ("/settings/data", "Data settings"),
    ("/settings/devices", "Devices"),
];

/// The document around a body: head, header, main, footer.
///
/// The header is three zones — the brand linking to `/`, the app's title linking to its
/// mount, and the controls: an Apps link in host mode and the one Menu. The menu lists the
/// app's items first, then a rule, then the framework's pages; the rule is hidden by the
/// stylesheet while the app's list is empty, so an item a script appends later shows it.
/// The footer carries the status slot `chrome.js` and `pv.status()` write to.
fn page(spec: &PageSpec<'_>, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 3072);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    // htmx never evaluates anything: the shell has no `hx-on` and no `js:` values, and the
    // config says so, which keeps the default CSP's `script-src 'self'` honest (AGENTS.md).
    // It also injects a `<style>` for its request indicators unless told not to, which the
    // default CSP (no `style-src`, so `default-src 'self'`) refuses with a console error on
    // every page; the shell's stylesheet is the only style there is. The history cache is
    // off, so no page is ever copied into browser storage, and the back button after a
    // swap reloads the page from the node.
    out.push_str(
        "<meta name=\"htmx-config\" content='{\"allowEval\":false,\"allowScriptTags\":false,\
         \"selfRequestsOnly\":true,\"includeIndicatorStyles\":false,\"historyCacheSize\":0,\
         \"refreshOnHistoryMiss\":true}'>\n",
    );
    let _ = writeln!(out, "<title>{} — Privatium</title>", escape(spec.title));
    out.push_str(&stylesheet_link("chrome.css"));
    out.push_str(&stylesheet_link("shell.css"));
    out.push_str(&script_tag("htmx.min.js", false));
    out.push_str(&script_tag("chrome.js", true));
    if let Some((frame, _)) = spec.app {
        for sheet in &frame.styles {
            let _ = writeln!(
                out,
                "<link rel=\"stylesheet\" href=\"{}\"{}>",
                escape(&sheet.href),
                integrity_attr(sheet.integrity.as_deref())
            );
        }
        for script in &frame.scripts {
            let _ = writeln!(
                out,
                "<script src=\"{}\"{} defer></script>",
                escape(&script.href),
                integrity_attr(script.integrity.as_deref())
            );
        }
    }
    let _ = writeln!(out, "</head>\n<body{}>", spec.body_attrs);
    write_header(&mut out, spec);
    out.push_str(if spec.swap {
        "<main id=\"main\" hx-boost=\"true\">\n"
    } else {
        "<main id=\"main\">\n"
    });
    out.push_str(body);
    out.push_str("\n</main>\n");
    write_footer(&mut out, spec.node_label.unwrap_or(""));
    out.push_str("</body>\n</html>\n");
    out
}

/// `<link rel="stylesheet">` for one embedded asset, at its addressed path with its hash.
fn stylesheet_link(name: &str) -> String {
    format!(
        "<link rel=\"stylesheet\" href=\"{}\" integrity=\"{}\">\n",
        crate::http::assets::versioned(name),
        crate::http::assets::integrity(name)
    )
}

/// `<script defer>` for one embedded asset, a module when `module`. A module script is
/// deferred by nature; `defer` is written anyway so the rule the reference tests hold —
/// every script on a page is deferred, so document order is execution order — reads from
/// the markup alone.
fn script_tag(name: &str, module: bool) -> String {
    format!(
        "<script{} src=\"{}\" integrity=\"{}\" defer></script>\n",
        if module { " type=\"module\"" } else { "" },
        crate::http::assets::versioned(name),
        crate::http::assets::integrity(name)
    )
}

/// The skip link and the three-zone header of `spec/lua-api.md §4.1`.
fn write_header(out: &mut String, spec: &PageSpec<'_>) {
    out.push_str("<a class=\"pv-skip\" href=\"#main\">Skip to content</a>\n");
    let (brand_open, brand_close) = if spec.brand_heading {
        ("<h1 class=\"pv-brand\">", "</h1>")
    } else {
        ("<p class=\"pv-brand\">", "</p>")
    };
    let logo = crate::http::assets::versioned("privatium-logo-light.svg");
    let mark = crate::http::assets::versioned("privatium-mark-white.svg");
    // The wordmark on a wide screen, the square mark on a narrow one: one request either
    // way, chosen by the browser, so the title zone keeps its room on a phone.
    let _ = write!(
        out,
        "<header class=\"pv-header\">\n{brand_open}<a href=\"/\"><picture><source media=\"(max-width: 30rem)\" srcset=\"{mark}\" width=\"34\" height=\"34\"><img class=\"pv-brand-logo\" src=\"{logo}\" alt=\"\" width=\"160\" height=\"34\"></picture><span class=\"pv-visually-hidden\">Privatium</span></a>{brand_close}\n"
    );
    if let Some((frame, _)) = spec.app {
        let _ = write!(
            out,
            "<p class=\"pv-app-title\"><a href=\"{}\">",
            escape(&url(&frame.mount, ""))
        );
        if let Some(name) = &frame.icon {
            out.push_str(&icon(name));
            out.push(' ');
        }
        let _ = writeln!(out, "<span>{}</span></a></p>", escape(&frame.title));
    }
    out.push_str("<nav class=\"pv-controls\" aria-label=\"Framework\">\n");
    if !spec.solo {
        let _ = writeln!(
            out,
            "<a href=\"/\"{}>{} Apps</a>",
            current(spec.active == Active::Launcher),
            icon("grid-3x3-gap")
        );
    }
    let _ = write!(
        out,
        "<details class=\"pv-menu\"><summary aria-label=\"Menu\" title=\"Menu\">{}</summary><nav aria-label=\"All pages\">\n<ul id=\"pv-app-menu\" class=\"pv-menu-app\"{}>",
        icon("list"),
        if spec.swap {
            " hx-swap-oob=\"true\""
        } else {
            ""
        }
    );
    if let Some((frame, page_menu)) = spec.app {
        for item in frame.menu.iter().chain(page_menu) {
            let _ = write!(
                out,
                "<li><a href=\"{}\">{}{}</a></li>",
                escape(&item.href),
                item.icon
                    .as_deref()
                    .map(|name| format!("{} ", icon(name)))
                    .unwrap_or_default(),
                escape(&item.label)
            );
        }
    }
    out.push_str("</ul>\n<hr class=\"pv-menu-rule\">\n<ul class=\"pv-menu-system\">");
    for (href, label) in SYSTEM_PAGES {
        let _ = write!(out, "<li><a href=\"{href}\">{label}</a></li>");
    }
    out.push_str("</ul></nav></details>\n</nav>\n</header>\n");
}

/// The footer of `spec/lua-api.md §4.1`: the project link, the status slot `chrome.js`
/// and `pv.status()` write to, and the space's name with the way to connect a device.
fn write_footer(out: &mut String, node_label: &str) {
    let _ = write!(
        out,
        "<footer class=\"pv-footer\"><a href=\"https://github.com/gabrielmongefranco/privatium\">Privatium</a>\n<p id=\"pv-status\" class=\"pv-status\" role=\"status\"></p>\n<div class=\"pv-footer-node\"><span>{}</span><a class=\"pv-join\" href=\"/settings/devices\" aria-label=\"Connect a device — opens Devices\" title=\"Connect a device\">{}</a></div></footer>\n",
        escape(node_label),
        icon("qr-code")
    );
}

/// What the node inserts into a document an app owns (`spec/app-contract.md §5`): the
/// chrome stylesheet and script for the head, the skip link and the header the frame
/// would draw for this app — with the manifest's menu items, in host or solo form — and
/// the footer. The document's own `<body>` is kept, so there is no `hx-headers` here; a
/// Tier 2 app talks to the data API, which takes no token.
#[must_use]
pub fn chrome_pieces(frame: &Frame, solo: bool, node_label: &str) -> ChromePieces {
    let spec = PageSpec {
        title: &frame.title,
        active: Active::None,
        solo,
        brand_heading: false,
        body_attrs: "",
        node_label: Some(node_label),
        app: Some((frame, &[])),
        swap: false,
    };
    let mut head = stylesheet_link("chrome.css");
    head.push_str(&script_tag("chrome.js", true));
    let mut body_start = String::with_capacity(2048);
    write_header(&mut body_start, &spec);
    let mut body_end = String::with_capacity(512);
    write_footer(&mut body_end, node_label);
    ChromePieces {
        head,
        body_start,
        body_end,
    }
}

/// ` integrity="…"` when a hash is known, nothing otherwise.
fn integrity_attr(hash: Option<&str>) -> String {
    hash.map(|hash| format!(" integrity=\"{}\"", escape(hash)))
        .unwrap_or_default()
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
    Ok(page(
        &PageSpec {
            title,
            active,
            solo,
            brand_heading: true,
            body_attrs: "",
            node_label: Some(&label),
            app: None,
            swap: false,
        },
        body,
    ))
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
