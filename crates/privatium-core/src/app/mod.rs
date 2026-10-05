// This file is part of Privatium
// crates/privatium-core/src/app/mod.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-02
// Last Modified: 2026-10-04
// Summary: The app loader — the lifecycle of spec/app-contract.md §8 up to and including mount.
//          Discovers app folders, refuses per app and loudly (§3.1), keeps sys_app as events
//          (§3.4), and owns each app's log, store and — for Tier 1 — its Lua host. The node's
//          one public write path, `Node::append_batch`, is here too, beside `append`,
//          load_seed, `open_app` (an embedder's app with no folder, §2.3) and `subscribe`, so
//          nothing above this module ever holds an app's log.
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
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Bytes;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::Mode;
use crate::log::{self, AppLog, Durability};
use crate::lua::Host;
use crate::store::{self, Schema, Store, StoreError};
use crate::{
    Error, Node, Result, audit_recovery, audit_restore, boxed, io_at, new_ulid, note_health, sys,
};

pub mod csp;
pub mod examples;
pub mod manifest;
pub mod scaffold;
pub mod seed;

pub use csp::Csp;
pub use manifest::{
    Chrome, MANIFEST_FILE, MAX_ADVERTISED_SLUG, Manifest, ManifestError, MenuItem, Permissions,
    RESERVED_SLUGS, SUPPORTED_API, Tier, Ui, Widening,
};
pub use seed::{SEED_PATH, SeedError, SeedEvent};

/// The optional typed-table declaration (`spec/app-contract.md §4.5`).
const SCHEMA_FILE: &str = "schema.sql";

/// Where an app folder came from — `sys_app.source` (`spec/data-dictionary.md §3.4`).
///
/// `bundled` is a folder shipped with the framework: the repository's `apps/` in a
/// development checkout, the package's at install (`apps/README.md`). `local` is the
/// owner's `<data-root>/apps/`. `url:<origin>` is reserved; there is no registry in `pv/1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// `<data-root>/apps/<slug>/`, writable, survives upgrades.
    Local,
    /// Ships with the framework, read-only at runtime.
    Bundled,
}

impl Source {
    /// The `sys_app.source` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Bundled => "bundled",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A directory holding app folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRoot {
    /// The directory. Absent is the same as empty.
    pub dir: PathBuf,
    /// What every folder inside it is recorded as.
    pub source: Source,
}

impl AppRoot {
    /// The owner's `<data-root>/apps/`.
    #[must_use]
    pub fn local(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            source: Source::Local,
        }
    }

    /// The framework's own `apps/`.
    #[must_use]
    pub fn bundled(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            source: Source::Bundled,
        }
    }
}

