// This file is part of Privatium
// crates/privatium-core/src/lua/lsp.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: LSP templates (spec/lua-api.md §4). The compiler turns views/<name>.lsp — HTML with <? ?>,
//          <?= ?>, <?raw ?> and <?-- --?> — into a Lua chunk plus a line map, so a traceback
//          names the .lsp line the author wrote. The compiled source is shared by every VM of
//          an app and swapped as one snapshot when a file changes; the loaded chunk is cached
//          per VM by generation; each render runs the chunk with a fresh environment holding
//          the ctx keys and the template-only helpers render, layout, menu and csrf, falling through
//          to the request-scoped environment handlers use.
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

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::SystemTime;

use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, Table, Value};

use crate::lua::html::{self, Html};
use crate::lua::{PageMenuItem, VmData, sandbox};

// ---------------------------------------------------------------------------------------
// The compiler
// ---------------------------------------------------------------------------------------

/// A template compiled to Lua.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// The chunk: a factory that, called with `(table.concat, esc, str)`, returns the
    /// render function `function(_ENV) … end`.
    pub lua: String,
    /// Generated line → `.lsp` line.
    pub map: LineMap,
    /// The template as written, for the error page's context lines.
    pub source: String,
}

/// Where the compiler stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    /// 1-based `.lsp` line of the tag that could not be read.
    pub line: u32,
    /// What was wrong with it.
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Generated line → source line, one entry per generated line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineMap {
    source: Vec<u32>,
}

impl LineMap {
    /// The `.lsp` line a generated line came from.
    #[must_use]
    pub fn source_line(&self, generated: u32) -> Option<u32> {
        usize::try_from(generated)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| self.source.get(i).copied())
    }

    /// Every `<chunk>:<generated line>` in `message` rewritten to the `.lsp` line, so a
    /// Lua error or traceback names what the author wrote. Anything else is untouched.
    #[must_use]
    pub fn rewrite(&self, message: &str, chunk: &str) -> String {
        let mut out = String::with_capacity(message.len());
        let mut last = 0;
        let needle = format!("{chunk}:");
        for (at, _) in message.match_indices(&needle) {
            if at < last {
                continue;
            }
            let digits_start = at + needle.len();
            let digits_end = message[digits_start..]
                .find(|c: char| !c.is_ascii_digit())
                .map_or(message.len(), |n| digits_start + n);
            if digits_end == digits_start {
                continue;
            }
            let Some(mapped) = message[digits_start..digits_end]
                .parse::<u32>()
                .ok()
                .and_then(|generated| self.source_line(generated))
            else {
                continue;
            };
            out.push_str(&message[last..digits_start]);
            out.push_str(&mapped.to_string());
            last = digits_end;
        }
        out.push_str(&message[last..]);
        out
    }
}

/// Collects generated lines and the source line each began on.
struct Emitter {
    out: String,
    map: Vec<u32>,
}

impl Emitter {
    /// One generated line from source line `src`.
    fn line(&mut self, src: u32, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
        self.map.push(src);
    }

    /// Author-written Lua copied as it is, each of its lines mapped to its own; the
    /// trailing newline ends any `--` comment on its last line.
    fn verbatim(&mut self, src: u32, code: &str) {
        for (offset, piece) in code.split('\n').enumerate() {
            self.line(src.saturating_add(offset as u32), piece);
        }
    }

    /// Literal text between tags, as one Lua string on one generated line.
    fn literal(&mut self, src: u32, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut line = String::with_capacity(text.len() + 32);
        line.push_str("__n = __n + 1; __b[__n] = ");
        lua_string_into(&mut line, text);
        self.line(src, &line);
    }
}

/// `text` as a Lua string literal.
fn lua_string_into(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\{:03}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn newlines(text: &str) -> u32 {
    text.bytes().filter(|b| *b == b'\n').count() as u32
}

/// One piece of a template as the scanner reads it: the front end the compiler and the
/// linter share, so there is one reading of what a tag is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 1-based `.lsp` line the segment begins on.
    pub line: u32,
    /// What it is.
    pub kind: SegmentKind,
}

