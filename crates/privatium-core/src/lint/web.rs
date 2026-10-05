// This file is part of Privatium
// crates/privatium-core/src/lint/web.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: A Tier 2 app's web/ and a Tier 1 app's static/: HTML through the same tree the templates use
//          (PV401–405, 407, and PV404 over the whole document), attributes for PV301, PV207
//          and PV504, JavaScript through a small lexer — strings, template literals, comments,
//          identifiers — for PV201, PV206, PV302, PV304, PV305, PV306, PV301, PV505 and the
//          two origin rules, and stylesheets for PV406's contrast floors and any origin in a
//          url(). PV506 names a top-level web/ entry a framework prefix shadows. Under the
//          standard chrome, PV109 asks each document for its anchors and its main region, and
//          PV408 for no second way back — an href to the launcher or settings, or a path
//          that climbs out of the mount in browser code.
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
use std::fs;
use std::path::Path;

use crate::app::manifest::{Chrome, Tier};
use crate::lint::{
    Columns, Ctx, Edit, RuleId, check_chrome_anchors, css, html, is_absolute_fs_path,
    is_way_back_href, is_way_back_path, line_of, lua, mount_path, origin_of,
};

/// Everything under `web/` (Tier 2) and `static/` (Tier 1).
pub(crate) fn check(ctx: &mut Ctx<'_>, _facts: &lua::Facts) {
    let is_web = ctx
        .manifest
        .as_ref()
        .is_some_and(|m| m.app.tier == Tier::Web);
    let mut files = Vec::new();
    if is_web {
        collect(&ctx.dir.join("web"), "web", &mut files);
        check_shadowed_entries(ctx);
    }
    collect(&ctx.dir.join("static"), "static", &mut files);
    files.sort();
    let columns = Columns::of(ctx.schema.as_ref());
    for rel in files {
        if rel.contains("/vendor/") || rel.ends_with(".min.js") || rel.ends_with(".min.css") {
            continue;
        }
        let Some(text) = ctx.read(&rel) else {
            continue;
        };
        if rel.ends_with(".html") {
            check_html(ctx, &rel, &text, &columns);
        } else if rel.ends_with(".js") || rel.ends_with(".mjs") {
            check_js(ctx, &rel, &text, &columns);
        } else if rel.ends_with(".css") {
            check_css(ctx, &rel, &text);
        }
    }
}

fn collect(dir: &Path, rel: &str, into: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect(&path, &format!("{rel}/{name}"), into);
        } else {
            into.push(format!("{rel}/{name}"));
        }
    }
}

/// `PV506` for a Tier 2 app: a top-level entry of `web/` named after a framework prefix.
fn check_shadowed_entries(ctx: &mut Ctx<'_>) {
    let Ok(entries) = fs::read_dir(ctx.dir.join("web")) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let route = format!("/{}", name.trim_end_matches(".html"));
        if let Some(prefix) = crate::wire::router::shadowing_prefix(&route) {
            ctx.push(
                RuleId::PV506,
                &format!("web/{name}"),
                0,
                format!("web/{name} is shadowed by the framework prefix {prefix} in solo mode, where the app owns /"),
            )
            .fix = Some("rename the entry".into());
        }
    }
}

/// Whether this app's own documents receive the standard chrome: a Tier 2 app that did
/// not set `[ui] chrome = "none"` (`spec/app-contract.md §5`). A Tier 1 app's `static/`
/// holds no document the chrome goes into.
fn under_standard_chrome(ctx: &Ctx<'_>) -> bool {
    ctx.manifest
        .as_ref()
        .is_some_and(|m| m.app.tier == Tier::Web && m.ui.chrome == Chrome::Standard)
}