/// Where in `spec/app-contract.md §8`'s lifecycle an app was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// `app.toml` could not be read or is not the manifest `§3` describes.
    Parse,
    /// `§3.1`: slug, folder, `api`, and the rest of `§3`'s constraints.
    Validate,
    /// `§8`'s tier step: `app.lua` or `web/index.html` present, and for Tier 1 `app.lua`
    /// loaded into the VM pool with its routes registered identically in every VM.
    Tier,
    /// `schema.sql` does not declare tables the engine will accept.
    Schema,
    /// Another folder already owns the slug (`§3.1`).
    Collision,
    /// Opening the log or building the cache failed.
    Materialize,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Validate => "validate",
            Self::Tier => "tier",
            Self::Schema => "schema",
            Self::Collision => "collision",
            Self::Materialize => "materialize",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One folder that was not loaded, and why. The node started anyway (`§3.1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    /// The folder name — the would-be slug.
    pub folder: String,
    /// Which root it was found under.
    pub source: Source,
    /// Where it stopped.
    pub stage: Stage,
    /// The reason, as `sys_app.last_error` carries it.
    pub reason: String,
}

impl fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, {}): {}",
            self.folder, self.source, self.stage, self.reason
        )
    }
}

/// Something the owner should hear about an app that did load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// A non-default permission (`spec/app-contract.md §5.4`).
    Permission {
        /// The app.
        slug: String,
        /// What it asked for.
        widening: Widening,
    },
    /// `nav.advertise = true` on a slug longer than a DNS label allows
    /// (`spec/protocol.md §6.1`: MUST be surfaced at load). Owed whether or not discovery
    /// is running, because the owner finds out at load or not at all.
    SlugTooLongToAdvertise {
        /// The app.
        slug: String,
    },
    /// `[node] mode = "solo"` names an app that did not load, so nothing is mounted at `/`.
    SoloAppNotLoaded {
        /// The app.
        slug: String,
    },
    /// A route of the solo app matches a framework prefix and is shadowed by it
    /// (`spec/protocol.md §9.1`: MUST warn at load, naming the route and the prefix). For a
    /// Tier 2 app the routes are the paths under `web/`, so this is a top-level entry there;
    /// for a Tier 1 app it is a pattern `app.lua` registered.
    RouteShadowed {
        /// The app.
        slug: String,
        /// The app's route.
        route: String,
        /// The framework prefix that wins.
        prefix: &'static str,
    },
    /// `[app] icon` names an icon the vendored set lacks (`docs/icons.md`): the launcher
    /// shows `question-circle` in its place, and the app loads.
    UnknownIcon {
        /// The app.
        slug: String,
        /// The name that was asked for.
        icon: String,
    },
    /// An HTML document under `web/` lacks one of the three anchors the standard chrome
    /// is inserted at (`spec/app-contract.md §5`), so it is served as written, without
    /// the bar and footer. Named at load rather than discovered by a missing bar.
    ChromeAnchorMissing {
        /// The app.
        slug: String,
        /// The file beneath the app folder, as `web/index.html`.
        file: String,
        /// The tag that was not found: `</head>`, `<body>` or `</body>`.
        anchor: &'static str,
    },
}

impl Warning {
    /// The app the warning is about.
    #[must_use]
    pub fn slug(&self) -> &str {
        match self {
            Self::Permission { slug, .. }
            | Self::SlugTooLongToAdvertise { slug }
            | Self::SoloAppNotLoaded { slug }
            | Self::RouteShadowed { slug, .. }
            | Self::UnknownIcon { slug, .. }
            | Self::ChromeAnchorMissing { slug, .. } => slug,
        }
    }

    /// The heading the owner reads the warning under, in sentence case. A permission
    /// widening is a privacy warning, because it says what the app can reach or run
    /// (`spec/app-contract.md §5.4`); everything else is a load warning about the app's
    /// own shape.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Permission { .. } => "Privacy warning",
            _ => "Load warning",
        }
    }

    /// The text without the app's slug, for a surface that already names the app.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::Permission { widening, .. } => widening.to_string(),
            other => {
                let text = other.to_string();
                match text.strip_prefix(&format!("{}: ", other.slug())) {
                    Some(rest) => rest.to_owned(),
                    None => text,
                }
            }
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permission { slug, widening } => write!(f, "{slug}: {widening}"),
            Self::SlugTooLongToAdvertise { slug } => write!(
                f,
                "{slug}: longer than {MAX_ADVERTISED_SLUG} characters, so it cannot be \
                 advertised as a DNS-SD subtype (spec/protocol.md §6.1)"
            ),
            Self::SoloAppNotLoaded { slug } => write!(
                f,
                "{slug}: named by [node] app in solo mode but not loaded; nothing is mounted at /"
            ),
            Self::RouteShadowed {
                slug,
                route,
                prefix,
            } => write!(
                f,
                "{slug}: route {route} is shadowed by the framework prefix {prefix} in solo mode \n                 (spec/protocol.md §9.1)"
            ),
            Self::UnknownIcon { slug, icon } => write!(
                f,
                "{slug}: icon {icon:?} is not in the vendored Bootstrap Icons set; question-circle \n                 is shown instead (docs/icons.md)"
            ),
            Self::ChromeAnchorMissing { slug, file, anchor } => write!(
                f,
                "{slug}: {file} has no {anchor}, so the standard chrome is not inserted and the \
                 document is served as written; add the tag, or set [ui] chrome = \"none\" \
                 (spec/app-contract.md §5)"
            ),
        }
    }
}

/// What one [`Node::load_apps`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Loaded, in discovery order.
    pub loaded: Vec<String>,
    /// Valid, indexed, and `enabled = false`: data kept, nothing materialized or mounted.
    pub disabled: Vec<String>,
    /// Refused, in discovery order. Each also has an `app.load_failed` audit row, once.
    pub failed: Vec<LoadFailure>,
    /// Indexed apps whose folder was not found under any root, now marked
    /// `last_error = "folder missing"`.
    pub missing: Vec<String>,
    /// Everything surfaced to the owner.
    pub warnings: Vec<Warning>,
}

/// What [`Node::load_seed`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seeded {
    /// The app.
    pub slug: String,
    /// Events appended, as this node's, in one batch.
    pub events: usize,
    /// The batch as written, for whatever reacts to an append — `pv.on('append')` in a
    /// Tier 1 app. `None` when the seed was empty.
    pub appended: Option<Appended>,
}

/// One event to append (`spec/app-contract.md §2.3`, `§6`): a `put` with `d`, or a
/// tombstone without (`spec/protocol.md §4.1`). The id is the caller's — minted by it
/// ([`new_ulid`](crate::new_ulid)), or a key it chose (`§4.1`, `§4.6`). `seq`, `lam`,
/// `ts`, `dev` and `app` are the node's to stamp, never the caller's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The table.
    pub tbl: String,
    /// The row key.
    pub id: String,
    /// The column values, or `None` for a `del`.
    pub d: Option<serde_json::Value>,
}

impl Event {
    /// A `put`: row `id` of `tbl` becomes `d`, a JSON object of column values
    /// (`spec/protocol.md §4.1`, `§4.5`). A value naming a declared column is typed on
    /// the way in ([`Node::append_batch`]).
    pub fn put(tbl: impl Into<String>, id: impl Into<String>, d: serde_json::Value) -> Self {
        Self {
            tbl: tbl.into(),
            id: id.into(),
            d: Some(d),
        }
    }

    /// A `del`: a tombstone for row `id` of `tbl` (`spec/protocol.md §4.6`).
    pub fn del(tbl: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            tbl: tbl.into(),
            id: id.into(),
            d: None,
        }
    }
}

/// What one [`Node::append`] wrote: one batch, one `ts`, contiguous `seq` and `lam` from
/// the first event's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended {
    /// The batch's `ts`.
    pub ts: String,
    /// The first event's `seq`; each later one is one higher.
    pub seq: u64,
    /// The first event's `lam`; each later one is one higher.
    pub lam: u64,
    /// This node's ID.
    pub dev: String,
    /// The app.
    pub app: String,
    /// The events, in order.
    pub events: Vec<Event>,
    /// The log lines as written, one per change and without the newline — the bytes on
    /// disk, never a re-serialization (`spec/protocol.md §4.2`).
    pub lines: Vec<Vec<u8>>,
}

/// What an app's live stream carries (`spec/data-api.md §3`): every event this node
/// appends to the app, and a notice that its cache was rebuilt underneath a reader.
///
/// Sent from [`Node::append`] and [`Node::refresh_app`] on the app's broadcast channel
/// ([`App::stream`]); the data API's `/api/stream` is one subscriber, and there may be
/// none, which is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// One event, as its log line.
    Append {
        /// The event's `lam`, so a subscriber resuming from `after=` can filter.
        lam: u64,
        /// The line, byte for byte, without the newline.
        line: Bytes,
    },
    /// The cache was rematerialized: a changed `schema.sql`, a restore, a log that grew
    /// behind the tables. A reader re-queries.
    Resync {
        /// Why — `rematerialized`.
        reason: &'static str,
        /// The app's Lamport high-water mark now.
        lam: u64,
    },
}

/// How many events a slow stream subscriber may fall behind before it is told to
/// resync rather than handed the backlog.
pub const STREAM_CAPACITY: usize = 1024;

/// A loaded app: its manifest, its folder, its log, its cache, and for Tier 1 its VMs.
#[derive(Debug)]
pub struct App {
    manifest: Manifest,
    manifest_hash: String,
    /// The folder — `None` for an app an embedder opened with no folder
    /// ([`Node::open_app`]).
    dir: Option<PathBuf>,
    source: Source,
    mount: Option<String>,
    csp: Csp,
    warnings: Vec<Warning>,
    pub(crate) log: AppLog,
    pub(crate) store: Store,
    /// The VM pool, for a Tier 1 app. Shared with a request in flight, which runs its
    /// handler outside the node lock.
    lua: Option<Arc<Host>>,
    /// The code files as last loaded, so the read path notices an edit (`spec/cli.md §3`).
    fingerprint: Fingerprint,
    /// Why the last reload failed, while it stands: the app as last loaded is kept but
    /// not served until the next edit succeeds.
    reload_error: Option<ReloadError>,
    /// The live stream (`spec/data-api.md §3`): every append and every resync, to
    /// whoever is subscribed. Survives a reload; only the load creates one.
    pub(crate) stream: broadcast::Sender<StreamEvent>,
    /// Durable received batches awaiting Lua callbacks outside the node lock.
    pub(crate) unfired: Vec<(Appended, String)>,
}

/// `(mtime, len)` of every file whose change reloads the app in place: `app.toml`,
/// `app.lua`, `schema.sql` and `lib/**/*.lua`. Templates are the Lua host's own stat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Fingerprint(BTreeMap<PathBuf, (Option<SystemTime>, u64)>);

impl Fingerprint {
    /// Stat the files now. A file that cannot be read is simply absent, and an app with
    /// no folder has nothing to stat, so nothing ever changes.
    fn take(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::default();
        };
        let mut files = BTreeMap::new();
        for name in [MANIFEST_FILE, "app.lua", SCHEMA_FILE] {
            let path = dir.join(name);
            if let Ok(meta) = fs::metadata(&path)
                && meta.is_file()
            {
                files.insert(path, (meta.modified().ok(), meta.len()));
            }
        }
        Self::walk_lua(&dir.join("lib"), &mut files);
        Self(files)
    }

    fn walk_lua(dir: &Path, into: &mut BTreeMap<PathBuf, (Option<SystemTime>, u64)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                Self::walk_lua(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "lua") {
                into.insert(path, (meta.modified().ok(), meta.len()));
            }
        }
    }
}

/// What a failed reload was about, so a fix to the right file clears it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReloadError {
    /// `app.toml`, `app.lua`, `lib/` or `schema.sql`: cleared by the next code edit
    /// that loads.
    Code(String),
    /// A template: cleared by the next `views/` edit that compiles.
    Views(String),
}

impl ReloadError {
    fn reason(&self) -> &str {
        match self {
            Self::Code(reason) | Self::Views(reason) => reason,
        }
    }
}

impl App {
    /// The slug.
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.manifest.app.slug
    }

    /// `app.toml`'s title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.manifest.app.title
    }

    /// Why the app is not being served since its last edit, if a reload failed.
    #[must_use]
    pub fn reload_error(&self) -> Option<&str> {
        self.reload_error.as_ref().map(ReloadError::reason)
    }

    /// `app.toml`, parsed.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// SHA-256 of `app.toml` — `sys_app.manifest_hash`.
    #[must_use]
    pub fn manifest_hash(&self) -> &str {
        &self.manifest_hash
    }

    /// The folder — `None` for an app an embedder opened with none ([`Node::open_app`]).
    #[must_use]
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Where the folder came from.
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// The mount path — `/a/<slug>/` in host mode, `/` for the solo app — or `None` for
    /// an app that exists without being served: a tier 3 index entry, or any other app in
    /// solo mode. This is where `§8`'s "mount" lands, and what the router reads.
    #[must_use]
    pub fn mount(&self) -> Option<&str> {
        self.mount.as_deref()
    }

    /// The app's Content-Security-Policy, computed at load.
    #[must_use]
    pub fn csp(&self) -> &Csp {
        &self.csp
    }

    /// What the owner was told at load.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The app's log. This node's writer, and the Lamport counter.
    #[must_use]
    pub fn log(&self) -> &AppLog {
        &self.log
    }

    /// The app's cache. [`Store::app_conn`] is the sandboxed read-only connection app SQL
    /// runs on; the framework's own is [`Store::conn`].
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The store, for the node-level maintenance.
    pub(crate) fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// The Lua host, for a Tier 1 app (`spec/lua-api.md`).
    #[must_use]
    pub fn lua_host(&self) -> Option<&Arc<Host>> {
        self.lua.as_ref()
    }

    /// `sample/seed.jsonl`, if the folder ships one (`spec/app-contract.md §9`).
    #[must_use]
    pub fn seed_path(&self) -> Option<PathBuf> {
        let path = self.dir.as_ref()?.join(SEED_PATH);
        path.is_file().then_some(path)
    }

    /// The app's live stream, to subscribe to (`spec/data-api.md §3`). Subscribe under
    /// the node lock and read the log under the same lock, and nothing appended in
    /// between can be missed: appends take the lock too.
    #[must_use]
    pub fn stream(&self) -> &broadcast::Sender<StreamEvent> {
        &self.stream
    }
}

/// A folder found under a root, before anything was read from it.
struct Candidate {
    folder: String,
    dir: PathBuf,
    source: Source,
}

/// Every app folder under `root`, by name.
///
/// A folder whose name starts with `_` is not an app: `_sys` (`docs/plans/phase-1.md
/// §2.6`) and the lint corpus `_lint` alike. Files are not apps either. Everything else is,
/// and a folder with no `app.toml` is refused loudly rather than skipped, because
/// `spec/app-contract.md` defines an app as a folder containing one.
fn discover(root: &AppRoot) -> Result<Vec<Candidate>> {
    let entries = match fs::read_dir(&root.dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_at(&root.dir)(error)),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_at(&root.dir))?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        if folder.starts_with('_') {
            continue;
        }
        candidates.push(Candidate {
            folder,
            dir,
            source: root.source,
        });
    }
    candidates.sort_by(|a, b| a.folder.cmp(&b.folder));
    Ok(candidates)
}