/// The kinds of segment a template is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKind {
    /// Literal text between tags, as written.
    Text(String),
    /// `<? code ?>` — Lua that emits nothing.
    Code(String),
    /// `<?= expr ?>` — emitted, escaped.
    Emit(String),
    /// `<?raw expr ?>` — emitted as it is. Every use is `PV202`.
    Raw(String),
    /// `<?-- --?>` — stripped, whatever it contains.
    Comment(String),
}

/// Split a template into its segments: the tags of `spec/lua-api.md §4`, and the text
/// between them. An unclosed tag is the one error.
pub fn scan(source: &str) -> Result<Vec<Segment>, CompileError> {
    let mut segments = Vec::new();
    let mut pos = 0;
    let mut line = 1u32;
    loop {
        let Some(open) = source[pos..].find("<?").map(|at| pos + at) else {
            if pos < source.len() {
                segments.push(Segment {
                    line,
                    kind: SegmentKind::Text(source[pos..].to_owned()),
                });
            }
            break;
        };
        if open > pos {
            segments.push(Segment {
                line,
                kind: SegmentKind::Text(source[pos..open].to_owned()),
            });
        }
        line = line.saturating_add(newlines(&source[pos..open]));
        let tag_line = line;
        let after = open + 2;
        let rest = &source[after..];

        // A comment runs to `--?>`, whatever it contains — including another tag spelled
        // out for the reader, as apps/hello/views/index.lsp's header does.
        if let Some(body) = rest.strip_prefix("--") {
            let Some(end) = body.find("--?>") else {
                return Err(CompileError {
                    line: tag_line,
                    message: "unclosed <?-- comment: no --?> follows it".to_owned(),
                });
            };
            segments.push(Segment {
                line: tag_line,
                kind: SegmentKind::Comment(body[..end].to_owned()),
            });
            line = line.saturating_add(newlines(&body[..end]));
            pos = after + 2 + end + 4;
            continue;
        }

        let (kind, body_start) = if rest.starts_with('=') {
            (Tag::Emit, after + 1)
        } else if rest.starts_with("raw")
            && rest[3..].starts_with(|c: char| c.is_ascii_whitespace())
        {
            (Tag::Raw, after + 3)
        } else {
            (Tag::Code, after)
        };
        let Some(end) = source[body_start..].find("?>").map(|at| body_start + at) else {
            return Err(CompileError {
                line: tag_line,
                message: format!("unclosed {}: no ?> follows it", kind.spelling()),
            });
        };
        let body = source[body_start..end].to_owned();
        segments.push(Segment {
            line: tag_line,
            kind: match kind {
                Tag::Code => SegmentKind::Code(body.clone()),
                Tag::Emit => SegmentKind::Emit(body.clone()),
                Tag::Raw => SegmentKind::Raw(body.clone()),
            },
        });
        line = line.saturating_add(newlines(&body));
        pos = end + 2;
    }
    Ok(segments)
}