fn check_html(ctx: &mut Ctx<'_>, rel: &str, text: &str, columns: &Columns) {
    let root = html::parse(text);
    for finding in html::element_findings(&root) {
        let f = ctx.push(finding.rule, rel, finding.line, finding.message);
        if finding.fixable {
            f.fix = Some("focusable=\"false\"".into());
        }
    }
    // PV109 and PV408 under the standard chrome: the anchors and the main region, and no
    // link of the app's own to where the bar already leads.
    if under_standard_chrome(ctx) {
        check_chrome_anchors(ctx, rel, text);
        for link in root.find_all("a") {
            if let Some(href) = link.attr("href")
                && is_way_back_href(href)
            {
                ctx.push(
                    RuleId::PV408,
                    rel,
                    link.line,
                    format!("<a href=\"{href}\"> is a second way back — the bar already carries the Privatium mark, the Apps link and the Menu"),
                )
                .fix = Some("drop the link, or set [ui] chrome = \"none\" and keep your own".into());
            }
        }
    }
    // PV404 over the whole document.
    let headings = html::headings(&root);
    let h1s = headings.iter().filter(|(l, _)| *l == 1).count();
    if h1s != 1 {
        ctx.push(
            RuleId::PV404,
            rel,
            headings.first().map_or(1, |h| h.1),
            format!("{h1s} <h1> elements; a page carries exactly one"),
        );
    }
    if let Some((level, line)) = headings.first()
        && *level != 1
    {
        ctx.push(
            RuleId::PV404,
            rel,
            *line,
            format!("the first heading is <h{level}>, not <h1>"),
        );
    }
    for pair in headings.windows(2) {
        let ((before, _), (after, line)) = (pair[0], pair[1]);
        if after > before + 1 {
            ctx.push(
                RuleId::PV404,
                rel,
                line,
                format!("<h{before}> is followed by <h{after}>; levels do not skip"),
            )
            .fix = Some(format!("<h{}>", before + 1));
        }
    }
    // Attributes: mount paths, origins.
    let slug = ctx.slug();
    let remote: Vec<String> = ctx
        .manifest
        .as_ref()
        .map(|m| m.permissions.remote.clone())
        .unwrap_or_default();
    let path = ctx.path(rel);
    for attr in super::template::attributes_in(text) {
        let line = line_of(text, attr.value_offset);
        if let Some((target, beneath)) = mount_path(&attr.value) {
            let own = target == slug;
            let finding = ctx.push(
                RuleId::PV301,
                rel,
                line,
                format!(
                    "literal mount path '/a/{target}/' in {}=\"…\" breaks solo mode",
                    attr.name
                ),
            );
            if own {
                finding.fix = Some(format!(
                    "build the URL with pv.url('{beneath}') in app.js, or write a relative path"
                ));
            }
        }
        if let Some(origin) = origin_of(&attr.value) {
            let resource = matches!(attr.name.as_str(), "src" | "srcset" | "poster")
                || (attr.name == "href" && attr.tag == "link");
            if resource {
                ctx.push(RuleId::PV504, rel, line, format!("{} loaded from {origin} — a CDN is a third party on the critical path, an offline failure and an IP leak", attr.tag))
                    .fix = Some("vendor the file under web/vendor/".into());
                if !remote.iter().any(|r| r == &origin) {
                    ctx.push(RuleId::PV207, rel, line, format!("{origin} is referenced but not declared in permissions.remote; the CSP blocks it silently"))
                        .fix = Some("vendor it, or declare the origin in [permissions] remote with a comment saying why".into());
                }
            }
        }
    }
    // Inline scripts get the JavaScript rules too.
    for script in root.find_all("script") {
        if script.attr("src").is_none() {
            let body = script.all_text();
            if !body.trim().is_empty() {
                let offset = text.find(&body).unwrap_or(0);
                check_js_at(ctx, rel, &body, line_of(text, offset) - 1, columns, None);
            }
        }
    }
    // The focusable fix: an inline <svg> without it.
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<svg") {
        let start = from + at;
        let Some(end) = lower[start..].find('>') else {
            break;
        };
        if !text[start..start + end].contains("focusable") {
            let line = line_of(text, start);
            if let Some(finding) = ctx.findings.iter_mut().find(|f| {
                f.id == RuleId::PV401 && f.line == line && f.message.contains("focusable")
            }) {
                finding.edit = Some(Edit {
                    file: path.clone(),
                    start: start + 4,
                    end: start + 4,
                    replacement: " focusable=\"false\"".into(),
                });
            }
        }
        from = start + end;
    }
}

// ---------------------------------------------------------------------------------------
// JavaScript
// ---------------------------------------------------------------------------------------

/// One JavaScript token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    Ident(String),
    /// A string; `interpolated` when a template literal carries `${`.
    Str {
        text: String,
        interpolated: bool,
    },
    Num,
    Punct(String),
}