/// Everything `§8` learns about an app before it touches the log.
struct Prepared {
    manifest: Manifest,
    manifest_hash: String,
    schema: Schema,
    mount: Option<String>,
    csp: Csp,
    warnings: Vec<Warning>,
    lua: Option<Arc<Host>>,
}

/// Where `prepare` stopped, with whatever it had learned by then — the row still gets
/// written with it (`spec/data-dictionary.md §3.4`).
struct Refused {
    stage: Stage,
    reason: String,
    manifest: Option<Manifest>,
    manifest_hash: Option<String>,
}

enum Outcome {
    Loaded(Box<App>),
    Disabled,
    Refused(LoadFailure),
}

impl Node {
    /// Step 5 of `docs/plans/phase-1.md §2.6`: load every app folder under `roots`, in
    /// root order and by name within a root, through `spec/app-contract.md §8` up to and
    /// including mount.
    ///
    /// Refusal is per app and loud (`§3.1`): a folder that fails is in the report and in
    /// `sys_audit` as `app.load_failed`, its `sys_app` row carries `last_error`, and the
    /// node goes on. This never returns `Err` for anything one app did; an `Err` is the
    /// node's own trouble — a root that cannot be read, a `_sys` log that cannot be
    /// written.
    ///
    /// A slug already loaded from the same folder is reloaded in place; from a different
    /// folder it is a collision and the newcomer is refused. Indexed apps found under no
    /// root are marked `last_error = "folder missing"` (`§3.4`) — so pass every root at
    /// once rather than one call per root.
    ///
    /// Nothing here loads `sample/seed.jsonl`. That is [`load_seed`](Self::load_seed), on
    /// the owner's say-so (`§9`).
    pub fn load_apps(&mut self, roots: &[AppRoot]) -> Result<LoadReport> {
        let cutoff = store::cutoff_now();
        // `sys.sys_app` and `sys.sys_audit` have to be current: the upsert compares
        // against the row, and the audit is bounded by the last one.
        self.store.refresh(&cutoff).map_err(boxed)?;

        let mut report = LoadReport::default();
        let mut claimed: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in roots {
            for candidate in discover(root)? {
                let elsewhere = claimed.contains_key(&candidate.folder)
                    || self
                        .apps
                        .get(&candidate.folder)
                        .is_some_and(|app| app.dir.as_deref() != Some(candidate.dir.as_path()));
                if elsewhere {
                    let failure = LoadFailure {
                        folder: candidate.folder.clone(),
                        source: candidate.source,
                        stage: Stage::Collision,
                        reason: "collides with an app of the same slug that was installed first \
                                 (spec/app-contract.md §3.1)"
                            .to_owned(),
                    };
                    self.audit_load_failed(&failure, None)?;
                    report.failed.push(failure);
                    continue;
                }
                claimed.insert(candidate.folder.clone(), candidate.dir.clone());
                // A reload: the old store is dropped and a fresh one opened.
                self.apps.remove(&candidate.folder);

                match self.load_one(&candidate, &cutoff)? {
                    Outcome::Loaded(app) => {
                        report.warnings.extend(app.warnings.iter().cloned());
                        report.loaded.push(candidate.folder.clone());
                        self.apps.insert(candidate.folder, *app);
                    }
                    Outcome::Disabled => report.disabled.push(candidate.folder),
                    Outcome::Refused(failure) => report.failed.push(failure),
                }
            }
        }

        // `§3.4`: removing a folder MUST NOT delete the index row or the data.
        for slug in self.indexed_slugs()? {
            if claimed.contains_key(&slug) || self.apps.contains_key(&slug) {
                continue;
            }
            if self.mark_folder_missing(&slug)? {
                report.missing.push(slug);
            }
        }

        if let (Mode::Solo, Some(solo)) = (self.config.node.mode, self.config.node.app.as_deref())
            && !self.apps.contains_key(solo)
        {
            report.warnings.push(Warning::SoloAppNotLoaded {
                slug: solo.to_owned(),
            });
        }

        self.store.refresh(&cutoff).map_err(boxed)?;
        // The `apps` key of the TXT record follows what is mounted (`spec/protocol.md §6.1`).
        self.publish_facts()?;
        self.flush()?;
        Ok(report)
    }