/// Compile one template. `name` is the chunk's spelling in messages, `views/index.lsp`.
pub fn compile(source: &str) -> Result<Compiled, CompileError> {
    let segments = scan(source)?;
    let mut emitter = Emitter {
        out: String::with_capacity(source.len() + source.len() / 4 + 128),
        map: Vec::new(),
    };
    emitter.line(1, "local __concat, __esc, __str = ...");
    emitter.line(1, "return function(_ENV)");
    emitter.line(1, "local __b, __n = {}, 0");

    let mut last = 1u32;
    for segment in &segments {
        let line = segment.line;
        match &segment.kind {
            SegmentKind::Text(text) => {
                emitter.literal(line, text);
                last = line.saturating_add(newlines(text));
            }
            SegmentKind::Comment(body) => last = line.saturating_add(newlines(body)),
            SegmentKind::Code(body) => {
                emitter.verbatim(line, body);
                last = line.saturating_add(newlines(body));
            }
            SegmentKind::Emit(body) | SegmentKind::Raw(body) => {
                let helper = if matches!(segment.kind, SegmentKind::Emit(_)) {
                    "__esc"
                } else {
                    "__str"
                };
                emitter.line(line, &format!("__n = __n + 1; __b[__n] = {helper}("));
                emitter.verbatim(line, body);
                emitter.line(line.saturating_add(newlines(body)), ")");
                last = line.saturating_add(newlines(body));
            }
        }
    }
    emitter.line(last, "return __concat(__b)");
    emitter.line(last, "end");
    Ok(Compiled {
        lua: emitter.out,
        map: LineMap {
            source: emitter.map,
        },
        source: source.to_owned(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    Code,
    Emit,
    Raw,
}

impl Tag {
    fn spelling(self) -> &'static str {
        match self {
            Self::Code => "<? ?>",
            Self::Emit => "<?= ?>",
            Self::Raw => "<?raw ?>",
        }
    }
}

/// A view name: the file under `views/` without `.lsp`, spelled with letters, digits,
/// `_` and `-` — never a path.
#[must_use]
pub fn is_view_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// ---------------------------------------------------------------------------------------
// The shared cache
// ---------------------------------------------------------------------------------------

/// One template as every VM of the app sees it.
#[derive(Debug)]
pub struct CompiledView {
    /// The view name.
    pub name: String,
    /// The chunk and its map.
    pub compiled: Compiled,
    mtime: Option<SystemTime>,
    len: u64,
    /// The generation this compile belongs to; a VM's loaded chunk is keyed by it.
    pub generation: u64,
}

impl CompiledView {
    /// `views/<name>.lsp`, as messages spell it.
    #[must_use]
    pub fn chunk_name(&self) -> String {
        format!("views/{}.lsp", self.name)
    }
}

/// Every view by name — one immutable snapshot, replaced whole on a reload.
pub type ViewMap = HashMap<String, Arc<CompiledView>>;

/// A recompiled generation that is not current yet: what a reload holds while a VM
/// checks that every chunk parses, and what it publishes only then. A broken edit never
/// becomes the snapshot a request resolves against — the error page shows in its place
/// until the next save loads (`spec/cli.md §3`), and what is current stays what last
/// loaded, unserved but intact.
#[derive(Debug)]
pub struct Candidate {
    views: Arc<ViewMap>,
    stat: BTreeMap<String, Stat>,
}

impl Candidate {
    /// The views this generation would publish.
    #[must_use]
    pub fn views(&self) -> &Arc<ViewMap> {
        &self.views
    }
}

/// An app's compiled templates: the current snapshot, the generation counter that
/// invalidates the per-VM chunks, and the stat of the last edit that failed to load — so
/// the same broken files are not recompiled on every request, only the next edit.
#[derive(Debug)]
pub struct Templates {
    views_dir: PathBuf,
    current: RwLock<Arc<ViewMap>>,
    generation: AtomicU64,
    failed: RwLock<Option<BTreeMap<String, Stat>>>,
}

type Stat = (Option<SystemTime>, u64);

impl Templates {
    /// Compile every `views/*.lsp` under `app_dir`. A template that does not compile
    /// fails the app's load naming the file and line (`spec/app-contract.md §8`).
    pub fn load(app_dir: &Path) -> Result<Self, String> {
        let views_dir = app_dir.join("views");
        let views = compile_dir(&views_dir, &HashMap::new(), 1)?;
        Ok(Self {
            views_dir,
            current: RwLock::new(Arc::new(views)),
            generation: AtomicU64::new(1),
            failed: RwLock::new(None),
        })
    }

    /// The current snapshot. A run takes one and resolves every view against it.
    #[must_use]
    pub fn snapshot(&self) -> Arc<ViewMap> {
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// The generation of the current snapshot.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Whether a `views/*.lsp` appeared, vanished, or moved by `(mtime, len)` since the
    /// snapshot was compiled — or since the last edit that failed to load, which is not
    /// worth compiling again until it changes. A stat per file, no reads.
    #[must_use]
    pub fn changed(&self) -> bool {
        let Ok(on_disk) = scan_views(&self.views_dir) else {
            return true;
        };
        if self
            .failed
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|failed| *failed == on_disk)
        {
            return false;
        }
        let current = self.snapshot();
        on_disk.len() != current.len()
            || on_disk
                .iter()
                .any(|(name, stat)| current.get(name).is_none_or(|v| (v.mtime, v.len) != *stat))
    }

    /// Recompile what changed into a candidate generation, publishing nothing.
    /// Unchanged views keep their `Arc` and their generation, so a VM's loaded chunk for
    /// them stays valid. A template that does not compile is the error, and the files as
    /// they stand are remembered so [`changed`](Self::changed) waits for the next edit.
    pub fn prepare(&self) -> Result<Candidate, String> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let previous = self.snapshot();
        let stat = scan_views(&self.views_dir)?;
        match compile_dir(&self.views_dir, &previous, generation) {
            Ok(views) => Ok(Candidate {
                views: Arc::new(views),
                stat,
            }),
            Err(error) => {
                self.remember_failure(stat);
                Err(error)
            }
        }
    }

    /// Make a candidate the snapshot every later request resolves against. Only after
    /// the caller has proven it loads (`preload`); a candidate that did not is dropped
    /// and [`remember_failure`](Self::remember_failure) is told.
    pub fn publish(&self, candidate: Candidate) {
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = candidate.views;
        *self.failed.write().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// A candidate that compiled but did not load in a VM: keep serving what is current
    /// — unserved, behind the error page — and do not try these files again until they
    /// change.
    pub fn refuse(&self, candidate: Candidate) {
        self.remember_failure(candidate.stat);
    }

    fn remember_failure(&self, stat: BTreeMap<String, Stat>) {
        *self.failed.write().unwrap_or_else(PoisonError::into_inner) = Some(stat);
    }

    /// `message` with every generated line of every view rewritten to its `.lsp` line.
    #[must_use]
    pub fn rewrite(&self, message: &str) -> String {
        rewrite_with(&self.snapshot(), message)
    }
}

/// [`Templates::rewrite`] over one snapshot.
fn rewrite_with(views: &ViewMap, message: &str) -> String {
    let mut text = message.to_owned();
    for view in views.values() {
        let chunk = view.chunk_name();
        if text.contains(&chunk) {
            text = view.compiled.map.rewrite(&text, &chunk);
        }
    }
    text
}

/// `views/*.lsp` by name with their `(mtime, len)`. A missing `views/` is an app with no
/// templates, which is fine.
fn scan_views(views_dir: &Path) -> Result<BTreeMap<String, Stat>, String> {
    let mut found = BTreeMap::new();
    let entries = match fs::read_dir(views_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(format!("views/: {error}")),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("views/: {error}"))?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "lsp") || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let meta = entry
            .metadata()
            .map_err(|error| format!("views/{name}.lsp: {error}"))?;
        found.insert(name.to_owned(), (meta.modified().ok(), meta.len()));
    }
    Ok(found)
}

