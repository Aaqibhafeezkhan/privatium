// This file is part of Privatium
// crates/privatium-core/src/lint/template.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: The template rules over the compiler's own front end: each views/*.lsp is scanned into
//          segments (PV202 is every raw tag), compiled to the chunk the host runs, and that
//          chunk is parsed with full_moon — so an `if` in the template is an `If` in the tree,
//          a loop a loop, and the author's Lua gets the Lua rules through the line map. The
//          HTML between tags is parsed line-aligned with the .lsp for the element rules
//          (PV401–403, 405, 407) and PV204; PV404 is judged over the page as rendered — a view
//          with its partials in the frame, or the document a layout() owns — with each branch
//          a state of the page, never a file on its own (spec/cli.md §5.1).
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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use full_moon::ast::{
    Block, Call, Expression, FunctionArgs, Index, LastStmt, Prefix, Stmt, Suffix, Var,
};
use full_moon::node::Node as _;

use crate::lint::{Ctx, Edit, RuleId, html, line_of, lua, mount_path, origin_of};
use crate::lua::lsp::{self, SegmentKind};

const VIEWS: &str = "views";

/// One view's shape and what it names.
#[derive(Debug, Clone, Default)]
struct View {
    /// `views/<name>.lsp`.
    rel: String,
    /// The rendered shape: text, emits, branches, loops.
    shape: Vec<Item>,
    /// Partials this view includes with `render('<name>')`.
    includes: BTreeSet<String>,
    /// The layout it asks for with `layout('<name>')`.
    layout: Option<String>,
}

/// A piece of a page as rendered, from the compiled chunk's tree.
#[derive(Debug, Clone)]
enum Item {
    /// Literal HTML, with the `.lsp` line it starts on.
    Text(String, u32),
    /// `render('<name>')` — the partial's shape belongs here.
    Partial(String),
    /// `content` — where a layout places the view.
    Content,
    /// Each alternative of an `if`, the implicit empty `else` included.
    Branch(Vec<Vec<Item>>),
    /// A loop body: rendered any number of times.
    Loop(Vec<Item>),
}

/// Every `views/*.lsp`: the file rules, then the page rules over all of them.
pub(crate) fn check(ctx: &mut Ctx<'_>, facts: &lua::Facts) {
    let dir = ctx.dir.join(VIEWS);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".lsp").map(str::to_owned)
        })
        .collect();
    names.sort();
    let env = lua::Env {
        slug: ctx.slug(),
        columns: crate::lint::Columns::of(ctx.schema.as_ref()),
        template: true,
    };
    let mut views: BTreeMap<String, View> = BTreeMap::new();
    for name in names {
        let rel = format!("{VIEWS}/{name}.lsp");
        let Some(text) = ctx.read(&rel) else {
            continue;
        };
        if let Some(view) = check_file(ctx, &rel, &text, &env) {
            views.insert(name, view);
        }
    }
    check_pages(ctx, &views, facts);
}

/// One template: segments, the chunk, the tree, the element rules.
fn check_file(ctx: &mut Ctx<'_>, rel: &str, text: &str, env: &lua::Env) -> Option<View> {
    let segments = match lsp::scan(text) {
        Ok(segments) => segments,
        Err(error) => {
            ctx.push(
                RuleId::PV105,
                rel,
                error.line,
                format!("template: {}", error.message),
            );
            return None;
        }
    };
    for segment in &segments {
        if let SegmentKind::Raw(body) = &segment.kind {
            ctx.push(
                RuleId::PV202,
                rel,
                segment.line,
                format!(
                    "<?raw {} ?> emits unescaped markup; review that the value is trusted",
                    body.trim()
                ),
            )
            .fix = Some(
                "<?= ?> escapes, and passes icon(), csrf() and render() through as HTML values"
                    .into(),
            );
        }
    }
    let compiled = lsp::compile(text).ok()?;
    let ast = match full_moon::parse(&compiled.lua) {
        Ok(ast) => ast,
        Err(errors) => {
            for error in errors {
                let generated = error.range().0.line() as u32;
                let line = compiled.map.source_line(generated).unwrap_or(generated);
                ctx.push(
                    RuleId::PV105,
                    rel,
                    line,
                    format!("template Lua does not parse: {}", error.error_message()),
                );
            }
            return None;
        }
    };
    let map = |generated: u32| compiled.map.source_line(generated).unwrap_or(generated);
    let (pending, _) = lua::walk(&ast, env);
    lua::attach(ctx, rel, pending, &map);

    // The HTML skeleton, line-aligned with the source.
    let synthesized = synthesize(&segments);
    let root = html::parse(&synthesized);
    for finding in html::element_findings(&root) {
        ctx.push(finding.rule, rel, finding.line, finding.message);
    }
    check_forms(ctx, rel, &root);
    check_attributes(ctx, rel, text, &segments);
    fix_focusable(ctx, rel, text);

    // The shape, from the render function's block.
    let mut view = View {
        rel: rel.to_owned(),
        ..View::default()
    };
    if let Some(LastStmt::Return(r)) = ast.nodes().last_stmt()
        && let Some(Expression::Function(function)) = r.returns().iter().next()
    {
        view.shape = shape_of(function.body().block(), &map, &mut view);
    }
    Some(view)
}

