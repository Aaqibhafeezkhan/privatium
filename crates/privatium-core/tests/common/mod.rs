// This file is part of Privatium
// crates/privatium-core/tests/common/mod.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-02
// Last Modified: 2026-10-04
// Summary: What tests/store.rs, tests/snapshot.rs and tests/apps.rs share: a node plus one app store,
//          the event line of spec/protocol.md §4.1 spelled by hand, `echo >>`, the digests the
//          §2.5 comparisons are made with, app folders written into a data root, and the sys
//          tables read back as JSON.
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

// AGENTS.md, Style: unwrap() is permitted in tests. Each test binary uses a different
// subset of these helpers, so the unused ones are not a finding.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

pub mod a11y;

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use privatium_core::Node;
use privatium_core::local::State;
use privatium_core::log::{AppLog, Durability};
use privatium_core::store::{self, Restored, Snapshot, Store};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// An ordinary app, so these tests exercise the path an app author gets rather than a
/// special case reserved for `_sys`.
pub const APP: &str = "hello";

/// `apps/hello/schema.sql`, near enough — one table, one column.
pub const HELLO_DDL: &str = "CREATE TABLE profile (
     id           VARCHAR PRIMARY KEY,
     display_name VARCHAR NOT NULL
 );";

/// A schema exercising every row of `spec/data-dictionary.md §2.1`.
pub const TYPED_DDL: &str = "CREATE TABLE thing (
     id           VARCHAR PRIMARY KEY,
     name         VARCHAR,
     copay_amount DECIMAL(18,2),
     count        BIGINT,
     ok           BOOLEAN,
     filled_on    DATE,
     seen_at      TIMESTAMPTZ,
     tags         VARCHAR[]
 );";

/// A node plus a store over one app, the pairing the app host assembles for real.
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub node: Node,
    pub store: Store,
    pub dev: String,
}

impl Fixture {
    pub fn open(ddl: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        Self::open_in(root, ddl)
    }

    /// Open against an existing root and build the tables by the full replay (tier 3).
    pub fn open_in(root: tempfile::TempDir, ddl: &str) -> Self {
        let mut fixture = Self::open_untouched(root, ddl);
        fixture.store.materialize(&store::cutoff_now()).unwrap();
        fixture
    }

    /// Open against an existing root without building anything yet.
    pub fn open_untouched(root: tempfile::TempDir, ddl: &str) -> Self {
        let node = Node::open(root.path()).unwrap();
        let dev = node.id().as_str().to_owned();
        // The app's log directory has to exist before the store globs it.
        let state = State::load(&node.paths().local_state()).unwrap();
        let (_log, _) = AppLog::open(node.paths(), APP, node.id(), Durability::Os, &state).unwrap();
        let store = Store::open(node.paths(), APP, ddl).unwrap();
        Self {
            root,
            node,
            store,
            dev,
        }
    }

    /// Reopen the store the way a restart would, keeping the same data root, and build
    /// the tables by the full replay.
    ///
    /// The store is dropped explicitly and first, so the replacement opens the same file
    /// with no reader of ours still on it.
    pub fn reopen(self, ddl: &str) -> Self {
        Self::open_in(self.release(), ddl)
    }

    /// Reopen and build the tables by `spec/protocol.md §5.3`'s three tiers instead.
    pub fn reopen_restoring(self, ddl: &str) -> (Self, Restored) {
        let mut fixture = Self::open_untouched(self.release(), ddl);
        let restored = fixture.restore();
        (fixture, restored)
    }

    /// Drop the node and the store, keeping the data root.
    pub fn release(self) -> tempfile::TempDir {
        let Fixture {
            root, node, store, ..
        } = self;
        drop(store);
        drop(node);
        root
    }

    pub fn log_path(&self) -> PathBuf {
        self.node.paths().app_log(APP, self.node.id())
    }

    pub fn snap_dir(&self) -> PathBuf {
        self.node.paths().app_snap_dir(APP)
    }

    pub fn append(&self, line: &str) {
        hand_append(&self.log_path(), line, "\n");
    }

    pub fn rematerialize(&mut self) {
        self.store.materialize(&store::cutoff_now()).unwrap();
    }

    /// The three-tier read.
    pub fn restore(&mut self) -> Restored {
        self.store.restore(&store::cutoff_now()).unwrap()
    }

    /// A snapshot at `now`, by this node.
    pub fn snapshot(&self, now: jiff::Timestamp) -> Snapshot {
        self.store.snapshot(self.node.id(), now).unwrap()
    }

    /// One column of one row, as a string. `<NULL>` where the row exists but the value is
    /// NULL; `<MISSING>` where there is no row at all.
    pub fn cell(&self, table: &str, id: &str, column: &str) -> String {
        let sql = format!(
            "SELECT coalesce(CAST(\"{column}\" AS TEXT), '<NULL>') FROM \"{table}\" WHERE id = ?"
        );
        self.store
            .conn()
            .query_row(&sql, rusqlite::params![id], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "<MISSING>".to_owned())
    }

