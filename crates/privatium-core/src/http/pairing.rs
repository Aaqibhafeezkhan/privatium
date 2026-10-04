// This file is part of Privatium
// crates/privatium-core/src/http/pairing.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: The data-free browser bootstrap (§8.4) with the pairing screen inside it (§7.2, §7.7): the
//          word field first, then the sixteen-glyph pad with a label beneath every glyph, and
//          the status region the three outcomes are said in. Both renderings of the code are
//          always offered (§7.2). The markup is rendered here so the PV4xx checks hold it;
//          client.js shows it when the browser holds no pairing and wires it, and a <noscript>
//          browser never reaches it. A paired device replaces the bootstrap with the page it
//          asked for, so the bootstrap's own text stays out of sight for a moment (§7.7).
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

use super::{assets, headers};
use crate::icons::escape;
use crate::pair::GLYPHS;
use crate::wire::Response;
use axum::http::{StatusCode, Uri};

/// The disclosure of spec/protocol.md §7.7, shown where a device pairs and where paired
/// devices are managed.
pub const DISCLOSURE: &str = "On every visit over plain HTTP, someone who can change network traffic can replace this client and read your data and stored device keys. Encryption protects against listening, but cannot verify the downloaded client.";

/// Render a bootstrap with the requested path, the public node ID and its display name
/// only. Query strings are attribute-escaped; neither application content nor a CSRF
/// token is read. The heading and the connecting status carry `pv-quiet`, which
/// `shell.css` keeps invisible for a moment: a paired device replaces this document
/// with the requested page well within that, so navigation shows no interstitial
/// (`spec/protocol.md §7.7`). The pairing screen lifts the class when it has to appear.
pub fn bootstrap(uri: &Uri, node: &str, name: &str) -> Response {
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    let mut html = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Connect — Privatium</title>
<link rel="stylesheet" href="/static/shell.css" integrity="{}">
<script type="module" src="/static/client.js" integrity="{}"></script>
</head><body data-pv-bootstrap data-path="{}" data-node="{}" data-name="{}">
<main id="main" tabindex="-1"><h1 class="pv-quiet">Connect to Privatium</h1>
<p id="pv-connecting" class="pv-quiet" role="status">Connecting to your space.</p>
<noscript><p>Pairing from another device needs JavaScript. You can use Privatium without JavaScript in a browser on the space itself.</p></noscript>
"#,
        assets::integrity("shell.css"),
        assets::integrity("client.js"),
        escape(path),
        escape(node),
        escape(name)
    );
    pairing_screen(&mut html, node, name);
    html.push_str("</main></body></html>");
    headers::html(StatusCode::OK, html)
}

/// The pairing screen (`spec/protocol.md §7.2`), hidden until the client finds no
/// pairing in storage. Completable without reading — four taps on the pad, then Pair —
/// and without seeing: every control is labelled, the glyph inside each key is hidden
/// from a screen reader so its label is read once, and the word field takes the two
/// words `§7.2` renders the same code as. The disclosure of `§7.7` closes the screen,
/// so a device reads it when it pairs and not on every page it opens afterwards.
fn pairing_screen(out: &mut String, node: &str, name: &str) {
    let _ = write!(
        out,
        "<section id=\"pv-pair\" hidden aria-labelledby=\"pv-pair-title\">\n\
         <h2 id=\"pv-pair-title\">Pair this device with {} <code>{}</code></h2>\n\
         <p>Read the pairing code from the space's screen. Type its two words here, or tap its \
         four emoji in order.</p>\n\
         <form id=\"pv-pair-form\" method=\"post\" action=\"#\">\n\
         <label for=\"pv-words\">Type the two words</label>\n\
         <input id=\"pv-words\" name=\"words\" autocomplete=\"off\" autocapitalize=\"none\" \
         spellcheck=\"false\" placeholder=\"amber otter\">\n\
         <div class=\"pv-pad\" role=\"group\" aria-label=\"Or tap the four emoji\">\n",
        escape(name),
        escape(node)
    );
    for (index, glyph) in GLYPHS.iter().enumerate() {
        let _ = writeln!(
            out,
            "<button type=\"button\" class=\"pv-pad-key\" data-glyph=\"{index}\">\
             <span class=\"pv-glyph\" aria-hidden=\"true\">{}</span><span class=\"pv-glyph-label\">{}</span></button>",
            glyph.glyph, glyph.label
        );
    }
    out.push_str(
        "</div>\n\
         <p class=\"pv-chosen\">Chosen: <output id=\"pv-chosen\" aria-live=\"polite\">none yet</output> \
         <button type=\"button\" id=\"pv-undo\" class=\"pv-btn\">Remove last</button> \
         <button type=\"button\" id=\"pv-clear\" class=\"pv-btn\">Clear</button></p>\n\
         <label for=\"pv-label\">Name this device (optional)</label>\n\
         <input id=\"pv-label\" name=\"label\" maxlength=\"80\" autocomplete=\"off\">\n\
         <button type=\"submit\" id=\"pv-pair-submit\" class=\"pv-btn pv-btn-primary\">Pair</button>\n\
         </form>\n\
         <p id=\"pv-pair-status\" role=\"status\"></p>\n\
         <p class=\"pv-muted\">Pairing has to be open on the space: its Settings › Devices › \
         Open pairing, or <code>privatium pair</code> in a terminal there.</p>\n",
    );
    let _ = writeln!(
        out,
        "<p class=\"pv-muted pv-disclosure\">{DISCLOSURE}</p>\n</section>"
    );
}