/// Whether a `/` at this point begins a regular expression rather than a division.
fn regex_may_follow(last: Option<&Tok>) -> bool {
    match last {
        None => true,
        Some(Tok::Punct(p)) => !matches!(p.as_str(), ")" | "]" | "}"),
        Some(Tok::Ident(i)) => matches!(
            i.as_str(),
            "return" | "typeof" | "in" | "of" | "case" | "await" | "yield"
        ),
        Some(_) => false,
    }
}

/// Tokenize JavaScript: comments and whitespace dropped, each token with its byte offset.
pub(crate) fn tokens(js: &str) -> Vec<(usize, Tok)> {
    let bytes = js.as_bytes();
    let mut out: Vec<(usize, Tok)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i = js[i..].find('\n').map_or(bytes.len(), |at| i + at);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = js[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |at| i + 2 + at + 2);
            continue;
        }
        if b == b'\'' || b == b'"' || b == b'`' {
            let start = i;
            let mut interpolated = false;
            i += 1;
            while i < bytes.len() && bytes[i] != b {
                if bytes[i] == b'\\' {
                    i += 1;
                } else if b == b'`' && bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
                    interpolated = true;
                }
                i += 1;
            }
            let text = js[start + 1..i.min(bytes.len())].to_owned();
            i += 1;
            out.push((start, Tok::Str { text, interpolated }));
            continue;
        }
        if b == b'/' && regex_may_follow(out.last().map(|(_, t)| t)) {
            let start = i;
            i += 1;
            let mut in_class = false;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => i += 1,
                    b'[' => in_class = true,
                    b']' => in_class = false,
                    b'/' if !in_class => break,
                    b'\n' => break,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push((
                start,
                Tok::Str {
                    text: String::new(),
                    interpolated: false,
                },
            ));
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
            {
                i += 1;
            }
            out.push((start, Tok::Ident(js[start..i].to_owned())));
            continue;
        }
        if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            out.push((start, Tok::Num));
            continue;
        }
        let three = &js[i..(i + 3).min(bytes.len())];
        let two = &js[i..(i + 2).min(bytes.len())];
        if ["===", "!==", "**=", "..."].contains(&three) {
            out.push((i, Tok::Punct(three.to_owned())));
            i += 3;
            continue;
        }
        if [
            "=>", "==", "!=", "+=", "-=", "&&", "||", "??", "?.", "++", "--", "<=", ">=",
        ]
        .contains(&two)
        {
            out.push((i, Tok::Punct(two.to_owned())));
            i += 2;
            continue;
        }
        let ch = js[i..].chars().next().unwrap_or(' ');
        out.push((i, Tok::Punct(ch.to_string())));
        i += ch.len_utf8();
    }
    out
}

/// The tokens of one argument: from `from` (just past `(`) to the matching `,` or `)`.
fn argument(toks: &[(usize, Tok)], from: usize) -> &[(usize, Tok)] {
    let mut depth = 0i32;
    let mut i = from;
    while i < toks.len() {
        match &toks[i].1 {
            Tok::Punct(p) if p == "(" || p == "[" || p == "{" => depth += 1,
            Tok::Punct(p) if p == ")" || p == "]" || p == "}" => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            Tok::Punct(p) if p == "," && depth == 0 => break,
            _ => {}
        }
        i += 1;
    }
    &toks[from..i]
}

/// Name fragments that mean an outbox is being deduplicated by hand (`PV305`).
const DEDUPE_NAMES: &[&str] = &[
    "dedupe",
    "dedup",
    "txid",
    "tx_id",
    "transaction_id",
    "transactionid",
    "acked",
    "ack_at",
    "acknowledg",
];

/// The envelope fields a client never sets (`PV304`, `spec/data-api.md §2`).
const ENVELOPE: &[&str] = &["seq", "lam", "ts", "dev", "app"];

fn check_js(ctx: &mut Ctx<'_>, rel: &str, text: &str, columns: &Columns) {
    let path = ctx.path(rel);
    check_js_at(ctx, rel, text, 0, columns, Some(&path));
}