    /// Every loaded app, by slug.
    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.values()
    }

    /// One loaded app.
    #[must_use]
    pub fn app(&self, slug: &str) -> Option<&App> {
        self.apps.get(slug)
    }

    /// The mount table: `(path, app)` for every app that is served, which is what the
    /// router is built from.
    pub fn mounts(&self) -> impl Iterator<Item = (&str, &App)> {
        self.apps
            .values()
            .filter_map(|app| app.mount.as_deref().map(|mount| (mount, app)))
    }

    /// `sample/seed.jsonl` for a loaded app, if it ships one — the offer
    /// `spec/app-contract.md §9` describes, for whichever surface makes it.
    #[must_use]
    pub fn seed_available(&self, slug: &str) -> Option<PathBuf> {
        self.apps.get(slug).and_then(App::seed_path)
    }

    /// Load `sample/seed.jsonl` into `slug`, on the owner's explicit say-so.
    ///
    /// Never called by [`load_apps`](Self::load_apps). Refused when the app's log already
    /// holds an event from any device, so a seed can only ever populate an empty app.
    /// Every line is appended through the app's own log as one batch of **this node's**
    /// events — fresh `seq`, `lam`, `ts` and `dev` — and applied to the cache
    /// incrementally; the seed's own envelope fields are discarded (`seed`).
    pub fn load_seed(&mut self, slug: &str) -> Result<Seeded> {
        let app = self.apps.get_mut(slug).ok_or_else(|| Error::AppNotLoaded {
            slug: slug.to_owned(),
        })?;
        let path = app.seed_path().ok_or_else(|| Error::NoSeed {
            slug: slug.to_owned(),
        })?;
        if app.log.seq() > 0 || !app.log.heads().is_empty() {
            return Err(Error::SeedRefused {
                slug: slug.to_owned(),
                events: app.log.heads().values().sum(),
            });
        }

        let text = fs::read_to_string(&path).map_err(io_at(&path))?;
        let events = seed::parse(&text).map_err(|error| Error::Seed {
            path: path.clone(),
            line: error.line,
            problem: error.problem,
        })?;
        if events.is_empty() {
            return Ok(Seeded {
                slug: slug.to_owned(),
                events: 0,
                appended: None,
            });
        }

        let changes: Vec<Event> = events
            .into_iter()
            .map(|event| Event {
                tbl: event.tbl,
                id: event.id,
                d: event.d,
            })
            .collect();
        let appended = self.append_batch(slug, changes)?;
        Ok(Seeded {
            slug: slug.to_owned(),
            events: appended.events.len(),
            appended: Some(appended),
        })
    }

    /// `ui.date_format` as the typed writes read it (`spec/lua-api.md §3.3`): whether a
    /// `3/9` is March or September. What [`append_batch`](Self::append_batch) normalizes
    /// with, and what the data API normalizes with when it compares a replayed event
    /// against the log (`spec/data-api.md §2`).
    pub fn date_order(&self) -> Result<store::normalize::DateOrder> {
        Ok(store::normalize::DateOrder::from_setting(
            self.setting_value("ui.date_format")?
                .and_then(|text| serde_json::from_str::<String>(&text).ok())
                .as_deref()
                .unwrap_or("iso"),
        ))
    }

    /// Append one event to a loaded app (`spec/app-contract.md §6`, `append`): a batch of
    /// one, so everything [`append_batch`](Self::append_batch) says holds here.
    pub fn append(&mut self, slug: &str, event: Event) -> Result<Appended> {
        self.append_batch(slug, vec![event])
    }

    /// Append `changes` to a loaded app as one batch of this node's events, and apply the
    /// batch to the cache incrementally, in one transaction (`docs/plans/phase-1.md
    /// §2.3`; `spec/app-contract.md §6`, `append_batch`). A cache the apply cannot update
    /// is rebuilt from the log before this returns, and the stream hears `resync`.
    ///
    /// The one public write path: [`append`](Self::append), `pv.append`, `pv.delete` and
    /// `pv.batch` reach the log through here (`spec/lua-api.md §3.3`), and so does
    /// [`load_seed`](Self::load_seed). All or nothing — the log writer stages the batch
    /// and writes it once — with one `ts` and contiguous `seq`, as `§3.3` promises. An
    /// empty `changes` writes nothing.
    ///
    /// **Typed writes.** Every value that names a column `schema.sql` declares is checked
    /// and normalized first (`store::normalize`): digits for `BIGINT` and `DECIMAL`, a
    /// boolean for `BOOLEAN`, and the ISO spelling for `DATE`, `TIME` and `TIMESTAMPTZ`
    /// from whatever form was typed, with `ui.date_format` deciding whether `3/9` is
    /// March or September. A value that is not its type refuses the whole batch with
    /// [`Error::Value`] naming the column, before anything is written. Then every put is
    /// held against the schema's `NOT NULL` and `CHECK` constraints (`store::validate`),
    /// and a violation refuses the batch with [`Error::Constraint`] naming the index
    /// (`spec/data-api.md §2`, `spec/lua-api.md §3.3`).
    ///
    /// Once written, each line goes out on the app's stream ([`App::stream`]).
    pub fn append_batch(&mut self, slug: &str, events: Vec<Event>) -> Result<Appended> {
        self.drain()?;
        let mut changes = events;
        let order = self.date_order()?;
        let bound = crate::wire::ApiSettings::read(self).max_body;
        let app = self.apps.get_mut(slug).ok_or_else(|| Error::AppNotLoaded {
            slug: slug.to_owned(),
        })?;
        for (index, change) in changes.iter_mut().enumerate() {
            if let (Some(serde_json::Value::Object(d)), Some(table)) =
                (change.d.as_mut(), app.store.schema().table(&change.tbl))
            {
                store::normalize::normalize_row(table, d, order).map_err(|refused| {
                    Error::Value {
                        app: slug.to_owned(),
                        tbl: change.tbl.clone(),
                        index,
                        column: refused.column,
                        problem: refused.problem,
                    }
                })?;
            }
        }
        store::validate(
            app.store.schema(),
            changes
                .iter()
                .map(|change| (change.tbl.as_str(), change.id.as_str(), change.d.as_ref())),
        )
        .map_err(|violation| Error::Constraint {
            app: slug.to_owned(),
            tbl: violation.tbl,
            index: violation.index,
            problem: violation.problem,
        })?;
        let dev = self.identity.id().as_str().to_owned();
        if changes.is_empty() {
            return Ok(Appended {
                ts: log::now(),
                seq: app.log.seq().saturating_add(1),
                lam: app.log.lam().saturating_add(1),
                dev,
                app: slug.to_owned(),
                events: changes,
                lines: Vec::new(),
            });
        }

        let mut ts = String::new();
        app.log.set_append_bound(bound);
        let lines = app.log.batch(|batch| {
            ts = batch.ts().to_owned();
            for change in &changes {
                match &change.d {
                    Some(d) => batch.put(&change.tbl, &change.id, d)?,
                    None => batch.del(&change.tbl, &change.id)?,
                }
            }
            Ok(())
        })?;
        // The batch is durable; now the cache, in one transaction. This is the mutation of
        // a derived table that `docs/plans/phase-1.md §2.3` describes, not a write to a
        // log — no code here touches one.
        let applied = app.store.apply_batch(
            changes
                .iter()
                .map(|change| (change.tbl.as_str(), change.id.as_str(), change.d.as_ref())),
        );
        let rebuilt = match applied {
            Ok(()) => false,
            Err(apply) => {
                // The log holds the batch and the tables no longer match it. Rebuild from
                // the log now, under the lock the caller already holds, so no read sees
                // the mismatch. A rebuild that fails too leaves the watermark stale for
                // the next read to try again, and is the error the caller gets — the
                // write itself is not in question.
                app.store
                    .restore(&store::cutoff_now())
                    .map_err(|rebuild| store::StoreError::Schema {
                        problem: format!(
                            "the incremental apply failed ({apply}) and the rebuild that \
                             followed failed too: {rebuild}"
                        ),
                    })
                    .map_err(boxed)?;
                true
            }
        };
        app.log.save_to(&mut self.state);
        app.store.save_to(&mut self.state);
        self.state.flush()?;

        let count = changes.len() as u64;
        let seq = app.log.seq().saturating_add(1).saturating_sub(count);
        let lam = app.log.lam().saturating_add(1).saturating_sub(count);
        // Durable now, so the stream may say so. No subscriber is not an error. A cache
        // that had to be rebuilt says `resync` — the tables changed underneath every
        // reader — rather than handing out lines the rebuild already reflects.
        if rebuilt {
            let _ = app.stream.send(StreamEvent::Resync {
                reason: "rematerialized",
                lam: app.log.lam(),
            });
        } else {
            for (offset, line) in lines.iter().enumerate() {
                let _ = app.stream.send(StreamEvent::Append {
                    lam: lam.saturating_add(offset as u64),
                    line: Bytes::from(line.clone()),
                });
            }
        }
        self.notify_append();
        Ok(Appended {
            ts,
            seq,
            lam,
            dev,
            app: slug.to_owned(),
            events: changes,
            lines,
        })
    }

    /// Open an app this program owns and no folder holds — embedded mode
    /// (`spec/app-contract.md §2.3`, `§6`). `schema` is the text `schema.sql` would carry,
    /// or empty for the log as a document store (`§4.5`). The app gets what a folder's
    /// gets and nothing more: its log under `data/<slug>/`, its cache through the
    /// three-tier restore, and its stream ([`subscribe`](Self::subscribe)); no mount, no
    /// Lua host, and no `sys_app` row, since the index is of folders
    /// (`spec/data-dictionary.md §3.4`). Call it at every start, after
    /// [`open`](Self::open). A second call for the same slug reopens it, as a folder's
    /// reload does, and a changed `schema` rematerializes the cache.
    ///
    /// Refused ([`Error::AppRefused`]) for a slug that is reserved or malformed
    /// (`spec/protocol.md §1.1`) and for one a folder already holds
    /// (`spec/app-contract.md §3.1`).
    pub fn open_app(&mut self, slug: &str, schema: &str) -> Result<()> {
        let refused = |reason: String| Error::AppRefused {
            slug: slug.to_owned(),
            reason,
        };
        if manifest::is_reserved(slug) {
            return Err(refused(
                "a reserved slug (spec/protocol.md §1.1)".to_owned(),
            ));
        }
        if !manifest::is_valid_slug(slug) {
            return Err(refused(
                "not a slug — ^[a-z][a-z0-9-]{1,30}$ (spec/protocol.md §1.1)".to_owned(),
            ));
        }
        if let Some(dir) = self.apps.get(slug).and_then(|app| app.dir.as_deref()) {
            return Err(refused(format!(
                "already loaded from the folder {} (spec/app-contract.md §3.1)",
                dir.display()
            )));
        }
        let schema = Schema::parse(schema).map_err(boxed)?;
        // A Tier 3 app's manifest, as `privatium new --tier rust` would write it, so the
        // `App` an embedder opens answers `manifest()` like any other.
        let text = format!(
            "[app]\nslug = {slug:?}\ntitle = {slug:?}\nversion = \"0.0.0\"\napi = {SUPPORTED_API}\n\
             tier = \"rust\"\n"
        );
        let manifest = Manifest::parse(&text).map_err(|error| refused(error.to_string()))?;
        let manifest_hash = manifest::manifest_hash(&text);

        // A reopen: the old store is dropped and a fresh one opened, as `load_apps` does.
        self.apps.remove(slug);
        let (log, store) = self.materialize(slug, schema, &store::cutoff_now())?;
        self.apps.insert(
            slug.to_owned(),
            App {
                manifest,
                manifest_hash,
                dir: None,
                source: Source::Local,
                mount: None,
                csp: Csp::for_app(None, &Permissions::default()),
                warnings: Vec::new(),
                log,
                store,
                lua: None,
                fingerprint: Fingerprint::default(),
                reload_error: None,
                stream: broadcast::channel(STREAM_CAPACITY).0,
                unfired: Vec::new(),
            },
        );
        self.flush()
    }

    /// The app's live stream (`spec/app-contract.md §6`, `subscribe`): every event this
    /// node appends to `slug` from now on, as its log line with its `lam`, and a notice
    /// when the cache was rebuilt underneath a reader ([`StreamEvent`]). Subscribe
    /// before the query whose rows the events should follow and nothing appended in
    /// between is missed; a receiver more than [`STREAM_CAPACITY`] events behind is told
    /// so by the channel and re-queries. The data API's `/api/stream` is one such
    /// subscriber (`spec/data-api.md §3`).
    pub fn subscribe(&self, slug: &str) -> Result<broadcast::Receiver<StreamEvent>> {
        self.apps
            .get(slug)
            .map(|app| app.stream.subscribe())
            .ok_or_else(|| Error::AppNotLoaded {
                slug: slug.to_owned(),
            })
    }

    /// One `sys_setting` value, as the JSON text `§3.6` stores, if the key is set.
    pub fn setting_value(&self, key: &str) -> Result<Option<String>> {
        match self.store.conn().query_row(
            &format!("SELECT value FROM {} WHERE id = ?", sys::SETTING),
            rusqlite::params![key],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(value) => Ok(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(boxed(StoreError::Sql(error))),
        }
    }

    /// `lua.limit_exceeded` (`spec/lua-api.md §5`, `spec/data-dictionary.md §3.10`): a Tier 1
    /// request of `slug` exceeded a `[lua]` limit. `detail` is a JSON object naming the
    /// route, the limit and what was measured.
    pub fn audit_lua_limit(&mut self, slug: &str, detail: &str) -> Result<()> {
        let at = log::now();
        self.sys.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::warn(&at, sys::KIND_LUA_LIMIT_EXCEEDED, Some(slug), detail),
        )?;
        Ok(())
    }

    /// Bring a loaded app up to date with its folder and its log, per request — the
    /// `echo >>` reload of `apps/hello/README.md`, and the edit loop of `spec/cli.md §3`
    /// and `spec/lua-api.md §7`: an edited `app.lua`, `lib/`, `app.toml` or `schema.sql`
    /// reloads the app in place through `§8`'s steps with its routes re-registered and,
    /// for a new schema, the cache rematerialized; an edited template is recompiled; a
    /// log that grew is applied. No restart, ever.
    ///
    /// A stat decides; only a change pays. A reload that fails — a syntax error just
    /// saved — leaves the app as last loaded, records the reason in `sys_app.last_error`,
    /// and is [`Error::AppReloadFailed`] on this and every later request until an edit
    /// loads again: the error is what the author is shown, not the code from before it.
    /// Returns whether the cache was rebuilt.
    pub fn refresh_app(&mut self, slug: &str) -> Result<bool> {
        self.drain()?;
        let code_changed = {
            let app = self.apps.get_mut(slug).ok_or_else(|| Error::AppNotLoaded {
                slug: slug.to_owned(),
            })?;
            let now = Fingerprint::take(app.dir.as_deref());
            let changed = now != app.fingerprint;
            app.fingerprint = now;
            changed
        };
        let mut rebuilt = false;
        if code_changed {
            match self.reload_app(slug)? {
                Ok(schema_changed) => rebuilt = schema_changed,
                Err(reason) => {
                    if let Some(app) = self.apps.get_mut(slug) {
                        app.reload_error = Some(ReloadError::Code(reason));
                    }
                }
            }
        } else if let Some(outcome) = self
            .apps
            .get(slug)
            .and_then(|app| app.lua.as_ref())
            .filter(|host| host.views_changed())
            .map(|host| host.reload_views())
        {
            match outcome {
                Ok(()) => {
                    let fixed = self.apps.get_mut(slug).is_some_and(|app| {
                        let was_views = matches!(app.reload_error, Some(ReloadError::Views(_)));
                        if was_views {
                            app.reload_error = None;
                        }
                        was_views
                    });
                    if fixed {
                        self.record_reload_ok(slug)?;
                    }
                }
                Err(reason) => {
                    if let Some(app) = self.apps.get_mut(slug) {
                        app.reload_error = Some(ReloadError::Views(reason.clone()));
                    }
                    self.record_reload_failure(slug, reason)?;
                }
            }
        }

        let app = self.apps.get_mut(slug).ok_or_else(|| Error::AppNotLoaded {
            slug: slug.to_owned(),
        })?;
        if let Some(error) = &app.reload_error {
            return Err(Error::AppReloadFailed {
                slug: slug.to_owned(),
                reason: error.reason().to_owned(),
            });
        }
        if app.store.is_stale().map_err(boxed)? {
            // The log moved behind the tables — a line appended by hand, a copy restored
            // — so the counters follow it first (`spec/protocol.md §4.1`, `§4.3`): the
            // next event this node writes takes the next `seq` in its own file and a
            // `lam` past everything the file now holds.
            let recovered = app.log.rescan()?;
            audit_recovery(&mut self.sys, slug, &recovered)?;
            let previous = app.store.restore_record().cloned();
            app.store.refresh(&store::cutoff_now()).map_err(boxed)?;
            if let Some(restored) = app.store.restored().cloned() {
                audit_restore(&mut self.sys, slug, &restored, previous.as_ref(), false)?;
                note_health(&self.store, &app.store, slug)?;
            }
            self.flush()?;
            rebuilt = true;
        }
        if rebuilt {
            // The tables changed underneath every reader (`docs/plans/phase-1.md §8`, R5):
            // a hand-appended line, a restore, a new `schema.sql`. The stream says so.
            let app = self.apps.get(slug).ok_or_else(|| Error::AppNotLoaded {
                slug: slug.to_owned(),
            })?;
            let _ = app.stream.send(StreamEvent::Resync {
                reason: "rematerialized",
                lam: app.log.lam(),
            });
        }
        Ok(rebuilt)
    }

    /// A template edit that did not compile: `sys_app.last_error` says so, and the audit
    /// once (`spec/data-dictionary.md §3.4`), exactly as a load failure would.
    fn record_reload_failure(&mut self, slug: &str, reason: String) -> Result<()> {
        let existing = self.read_app_row(slug)?;
        let source = self.apps.get(slug).map_or(Source::Local, |app| app.source);
        if let Some(row) = &existing {
            let updated = sys::AppRow {
                last_error: Some(reason.clone()),
                updated_at: Some(log::now()),
                ..row.clone()
            };
            self.upsert_app_row(slug, &updated, Some(row))?;
        }
        let failure = LoadFailure {
            folder: slug.to_owned(),
            source,
            stage: Stage::Tier,
            reason,
        };
        self.audit_load_failed(&failure, existing.as_ref())?;
        self.flush()
    }

    /// The template edit that compiled again: the row's `last_error` is cleared.
    fn record_reload_ok(&mut self, slug: &str) -> Result<()> {
        if let Some(row) = self.read_app_row(slug)?
            && row.last_error.is_some()
        {
            let updated = sys::AppRow {
                last_error: None,
                updated_at: Some(log::now()),
                ..row.clone()
            };
            self.upsert_app_row(slug, &updated, Some(&row))?;
            self.flush()?;
        }
        Ok(())
    }

    /// Reload an app whose code files changed: `§8` again from "parse app.toml" through
    /// "compute CSP" — `prepare`, which is also what compiles `views/` and loads
    /// `app.lua` into a fresh pool with its routes — then the swap. The log is never
    /// reopened; the cache is reopened and rematerialized only when `schema.sql`'s hash
    /// moved (`spec/app-contract.md §4.5`). A request in flight holds the old host and
    /// finishes on it (`docs/plans/phase-1.md §8`, R3).
    ///
    /// The outer `Err` is the node's own trouble. The inner `Err` is the app's: nothing
    /// is swapped, the row carries `last_error`, the audit says so once.
    fn reload_app(&mut self, slug: &str) -> Result<std::result::Result<bool, String>> {
        let (dir, source, old_hash) = {
            let app = self.apps.get(slug).ok_or_else(|| Error::AppNotLoaded {
                slug: slug.to_owned(),
            })?;
            let Some(dir) = app.dir.clone() else {
                // An embedder's app has no folder to reload from (`open_app`), and its
                // fingerprint is empty, so this is not reached; said anyway.
                return Ok(Ok(false));
            };
            (dir, app.source, app.store.schema().hash.clone())
        };
        let candidate = Candidate {
            folder: slug.to_owned(),
            dir,
            source,
        };
        let existing = self.read_app_row(slug)?;
        let now = log::now();
        let prepared = match self.prepare(&candidate) {
            Ok(prepared) => prepared,
            Err(refused) => {
                let failure = LoadFailure {
                    folder: slug.to_owned(),
                    source,
                    stage: refused.stage,
                    reason: refused.reason,
                };
                let row = app_row(
                    &candidate,
                    refused.manifest.as_ref(),
                    None,
                    refused.manifest_hash,
                    existing.as_ref(),
                    Some(&failure.reason),
                    &now,
                );
                self.upsert_app_row(slug, &row, existing.as_ref())?;
                self.audit_load_failed(&failure, existing.as_ref())?;
                self.flush()?;
                return Ok(Err(failure.reason));
            }
        };

        let schema_changed = prepared.schema.hash != old_hash;
        let store = if schema_changed {
            Some(self.open_store(slug, prepared.schema.clone(), &store::cutoff_now())?)
        } else {
            None
        };
        let row = app_row(
            &candidate,
            Some(&prepared.manifest),
            Some(&prepared.schema.hash),
            Some(prepared.manifest_hash.clone()),
            existing.as_ref(),
            None,
            &now,
        );
        self.upsert_app_row(slug, &row, existing.as_ref())?;

        let app = self.apps.get_mut(slug).ok_or_else(|| Error::AppNotLoaded {
            slug: slug.to_owned(),
        })?;
        app.manifest = prepared.manifest;
        app.manifest_hash = prepared.manifest_hash;
        app.mount = prepared.mount;
        app.csp = prepared.csp;
        app.warnings = prepared.warnings;
        app.lua = prepared.lua;
        if let Some(store) = store {
            app.store = store;
        }
        app.reload_error = None;
        self.flush()?;
        Ok(Ok(schema_changed))
    }

    /// One folder through `§8`, writing its row and its audit as it goes.
    fn load_one(&mut self, candidate: &Candidate, cutoff: &str) -> Result<Outcome> {
        // Taken before anything is read, so an edit that lands while the app loads is
        // seen by the next request rather than missed.
        let fingerprint = Fingerprint::take(Some(&candidate.dir));
        // The row is keyed by the folder name (`spec/app-contract.md §3.1`: the slug
        // equals the folder). A folder that could never be a slug has no row to carry
        // `last_error`; it gets the audit alone.
        let keyable =
            manifest::is_valid_slug(&candidate.folder) && !manifest::is_reserved(&candidate.folder);
        let existing = if keyable {
            self.read_app_row(&candidate.folder)?
        } else {
            None
        };
        let now = log::now();

        let prepared = match self.prepare(candidate) {
            Ok(prepared) => prepared,
            Err(refused) => {
                let failure = LoadFailure {
                    folder: candidate.folder.clone(),
                    source: candidate.source,
                    stage: refused.stage,
                    reason: refused.reason,
                };
                if keyable {
                    let row = app_row(
                        candidate,
                        refused.manifest.as_ref(),
                        None,
                        refused.manifest_hash,
                        existing.as_ref(),
                        Some(&failure.reason),
                        &now,
                    );
                    self.upsert_app_row(&candidate.folder, &row, existing.as_ref())?;
                }
                self.audit_load_failed(&failure, existing.as_ref())?;
                return Ok(Outcome::Refused(failure));
            }
        };

        // `§8`: upsert sys_app, then materialize.
        let row = app_row(
            candidate,
            Some(&prepared.manifest),
            Some(&prepared.schema.hash),
            Some(prepared.manifest_hash.clone()),
            existing.as_ref(),
            None,
            &now,
        );
        self.upsert_app_row(&candidate.folder, &row, existing.as_ref())?;
        if !row.enabled {
            return Ok(Outcome::Disabled);
        }

        match self.materialize(&prepared.manifest.app.slug, prepared.schema.clone(), cutoff) {
            Ok((log, store)) => Ok(Outcome::Loaded(Box::new(App {
                manifest: prepared.manifest,
                manifest_hash: prepared.manifest_hash,
                dir: Some(candidate.dir.clone()),
                source: candidate.source,
                mount: prepared.mount,
                csp: prepared.csp,
                warnings: prepared.warnings,
                log,
                store,
                lua: prepared.lua,
                fingerprint,
                reload_error: None,
                stream: broadcast::channel(STREAM_CAPACITY).0,
                unfired: Vec::new(),
            }))),
            Err(error) => {
                let failure = LoadFailure {
                    folder: candidate.folder.clone(),
                    source: candidate.source,
                    stage: Stage::Materialize,
                    reason: error.to_string(),
                };
                let failed = sys::AppRow {
                    last_error: Some(failure.reason.clone()),
                    ..row.clone()
                };
                self.upsert_app_row(&candidate.folder, &failed, Some(&row))?;
                self.audit_load_failed(&failure, existing.as_ref())?;
                Ok(Outcome::Refused(failure))
            }
        }
    }

    /// `§8` from "parse app.toml" through "compute CSP", touching no log and no cache.
    fn prepare(&self, candidate: &Candidate) -> std::result::Result<Prepared, Box<Refused>> {
        let refused = |stage: Stage, reason: String| {
            Box::new(Refused {
                stage,
                reason,
                manifest: None,
                manifest_hash: None,
            })
        };

        let path = candidate.dir.join(MANIFEST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(refused(
                    Stage::Parse,
                    format!(
                        "{MANIFEST_FILE}: not found — an app is a folder containing one \
                         (spec/app-contract.md §3)"
                    ),
                ));
            }
            Err(error) => {
                return Err(refused(Stage::Parse, format!("{MANIFEST_FILE}: {error}")));
            }
        };
        let manifest_hash = manifest::manifest_hash(&text);
        let manifest = Manifest::parse(&text).map_err(|error| {
            Box::new(Refused {
                manifest_hash: Some(manifest_hash.clone()),
                ..*refused(Stage::Parse, error.to_string())
            })
        })?;
        let refused_with = |stage: Stage, reason: String| {
            Box::new(Refused {
                stage,
                reason,
                manifest: Some(manifest.clone()),
                manifest_hash: Some(manifest_hash.clone()),
            })
        };

        manifest
            .validate(&candidate.folder, self.config.node.mode)
            .map_err(|error| refused_with(Stage::Validate, error.to_string()))?;

        // Tier check: the file first; `app.lua` is loaded below, once the schema and the
        // mount it needs are known.
        if let Some(file) = manifest.app.tier.required_file()
            && !candidate.dir.join(file).is_file()
        {
            let error = ManifestError::TierFileMissing {
                tier: manifest.app.tier,
                file,
            };
            return Err(refused_with(Stage::Tier, error.to_string()));
        }
        // `§3`: a script or stylesheet the frame would load on every page must exist, or
        // every page of the app would carry a request that fails. Refused here, loud,
        // rather than discovered as a missing file in the browser.
        manifest
            .ui
            .check_files(&candidate.dir)
            .map_err(|error| refused_with(Stage::Validate, error.to_string()))?;

        let schema = match fs::read_to_string(candidate.dir.join(SCHEMA_FILE)) {
            Ok(sql) if sql.trim().is_empty() => Schema::empty(),
            Ok(sql) => Schema::parse(&sql)
                .map_err(|error| refused_with(Stage::Schema, error.to_string()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Schema::empty(),
            Err(error) => {
                return Err(refused_with(
                    Stage::Schema,
                    format!("{SCHEMA_FILE}: {error}"),
                ));
            }
        };

        let slug = &manifest.app.slug;
        let mount = match (manifest.app.tier, self.config.node.mode) {
            (Tier::Rust, _) => None,
            (_, Mode::Host) => Some(format!("/a/{slug}/")),
            (_, Mode::Solo) => {
                (self.config.node.app.as_deref() == Some(slug.as_str())).then(|| "/".to_owned())
            }
        };
        let csp = Csp::for_app(mount.as_deref(), &manifest.permissions);

        let mut warnings: Vec<Warning> = manifest
            .permissions
            .widenings()
            .into_iter()
            .map(|widening| Warning::Permission {
                slug: slug.clone(),
                widening,
            })
            .collect();
        if manifest.nav.advertise && slug.len() > MAX_ADVERTISED_SLUG {
            warnings.push(Warning::SlugTooLongToAdvertise { slug: slug.clone() });
        }
        // An icon the set lacks is drawn as the fallback glyph, so the app loads and the
        // owner is told — for the launcher's icon and for each menu item's alike.
        for icon in std::iter::once(&manifest.app.icon)
            .chain(manifest.ui.menu.iter().map(|item| &item.icon))
            .flatten()
        {
            if !crate::icons::exists(icon) {
                warnings.push(Warning::UnknownIcon {
                    slug: slug.clone(),
                    icon: icon.clone(),
                });
            }
        }
        // `§8`, tier 1: load `app.lua` in a sandboxed VM and register its routes — in
        // every VM of the pool, identically (`docs/plans/phase-1.md §2.4`). A load that
        // fails is refused here, before any log or cache is touched.
        let lua = if manifest.app.tier == Tier::Lua {
            let host = Host::build(
                slug,
                &candidate.dir,
                mount.as_deref(),
                schema.clone(),
                &self.config.lua,
            )
            .map_err(|reason| refused_with(Stage::Tier, reason))?;
            Some(Arc::new(host))
        } else {
            None
        };

        // `§9.1`: the solo app owns `/`, so a route named after a framework prefix is
        // unreachable — a top-level entry of a Tier 2 app's `web/`, or a pattern a Tier 1
        // app registered. Named here, once, rather than discovered by a 404.
        if mount.as_deref() == Some("/") {
            let shadowed: Vec<String> = match &lua {
                Some(host) => host
                    .routes()
                    .iter()
                    .map(|route| route.pattern.clone())
                    .filter(|pattern| crate::wire::router::shadowing_prefix(pattern).is_some())
                    .collect(),
                None => shadowed_web_routes(&candidate.dir.join("web")),
            };
            for route in shadowed {
                let prefix = crate::wire::router::shadowing_prefix(&route).unwrap_or_default();
                warnings.push(Warning::RouteShadowed {
                    slug: slug.clone(),
                    route,
                    prefix,
                });
            }
        }

        // `spec/app-contract.md §5`: a Tier 2 document the chrome would go into needs its
        // three anchors. Checked at load, once per HTML file, so the owner reads why a bar
        // is missing instead of wondering; the serve path serves such a file untouched.
        if manifest.app.tier == Tier::Web && manifest.ui.chrome == manifest::Chrome::Standard {
            for (file, anchor) in documents_without_anchors(&candidate.dir) {
                warnings.push(Warning::ChromeAnchorMissing {
                    slug: slug.clone(),
                    file,
                    anchor,
                });
            }
        }

        Ok(Prepared {
            manifest,
            manifest_hash,
            schema,
            mount,
            csp,
            warnings,
            lua,
        })
    }

    /// `§8`'s "materialize from data/<slug>/": the log, the three-tier restore, the
    /// health row — exactly what `Node::open` does for `_sys`, per app, for a folder's
    /// app and for an embedder's ([`open_app`](Self::open_app)) alike.
    fn materialize(&mut self, slug: &str, schema: Schema, cutoff: &str) -> Result<(AppLog, Store)> {
        for dir in [self.paths.app_log_dir(slug), self.paths.app_snap_dir(slug)] {
            fs::create_dir_all(&dir).map_err(io_at(&dir))?;
        }

        let (log, recovered) = AppLog::open(
            &self.paths,
            slug,
            self.identity.id(),
            Durability::Sync,
            &self.state,
        )?;
        audit_recovery(&mut self.sys, slug, &recovered)?;
        log.save_to(&mut self.state);
        let store = self.open_store(slug, schema, cutoff)?;
        Ok((log, store))
    }

    /// Open `cache/<slug>.sqlite` for `schema` and bring it up to date through the
    /// three-tier restore, with its audit and health row — at load, and again when
    /// `schema.sql` changes under a running app.
    fn open_store(&mut self, slug: &str, schema: Schema, cutoff: &str) -> Result<Store> {
        let previous = self
            .state
            .get(slug)
            .and_then(|record| record.materialized.restore.clone());
        let mut store = Store::open_with(&self.paths, slug, schema).map_err(boxed)?;
        if let Some(record) = self.state.get(slug) {
            store.restore_watermark(record.materialized.clone());
        }
        store.refresh(cutoff).map_err(boxed)?;
        if let Some(restored) = store.restored().cloned() {
            audit_restore(&mut self.sys, slug, &restored, previous.as_ref(), false)?;
        }
        note_health(&self.store, &store, slug)?;
        store.save_to(&mut self.state);
        Ok(store)
    }

    /// Append the `sys_app` row if it says anything the current row does not, plus
    /// `app.installed` the first time the app loads cleanly. Returns whether it wrote.
    fn upsert_app_row(
        &mut self,
        slug: &str,
        row: &sys::AppRow,
        existing: Option<&sys::AppRow>,
    ) -> Result<bool> {
        if existing.is_some_and(|current| current.same_facts(row)) {
            return Ok(false);
        }
        let installed = row.installed_at.is_some()
            && existing.is_none_or(|current| current.installed_at.is_none());
        let at = log::now();
        let detail = serde_json::to_string(&serde_json::json!({
            "slug": slug,
            "source": row.source,
            "tier": row.tier,
            "version": row.version,
        }))?;
        self.sys.batch(|batch| {
            batch.put(sys::APP, slug, row)?;
            if installed {
                batch.put(
                    sys::AUDIT,
                    &new_ulid(),
                    &sys::AuditRow::info(&at, sys::KIND_APP_INSTALLED, Some(slug), &detail),
                )?;
            }
            Ok(())
        })?;
        Ok(true)
    }

    /// `app.load_failed`, bounded to transitions like `audit_restore`: written when the
    /// reason differs from the last one recorded — the row's `last_error` where the folder
    /// has a row, the newest audit for that subject where it cannot. A broken folder is
    /// loud once, not once per start.
    fn audit_load_failed(
        &mut self,
        failure: &LoadFailure,
        existing: Option<&sys::AppRow>,
    ) -> Result<bool> {
        let previous = match existing {
            Some(row) => row.last_error.clone(),
            None => self.last_load_failure_reason(&failure.folder)?,
        };
        if previous.as_deref() == Some(failure.reason.as_str()) {
            return Ok(false);
        }
        // The folder name and the root's kind, never a path: `sys_audit` replicates.
        let detail = serde_json::to_string(&serde_json::json!({
            "folder": failure.folder,
            "source": failure.source.as_str(),
            "stage": failure.stage.as_str(),
            "reason": failure.reason,
        }))?;
        let at = log::now();
        self.sys.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::warn(
                &at,
                sys::KIND_APP_LOAD_FAILED,
                Some(&failure.folder),
                &detail,
            ),
        )?;
        Ok(true)
    }

    /// `§3.4`: a folder that is gone leaves its row, with `last_error = "folder missing"`.
    /// Returns whether the row changed.
    fn mark_folder_missing(&mut self, slug: &str) -> Result<bool> {
        let Some(existing) = self.read_app_row(slug)? else {
            return Ok(false);
        };
        if existing.last_error.as_deref() == Some(sys::FOLDER_MISSING) {
            return Ok(false);
        }
        let row = sys::AppRow {
            last_error: Some(sys::FOLDER_MISSING.to_owned()),
            updated_at: Some(log::now()),
            ..existing.clone()
        };
        self.upsert_app_row(slug, &row, Some(&existing))
    }

    /// The current `sys.sys_app` row, read back in the shape the writer uses so the two
    /// can be compared.
    fn read_app_row(&self, slug: &str) -> Result<Option<sys::AppRow>> {
        let sql = format!(
            "SELECT title, version, api, tier, icon, source, enabled, nav_order,
                    installed_at, updated_at,
                    schema_hash, manifest_hash, advertise, permissions, last_error
             FROM {} WHERE id = ?",
            sys::APP,
        );
        let row = self
            .store
            .conn()
            .query_row(&sql, rusqlite::params![slug], |row| {
                Ok(sys::AppRow {
                    title: row.get(0)?,
                    version: row.get(1)?,
                    api: row.get(2)?,
                    tier: row.get(3)?,
                    icon: row.get(4)?,
                    source: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    enabled: row.get::<_, Option<bool>>(6)?.unwrap_or(true),
                    nav_order: row.get(7)?,
                    installed_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    schema_hash: row.get(10)?,
                    manifest_hash: row.get(11)?,
                    advertise: row.get(12)?,
                    permissions: row.get(13)?,
                    last_error: row.get(14)?,
                })
            });
        match row {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(boxed(StoreError::Sql(error))),
        }
    }

    /// Every slug in `sys.sys_app`.
    fn indexed_slugs(&self) -> Result<Vec<String>> {
        let sql = format!("SELECT id FROM {} ORDER BY id", sys::APP);
        let mut statement = self
            .store
            .conn()
            .prepare(&sql)
            .map_err(|error| boxed(StoreError::Sql(error)))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| boxed(StoreError::Sql(error)))?;
        let mut slugs = Vec::new();
        for row in rows {
            slugs.push(row.map_err(|error| boxed(StoreError::Sql(error)))?);
        }
        Ok(slugs)
    }

    /// The reason in the newest `app.load_failed` audit row about `folder`, if any.
    fn last_load_failure_reason(&self, folder: &str) -> Result<Option<String>> {
        let sql = format!(
            "SELECT detail FROM {} WHERE kind = ? AND subject = ?
             ORDER BY \"at\" DESC, id DESC LIMIT 1",
            sys::AUDIT
        );
        let detail: Option<String> = match self.store.conn().query_row(
            &sql,
            rusqlite::params![sys::KIND_APP_LOAD_FAILED, folder],
            |row| row.get(0),
        ) {
            Ok(detail) => detail,
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(boxed(StoreError::Sql(error))),
        };
        Ok(detail
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("reason")?.as_str().map(str::to_owned)))
    }
}