    pub fn count(&self, table: &str) -> i64 {
        self.store
            .conn()
            .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    /// A deterministic fingerprint of the whole tombstone set, `(tbl, id)` pairs in order.
    pub fn tombstone_digest(&self) -> String {
        digest_rows(
            self.store.conn(),
            "SELECT tbl, id FROM pv_tombstone ORDER BY tbl, id",
        )
    }

    /// A deterministic fingerprint of a table's whole contents.
    pub fn digest(&self, table: &str) -> String {
        digest_via(self.store.conn(), table)
    }

    /// The table's digest and the tombstone digest together — what every `§2.5`
    /// comparison is made on.
    pub fn digests(&self, table: &str) -> (String, String) {
        (self.digest(table), self.tombstone_digest())
    }
}

/// Append a line by hand, exactly as `apps/hello/README.md` blesses `echo >>`.
pub fn hand_append(path: &Path, line: &str, terminator: &str) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(line.as_bytes()).unwrap();
    file.write_all(terminator.as_bytes()).unwrap();
}

/// One event line, spelled the way `spec/protocol.md §4.1` spells one.
pub fn event(
    seq: u64,
    lam: u64,
    ts: &str,
    dev: &str,
    tbl: &str,
    id: &str,
    d: Option<&str>,
) -> String {
    match d {
        Some(d) => format!(
            r#"{{"seq":{seq},"lam":{lam},"ts":"{ts}","dev":"{dev}","app":"{APP}","op":"put","tbl":"{tbl}","id":"{id}","d":{d}}}"#
        ),
        // §4.1: `d` MUST be absent on a del — not null, not `{}`.
        None => format!(
            r#"{{"seq":{seq},"lam":{lam},"ts":"{ts}","dev":"{dev}","app":"{APP}","op":"del","tbl":"{tbl}","id":"{id}"}}"#
        ),
    }
}

/// An RFC 3339 UTC timestamp offset from now, to the millisecond (`§4.1`).
pub fn ts_offset_secs(seconds: i64) -> String {
    privatium_core::log::format_ts(
        jiff::Timestamp::now() + jiff::SignedDuration::from_secs(seconds),
    )
}

/// A fixed instant.
pub fn at(rfc3339: &str) -> jiff::Timestamp {
    rfc3339.parse().unwrap()
}

/// Corrupt a file on disk: invert one byte in the middle. Real bytes, so the SHA-256 path
/// of `spec/protocol.md §5.3` is what notices, not a mock.
pub fn flip_byte(path: &Path) {
    let mut bytes = fs::read(path).unwrap();
    assert!(!bytes.is_empty(), "{}: nothing to flip", path.display());
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    fs::write(path, bytes).unwrap();
}

// ---------------------------------------------------------------------------------------
// App folders (tests/apps.rs)
// ---------------------------------------------------------------------------------------

/// The repository's `apps/` — the three reference apps, loaded as `bundled`.
pub fn repo_apps_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("apps")
}

/// The smallest valid Tier 1 manifest for `slug` (`spec/app-contract.md §3`).
pub fn lua_manifest(slug: &str) -> String {
    format!(
        "[app]\nslug = \"{slug}\"\ntitle = \"{slug}\"\nversion = \"1.0.0\"\napi = 1\ntier = \"lua\"\n"
    )
}

/// Write an app folder under `apps_dir/<folder>/`: `app.toml` if given, plus `files` as
/// `(relative path, contents)`. Returns the folder.
pub fn write_app(
    apps_dir: &Path,
    folder: &str,
    toml: Option<&str>,
    files: &[(&str, &str)],
) -> PathBuf {
    let dir = apps_dir.join(folder);
    fs::create_dir_all(&dir).unwrap();
    if let Some(toml) = toml {
        fs::write(dir.join("app.toml"), toml).unwrap();
    }
    for (name, contents) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

/// A loadable Tier 1 app: the manifest and an empty `app.lua`, plus `files`.
pub fn write_lua_app(apps_dir: &Path, slug: &str, files: &[(&str, &str)]) -> PathBuf {
    let mut all: Vec<(&str, &str)> = vec![("app.lua", "")];
    all.extend_from_slice(files);
    write_app(apps_dir, slug, Some(&lua_manifest(slug)), &all)
}

/// SHA-256, lowercase hex — what `sys_app.schema_hash` and `manifest_hash` hold.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One `sys_*` row as JSON, keyed by `id`, after the caller has refreshed `_sys`. A
/// `BOOLEAN` column of the dictionary comes back as a JSON boolean, as `§2.1` spells it.
pub fn sys_row(node: &Node, table: &str, id: &str) -> Option<Value> {
    let sql = format!("SELECT * FROM {table} WHERE id = ?");
    let mut statement = node.store().conn().prepare(&sql).unwrap();
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let mut rows = statement.query(rusqlite::params![id]).unwrap();
    let mut json = rows.next().unwrap().map(|row| row_json(row, &names))?;
    let schema = store::Schema::parse(store::SYS_DDL).unwrap();
    if let Some(declared) = schema.table(table) {
        for column in &declared.columns {
            if column.kind == store::Kind::Boolean
                && let Some(Value::Number(n)) = json.get(&column.name)
            {
                let flag = Value::Bool(n.as_i64() != Some(0));
                json[column.name.as_str()] = flag;
            }
        }
    }
    Some(json)
}

/// One row as a JSON object, typed by its storage class: integers and floats as numbers,
/// text as strings, NULL as null.
pub fn row_json(row: &rusqlite::Row<'_>, names: &[String]) -> Value {
    use rusqlite::types::ValueRef;
    let mut object = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        let value = match row.get_ref(index).unwrap() {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(i) => Value::from(i),
            ValueRef::Real(r) => Value::from(r),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Value::String(format!("<{} bytes>", b.len())),
        };
        object.insert(name.clone(), value);
    }
    Value::Object(object)
}

