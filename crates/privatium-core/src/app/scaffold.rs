// This file is part of Privatium
// crates/privatium-core/src/app/scaffold.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-10-04
// Summary: What `privatium new` writes (spec/cli.md §4, spec/app-contract.md §4.7): the files of an
//          empty app for each tier, a copy of an existing app with its slug and title
//          rewritten, and the list / detail / create / edit screens for one table of
//          schema.sql. Pure functions returning files — the CLI decides where they go and
//          refuses to overwrite. Nothing here has a runtime presence: the output is ordinary
//          source an author edits, deletes or rewrites, and no file describes a UI.
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

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::manifest::Tier;
use crate::store::schema::{Column, Kind, Schema, Table};

/// One file the generator wants written: a slash-separated path relative to the app
/// folder, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// `app.toml`, `views/index.lsp`, `web/app.js`, …
    pub path: String,
    /// The contents. Text for everything the generator writes itself; a copy keeps the
    /// source's bytes for anything it does not recognise as text.
    pub contents: Vec<u8>,
}

impl File {
    fn text(path: &str, contents: String) -> Self {
        Self {
            path: path.to_owned(),
            contents: contents.into_bytes(),
        }
    }
}

/// What can go wrong generating.
#[derive(Debug, Error)]
pub enum ScaffoldError {
    /// Reading the app being copied failed.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// What the OS said.
        source: std::io::Error,
    },

    /// `schema.sql` declares no such table.
    #[error("schema.sql declares no table {table:?}; it declares {available}")]
    NoTable {
        /// The table asked for.
        table: String,
        /// The tables it does declare, comma-separated, or `none`.
        available: String,
    },

    /// The table has no column the form could edit.
    #[error("table {table:?} has no column besides id for a form to edit")]
    NoColumns {
        /// The table.
        table: String,
    },
}