/// The `sys_app` row for a folder, from whatever was learned about it.
///
/// `installed_at` is when the app first loaded cleanly, carried forward from the existing
/// row; a folder that has never loaded has none. `enabled` is the owner's and is carried
/// forward too (`§3.4`: uninstalling is an explicit owner action).
fn app_row(
    candidate: &Candidate,
    manifest: Option<&Manifest>,
    schema_hash: Option<&str>,
    manifest_hash: Option<String>,
    existing: Option<&sys::AppRow>,
    last_error: Option<&str>,
    now: &str,
) -> sys::AppRow {
    let installed_at = existing
        .and_then(|row| row.installed_at.clone())
        .or_else(|| last_error.is_none().then(|| now.to_owned()));
    sys::AppRow {
        title: manifest.map(|m| m.app.title.clone()),
        version: manifest.map(|m| m.app.version.clone()),
        api: manifest.and_then(|m| i32::try_from(m.app.api).ok()),
        tier: manifest.map(|m| m.app.tier.as_str().to_owned()),
        icon: manifest.and_then(|m| m.app.icon.clone()),
        source: candidate.source.as_str().to_owned(),
        enabled: existing.is_none_or(|row| row.enabled),
        nav_order: manifest.and_then(|m| m.nav.order),
        installed_at,
        updated_at: Some(now.to_owned()),
        schema_hash: schema_hash.map(str::to_owned),
        manifest_hash,
        advertise: manifest.map(|m| m.nav.advertise),
        permissions: manifest.map(|m| m.permissions.non_default_json()),
        last_error: last_error.map(str::to_owned),
    }
}