/// The `sys_app` row for `slug`.
pub fn sys_app_row(node: &Node, slug: &str) -> Option<Value> {
    sys_row(node, "sys_app", slug)
}

/// Every `sys_audit` row of one kind, oldest first, as JSON.
pub fn audit_rows(node: &Node, kind: &str) -> Vec<Value> {
    let mut statement = node
        .store()
        .conn()
        .prepare("SELECT * FROM sys_audit WHERE kind = ? ORDER BY \"at\", id")
        .unwrap();
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    statement
        .query_map(rusqlite::params![kind], |row| Ok(row_json(row, &names)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// A log file's lines, parsed.
pub fn log_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// This node's `_sys` log, parsed.
pub fn sys_lines(node: &Node) -> Vec<Value> {
    log_lines(&node.paths().app_log("_sys", node.id()))
}

/// The names directly inside `dir`.
pub fn files_in(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// Every path below `root`, relative and slash-separated, directories with a trailing `/`
/// — the exhaustive shape `test_spec_3_layout_created` asserts.
pub fn tree(root: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, dir: &Path, into: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().unwrap().is_dir() {
                into.insert(format!("{relative}/"));
                walk(base, &path, into);
            } else {
                into.insert(relative);
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(root, root, &mut found);
    found
}

/// A deterministic fingerprint of a table's contents through any connection — every
/// column of every row, typed, in `id` order; `empty` for no rows.
pub fn digest_via(conn: &rusqlite::Connection, table: &str) -> String {
    digest_rows(conn, &format!("SELECT * FROM \"{table}\" ORDER BY id"))
}

/// SHA-256 over every value a query returns, with its storage class, so a REAL where a
/// TEXT was expected changes the digest even when it prints the same.
pub fn digest_rows(conn: &rusqlite::Connection, sql: &str) -> String {
    use rusqlite::types::ValueRef;
    let mut statement = conn.prepare(sql).unwrap();
    let columns = statement.column_count();
    let mut rows = statement.query([]).unwrap();
    let mut hasher = Sha256::new();
    let mut any = false;
    while let Some(row) = rows.next().unwrap() {
        any = true;
        for index in 0..columns {
            match row.get_ref(index).unwrap() {
                ValueRef::Null => hasher.update(b"N|"),
                ValueRef::Integer(i) => hasher.update(format!("I{i}|").as_bytes()),
                ValueRef::Real(r) => hasher.update(format!("R{r}|").as_bytes()),
                ValueRef::Text(t) => {
                    hasher.update(b"T");
                    hasher.update(t);
                    hasher.update(b"|");
                }
                ValueRef::Blob(b) => {
                    hasher.update(b"B");
                    hasher.update(b);
                    hasher.update(b"|");
                }
            }
        }
        hasher.update(b"\n");
    }
    if !any {
        return "empty".to_owned();
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------------------
// Tier 2 app folders and the wire (tests/wire.rs, crates/privatium/tests/adapter.rs)
// ---------------------------------------------------------------------------------------

/// The smallest valid Tier 2 manifest for `slug`.
pub fn web_manifest(slug: &str) -> String {
    format!(
        "[app]\nslug = \"{slug}\"\ntitle = \"{slug}\"\nversion = \"1.0.0\"\napi = 1\ntier = \"web\"\n"
    )
}

/// A loadable Tier 2 app: the manifest and the smallest `web/index.html` that takes the
/// standard chrome — the three anchors and a main region (`spec/app-contract.md §5`) —
/// plus `files`.
pub fn write_web_app(apps_dir: &Path, slug: &str, files: &[(&str, &str)]) -> PathBuf {
    let index = format!(
        "<!doctype html><html lang=\"en\"><head><title>{slug}</title></head>\
         <body><main id=\"main\"><p>{slug} index</p></main></body></html>\n"
    );
    let mut all: Vec<(&str, &str)> = vec![("web/index.html", &index)];
    all.extend_from_slice(files);
    write_app(apps_dir, slug, Some(&web_manifest(slug)), &all)
}