fn io_at(path: &Path) -> impl FnOnce(std::io::Error) -> ScaffoldError {
    move |source| ScaffoldError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `my-med-tracker` → `My Med Tracker`: the title `app.toml` requires, from the slug, for
/// an author who gave none.
#[must_use]
pub fn title_from_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The manifest every tier starts from (`spec/app-contract.md §3`).
fn manifest(slug: &str, title: &str, tier: Tier) -> String {
    format!(
        "# Written by `privatium new` (spec/cli.md §4). Every key is documented in\n\
         # spec/app-contract.md §3; the slug must equal this folder's name.\n\
         \n\
         [app]\n\
         slug        = \"{slug}\"\n\
         title       = \"{title}\"\n\
         version     = \"0.1.0\"\n\
         api         = 1\n\
         tier        = \"{}\"\n\
         description = \"\"\n\
         \n\
         [nav]\n\
         order     = 100\n\
         advertise = false\n",
        tier.as_str()
    )
}

/// The files of an empty app for `tier` (`spec/cli.md §4`, "populated for the chosen
/// tier"): the manifest, the tier's required file, and enough beside it that the app
/// loads and renders one page. A Tier 3 folder is an index entry only
/// (`spec/app-contract.md §8`), so it gets the manifest and a note saying where the
/// program lives.
#[must_use]
pub fn fresh(slug: &str, title: &str, tier: Tier) -> Vec<File> {
    let mut files = vec![File::text("app.toml", manifest(slug, title, tier))];
    match tier {
        Tier::Lua => {
            files.push(File::text(
                "app.lua",
                "-- Written by `privatium new` (spec/cli.md §4). Routes live here; templates in\n\
                 -- views/. A save is picked up on the next request, with no restart (§3).\n\
                 \n\
                 local pv = require 'privatium'\n\
                 \n\
                 pv.get('/', function()\n\
                 \x20 return pv.render('index', {})\n\
                 end)\n"
                    .to_owned(),
            ));
            files.push(File::text(
                "views/index.lsp",
                format!(
                    "<h1>{title}</h1>\n\
                     <p>This page is <code>views/index.lsp</code>. Edit it and reload the browser;\n\
                     \x20  the change shows on the next request (spec/cli.md §3).</p>\n\
                     <p>Every link goes through <code>url()</code>, so the app works at\n\
                     \x20  <code>/a/{slug}/</code> and, in solo mode, at <code>/</code>:\n\
                     \x20  <a href=\"<?= url('/') ?>\">this page again</a>.</p>\n",
                    title = escape(title),
                ),
            ));
        }
        Tier::Web => {
            files.push(File::text(
                "web/index.html",
                format!(
                    "<!doctype html>\n\
                     <html lang=\"en\">\n\
                     <head>\n\
                     <meta charset=\"utf-8\">\n\
                     <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
                     <title>{title}</title>\n\
                     <link rel=\"stylesheet\" href=\"style.css\">\n\
                     </head>\n\
                     <body>\n\
                     <!-- The framework's bar and footer are inserted around this document\n\
                     \x20    unless app.toml sets [ui] chrome = \"none\" (spec/app-contract.md §5);\n\
                     \x20    the bar's skip link lands on the main region below. -->\n\
                     <main id=\"main\">\n\
                     \x20 <h1>{title}</h1>\n\
                     \x20 <p>Served from <code>web/</code>, wearing the framework's bar and footer.\n\
                     \x20    Talk to the node through <code>pv.js</code> (spec/data-api.md §5).</p>\n\
                     </main>\n\
                     \n\
                     <script type=\"module\" src=\"app.js\"></script>\n\
                     </body>\n\
                     </html>\n",
                    title = escape(title),
                ),
            ));
            files.push(File::text(
                "web/app.js",
                "// Written by `privatium new` (spec/cli.md §4). Plain ES modules, no build step.\n\
                 // The default CSP (spec/protocol.md §9.3) allows no inline script and no CDN:\n\
                 // vendor libraries under web/vendor/ and keep every script in a file.\n\
                 import { pv } from '/static/pv.js';\n\
                 \n\
                 // Task wording goes to the footer's status line, which the framework's chrome\n\
                 // renders (spec/app-contract.md §5.2); the connection wording there is its own.\n\
                 const node = await pv.node();\n\
                 pv.status(`Connected to ${node.name} (${node.protocol}).`);\n"
                    .to_owned(),
            ));
            files.push(File::text(
                "web/style.css",
                "/* Written by `privatium new`. The default CSP drops inline styles; keep them here. */\n\
                 main { max-width: 40rem; margin: 2rem auto; padding: 0 1rem; font-family: system-ui, sans-serif; }\n"
                    .to_owned(),
            ));
        }
        Tier::Rust => {
            files.push(File::text(
                "README.md",
                format!(
                    "# {title}\n\
                     \n\
                     A Tier 3 app: this folder is its index entry (spec/app-contract.md §8), and\n\
                     the program is your own Cargo project linking `privatium-core`\n\
                     (spec/app-contract.md §2.3, §6). `privatium skill export privatium-tier3-rust`\n\
                     writes the skill that describes that API to disk.\n",
                    title = title,
                ),
            ));
        }
    }
    files
}

/// Extensions whose contents are rewritten during a copy. Anything else is copied as bytes.
const TEXT_EXTENSIONS: &[&str] = &[
    "toml", "lua", "lsp", "sql", "md", "html", "js", "css", "json", "jsonl", "txt", "svg",
];

/// Copy the app at `source` — a reference app or an installed one — as `slug`, with its
/// slug and title rewritten (`spec/cli.md §4`, `--from`).
///
/// What is rewritten is what names the app: `slug` and `title` in `app.toml`; the path
/// `apps/<old>/` wherever a file header or a README spells it; the `privatium-app-<old>`
/// skill name; a Markdown heading that is the bare slug; and an HTML `<title>` equal to
/// the old title. Prose is left alone — a README that says "hello" in a sentence still
/// says it, because rewriting words is how a copy becomes wrong in ways nobody reads.
pub fn copy(source: &Path, slug: &str, title: &str) -> Result<Vec<File>, ScaffoldError> {
    let mut files = Vec::new();
    walk(source, source, &mut files)?;
    Ok(copy_files(files, slug, title))
}

/// [`copy`] over files already in memory — an example app the binary carries
/// (`app::examples`) rather than a folder on disk. The manifest is the `app.toml` among
/// them; without one nothing is renamed and the files come back sorted and otherwise
/// untouched.
#[must_use]
pub fn copy_files(mut files: Vec<File>, slug: &str, title: &str) -> Vec<File> {
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest_text = files
        .iter()
        .find(|file| file.path == "app.toml")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .unwrap_or_default();
    let old_slug = toml_string(manifest_text, "slug").unwrap_or_default();
    let old_title = toml_string(manifest_text, "title").unwrap_or_default();

    for file in &mut files {
        let extension = Path::new(&file.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !TEXT_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&file.contents) else {
            continue;
        };
        let mut text = text.to_owned();
        if file.path == "app.toml" {
            text = set_toml_string(&text, "slug", slug);
            text = set_toml_string(&text, "title", title);
        }
        if !old_slug.is_empty() {
            text = replace_path(&text, &old_slug, slug);
            text = text.replace(
                &format!("privatium-app-{old_slug}"),
                &format!("privatium-app-{slug}"),
            );
            text = rewrite_headings(&text, &old_slug, slug);
        }
        if !old_title.is_empty() {
            text = text.replace(
                &format!("<title>{old_title}</title>"),
                &format!("<title>{}</title>", escape(title)),
            );
        }
        file.contents = text.into_bytes();
    }
    files
}

fn walk(base: &Path, dir: &Path, into: &mut Vec<File>) -> Result<(), ScaffoldError> {
    for entry in fs::read_dir(dir).map_err(io_at(dir))? {
        let entry = entry.map_err(io_at(dir))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_at(&path))?;
        if file_type.is_dir() {
            walk(base, &path, into)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let contents = fs::read(&path).map_err(io_at(&path))?;
            into.push(File {
                path: relative,
                contents,
            });
        }
    }
    Ok(())
}