/// The HTML a template's literal text makes, each tag replaced by what the host would
/// emit — a decorative or labelled `<svg>` for `icon()`, the hidden field for `csrf()`,
/// nothing for a partial or `content`, a word for any other value — with every newline
/// of the tag kept, so a line of the synthesized document is the line of the `.lsp`.
fn synthesize(segments: &[lsp::Segment]) -> String {
    let mut out = String::new();
    for segment in segments {
        match &segment.kind {
            SegmentKind::Text(text) => out.push_str(text),
            SegmentKind::Comment(body) | SegmentKind::Code(body) => {
                out.extend(body.chars().filter(|c| *c == '\n'));
            }
            SegmentKind::Emit(body) | SegmentKind::Raw(body) => {
                out.push_str(&emit_html(body));
                out.extend(body.chars().filter(|c| *c == '\n'));
            }
        }
    }
    out
}

/// What one `<?= expr ?>` stands for in the skeleton.
fn emit_html(body: &str) -> String {
    let Ok(ast) = full_moon::parse(&format!("return {}", body.trim())) else {
        return "x".into();
    };
    let Some(LastStmt::Return(r)) = ast.nodes().last_stmt() else {
        return "x".into();
    };
    let Some(expression) = r.returns().iter().next() else {
        return String::new();
    };
    if let Some((path, args)) = lua::callee_of(expression) {
        match path.join(".").as_str() {
            "icon" => {
                return match args.get(1) {
                    Some(Some(label)) => format!(
                        "<svg class=\"pv-icon\" role=\"img\" focusable=\"false\"><title>{label}</title></svg>"
                    ),
                    Some(None) => "<svg class=\"pv-icon\" role=\"img\" focusable=\"false\"><title>x</title></svg>".into(),
                    None => "<svg class=\"pv-icon\" aria-hidden=\"true\" focusable=\"false\"></svg>".into(),
                };
            }
            "csrf" => return "<input type=\"hidden\" name=\"_csrf\" value=\"x\">".into(),
            // `render` places a partial the page walk follows separately; `menu` adds a
            // link to the frame's menu and emits nothing into the page.
            "render" | "menu" => return String::new(),
            _ => {}
        }
    }
    if lua::name_of(expression).as_deref() == Some("content") {
        return String::new();
    }
    "x".into()
}

/// `PV204`: a form that is not a GET carries the token.
fn check_forms(ctx: &mut Ctx<'_>, rel: &str, root: &html::Element) {
    for form in root.find_all("form") {
        let method = form
            .attr("method")
            .unwrap_or("get")
            .trim()
            .to_ascii_lowercase();
        let htmx_write = ["hx-post", "hx-put", "hx-patch", "hx-delete"]
            .iter()
            .any(|a| form.attr(a).is_some());
        if method == "get" && !htmx_write {
            continue;
        }
        let has_token = form
            .find_all("input")
            .iter()
            .any(|input| input.attr("name") == Some("_csrf"));
        if !has_token {
            ctx.push(
                RuleId::PV204,
                rel,
                form.line,
                format!(
                    "non-GET form without csrf(): {} — the host answers 403 without the token",
                    form.describe()
                ),
            )
            .fix = Some("put <?= csrf() ?> inside the form".into());
        }
    }
}