fn compile_dir(views_dir: &Path, previous: &ViewMap, generation: u64) -> Result<ViewMap, String> {
    let mut views = HashMap::new();
    for (name, (mtime, len)) in scan_views(views_dir)? {
        if let Some(view) = previous.get(&name)
            && (view.mtime, view.len) == (mtime, len)
        {
            views.insert(name, Arc::clone(view));
            continue;
        }
        let path = views_dir.join(format!("{name}.lsp"));
        let source =
            fs::read_to_string(&path).map_err(|error| format!("views/{name}.lsp: {error}"))?;
        let compiled = compile(&source)
            .map_err(|error| format!("views/{name}.lsp:{}: {}", error.line, error.message))?;
        views.insert(
            name.clone(),
            Arc::new(CompiledView {
                name,
                compiled,
                mtime,
                len,
                generation,
            }),
        );
    }
    Ok(views)
}

// ---------------------------------------------------------------------------------------
// The runtime: per-VM chunks, the render environment, the template-only helpers
// ---------------------------------------------------------------------------------------

/// Registry key: `name → { gen, fn }`, this VM's loaded chunks.
const VIEWS_KEY: &str = "pv.views";
const ESC_KEY: &str = "pv.tpl.esc";
const STR_KEY: &str = "pv.tpl.str";
const CONCAT_KEY: &str = "pv.tpl.concat";
const RENDER_KEY: &str = "pv.tpl.render";
const LAYOUT_KEY: &str = "pv.tpl.layout";
const MENU_KEY: &str = "pv.tpl.menu";
const CSRF_KEY: &str = "pv.tpl.csrf";
const ASSIGN_KEY: &str = "pv.tpl.assign";