/// `apps/<old>` → `apps/<new>` wherever the old slug is the whole path segment — a file
/// header's `apps/hello/app.lua`, a README's `privatium lint apps/hello` — and not inside
/// a longer slug (`apps/hello-world/` stays).
fn replace_path(text: &str, old: &str, new: &str) -> String {
    let needle = format!("apps/{old}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        let ends_segment = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'));
        out.push_str(&rest[..at]);
        if ends_segment {
            out.push_str("apps/");
            out.push_str(new);
        } else {
            out.push_str(&needle);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The value of `key = "…"` on its own line, if any. A line-level read rather than a TOML
/// parse, because the rewrite below has to keep every comment and every other line as
/// the author wrote it.
fn toml_string(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim();
        let rest = rest.strip_prefix('"')?;
        let end = rest.find('"')?;
        Some(rest[..end].to_owned())
    })
}

/// `key = "…"` rewritten in place, alignment kept, everything else untouched.
fn set_toml_string(text: &str, key: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let _ = index;
        let stripped = line.trim_end_matches(['\r', '\n']);
        let is_key = stripped
            .strip_prefix(key)
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|rest| rest.trim_start().starts_with('"'));
        if is_key && let Some(eq) = stripped.find('=') {
            let (head, tail) = stripped.split_at(eq + 1);
            // Keep a trailing comment.
            let comment = tail.find('#').map(|at| &tail[at..]);
            out.push_str(head);
            out.push_str(" \"");
            out.push_str(&value.replace('"', "\\\""));
            out.push('"');
            if let Some(comment) = comment {
                out.push_str("  ");
                out.push_str(comment);
            }
            out.push_str(&line[stripped.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// A Markdown heading that is exactly the old slug (`# hello`) becomes the new one; a
/// heading that goes on (`# hello — the reference app`) keeps its words after the slug.
fn rewrite_headings(text: &str, old: &str, new: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let stripped = line.trim_end_matches(['\r', '\n']);
            let hashes = stripped.len() - stripped.trim_start_matches('#').len();
            if hashes == 0 {
                return line.to_owned();
            }
            let after = &stripped[hashes..];
            let Some(rest) = after.strip_prefix(' ') else {
                return line.to_owned();
            };
            let Some(tail) = rest.strip_prefix(old) else {
                return line.to_owned();
            };
            if !(tail.is_empty() || tail.starts_with(' ')) {
                return line.to_owned();
            }
            format!(
                "{} {new}{tail}{}",
                &stripped[..hashes],
                &line[stripped.len()..]
            )
        })
        .collect()
}

/// The list, detail, create and edit screens for `table` (`spec/cli.md §4`,
/// `--scaffold`): `app.lua` and three views, generated from the columns `schema.sql`
/// declares. Structured columns (`JSON`, `VARCHAR[]`) are listed but not edited — a form
/// field cannot hold one honestly — and `id` is the framework's.
pub fn crud(schema: &Schema, table: &str) -> Result<Vec<File>, ScaffoldError> {
    let Some(declared) = schema.table(table) else {
        let mut names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        return Err(ScaffoldError::NoTable {
            table: table.to_owned(),
            available: if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            },
        });
    };
    let fields: Vec<&Column> = declared
        .columns
        .iter()
        .filter(|c| c.name != "id" && c.kind != Kind::Json)
        .collect();
    if fields.is_empty() {
        return Err(ScaffoldError::NoColumns {
            table: table.to_owned(),
        });
    }
    let shown: Vec<&Column> = declared.columns.iter().filter(|c| c.name != "id").collect();
    let label = label_of(&declared.name);

    Ok(vec![
        File::text("app.lua", crud_lua(declared, &fields, &label)),
        File::text(
            &format!("views/{}_index.lsp", declared.name),
            crud_index(declared, &shown, &label),
        ),
        File::text(
            &format!("views/{}_show.lsp", declared.name),
            crud_show(declared, &shown, &label),
        ),
        File::text(
            &format!("views/{}_form.lsp", declared.name),
            crud_form(declared, &fields),
        ),
    ])
}

/// `copay_amount` → `Copay amount`.
fn label_of(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A Lua string literal.
fn lua_str(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Text into HTML, for the few places the generator writes a literal into markup.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn crud_lua(table: &Table, fields: &[&Column], label: &str) -> String {
    let name = &table.name;
    let view = |suffix: &str| lua_str(&format!("{name}_{suffix}"));
    let mut read_form = String::new();
    for column in fields {
        let key = lua_str(&column.name);
        match column.kind {
            Kind::Boolean => read_form.push_str(&format!("  data[{key}] = form[{key}] ~= nil\n")),
            _ => read_form.push_str(&format!("  data[{key}] = present(form[{key}])\n")),
        }
    }
    let skipped: Vec<&str> = table
        .columns
        .iter()
        .filter(|c| c.kind == Kind::Json)
        .map(|c| c.name.as_str())
        .collect();
    let skipped_note = if skipped.is_empty() {
        String::new()
    } else {
        format!(
            "-- Structured columns are shown but not edited here: {}.\n",
            skipped.join(", ")
        )
    };
    format!(
        "-- Written by `privatium new --scaffold {name}` (spec/cli.md §4, spec/app-contract.md §4.7).\n\
         -- Ordinary source with no runtime presence: edit, delete or rewrite any of it.\n\
         {skipped_note}\
         \n\
         local pv = require 'privatium'\n\
         \n\
         local TABLE = {table_lit}\n\
         local LABEL = {label_lit}\n\
         \n\
         local function list()\n\
         \x20 return pv.query('SELECT * FROM \"{name}\" ORDER BY id DESC LIMIT 200')\n\
         end\n\
         \n\
         -- An empty field is an absent key, so NOT NULL is the schema's to refuse.\n\
         local function present(value)\n\
         \x20 if value == nil or value == '' then return nil end\n\
         \x20 return value\n\
         end\n\
         \n\
         local function read_form(form)\n\
         \x20 local data = {{}}\n\
         {read_form}\
         \x20 return data\n\
         end\n\
         \n\
         -- Typed writes and constraints refuse a bad value before it reaches the log\n\
         -- (spec/lua-api.md §3.3); the message names the column, so show it.\n\
         local function save(id, form)\n\
         \x20 local ok, result = pcall(pv.append, TABLE, id, read_form(form))\n\
         \x20 if ok then return result, nil end\n\
         \x20 return nil, tostring(result)\n\
         end\n\
         \n\
         pv.get('/', function()\n\
         \x20 return pv.render({index}, {{ rows = list() }})\n\
         end)\n\
         \n\
         pv.get('/new', function()\n\
         \x20 return pv.render({form}, {{ row = {{}}, action = url('/new'), heading = 'New ' .. LABEL }})\n\
         end)\n\
         \n\
         pv.post('/new', function(req)\n\
         \x20 local id, err = save(nil, req.form)\n\
         \x20 if err then\n\
         \x20   return pv.render({form}, {{ row = read_form(req.form), action = url('/new'),\n\
         \x20                                 heading = 'New ' .. LABEL, err = err }})\n\
         \x20 end\n\
         \x20 return pv.redirect(url('/' .. id))\n\
         end)\n\
         \n\
         pv.get('/:id', function(req)\n\
         \x20 local row = pv.get_row(TABLE, req.params.id)\n\
         \x20 if not row then return pv.redirect(url('/')) end\n\
         \x20 return pv.render({show}, {{ row = row }})\n\
         end)\n\
         \n\
         pv.get('/:id/edit', function(req)\n\
         \x20 local row = pv.get_row(TABLE, req.params.id)\n\
         \x20 if not row then return pv.redirect(url('/')) end\n\
         \x20 return pv.render({form}, {{ row = row, action = url('/' .. row.id .. '/edit'),\n\
         \x20                               heading = 'Edit ' .. LABEL }})\n\
         end)\n\
         \n\
         pv.post('/:id/edit', function(req)\n\
         \x20 local id, err = save(req.params.id, req.form)\n\
         \x20 if err then\n\
         \x20   local row = read_form(req.form)\n\
         \x20   row.id = req.params.id\n\
         \x20   return pv.render({form}, {{ row = row, action = url('/' .. req.params.id .. '/edit'),\n\
         \x20                                 heading = 'Edit ' .. LABEL, err = err }})\n\
         \x20 end\n\
         \x20 return pv.redirect(url('/' .. id))\n\
         end)\n\
         \n\
         -- A deletion is a tombstone (spec/protocol.md §4.6); the row's history stays in the log.\n\
         pv.post('/:id/delete', function(req)\n\
         \x20 pv.delete(TABLE, req.params.id)\n\
         \x20 return pv.redirect(url('/'))\n\
         end)\n",
        table_lit = lua_str(name),
        label_lit = lua_str(label),
        index = view("index"),
        show = view("show"),
        form = view("form"),
    )
}

/// How a column's value is printed in a view.
fn cell(column: &Column) -> String {
    let key = lua_str(&column.name);
    match column.kind {
        Kind::Boolean => {
            format!("<?= row[{key}] == nil and '' or (row[{key}] and 'Yes' or 'No') ?>")
        }
        Kind::Json => format!("<?= row[{key}] ~= nil and 'structured' or '' ?>"),
        _ => format!("<?= row[{key}] ?>"),
    }
}

fn crud_index(table: &Table, shown: &[&Column], label: &str) -> String {
    let mut head = String::new();
    let mut cells = String::new();
    for column in shown {
        head.push_str(&format!(
            "      <th scope=\"col\">{}</th>\n",
            escape(&label_of(&column.name))
        ));
        cells.push_str(&format!("        <td>{}</td>\n", cell(column)));
    }
    format!(
        "<?-- Written by `privatium new --scaffold {name}`: the list screen. --?>\n\
         <h1>{label}</h1>\n\
         <p><a class=\"pv-btn pv-btn-primary\" href=\"<?= url('/new') ?>\">New {label_lower}</a></p>\n\
         \n\
         <? if #rows == 0 then ?>\n\
         \x20 <p>Nothing here yet.</p>\n\
         <? else ?>\n\
         \x20 <table>\n\
         \x20   <thead>\n\
         \x20   <tr>\n\
         {head}\
         \x20     <th scope=\"col\">Actions</th>\n\
         \x20   </tr>\n\
         \x20   </thead>\n\
         \x20   <tbody>\n\
         \x20   <? for _, row in ipairs(rows) do ?>\n\
         \x20     <tr>\n\
         {cells}\
         \x20       <td>\n\
         \x20         <a href=\"<?= url('/' .. row.id) ?>\">View</a>\n\
         \x20         <a href=\"<?= url('/' .. row.id .. '/edit') ?>\">Edit</a>\n\
         \x20       </td>\n\
         \x20     </tr>\n\
         \x20   <? end ?>\n\
         \x20   </tbody>\n\
         \x20 </table>\n\
         <? end ?>\n",
        name = table.name,
        label = escape(label),
        label_lower = escape(&label.to_lowercase()),
    )
}

fn crud_show(table: &Table, shown: &[&Column], label: &str) -> String {
    let mut rows = String::new();
    for column in shown {
        rows.push_str(&format!(
            "  <dt>{}</dt>\n  <dd>{}</dd>\n",
            escape(&label_of(&column.name)),
            cell(column)
        ));
    }
    format!(
        "<?-- Written by `privatium new --scaffold {name}`: the detail screen. --?>\n\
         <h1>{label}</h1>\n\
         <dl>\n\
         {rows}\
         </dl>\n\
         <p>\n\
         \x20 <a class=\"pv-btn\" href=\"<?= url('/' .. row.id .. '/edit') ?>\">Edit</a>\n\
         \x20 <a class=\"pv-btn\" href=\"<?= url('/') ?>\">Back to the list</a>\n\
         </p>\n\
         <form method=\"post\" action=\"<?= url('/' .. row.id .. '/delete') ?>\">\n\
         \x20 <?= csrf() ?>\n\
         \x20 <button type=\"submit\" class=\"pv-btn\">Delete</button>\n\
         </form>\n",
        name = table.name,
        label = escape(label),
    )
}

/// The `<input>` for a column, typed by its declaration
/// (`spec/data-dictionary.md §2.1`). A timestamp is a text field: the browser's
/// `datetime-local` neither shows nor submits the RFC 3339 the column holds.
fn input(column: &Column) -> String {
    let key = lua_str(&column.name);
    let id = format!("f-{}", column.name);
    let required = if column.not_null { " required" } else { "" };
    let declared = column.ty.trim().to_ascii_uppercase();
    let attributes = match column.kind {
        // A checkbox group of one, inside the fieldset PV403 asks of every group.
        Kind::Boolean => {
            return format!(
                "  <fieldset>\n    <legend>{label}</legend>\n    \
                 <input id=\"{id}\" name=\"{name}\" type=\"checkbox\" value=\"true\" <?= row[{key}] and 'checked' or '' ?>>\n    \
                 <label for=\"{id}\">Yes</label>\n  </fieldset>\n",
                name = column.name,
                label = escape(&label_of(&column.name)),
            );
        }
        Kind::Integer => "type=\"number\" step=\"1\"".to_owned(),
        Kind::Decimal { .. } => "type=\"text\" inputmode=\"decimal\"".to_owned(),
        Kind::Text if declared == "DATE" => "type=\"date\"".to_owned(),
        Kind::Text if declared == "TIME" => "type=\"time\"".to_owned(),
        Kind::Text | Kind::Json => "type=\"text\"".to_owned(),
    };
    format!(
        "  <label for=\"{id}\">{label}</label>\n  \
         <input id=\"{id}\" name=\"{name}\" {attributes} value=\"<?= row[{key}] ?>\"{required}>\n",
        name = column.name,
        label = escape(&label_of(&column.name)),
    )
}

fn crud_form(table: &Table, fields: &[&Column]) -> String {
    let inputs: String = fields.iter().map(|c| input(c)).collect();
    format!(
        "<?-- Written by `privatium new --scaffold {name}`: one form for create and edit. --?>\n\
         <h1><?= heading ?></h1>\n\
         \n\
         <? if err then ?>\n\
         \x20 <p class=\"pv-error\" role=\"alert\"><?= err ?></p>\n\
         <? end ?>\n\
         \n\
         <form method=\"post\" action=\"<?= action ?>\">\n\
         \x20 <?= csrf() ?>\n\
         {inputs}\
         \x20 <button type=\"submit\" class=\"pv-btn pv-btn-primary\">Save</button>\n\
         \x20 <a class=\"pv-btn\" href=\"<?= url('/') ?>\">Cancel</a>\n\
         </form>\n",
        name = table.name,
    )
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_comes_from_the_slug() {
        assert_eq!(title_from_slug("my-med-tracker"), "My Med Tracker");
        assert_eq!(title_from_slug("hello"), "Hello");
    }

    #[test]
    fn toml_strings_are_rewritten_in_place_and_nothing_else_moves() {
        let text = "[app]\nslug        = \"hello\"   # the folder\ntitle       = \"Hello\"\nversion = \"1.0.0\"\n";
        let out = set_toml_string(&set_toml_string(text, "slug", "mine"), "title", "Mine");
        assert_eq!(
            out,
            "[app]\nslug        = \"mine\"  # the folder\ntitle       = \"Mine\"\nversion = \"1.0.0\"\n"
        );
        assert_eq!(toml_string(&out, "slug").as_deref(), Some("mine"));
    }

    #[test]
    fn paths_are_rewritten_by_whole_segment() {
        assert_eq!(
            replace_path(
                "apps/hello/app.lua apps/hello` apps/hello-world/ apps/hello",
                "hello",
                "mine"
            ),
            "apps/mine/app.lua apps/mine` apps/hello-world/ apps/mine"
        );
    }

    #[test]
    fn headings_that_are_the_slug_are_rewritten() {
        let text = "# hello\n\n# hello — the app\n\n# hellothere\nprose hello\n";
        assert_eq!(
            rewrite_headings(text, "hello", "mine"),
            "# mine\n\n# mine — the app\n\n# hellothere\nprose hello\n"
        );
    }

    #[test]
    fn every_tier_has_its_required_file() {
        for tier in [Tier::Lua, Tier::Web, Tier::Rust] {
            let files = fresh("mine", "Mine", tier);
            assert_eq!(files[0].path, "app.toml");
            if let Some(required) = tier.required_file() {
                assert!(files.iter().any(|f| f.path == required), "{tier:?}");
            }
            let manifest = std::str::from_utf8(&files[0].contents).unwrap();
            assert!(manifest.contains("slug        = \"mine\""));
            assert!(manifest.contains(&format!("tier        = \"{}\"", tier.as_str())));
        }
    }

    #[test]
    fn crud_names_every_column_and_refuses_an_unknown_table() {
        let schema = Schema::parse(
            "CREATE TABLE fill (id VARCHAR PRIMARY KEY, drug VARCHAR NOT NULL, \
             copay DECIMAL(18,2), taken BOOLEAN, on_day DATE, tags VARCHAR[]);",
        )
        .unwrap();
        let files = crud(&schema, "fill").unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "app.lua",
                "views/fill_index.lsp",
                "views/fill_show.lsp",
                "views/fill_form.lsp"
            ]
        );
        let form = std::str::from_utf8(&files[3].contents).unwrap();
        assert!(form.contains("name=\"drug\" type=\"text\" value=\"<?= row['drug'] ?>\" required"));
        assert!(form.contains("inputmode=\"decimal\""));
        assert!(form.contains("type=\"checkbox\""));
        assert!(form.contains("type=\"date\""));
        assert!(
            !form.contains("name=\"tags\""),
            "structured columns are not edited"
        );
        let lua = std::str::from_utf8(&files[0].contents).unwrap();
        assert!(lua.contains("pv.render('fill_index'"));
        assert!(lua.contains("data['taken'] = form['taken'] ~= nil"));

        let error = crud(&schema, "nope").unwrap_err();
        assert!(error.to_string().contains("fill"), "{error}");
    }
}