/// `PV301`, `PV207`, `PV504` over the attribute values of literal text, with the edit
/// `--fix` makes for a mount path (`spec/cli.md §5.3`).
fn check_attributes(ctx: &mut Ctx<'_>, rel: &str, text: &str, segments: &[lsp::Segment]) {
    let slug = ctx.slug();
    let remote: Vec<String> = ctx
        .manifest
        .as_ref()
        .map(|m| m.permissions.remote.clone())
        .unwrap_or_default();
    let path = ctx.path(rel);
    let mut offset = 0usize;
    for segment in segments {
        let len = segment_len(segment);
        if let SegmentKind::Text(body) = &segment.kind {
            for attr in attributes_in(body) {
                let value_start = offset + attr.value_offset;
                let line = crate::lint::line_of(text, value_start);
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
                        finding.fix = Some(format!("{}=\"<?= url('{beneath}') ?>\"", attr.name));
                        finding.edit = Some(Edit {
                            file: path.clone(),
                            start: value_start,
                            end: value_start + attr.value.len(),
                            replacement: format!("<?= url('{beneath}') ?>"),
                        });
                    } else {
                        finding.fix = Some(
                            "another app has no mount in solo mode; link through the launcher"
                                .into(),
                        );
                    }
                }
                if let Some(origin) = origin_of(&attr.value) {
                    let resource = matches!(attr.name.as_str(), "src" | "srcset" | "poster")
                        || (attr.name == "href" && attr.tag == "link");
                    if resource {
                        ctx.push(
                            RuleId::PV504,
                            rel,
                            line,
                            format!("{} loaded from {origin} — a CDN is a third party on the critical path, an offline failure and an IP leak", attr.tag),
                        )
                        .fix = Some("vendor the file under static/ (Tier 1) or web/vendor/ (Tier 2)".into());
                    }
                    if resource && !remote.iter().any(|r| r == &origin) {
                        ctx.push(
                            RuleId::PV207,
                            rel,
                            line,
                            format!("{origin} is referenced but not declared in permissions.remote; the CSP blocks it silently"),
                        )
                        .fix = Some("vendor it, or declare the origin in [permissions] remote with a comment saying why".into());
                    }
                }
            }
        }
        offset += len;
    }
}