/// How many partials may nest before the render is called what it is: a loop.
const MAX_DEPTH: u32 = 32;

/// Install the template runtime on a fresh state, before any app code runs.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    html::install(lua)?;
    let concat: Function = lua.globals().get::<Table>("table")?.get("concat")?;
    lua.set_named_registry_value(CONCAT_KEY, concat)?;
    lua.set_named_registry_value(VIEWS_KEY, lua.create_table()?)?;
    lua.set_named_registry_value(
        ESC_KEY,
        lua.create_function(|lua, value: Value| html::emit_escaped(lua, &value))?,
    )?;
    lua.set_named_registry_value(
        STR_KEY,
        lua.create_function(|lua, value: Value| html::emit_raw(lua, &value))?,
    )?;
    lua.set_named_registry_value(RENDER_KEY, lua.create_function(render)?)?;
    lua.set_named_registry_value(LAYOUT_KEY, lua.create_function(layout)?)?;
    lua.set_named_registry_value(MENU_KEY, lua.create_function(menu)?)?;
    lua.set_named_registry_value(CSRF_KEY, lua.create_function(csrf)?)?;
    lua.set_named_registry_value(ASSIGN_KEY, lua.create_function(sandbox::assign_global)?)?;
    Ok(())
}

/// Load every view's chunk into this VM, so a Lua syntax error the scanner cannot see —
/// `<? if x ?>` without its `then` — fails the load naming the `.lsp` line.
pub(crate) fn preload(lua: &Lua, views: &ViewMap) -> Result<(), String> {
    let mut names: Vec<&String> = views.keys().collect();
    names.sort();
    for name in names {
        let view = &views[name];
        if let Err(error) = chunk_for(lua, view) {
            let chunk = view.chunk_name();
            let text = view.compiled.map.rewrite(&error.to_string(), &chunk);
            return Err(if text.contains(&format!("{chunk}:")) {
                text
            } else {
                format!("{chunk}: {text}")
            });
        }
    }
    Ok(())
}

/// This VM's render function for `view`, loaded from the shared source when the VM has
/// none or has one from an earlier generation.
fn chunk_for(lua: &Lua, view: &CompiledView) -> mlua::Result<Function> {
    let cache: Table = lua.named_registry_value(VIEWS_KEY)?;
    if let Value::Table(entry) = cache.raw_get::<Value>(view.name.as_str())?
        && entry.raw_get::<u64>("gen")? == view.generation
    {
        return entry.raw_get("fn");
    }
    let factory = lua
        .load(view.compiled.lua.as_str())
        .set_name(format!("@{}", view.chunk_name()))
        .set_mode(ChunkMode::Text)
        .set_environment(sandbox::env(lua)?)
        .into_function()?;
    let concat: Function = lua.named_registry_value(CONCAT_KEY)?;
    let esc: Function = lua.named_registry_value(ESC_KEY)?;
    let raw: Function = lua.named_registry_value(STR_KEY)?;
    let function: Function = factory.call((concat, esc, raw))?;
    let entry = lua.create_table()?;
    entry.raw_set("gen", view.generation)?;
    entry.raw_set("fn", function.clone())?;
    cache.raw_set(view.name.as_str(), entry)?;
    Ok(function)
}