/// The JavaScript rules over `js`, whose first line is `base + 1` in the file; edits are
/// made only when `edit_path` is given (a whole file, not a script inside HTML).
fn check_js_at(
    ctx: &mut Ctx<'_>,
    rel: &str,
    js: &str,
    base: u32,
    columns: &Columns,
    edit_path: Option<&Path>,
) {
    let toks = tokens(js);
    let slug = ctx.slug();
    let remote: Vec<String> = ctx
        .manifest
        .as_ref()
        .map(|m| m.permissions.remote.clone())
        .unwrap_or_default();
    let way_back = under_standard_chrome(ctx);
    let line = |offset: usize| base + line_of(js, offset);
    let mut noted: BTreeSet<String> = BTreeSet::new();
    // Object literal keys by brace depth, for PV304.
    let mut objects: Vec<BTreeSet<String>> = Vec::new();
    // pv writes per brace depth, for PV306.
    let mut writes: Vec<u32> = vec![0];

    for (i, (offset, tok)) in toks.iter().enumerate() {
        let prev = if i > 0 { Some(&toks[i - 1].1) } else { None };
        let next = toks.get(i + 1).map(|(_, t)| t);
        match tok {
            Tok::Punct(p) if p == "{" => {
                objects.push(BTreeSet::new());
                writes.push(0);
            }
            Tok::Punct(p) if p == "}" => {
                if let Some(keys) = objects.pop() {
                    let event = keys.contains("op") || keys.contains("tbl");
                    let stamped: Vec<&str> = ENVELOPE
                        .iter()
                        .copied()
                        .filter(|k| keys.contains(*k))
                        .collect();
                    if event && !stamped.is_empty() {
                        ctx.push(RuleId::PV304, rel, line(*offset), format!("an event sets {} — the framework stamps the envelope and the server rejects a client that does", stamped.join(", ")))
                            .fix = Some("send op, tbl, id and d only".into());
                    }
                }
                writes.pop();
            }
            Tok::Ident(name) => {
                // An object key: `name:` not after `?`.
                if matches!(next, Some(Tok::Punct(p)) if p == ":")
                    && !matches!(prev, Some(Tok::Punct(p)) if p == "?")
                    && let Some(keys) = objects.last_mut()
                {
                    keys.insert(name.clone());
                }
                let lower = name.to_ascii_lowercase();
                if DEDUPE_NAMES.iter().any(|n| lower.contains(n)) && noted.insert(lower) {
                    ctx.push(RuleId::PV305, rel, line(*offset), format!("`{name}` looks like outbox bookkeeping — a dedupe key, a transaction id, an acknowledgement; ULIDs already make replay idempotent (pv.js's outbox needs none)"))
                        .fix = Some("drop it: a queued write retried with its id converges".into());
                }
                // innerHTML = <non-literal>
                if matches!(name.as_str(), "innerHTML" | "outerHTML")
                    && matches!(prev, Some(Tok::Punct(p)) if p == "." || p == "?.")
                    && matches!(next, Some(Tok::Punct(p)) if p == "=" || p == "+=")
                {
                    let rhs = argument(&toks, i + 2);
                    let literal = matches!(
                        rhs,
                        [(
                            _,
                            Tok::Str {
                                interpolated: false,
                                ..
                            }
                        )]
                    );
                    if !literal || matches!(next, Some(Tok::Punct(p)) if p == "+=") {
                        ctx.push(RuleId::PV206, rel, line(*offset), format!("{name} assigned from data — markup built from a value is the injection the CSP cannot see"))
                            .fix = Some("textContent for text; createElement and append for structure".into());
                    }
                }
                if matches!(name.as_str(), "insertAdjacentHTML")
                    && matches!(next, Some(Tok::Punct(p)) if p == "(")
                {
                    let second = argument(&toks, i + 2).len() + i + 3;
                    let rhs = argument(&toks, second);
                    if !matches!(
                        rhs,
                        [(
                            _,
                            Tok::Str {
                                interpolated: false,
                                ..
                            }
                        )]
                    ) {
                        ctx.push(RuleId::PV206, rel, line(*offset), "insertAdjacentHTML with data — markup built from a value is the injection the CSP cannot see")
                            .fix = Some("textContent, or createElement and append".into());
                    }
                }
                // Number(x.col) / parseFloat / parseInt / +x.col
                if matches!(name.as_str(), "Number" | "parseFloat" | "parseInt")
                    && matches!(next, Some(Tok::Punct(p)) if p == "(")
                {
                    let arg = argument(&toks, i + 2);
                    if let Some(column) = column_in(arg, columns) {
                        ctx.push(RuleId::PV302, rel, line(*offset), format!("{name}() on `{column}`, a DECIMAL or BIGINT column that arrives as a string — a double loses what the text keeps"))
                            .fix = Some("keep it a string; a decimal library or integer cents for arithmetic".into());
                    }
                }
                // pv.<call>
                if name == "pv"
                    && matches!(next, Some(Tok::Punct(p)) if p == ".")
                    && let Some(Tok::Ident(method)) = toks.get(i + 2).map(|(_, t)| t)
                {
                    {
                        let is_call = matches!(toks.get(i + 3).map(|(_, t)| t), Some(Tok::Punct(p)) if p == "(");
                        if is_call && method == "sql" {
                            let arg = argument(&toks, i + 4);
                            let concatenated = arg.iter().any(|(_, t)| {
                                matches!(t, Tok::Punct(p) if p == "+")
                                    || matches!(
                                        t,
                                        Tok::Str {
                                            interpolated: true,
                                            ..
                                        }
                                    )
                            });
                            if concatenated {
                                ctx.push(RuleId::PV201, rel, line(*offset), "pv.sql is given SQL built by concatenation or interpolation; a value reaches the engine as SQL")
                                    .fix = Some("write ? in the SQL and pass the values as the second argument".into());
                            }
                            if let [(_, Tok::Str { text: sql, .. })] = arg {
                                for problem in super::sql::write_problems(sql) {
                                    ctx.push(
                                        RuleId::PV303,
                                        rel,
                                        line(*offset),
                                        problem.message.clone(),
                                    )
                                    .fix = Some(problem.fix.clone());
                                }
                                for problem in super::sql::arithmetic_problems(sql, columns) {
                                    ctx.push(
                                        RuleId::PV308,
                                        rel,
                                        line(*offset),
                                        problem.message.clone(),
                                    )
                                    .fix = Some(problem.fix.clone());
                                }
                            }
                        }
                        if is_call
                            && matches!(method.as_str(), "put" | "del" | "append")
                            && let Some(count) = writes.last_mut()
                        {
                            *count += 1;
                            if *count == 2 {
                                ctx.push(RuleId::PV306, rel, line(*offset), "a second pv.put/pv.del/pv.append in the same block lands as its own batch; if the two must land together, send them in one pv.append([...])")
                                    .fix = Some("pv.append([{ op:'put', … }, { op:'del', … }])".into());
                            }
                        }
                    }
                }
                if name == "importScripts"
                    && matches!(next, Some(Tok::Punct(p)) if p == "(")
                    && let Some((_, Tok::Str { text: url, .. })) = toks.get(i + 2)
                    && let Some(origin) = origin_of(url)
                {
                    ctx.push(
                        RuleId::PV504,
                        rel,
                        line(*offset),
                        format!("importScripts from {origin}"),
                    )
                    .fix = Some("vendor it under web/vendor/".into());
                }
            }
            Tok::Punct(p) if p == "+" => {
                // Unary plus: `+row.copay` after `(`, `,`, `=`, `return`, `:`.
                let unary = matches!(prev, None | Some(Tok::Punct(_)))
                    || matches!(prev, Some(Tok::Ident(i)) if i == "return");
                if unary && !matches!(prev, Some(Tok::Punct(p)) if p == ")" || p == "]") {
                    let operand = argument(&toks, i + 1);
                    let head: Vec<&(usize, Tok)> = operand
                        .iter()
                        .take_while(|(_, t)| {
                            matches!(t, Tok::Ident(_))
                                || matches!(t, Tok::Punct(p) if p == "." || p == "?.")
                        })
                        .collect();
                    if let Some((_, Tok::Ident(col))) = head.last()
                        && head.len() >= 3
                        && (columns.is_decimal(col) || columns.is_integer(col))
                    {
                        ctx.push(RuleId::PV302, rel, line(*offset), format!("unary + on `{col}`, a DECIMAL or BIGINT column that arrives as a string"))
                            .fix = Some("keep it a string; a decimal library or integer cents for arithmetic".into());
                    }
                }
            }
            Tok::Str { text: s, .. } => {
                let at = line(*offset);
                if let Some((target, beneath)) = mount_path(s) {
                    let own = target == slug;
                    let finding = ctx.push(
                        RuleId::PV301,
                        rel,
                        at,
                        format!("literal mount path '/a/{target}/' breaks solo mode"),
                    );
                    if own {
                        finding.fix = Some(format!("pv.url('{beneath}')"));
                        if let Some(path) = edit_path {
                            finding.edit = Some(Edit {
                                file: path.to_path_buf(),
                                start: *offset,
                                end: *offset + s.len() + 2,
                                replacement: format!("pv.url('{beneath}')"),
                            });
                        }
                    } else {
                        finding.fix = Some("another app has no mount in solo mode".into());
                    }
                }
                if is_absolute_fs_path(s) {
                    ctx.push(RuleId::PV505, rel, at, format!("absolute filesystem path {s:?} in browser code — nothing beside the binary or on the owner's disk is the app's"))
                        .fix = Some("everything an app stores is an event".into());
                }
                if way_back && is_way_back_path(s) {
                    ctx.push(RuleId::PV408, rel, at, format!("{s:?} climbs out of the mount to the launcher — a second way back the bar already provides"))
                        .fix = Some("drop the link, or set [ui] chrome = \"none\" and keep your own".into());
                }
                if let Some(origin) = origin_of(s) {
                    let imported =
                        matches!(prev, Some(Tok::Ident(i)) if i == "from" || i == "import");
                    if imported {
                        ctx.push(RuleId::PV504, rel, at, format!("import from {origin} — a CDN is a third party on the critical path, an offline failure and an IP leak"))
                            .fix = Some("vendor the module under web/vendor/".into());
                    }
                    if !remote.iter().any(|r| r == &origin) {
                        ctx.push(RuleId::PV207, rel, at, format!("{origin} is referenced but not declared in permissions.remote; connect-src blocks it silently"))
                            .fix = Some("keep the app on its own origin, or declare the origin in [permissions] remote with a comment saying why".into());
                    }
                }
            }
            _ => {}
        }
    }
}