/// The top-level entries of a Tier 2 app's `web/` — files or directories — that a
/// framework prefix would shadow in solo mode, as routes (`/settings`, `/static`, …).
/// Every `.html` file under `web/`, as `web/<path>`, that lacks a chrome anchor, with the
/// anchor it lacks (`spec/app-contract.md §5`). A file that cannot be read as text is
/// skipped: the serve path serves it as it is, and nothing can be said about its tags.
fn documents_without_anchors(app_dir: &Path) -> Vec<(String, &'static str)> {
    let mut files = Vec::new();
    collect_html(&app_dir.join("web"), "web", &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|rel| {
            let text = fs::read_to_string(app_dir.join(&rel)).ok()?;
            crate::http::apps::chrome_anchors(&text)
                .err()
                .map(|anchor| (rel, anchor))
        })
        .collect()
}

/// Walk `dir`, pushing every `.html` and `.htm` file as `<rel>/<name>`.
fn collect_html(dir: &Path, rel: &str, into: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        if path.is_dir() {
            collect_html(&path, &child, into);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
        {
            into.push(child);
        }
    }
}

fn shadowed_web_routes(web: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(web) else {
        return Vec::new();
    };
    let mut routes: Vec<String> = entries
        .flatten()
        .map(|entry| format!("/{}", entry.file_name().to_string_lossy()))
        .filter(|route| crate::wire::router::shadowing_prefix(route).is_some())
        .collect();
    routes.sort();
    routes
}