fn data_mut(lua: &Lua) -> mlua::Result<mlua::AppDataRefMut<'_, VmData>> {
    lua.app_data_mut::<VmData>()
        .ok_or_else(|| mlua::Error::runtime("pv: no app is loaded in this state"))
}

/// The environment one render runs in (`spec/lua-api.md §4.1`, §5): the ctx keys, the
/// four template-only helpers, `content` for a layout, then the request-scoped
/// environment handlers use — so `url`, `icon`, `fmt`, `t`, `pv`, the app's baseline and
/// the request's scratch are all one lookup away, and a bare assignment lands in the
/// scratch like a handler's would.
fn render_env(lua: &Lua, ctx: Option<&Table>, content: Option<Html>) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    if let Some(ctx) = ctx {
        for pair in ctx.pairs::<Value, Value>() {
            let (key, value) = pair?;
            env.raw_set(key, value)?;
        }
    }
    env.raw_set("render", lua.named_registry_value::<Function>(RENDER_KEY)?)?;
    env.raw_set("layout", lua.named_registry_value::<Function>(LAYOUT_KEY)?)?;
    env.raw_set("menu", lua.named_registry_value::<Function>(MENU_KEY)?)?;
    env.raw_set("csrf", lua.named_registry_value::<Function>(CSRF_KEY)?)?;
    if let Some(content) = content {
        env.raw_set("content", content)?;
    }
    let meta = lua.create_table()?;
    meta.raw_set("__index", sandbox::env(lua)?)?;
    meta.raw_set(
        "__newindex",
        lua.named_registry_value::<Function>(ASSIGN_KEY)?,
    )?;
    meta.raw_set("__metatable", "a template's environment")?;
    env.set_metatable(Some(meta))?;
    Ok(env)
}

/// Render `views/<name>.lsp` with `ctx` in this VM, against the run's snapshot.
fn render_view(
    lua: &Lua,
    what: &str,
    name: &str,
    ctx: Option<&Table>,
    content: Option<Html>,
) -> mlua::Result<String> {
    if !is_view_name(name) {
        return Err(mlua::Error::runtime(format!(
            "{what}: {name:?} is not a view name; views/<name>.lsp is named by letters, \
             digits, '_' and '-'"
        )));
    }
    let views = {
        let data = data_mut(lua)?;
        if data.render_depth >= MAX_DEPTH {
            return Err(mlua::Error::runtime(format!(
                "{what}: partials nest deeper than {MAX_DEPTH} — is a partial rendering itself?"
            )));
        }
        data.views
            .clone()
            .ok_or_else(|| mlua::Error::runtime(format!("{what} runs only inside a request")))?
    };
    let Some(view) = views.get(name) else {
        return Err(mlua::Error::runtime(format!(
            "{what}: views/{name}.lsp does not exist (a view is named by its file, without .lsp)"
        )));
    };
    let function = chunk_for(lua, view)?;
    let env = render_env(lua, ctx, content)?;
    // No app_data borrow is held across the call: the template calls back into Rust.
    data_mut(lua)?.render_depth += 1;
    let result = function.call::<String>(env);
    if let Some(mut data) = lua.app_data_mut::<VmData>() {
        data.render_depth = data.render_depth.saturating_sub(1);
    }
    result
}

/// `pv.render(view, ctx)` fulfilled: the body, then the layout the view asked for, if
/// any. Returns the HTML, whether the app supplied the whole document, and the menu items
/// the view added for the page.
pub(crate) fn render_response(
    lua: &Lua,
    view: &str,
    ctx: Option<Table>,
) -> mlua::Result<(String, bool, Vec<PageMenuItem>)> {
    {
        let mut data = data_mut(lua)?;
        data.render_depth = 0;
        data.layout = None;
        data.menu.clear();
    }
    let body = render_view(lua, "pv.render", view, ctx.as_ref(), None)?;
    let (layout, menu) = {
        let mut data = data_mut(lua)?;
        (data.layout.take(), std::mem::take(&mut data.menu))
    };
    match layout {
        Some(name) => {
            let html = render_view(lua, "layout", &name, ctx.as_ref(), Some(Html(body)))?;
            Ok((html, true, menu))
        }
        None => Ok((body, false, menu)),
    }
}