/// A `x.col` chain in `toks` whose column is DECIMAL or BIGINT.
fn column_in(toks: &[(usize, Tok)], columns: &Columns) -> Option<String> {
    for (i, (_, tok)) in toks.iter().enumerate() {
        if let Tok::Ident(name) = tok
            && i > 0
            && matches!(&toks[i - 1].1, Tok::Punct(p) if p == "." || p == "?.")
            && (columns.is_decimal(name) || columns.is_integer(name))
        {
            return Some(name.clone());
        }
        if let Tok::Str { text, .. } = tok
            && i > 0
            && matches!(&toks[i - 1].1, Tok::Punct(p) if p == "[")
            && (columns.is_decimal(text) || columns.is_integer(text))
        {
            return Some(text.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// CSS
// ---------------------------------------------------------------------------------------

/// `PV406` and the origin rules over a stylesheet.
fn check_css(ctx: &mut Ctx<'_>, rel: &str, text: &str) {
    let remote: Vec<String> = ctx
        .manifest
        .as_ref()
        .map(|m| m.permissions.remote.clone())
        .unwrap_or_default();
    // Origins in url() and @import.
    let lower = text.to_ascii_lowercase();
    for needle in ["url(", "@import "] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(needle) {
            let start = from + at + needle.len();
            let value = text[start..].trim_start_matches(['"', '\'', ' ']);
            if let Some(origin) = origin_of(value) {
                let line = line_of(text, start);
                if needle == "@import " {
                    ctx.push(RuleId::PV504, rel, line, format!("@import from {origin}"))
                        .fix = Some("vendor the stylesheet".into());
                }
                if !remote.iter().any(|r| r == &origin) {
                    ctx.push(RuleId::PV207, rel, line, format!("{origin} is referenced but not declared in permissions.remote; the CSP blocks it silently"))
                        .fix = Some("vendor the resource".into());
                }
            }
            from = start;
        }
    }

    let schemes = css::root_tokens(text);
    let rules = css::rules(text);
    let token_line = |name: &str| -> u32 {
        text.find(&format!("{name}:"))
            .map_or(0, |at| line_of(text, at))
    };
    // Paired tokens by stem: --x-fg with --x-bg, --fg with --bg, --text with --background.
    for tokens in &schemes {
        for (name, value) in tokens {
            let Some(stem) = ["-fg", "-text", "-ink", "-color", "-foreground"]
                .iter()
                .find_map(|s| name.strip_suffix(s))
                .or_else(|| (name == "--fg" || name == "--text").then_some("-"))
            else {
                continue;
            };
            let candidates = if stem == "-" {
                vec!["--bg".to_owned(), "--background".to_owned()]
            } else {
                vec![
                    format!("{stem}-bg"),
                    format!("{stem}-background"),
                    format!("{stem}-surface"),
                ]
            };
            for candidate in candidates {
                if let Some(bg) = tokens.get(&candidate)
                    && let Some(ratio) = css::contrast(value, bg)
                    && ratio < 4.5
                {
                    ctx.push(RuleId::PV406, rel, token_line(name), format!("{name} on {candidate} is {ratio:.2}:1; body text needs 4.5:1"))
                        .fix = Some("darken the text or lighten the background until the pair clears 4.5:1 in both schemes".into());
                }
            }
        }
    }
    let first = schemes.first().cloned().unwrap_or_default();
    let body_bg: Option<String> = rules
        .iter()
        .find(|r| r.selector == "body" || r.selector == "html")
        .and_then(|r| {
            r.declarations
                .get("background")
                .or_else(|| r.declarations.get("background-color"))
        })
        .and_then(|v| css::colour_in(v, &first).map(str::to_owned))
        .or_else(|| {
            ["--bg", "--pv-bg", "--background"]
                .iter()
                .find_map(|t| first.get(*t).cloned())
        });
    for rule in &rules {
        let background = rule
            .declarations
            .get("background")
            .or_else(|| rule.declarations.get("background-color"))
            .and_then(|v| css::colour_in(v, &first).map(str::to_owned))
            .or_else(|| body_bg.clone());
        let Some(background) = background else {
            continue;
        };
        if let Some(color) = rule
            .declarations
            .get("color")
            .and_then(|v| css::colour_in(v, &first))
            && let Some(ratio) = css::contrast(color, &background)
            && ratio < 4.5
        {
            ctx.push(
                RuleId::PV406,
                rel,
                rule.line,
                format!(
                    "`{}`: text {color} on {background} is {ratio:.2}:1; body text needs 4.5:1",
                    rule.selector
                ),
            )
            .fix = Some(
                "raise the contrast to 4.5:1, or 3:1 for large text (24px, or 19px bold)".into(),
            );
        }
        for property in ["outline", "border", "outline-color", "border-color"] {
            if let Some(colour) = rule
                .declarations
                .get(property)
                .and_then(|v| css::colour_in(v, &first))
                && let Some(ratio) = css::contrast(colour, &background)
                && ratio < 3.0
                && rule.selector.contains(":focus")
            {
                ctx.push(RuleId::PV406, rel, rule.line, format!("`{}`: {property} {colour} on {background} is {ratio:.2}:1; a focus ring needs 3:1", rule.selector))
                    .fix = Some("draw the focus ring in the scheme's accent".into());
            }
        }
        if let Some(outline) = rule.declarations.get("outline")
            && (outline.starts_with("none") || outline.trim() == "0")
            && !rule.declarations.contains_key("box-shadow")
            && !rule.declarations.contains_key("border")
        {
            ctx.push(
                RuleId::PV406,
                rel,
                rule.line,
                format!(
                    "`{}` removes the outline with no replacement; the focus indicator is at 1:1",
                    rule.selector
                ),
            )
            .fix = Some("keep an outline, or a box-shadow ring at 3:1".into());
        }
    }
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lexer_reads_strings_templates_comments_and_regexes() {
        let toks = tokens(
            "const a = 'x\\'y'; // c\nlet b = `t ${a}`; /* d */ x = /re[/]g/i.test(s) / 2; import { pv } from '/static/pv.js';",
        );
        let strings: Vec<(String, bool)> = toks
            .iter()
            .filter_map(|(_, t)| match t {
                Tok::Str { text, interpolated } => Some((text.clone(), *interpolated)),
                _ => None,
            })
            .collect();
        assert_eq!(strings[0], ("x\\'y".into(), false));
        assert_eq!(strings[1], ("t ${a}".into(), true));
        assert_eq!(strings[2], (String::new(), false), "the regex");
        assert_eq!(strings[3], ("/static/pv.js".into(), false));
        assert!(
            toks.iter()
                .any(|(_, t)| matches!(t, Tok::Punct(p) if p == "/")),
            "the division survived"
        );
    }
}