/// `PV401`'s one mechanical fix: an inline `<svg` in the literal text without
/// `focusable="false"` gets it (`spec/cli.md §5.3`).
fn fix_focusable(ctx: &mut Ctx<'_>, rel: &str, text: &str) {
    let path = ctx.path(rel);
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<svg") {
        let start = from + at;
        let Some(end) = lower[start..].find('>') else {
            break;
        };
        let tag = &text[start..start + end];
        if !tag.contains("focusable") {
            let line = crate::lint::line_of(text, start);
            let finding = ctx.findings.iter_mut().find(|f| {
                f.id == RuleId::PV401 && f.line == line && f.message.contains("focusable")
            });
            if let Some(finding) = finding {
                finding.fix = Some("focusable=\"false\"".into());
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

/// How many bytes of the source a segment spans, tags included.
fn segment_len(segment: &lsp::Segment) -> usize {
    match &segment.kind {
        SegmentKind::Text(t) => t.len(),
        SegmentKind::Comment(b) => b.len() + "<?--".len() + "--?>".len(),
        SegmentKind::Code(b) => b.len() + "<?".len() + "?>".len(),
        SegmentKind::Emit(b) => b.len() + "<?=".len() + "?>".len(),
        SegmentKind::Raw(b) => b.len() + "<?raw".len() + "?>".len(),
    }
}

/// One attribute of literal text.
pub(crate) struct Attribute {
    pub tag: String,
    pub name: String,
    pub value: String,
    /// Byte offset of the value's first character within the text.
    pub value_offset: usize,
}

/// The quoted attributes of `text`'s tags, in order. Text that begins inside a tag —
/// after an emit split it — attributes them to an unknown tag.
pub(crate) fn attributes_in(text: &str) -> Vec<Attribute> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut tag = String::from("?");
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
                j += 1;
            }
            if j > i + 1 {
                tag = text[i + 1..j].to_ascii_lowercase();
            }
            i = j;
            continue;
        }
        if bytes[i] == b'>' {
            tag = "?".into();
            i += 1;
            continue;
        }
        if bytes[i] == b'='
            && i > 0
            && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-')
        {
            let mut k = i;
            while k > 0
                && (bytes[k - 1].is_ascii_alphanumeric()
                    || bytes[k - 1] == b'-'
                    || bytes[k - 1] == b':')
            {
                k -= 1;
            }
            let name = text[k..i].to_ascii_lowercase();
            let mut v = i + 1;
            while v < bytes.len() && bytes[v].is_ascii_whitespace() {
                v += 1;
            }
            if v < bytes.len() && (bytes[v] == b'"' || bytes[v] == b'\'') {
                let quote = bytes[v];
                let start = v + 1;
                let mut e = start;
                while e < bytes.len() && bytes[e] != quote {
                    e += 1;
                }
                out.push(Attribute {
                    tag: tag.clone(),
                    name,
                    value: text[start..e.min(bytes.len())].to_owned(),
                    value_offset: start,
                });
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The shape of a block of the compiled chunk.
fn shape_of(block: &Block, map: &dyn Fn(u32) -> u32, view: &mut View) -> Vec<Item> {
    let mut items = Vec::new();
    for stmt in block.stmts() {
        let at = map(stmt.start_position().map_or(0, |p| p.line() as u32));
        match stmt {
            Stmt::Assignment(assignment) => {
                let target = assignment.variables().iter().next();
                let is_buffer = matches!(target, Some(Var::Expression(v)) if matches!(v.prefix(), Prefix::Name(t) if t.token().to_string() == "__b"));
                if !is_buffer {
                    continue;
                }
                let Some(value) = assignment.expressions().iter().next() else {
                    continue;
                };
                match value {
                    Expression::String(token) => {
                        if let Some(text) = lua::decode_literal(token) {
                            items.push(Item::Text(text, at));
                        }
                    }
                    Expression::FunctionCall(call) => {
                        // `__esc(expr)` / `__str(expr)`: look at the argument.
                        let inner = call.suffixes().find_map(|s| match s {
                            Suffix::Call(Call::AnonymousCall(FunctionArgs::Parentheses {
                                arguments,
                                ..
                            })) => arguments.iter().next(),
                            _ => None,
                        });
                        if let Some(inner) = inner {
                            if let Some((path, args)) = lua::callee_of(inner)
                                && path.join(".") == "render"
                            {
                                if let Some(Some(name)) = args.first() {
                                    view.includes.insert(name.clone());
                                    items.push(Item::Partial(name.clone()));
                                }
                                continue;
                            }
                            if lua::name_of(inner).as_deref() == Some("content") {
                                items.push(Item::Content);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Stmt::FunctionCall(call) => {
                let suffixes: Vec<&Suffix> = call.suffixes().collect();
                if let Prefix::Name(token) = call.prefix()
                    && token.token().to_string() == "layout"
                    && let Some(Suffix::Call(Call::AnonymousCall(args))) = suffixes.first()
                {
                    let name = match args {
                        FunctionArgs::String(t) => Some(t),
                        FunctionArgs::Parentheses { arguments, .. } => {
                            match arguments.iter().next() {
                                Some(Expression::String(t)) => Some(t),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(token) = name
                        && let full_moon::tokenizer::TokenType::StringLiteral { literal, .. } =
                            token.token().token_type()
                    {
                        view.layout = Some(literal.to_string());
                    }
                }
            }
            Stmt::If(i) => {
                let mut branches = vec![shape_of(i.block(), map, view)];
                if let Some(else_ifs) = i.else_if() {
                    for e in else_ifs {
                        branches.push(shape_of(e.block(), map, view));
                    }
                }
                branches.push(match i.else_block() {
                    Some(b) => shape_of(b, map, view),
                    None => Vec::new(),
                });
                items.push(Item::Branch(branches));
            }
            Stmt::NumericFor(f) => items.push(Item::Loop(shape_of(f.block(), map, view))),
            Stmt::GenericFor(f) => items.push(Item::Loop(shape_of(f.block(), map, view))),
            Stmt::While(w) => items.push(Item::Loop(shape_of(w.block(), map, view))),
            Stmt::Repeat(r) => items.extend(shape_of(r.block(), map, view)),
            Stmt::Do(d) => items.extend(shape_of(d.block(), map, view)),
            _ => {}
        }
    }
    let _ = Index::Dot {
        dot: full_moon::tokenizer::TokenReference::symbol(".").unwrap_or_else(|_| unreachable!()),
        name: full_moon::tokenizer::TokenReference::symbol(".").unwrap_or_else(|_| unreachable!()),
    };
    items
}

/// The heading open tags in literal text: `(level, line)`.
fn headings_in(text: &str, line: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while let Some(at) = lower[i..].find("<h") {
        let start = i + at;
        let level = bytes.get(start + 2).and_then(|b| (*b as char).to_digit(10));
        let ends = bytes
            .get(start + 3)
            .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/');
        if let Some(level) = level
            && (1..=6).contains(&level)
            && ends
        {
            let at_line = line + text[..start].bytes().filter(|b| *b == b'\n').count() as u32;
            out.push((level, at_line));
        }
        i = start + 2;
    }
    out
}

/// `(min, max)` of `<h1>` over every state of a shape, `max` saturating at 2.
fn h1_bounds(items: &[Item], views: &BTreeMap<String, View>, seen: &mut Vec<String>) -> (u32, u32) {
    let mut min = 0;
    let mut max = 0;
    for item in items {
        let (lo, hi) = match item {
            Item::Text(text, line) => {
                let n = headings_in(text, *line)
                    .iter()
                    .filter(|(l, _)| *l == 1)
                    .count() as u32;
                (n, n)
            }
            Item::Partial(name) => match views.get(name) {
                Some(view) if !seen.contains(name) => {
                    seen.push(name.clone());
                    let bounds = h1_bounds(&view.shape, views, seen);
                    seen.pop();
                    bounds
                }
                _ => (0, 0),
            },
            Item::Content => (0, 0),
            Item::Branch(branches) => {
                let bounds: Vec<(u32, u32)> =
                    branches.iter().map(|b| h1_bounds(b, views, seen)).collect();
                (
                    bounds.iter().map(|b| b.0).min().unwrap_or(0),
                    bounds.iter().map(|b| b.1).max().unwrap_or(0),
                )
            }
            Item::Loop(body) => {
                let (_, hi) = h1_bounds(body, views, seen);
                (0, if hi > 0 { 2 } else { 0 })
            }
        };
        min = (min + lo).min(2);
        max = (max + hi).min(2);
    }
    (min, max)
}

/// The heading-order check as an abstract interpretation over branches: the set of
/// levels the previous heading may have had, `None` for "no heading yet".
fn level_skips(
    items: &[Item],
    state: BTreeSet<Option<u32>>,
    views: &BTreeMap<String, View>,
    seen: &mut Vec<String>,
    findings: &mut Vec<(u32, u32, u32)>,
) -> BTreeSet<Option<u32>> {
    let mut state = state;
    for item in items {
        match item {
            Item::Text(text, line) => {
                for (level, at) in headings_in(text, *line) {
                    for previous in &state {
                        if let Some(previous) = previous
                            && level > previous + 1
                        {
                            findings.push((*previous, level, at));
                        } else if previous.is_none() && level > 1 {
                            findings.push((0, level, at));
                        }
                    }
                    state = BTreeSet::from([Some(level)]);
                }
            }
            Item::Partial(name) => {
                if let Some(view) = views.get(name)
                    && !seen.contains(name)
                {
                    seen.push(name.clone());
                    state = level_skips(&view.shape, state, views, seen, findings);
                    seen.pop();
                }
            }
            Item::Content => {}
            Item::Branch(branches) => {
                let mut merged = BTreeSet::new();
                for branch in branches {
                    merged.extend(level_skips(branch, state.clone(), views, seen, findings));
                }
                state = merged;
            }
            Item::Loop(body) => {
                let once = level_skips(body, state.clone(), views, seen, findings);
                let mut merged = state.clone();
                merged.extend(once.clone());
                let twice = level_skips(body, merged.clone(), views, seen, &mut Vec::new());
                merged.extend(twice);
                state = merged;
            }
        }
    }
    state
}

/// `PV404` over the pages: which views are pages, what each renders, one `<h1>` in
/// every state and no level skipped.
fn check_pages(ctx: &mut Ctx<'_>, views: &BTreeMap<String, View>, facts: &lua::Facts) {
    let included: BTreeSet<&String> = views.values().flat_map(|v| v.includes.iter()).collect();
    let layouts: BTreeSet<&String> = views.values().filter_map(|v| v.layout.as_ref()).collect();
    let chrome = ctx
        .manifest
        .as_ref()
        .is_none_or(|m| m.ui.chrome == crate::app::manifest::Chrome::Standard);
    // PV109: a document a layout() owns takes the chrome at its anchors unless the
    // manifest declined it (`spec/app-contract.md §5`). Its literal text, every branch
    // included, is what is asked for the tags; what a view emits cannot supply them.
    if chrome {
        for layout in &layouts {
            if let Some(document) = views.get(*layout) {
                let text = literal_text(&document.shape);
                crate::lint::check_chrome_anchors(ctx, &document.rel, &text);
            }
        }
    }
    // PV408: a framed view always has the bar, and a layout's document has it under the
    // standard chrome, so neither draws its own link to the launcher or settings.
    for view in views.values() {
        let name = view
            .rel
            .trim_start_matches("views/")
            .trim_end_matches(".lsp")
            .to_owned();
        let owns = layouts.contains(&name);
        if owns && !chrome {
            continue;
        }
        for (text, line) in literal_chunks(&view.shape) {
            for attr in attributes_in(&text) {
                if attr.tag == "a"
                    && attr.name == "href"
                    && crate::lint::is_way_back_href(&attr.value)
                {
                    ctx.push(
                        RuleId::PV408,
                        &view.rel,
                        line + line_of(&text, attr.value_offset) - 1,
                        format!("<a href=\"{}\"> is a second way back — the bar already carries the Privatium mark, the Apps link and the Menu", attr.value),
                    )
                    .fix = Some("drop the link; the frame's bar leads there".into());
                }
            }
        }
    }
    for (name, view) in views {
        let rendered = facts.rendered.contains(name);
        let is_partial = included.contains(name) || layouts.contains(name);
        // A view rendered directly and also included is a fragment answering htmx: it is
        // judged where it lands. A view neither rendered nor included is judged as a
        // page, since it is reached by a name the linter cannot see.
        if is_partial || (!rendered && name.starts_with('_')) {
            continue;
        }
        let mut shape = view.shape.clone();
        if let Some(layout) = &view.layout
            && let Some(document) = views.get(layout)
        {
            shape = place_content(&document.shape, &view.shape);
        }
        let mut seen = vec![name.clone()];
        let (min, max) = h1_bounds(&shape, views, &mut seen);
        if min == 0 {
            ctx.push(
                RuleId::PV404,
                &view.rel,
                1,
                "some state of this page renders no <h1> — the empty state included, every branch supplies the page's one heading",
            )
            .fix = Some("give each branch its <h1>, or hoist one above the branches".into());
        }
        if max > 1 {
            ctx.push(
                RuleId::PV404,
                &view.rel,
                1,
                "some state of this page renders more than one <h1> — a page with its partials carries exactly one",
            )
            .fix = Some("demote the second heading to <h2>, or move the <h1> into the branch that owns the page".into());
        }
        let mut skips = Vec::new();
        level_skips(&shape, BTreeSet::from([None]), views, &mut seen, &mut skips);
        skips.sort_unstable();
        skips.dedup();
        for (previous, level, line) in skips {
            let message = if previous == 0 {
                format!("the first heading is <h{level}>, not <h1>")
            } else {
                format!("<h{previous}> is followed by <h{level}>; levels do not skip")
            };
            ctx.push(RuleId::PV404, &view.rel, line, message).fix =
                Some(format!("<h{}>", previous + 1));
        }
    }
}

/// Every literal chunk of a shape in document order, each with the `.lsp` line it starts
/// on; branches and loops contribute every alternative, partials nothing.
fn literal_chunks(items: &[Item]) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Text(text, line) => out.push((text.clone(), *line)),
            Item::Branch(alternatives) => {
                for alternative in alternatives {
                    out.extend(literal_chunks(alternative));
                }
            }
            Item::Loop(body) => out.extend(literal_chunks(body)),
            Item::Partial(_) | Item::Content => {}
        }
    }
    out
}

/// The literal text of a shape, joined in document order.
fn literal_text(items: &[Item]) -> String {
    literal_chunks(items)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The layout's shape with the view's in place of `content`.
fn place_content(document: &[Item], content: &[Item]) -> Vec<Item> {
    document
        .iter()
        .flat_map(|item| match item {
            Item::Content => content.to_vec(),
            Item::Branch(branches) => vec![Item::Branch(
                branches.iter().map(|b| place_content(b, content)).collect(),
            )],
            Item::Loop(body) => vec![Item::Loop(place_content(body, content))],
            other => vec![other.clone()],
        })
        .collect()
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesized_html_is_line_aligned() {
        let src = "<h1><?= title ?></h1>\n<? if x then\n ?><p><?= icon('gear') ?><?= icon('gear', 'Settings') ?></p><? end ?>\n<form method=\"post\"><?= csrf() ?></form>";
        let segments = lsp::scan(src).unwrap();
        let html = synthesize(&segments);
        assert_eq!(html.lines().count(), src.lines().count());
        let root = html::parse(&html);
        assert_eq!(root.find_all("svg").len(), 2);
        assert_eq!(root.find_all("svg")[0].attr("aria-hidden"), Some("true"));
        assert_eq!(root.find_all("svg")[1].attr("role"), Some("img"));
        assert_eq!(root.find_all("form")[0].line, 4);
        assert_eq!(root.find_all("input")[0].attr("name"), Some("_csrf"));
    }

    #[test]
    fn attributes_are_found_with_their_tags_and_offsets() {
        let text = "<a href=\"/a/meds/x\">go</a><script src='https://cdn.x.com/a.js'></script>";
        let attrs = attributes_in(text);
        assert_eq!(attrs[0].tag, "a");
        assert_eq!(attrs[0].name, "href");
        assert_eq!(
            &text[attrs[0].value_offset..attrs[0].value_offset + attrs[0].value.len()],
            "/a/meds/x"
        );
        assert_eq!(attrs[1].tag, "script");
        assert_eq!(attrs[1].value, "https://cdn.x.com/a.js");
    }

    #[test]
    fn headings_and_bounds_follow_branches_and_loops() {
        assert_eq!(
            headings_in("<h1>a</h1>\n<h2 class=\"x\">b</h2><hr><html>", 3),
            vec![(1, 3), (2, 4)]
        );
        let views = BTreeMap::new();
        let shape = vec![Item::Branch(vec![
            vec![Item::Text("<h1>a</h1>".into(), 1)],
            vec![Item::Text("<h1>b</h1>".into(), 2)],
            Vec::new(),
        ])];
        assert_eq!(h1_bounds(&shape, &views, &mut Vec::new()), (0, 1));
        let shape = vec![
            Item::Text("<h1>a</h1>".into(), 1),
            Item::Loop(vec![Item::Text("<h1>x</h1>".into(), 2)]),
        ];
        assert_eq!(h1_bounds(&shape, &views, &mut Vec::new()), (1, 2));
        let mut skips = Vec::new();
        let shape = vec![
            Item::Text("<h1>a</h1>".into(), 1),
            Item::Branch(vec![vec![Item::Text("<h2>b</h2>".into(), 2)], Vec::new()]),
            Item::Text("<h3>c</h3>".into(), 3),
        ];
        level_skips(
            &shape,
            BTreeSet::from([None]),
            &views,
            &mut Vec::new(),
            &mut skips,
        );
        assert_eq!(
            skips,
            vec![(1, 3, 3)],
            "the empty branch leaves h1 before h3"
        );
    }
}