/// `render(name[, ctx])` — include a partial (`§4.1`).
fn render(lua: &Lua, (name, ctx): (String, Option<Table>)) -> mlua::Result<Html> {
    render_view(lua, "render", &name, ctx.as_ref(), None).map(Html)
}

/// `layout(name)` — wrap the view `pv.render` named in `views/<name>.lsp`, which sees the
/// same ctx plus `content`. For the view itself, not a partial.
fn layout(lua: &Lua, name: String) -> mlua::Result<()> {
    if !is_view_name(&name) {
        return Err(mlua::Error::runtime(format!(
            "layout: {name:?} is not a view name; views/<name>.lsp is named by letters, \
             digits, '_' and '-'"
        )));
    }
    let mut data = data_mut(lua)?;
    if data.render_depth != 1 {
        return Err(mlua::Error::runtime(
            "layout() belongs in the view pv.render named, not in a partial",
        ));
    }
    data.layout = Some(name);
    Ok(())
}

/// `menu(label, path[, icon])` — add a link to the page's menu, listed before the
/// framework's own pages for this page alone (`§4.1`). Like `layout`, it belongs to the
/// view `pv.render` named: a partial is included from several pages and cannot know
/// which page's menu it would be adding to. The path is mount-relative and the frame
/// resolves it through `url()`; the icon, when given, is a vendored icon name.
fn menu(lua: &Lua, (label, path, icon): (String, String, Option<String>)) -> mlua::Result<()> {
    let label = label.trim().to_owned();
    let chars = label.chars().count();
    if chars == 0 || chars > crate::app::manifest::MAX_MENU_LABEL_CHARS {
        return Err(mlua::Error::runtime(format!(
            "menu: the label must be 1 to {} characters, found {chars}",
            crate::app::manifest::MAX_MENU_LABEL_CHARS
        )));
    }
    if !crate::app::manifest::is_mount_relative(&path) {
        return Err(mlua::Error::runtime(format!(
            "menu: path {path:?} must be mount-relative — begin with a single / and name no \
             scheme; the frame resolves it with url()"
        )));
    }
    if let Some(icon) = &icon
        && !crate::icons::exists(icon)
    {
        return Err(mlua::Error::runtime(format!(
            "menu: icon {icon:?} is not in the vendored Bootstrap Icons set (docs/icons.md)"
        )));
    }
    let mut data = data_mut(lua)?;
    if data.render_depth != 1 {
        return Err(mlua::Error::runtime(
            "menu() belongs in the view pv.render named, not in a partial",
        ));
    }
    data.menu.push(PageMenuItem { label, path, icon });
    Ok(())
}

/// `csrf()` — the hidden `_csrf` field, bound to the app's mount (`§4.1`).
fn csrf(lua: &Lua, (): ()) -> mlua::Result<Html> {
    let token = data_mut(lua)?
        .ctx
        .as_ref()
        .map(|ctx| ctx.csrf_token.clone())
        .ok_or_else(|| mlua::Error::runtime("csrf() runs only inside a request"))?;
    Ok(Html(crate::http::csrf::field_for(&token)))
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Run a compiled template in a bare state with `ctx` as its environment.
    fn run(source: &str, ctx: &[(&str, &str)]) -> Result<String, String> {
        let lua = Lua::new();
        html::install(&lua).unwrap();
        let compiled = compile(source).map_err(|e| e.to_string())?;
        let factory = lua
            .load(compiled.lua.as_str())
            .set_name("@views/t.lsp")
            .into_function()
            .map_err(|e| compiled.map.rewrite(&e.to_string(), "views/t.lsp"))?;
        let concat: Function = lua
            .globals()
            .get::<Table>("table")
            .unwrap()
            .get("concat")
            .unwrap();
        let esc = lua
            .create_function(|lua, v: Value| html::emit_escaped(lua, &v))
            .unwrap();
        let raw = lua
            .create_function(|lua, v: Value| html::emit_raw(lua, &v))
            .unwrap();
        let function: Function = factory.call((concat, esc, raw)).unwrap();
        let env = lua.create_table().unwrap();
        for (k, v) in ctx {
            env.set(*k, *v).unwrap();
        }
        env.set(
            "tostring",
            lua.globals().get::<Function>("tostring").unwrap(),
        )
        .unwrap();
        env.set("ipairs", lua.globals().get::<Function>("ipairs").unwrap())
            .unwrap();
        env.set("error", lua.globals().get::<Function>("error").unwrap())
            .unwrap();
        function
            .call::<String>(env)
            .map_err(|e| compiled.map.rewrite(&e.to_string(), "views/t.lsp"))
    }

    #[test]
    fn the_four_tags() {
        let out = run(
            "<h1><?= title ?></h1>\n<? if x then ?>yes<? end ?>\n<?raw markup ?>\n<?-- gone <?= not this ?> --?>done",
            &[("title", "<b>&</b>"), ("x", "1"), ("markup", "<i>")],
        )
        .unwrap();
        assert_eq!(out, "<h1>&lt;b&gt;&amp;&lt;/b&gt;</h1>\nyes\n<i>\ndone");
    }

    #[test]
    fn nil_emits_nothing_and_quotes_survive() {
        let out = run("a\"b\\c\t<?= missing ?>'d'", &[]).unwrap();
        assert_eq!(out, "a\"b\\c\t'd'");
    }

    #[test]
    fn raw_needs_whitespace_and_code_may_end_in_a_comment() {
        // `<?rawr ?>` is code, not a raw tag.
        assert!(run("<?rawr ?>", &[]).is_err());
        let out = run("<? local n = 1 -- note ?>[<?= n -- also ?>]", &[]).unwrap();
        assert_eq!(out, "[1]");
    }

    #[test]
    fn errors_name_the_lsp_line() {
        let error = run("line one\n<? error('boom') ?>\n", &[]).unwrap_err();
        assert!(error.contains("views/t.lsp:2:"), "{error}");
        let error = run("a\nb\n<?= 1 +\n nope() ?>", &[]).unwrap_err();
        assert!(error.contains("views/t.lsp:4:"), "{error}");
        let error = run("x\n\n<? if y ?>", &[]).unwrap_err();
        assert!(error.contains("views/t.lsp:3:"), "{error}");
        let error = compile("ok\n<?= open").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("unclosed <?= ?>"), "{error}");
        let error = compile("\n\n<?-- never closed ?>").unwrap_err();
        assert_eq!(error.line, 3);
    }

    #[test]
    fn the_map_rewrites_every_occurrence_and_nothing_else() {
        let compiled = compile("a\n<? x() ?>\nb").unwrap();
        // Generated: 3 header lines, literal "a\n" (line 1), code (line 2), then the
        // literal "\nb", which begins on line 2 — the newline after `?>` is line 2's.
        assert_eq!(compiled.map.source_line(4), Some(1));
        assert_eq!(compiled.map.source_line(5), Some(2));
        assert_eq!(compiled.map.source_line(6), Some(2));
        assert_eq!(compiled.map.source_line(7), Some(3));
        assert_eq!(compiled.map.source_line(99), None);
        let text = "views/t.lsp:5: boom\n\tviews/t.lsp:5: in function <views/t.lsp:2>\n\tviews/u.lsp:5: other";
        assert_eq!(
            compiled.map.rewrite(text, "views/t.lsp"),
            "views/t.lsp:2: boom\n\tviews/t.lsp:2: in function <views/t.lsp:1>\n\tviews/u.lsp:5: other"
        );
    }

    #[test]
    fn view_names() {
        assert!(is_view_name("_board"));
        assert!(is_view_name("play-2"));
        assert!(!is_view_name("../x"));
        assert!(!is_view_name("a/b"));
        assert!(!is_view_name(""));
    }
}
