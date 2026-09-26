// This file is part of Privatium
// crates/privatium-core/src/lib.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-09-26
// Summary: Crate root. The error type, the engine linkage probe, `Node::open` and the bootstrap order
//          it follows, the sink that turns what a log scan found into sys_audit rows
//          (spec/protocol.md §4.4), and the node-level API of spec/app-contract.md §6 —
//          snapshot, restore, verify, prune and maintenance routed to every loaded app's
//          store, auth_layer, query, close, the discovery and pairing methods, and LAN
//          synchronization through a bounded receive inbox. core::handle itself is
//          wire::Handler; the Lua host behind a Tier 1 mount is `lua`; append, append_batch,
//          open_app and subscribe are `app`'s.
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

//! Privatium core.
//!
//! The contract this crate implements is `spec/protocol.md` and `spec/app-contract.md`.
//! Neither is optional reading, and where this code and those documents disagree, they
//! are right and this is a bug.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};
use thiserror::Error;

pub mod app;
pub mod backup;
pub mod config;
pub mod discover;
mod durable;
pub mod http;
pub mod icons;
pub mod identity;
pub mod lint;
pub mod local;
pub mod lock;
pub mod log;
pub mod lua;
pub mod pair;
pub mod registry;
pub mod session;
pub mod store;
/// LAN synchronization over the encrypted channel, with a durable bounded inbox.
pub mod sync;
pub mod sys;
pub mod wire;

pub use app::{
    App, AppRoot, Appended, Csp, Event, LoadFailure, LoadReport, Manifest, Permissions, Seeded,
    Source, Stage, StreamEvent, Warning,
};
pub use config::{Config, LuaConfig, Mode, NodeConfig, PORTABLE_DIR, Paths, RootSource};
pub use discover::Discovered;
pub use http::{AuthLayer, Device, Peer};
pub use identity::{Identity, NodeId};
pub use lock::DataLock;
pub use log::{AppLog, Durability, Op};
pub use pair::{Pairing, PairingSnapshot};
pub use store::{Restored, Schema, Snapshot, SnapshotId, SnapshotJob, Store, StoreError, Tier};
pub use wire::{Body, Handler, Request, Response, url};

use store::{
    Pruned, RestoreRecord, Retention, SnapshotError, SnapshotPolicy, Verification, snapshot,
};

/// The protocol this build speaks (`spec/protocol.md §12`).
///
/// Not the `--version` string: `spec/cli.md §1` requires a build that does not satisfy
/// every item of `§13` to qualify what it prints. That qualification belongs to the CLI,
/// which knows what this build implements, not here — this constant is the wire format,
/// and the wire format really is `pv/1`.
pub const PROTOCOL: &str = "pv/1";

/// The version of this build as a person reads it and as releases are tagged:
/// `major.minor`, so `0.3` for the manifest's `0.3.0`. Cargo needs three numbers and the
/// patch stays zero; releases are numbered by minor. The workspace manifest is the only
/// place the number is written, every crate inherits it, and `--version`, `skill export`
/// and the generated skill references all print this, so what they say is the tag the
/// reader downloaded.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

/// Anything that can go wrong opening or running a node.
///
/// One type rather than one per module: the binary, the shells, and eventually `uniffi`
/// all want a single thing to match on.
#[derive(Debug, Error)]
pub enum Error {
    /// Synchronization could not complete an internal operation; no peer input is echoed.
    #[error("synchronization: {problem}; retry after checking peer reachability and node storage")]
    Sync {
        /// Fixed operation diagnostic.
        problem: &'static str,
    },
    /// A foreign range does not continue its origin's sequence (`spec/protocol.md §10.2`).
    #[error(
        "receiving {app}/{dev}: expected seq {expected}, found {found}; pull the missing range"
    )]
    ForeignSeq {
        /// Validated app slug.
        app: String,
        /// Validated origin device ID.
        dev: String,
        /// Next sequence required by the file.
        expected: u64,
        /// First sequence that failed validation.
        found: u64,
    },
    /// A foreign line or destination fails validation without exposing row contents.
    #[error("receiving a log: {problem}; check the origin's log")]
    ForeignLine {
        /// A fixed diagnostic, never text supplied by a peer.
        problem: &'static str,
    },
    /// A sequence-bearing line has an invalid envelope; no part of its range is written.
    #[error("cannot receive envelope {seq}: {problem}")]
    ForeignEnvelope {
        /// Sequence of the first invalid envelope.
        seq: u64,
        /// Fixed diagnostic, without row contents.
        problem: &'static str,
    },
    /// One append would write more than a peer's request body can carry.
    ///
    /// A batch reaches a peer whole or not at all and a line reaches it whole
    /// (`spec/protocol.md §4.1`, `§10.2`), so bytes past `api.max_body` could never
    /// leave this node. Split the batch, or shrink the row.
    #[error(
        "appending to {app}: {bytes} bytes in one append; the most is {limit} (api.max_body) — split the batch or store less in one row"
    )]
    AppendTooLarge {
        /// The app whose log refused the append.
        app: String,
        /// Bytes the append would have written, newlines included.
        bytes: usize,
        /// The bound in force.
        limit: usize,
    },
    /// A line exceeds the caller's configured receive bound.
    #[error("receiving a log: line exceeds {limit} bytes; check the origin's log")]
    ForeignLineTooLong {
        /// Maximum bytes including the newline.
        limit: usize,
    },
    /// A torn foreign line is not a prefix of the line offered by its origin.
    #[error("{path}: foreign log differs at byte {offset}; restore a matching log copy")]
    ForeignDiverged {
        /// Foreign segment, available only to the owner-facing diagnostic.
        path: PathBuf,
        /// Byte offset of the incomplete line.
        offset: u64,
    },
    /// A system identity row cannot be safely amended from its log representation.
    #[error("cannot initialize node identity: invalid system row; restore a valid data backup")]
    IdentityRow,
    /// Certificate validation or issuance failed without exposing its input.
    #[error(transparent)]
    Certificate(#[from] identity::CertificateError),
    /// A step of pairing was refused (`spec/protocol.md §7`); the variant carries the
    /// WebSocket close code the channel answers with.
    #[error(transparent)]
    Pair(#[from] pair::PairError),
    /// The platform has no data directory and none was given.
    #[error("no platform data directory is available; pass an explicit data directory")]
    NoDataDir,
    /// Owner-typed text a registry column cannot hold (`spec/data-dictionary.md §3.1`,
    /// `§3.2`); the message names the field and the bound, never the text.
    #[error("{field}: {problem}")]
    InvalidText {
        /// Which field.
        field: &'static str,
        /// What was wrong with it.
        problem: String,
    },
    /// A device ID no `sys_device` row carries (`spec/data-dictionary.md §3.2`).
    #[error("{device}: no such device is paired with this node")]
    DeviceUnknown {
        /// The ID that was asked for.
        device: String,
    },
    /// The owner asked to revoke or relabel this node's own device row.
    #[error("this node's own device row cannot be revoked or relabelled")]
    OwnDevice,

    /// A filesystem operation failed, named by the path it failed on.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// What the OS said.
        source: std::io::Error,
    },

    /// `config.toml` did not parse.
    #[error("{path}: {source}")]
    Config {
        /// The config file.
        path: PathBuf,
        /// What TOML said, including the offending key.
        ///
        /// Boxed because it is 88 bytes on its own, which made this the widest variant and
        /// put the whole enum at 128 — a cost every `Result` in the crate paid, on the
        /// success path too, for the rarest failure there is.
        source: Box<toml::de::Error>,
    },

    /// `config.toml` parsed but says something inconsistent.
    #[error("{path}: {problem}")]
    ConfigInvalid {
        /// The config file.
        path: PathBuf,
        /// What is wrong with it.
        problem: String,
    },

    /// `identity/node.key` is not an Ed25519 private key.
    #[error("{path}: expected a 32-byte Ed25519 private key, found {found} bytes")]
    KeyLength {
        /// The key file.
        path: PathBuf,
        /// How long it actually was.
        found: usize,
    },

    /// A log file that was expected to be new already exists.
    #[error("{path}: log file already exists")]
    LogExists {
        /// The log file.
        path: PathBuf,
    },

    /// Another process — or another open in this one — holds the data root
    /// (`spec/protocol.md §3.1`). Two writers on one log would both mint `seq`.
    #[error(
        "{path}: another privatium process has this data directory open; stop it first \
         (spec/protocol.md §3.1)"
    )]
    Locked {
        /// `local/lock`.
        path: PathBuf,
    },

    /// A writer was aimed at a log file that is not this node's own.
    ///
    /// `AGENTS.md` 2 and `spec/protocol.md §3.1`: a device appends only to
    /// `data/<slug>/log/<its-own-node-id>.jsonl`. The one exception is `§10.2`'s sync
    /// receiver, which is [`log::foreign::Receiver`] and never comes through `log::Writer`.
    #[error("{path}: not this node's log — expected app {app} and device {dev}")]
    LogNotOurs {
        /// The file that was refused.
        path: PathBuf,
        /// The app the writer was for.
        app: String,
        /// The device the writer was for.
        dev: String,
    },

    /// The last append to this log failed and the file has not been re-read since
    /// (`spec/protocol.md §4.1`). The failed write may be on disk in whole or in part,
    /// so nothing is appended after it until the next scan says where the file ends —
    /// which the next append attempts itself.
    #[error(
        "{path}: the last append failed ({reason}); the log takes no writes until it is re-read"
    )]
    WriterPoisoned {
        /// The log file.
        path: PathBuf,
        /// What failed.
        reason: String,
    },

    /// A log file ends in the middle of a line.
    ///
    /// What a crash during an append leaves behind. Reported, never repaired: `§3.1` forbids
    /// truncating a log file, and the byte offset is what makes the damage inspectable
    /// instead of merely fatal.
    #[error("{path}: incomplete line at byte {offset}; the log was not modified")]
    PartialLine {
        /// The segment.
        path: PathBuf,
        /// Where the incomplete line starts.
        offset: u64,
    },

    /// An event could not be serialized.
    #[error("serializing an event: {0}")]
    Serialize(#[from] serde_json::Error),

    /// A statically linked engine failed to answer (`AGENTS.md`, Language and stack).
    #[error(transparent)]
    Engine(#[from] EngineError),

    /// Maintaining an app's `cache/<slug>.sqlite` failed (`spec/protocol.md §4.5`).
    ///
    /// Boxed for the same reason `Config` is: `rusqlite::Error` is wide, and every `Result`
    /// in the crate would otherwise pay for the rarest failure there is.
    #[error(transparent)]
    Store(#[from] Box<StoreError>),

    /// The node holds no store for this app.
    ///
    /// `_sys` always has one; any other slug has one only after [`Node::load_apps`]
    /// loaded it. Asking for anything else is answered with this rather than with a
    /// silent success an embedder would build on.
    #[error("{slug}: no store is open for this app")]
    AppNotLoaded {
        /// The app.
        slug: String,
    },

    /// An app's files changed on disk and the reload failed (`spec/cli.md §3`): a
    /// `app.lua` that does not load, a template that does not compile, an `app.toml` that
    /// does not parse. The app as last loaded is kept, unserved, until the next edit; the
    /// reason is what the error page and `sys_app.last_error` show.
    #[error("{slug}: not reloaded — {reason}")]
    AppReloadFailed {
        /// The app.
        slug: String,
        /// The load failure's text, naming the file and line where it can.
        reason: String,
    },

    /// `sample/seed.jsonl` was not loaded because the app already has events
    /// (`spec/app-contract.md §9`).
    #[error(
        "{slug}: sample/seed.jsonl was not loaded — the log already holds {events} event(s) \
         (spec/app-contract.md §9)"
    )]
    SeedRefused {
        /// The app.
        slug: String,
        /// Events across every device's log.
        events: u64,
    },

    /// The app ships no `sample/seed.jsonl`.
    #[error("{slug}: no sample/seed.jsonl to load")]
    NoSeed {
        /// The app.
        slug: String,
    },

    /// A seed line is not an event.
    #[error("{path}: line {line}: {problem}")]
    Seed {
        /// The seed file.
        path: PathBuf,
        /// 1-based line.
        line: usize,
        /// What is wrong with it.
        problem: String,
    },

    /// A value written to a typed column is not its type (`spec/data-dictionary.md §2.1`,
    /// `spec/lua-api.md §3.3`). Refused before the append, so the log stays clean.
    #[error("{app}: {tbl}.{column}: {problem}")]
    Value {
        /// The app.
        app: String,
        /// The table.
        tbl: String,
        /// The event's position in the batch, from 0 — what the data API reports as the
        /// offending index (`spec/data-api.md §2`).
        index: usize,
        /// The column whose value was refused.
        column: String,
        /// What was wrong with it.
        problem: String,
    },

    /// A row breaks a `NOT NULL` or `CHECK` constraint of `schema.sql`
    /// (`spec/data-api.md §2`, `spec/lua-api.md §3.3`). Refused before the append.
    #[error("{app}: {tbl}: event {index}: {problem}")]
    Constraint {
        /// The app.
        app: String,
        /// The table.
        tbl: String,
        /// The event's position in the batch, from 0.
        index: usize,
        /// What SQLite said.
        problem: String,
    },

    /// An area of `spec/app-contract.md §6` this build does not implement — discovery
    /// and sync, which `docs/roadmap.md` places in Phases 2 and 3. The method is
    /// present with its signature and answers with this rather than succeeding at
    /// nothing, which an embedder would build on; `privatium --version` says `partial`
    /// for the same reason (`spec/cli.md §1`).
    #[error(
        "{feature}: not in this build — Phase {phase} of docs/roadmap.md; {spec} is its contract"
    )]
    Unimplemented {
        /// The `§6` method.
        feature: &'static str,
        /// The roadmap phase it arrives in.
        phase: &'static str,
        /// The section of `spec/protocol.md` that is its contract.
        spec: &'static str,
    },

    /// A statement [`Node::query`] could not run: SQL the sandbox refuses
    /// (`spec/app-contract.md §7`), a parameter count that does not match the
    /// placeholders, a parameter that is not a scalar, or SQLite's own complaint.
    #[error("{app}: {problem}")]
    Sql {
        /// The app.
        app: String,
        /// What was wrong.
        problem: String,
    },

    /// [`Node::open_app`] refused the slug: reserved or malformed (`spec/protocol.md
    /// §1.1`), or already loaded from a folder (`spec/app-contract.md §3.1`).
    #[error("{slug}: {reason}")]
    AppRefused {
        /// The slug.
        slug: String,
        /// Why.
        reason: String,
    },

    /// A join (`spec/protocol.md §2.3.1`) did not complete: the URL, the socket, or the
    /// other node's refusal by name. Never the code.
    #[error("cannot join the cluster at {url}: {problem}")]
    Join {
        /// The URL that was dialed, as given.
        url: String,
        /// What went wrong, for the owner.
        problem: String,
    },

    /// This node's certificate has expired, so it serves its owner alone until it is
    /// re-admitted (`spec/protocol.md §2.3.1`); what was asked needs a member.
    #[error(
        "this node's certificate has expired; it serves its owner alone until it is re-admitted with `privatium pair --join` (spec/protocol.md §2.3.1)"
    )]
    CertificateExpired,

    /// This node's ID is in `sys_node_revocation` (`spec/protocol.md §2.3.4`): it syncs
    /// nothing and admits nobody, and the owner re-initializes it.
    #[error(
        "this node has been revoked from its cluster; re-initialize it with a fresh identity (spec/protocol.md §2.3.4, §2.4)"
    )]
    NodeRevoked,

    /// A node named in `sys_node_revocation` asked to be admitted, or to admit
    /// (`spec/protocol.md §2.3.4`).
    #[error("node {node} is revoked from this cluster (spec/protocol.md §2.3.4)")]
    PeerRevoked {
        /// The revoked node's ID.
        node: String,
    },
}

/// What this node may do for the cluster it belongs to (`spec/protocol.md §2.3.1`,
/// `§2.3.4`). A member does everything; the other two serve the owner alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A node with a valid certificate that no revocation names.
    Member,
    /// The certificate has expired; re-admission is the only way back.
    Expired,
    /// `sys_node_revocation` names this node; it is re-initialized, never re-admitted.
    Revoked,
}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Attach a path to an [`std::io::Error`].
///
/// `map_err(io_at(&path))` reads better than a match at every call site, and an IO error
/// without the path it happened on is close to useless in a bug report.
pub(crate) fn io_at(path: &Path) -> impl FnOnce(std::io::Error) -> Error {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// What one [`Node::maintain`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Maintenance {
    /// The snapshot written, if one was due.
    pub snapshot: Option<Snapshot>,
    /// What retention removed and kept.
    pub pruned: Pruned,
}

/// One installation of the Privatium server (`spec/protocol.md §1`).
///
/// Opening a node is the bootstrap order of `docs/plans/phase-1.md §2.6`, and the order
/// is not negotiable: the framework's own `sys_device` row has to be written through the
/// same log an app would use, before a materialized `_sys` or an app loader exists to help.
/// `open` performs steps 1 to 4 — the tree and the keypair, the `_sys` log with its
/// recovered `seq` and Lamport counter, this node's two rows in it, and a three-tier
/// restore of `_sys` into `cache/_sys.sqlite`.
/// Step 5, loading `apps/`, is [`load_apps`](Self::load_apps): explicit rather than part
/// of `open`, because embedded mode (`spec/app-contract.md §2.3`) opens a node and has no
/// folders to scan, and because only the caller knows where a development checkout's
/// bundled `apps/` is.
#[derive(Debug)]
pub struct Node {
    paths: Paths,
    config: Config,
    identity: Identity,
    sys: AppLog,
    store: Store,
    state: local::State,
    /// Every loaded app, by slug (`app::App`). Owned here so the node-level snapshot,
    /// restore and maintenance reach every store through one map.
    apps: BTreeMap<String, App>,
    /// The one open pairing window (`spec/data-dictionary.md §3.3`), in memory and never
    /// written; `None` while pairing is closed (`spec/protocol.md §7.1`).
    pairing: Option<Pairing>,
    /// The discovery mechanisms (`spec/protocol.md §6`) while they run; stopped when
    /// the node drops.
    discovery: Option<discover::Discovery>,
    /// Engine-owned sockets and threads; no writer crosses this boundary.
    sync: Option<sync::SyncHandle>,
    /// Bounded received pages waiting for the root-lock owner to land them.
    inbox: Option<sync::Inbox>,
    /// Recursive refreshes during a drain must not overtake its queued pages.
    draining: bool,
    /// Stable wake source across identity adoption and engine restarts in this process.
    sync_wake: Option<tokio::sync::watch::Sender<u64>>,
    sync_seen: std::collections::BTreeSet<String>,
    sync_expired: std::collections::BTreeSet<String>,
    /// The root's lock (`spec/protocol.md §3.1`). Last, so it is released after every
    /// log and store above it has closed.
    lock: DataLock,
}

impl Node {
    /// Open — or on first run, create — the node rooted at `data_dir`.
    ///
    /// This is the signature `spec/app-contract.md §2.3` gives embedded mode. It takes
    /// the root's lock (`spec/protocol.md §3.1`) and holds it until the node is dropped;
    /// a root another process has open is [`Error::Locked`].
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::open_paths(Paths::rooted(data_dir))
    }

    /// Open using the two global flags of `spec/cli.md §1`, either of which may be absent.
    ///
    /// `--data-dir` defaults to the platform data directory and `--config` to
    /// `config.toml` inside it.
    pub fn open_with(data_dir: Option<&Path>, config: Option<&Path>) -> Result<Self> {
        Self::open_paths(Paths::resolve(data_dir, config)?)
    }

    /// Open a root whose lock the caller already holds — `privatium restore`, which
    /// has to keep other processes out from before it copies a backup in until the
    /// rebuild is done (`spec/cli.md §7`).
    pub fn open_holding(lock: DataLock) -> Result<Self> {
        let paths = lock.paths().clone();
        Self::open_locked(paths, lock)
    }

    fn open_paths(paths: Paths) -> Result<Self> {
        paths.create_tree()?;
        let lock = DataLock::acquire(paths.clone())?;
        Self::open_locked(paths, lock)
    }

    fn open_locked(paths: Paths, lock: DataLock) -> Result<Self> {
        // 1. The tree, the keypair, and the Node ID derived from it.
        paths.create_tree()?;
        let identity = Identity::load_or_create(&paths.identity_dir())?;
        let config = Config::load(paths.config_file())?;

        // 2. Node-local state. A cache, and nothing below may depend on it for correctness:
        //    it makes the scan in step 3 cheap and keeps the Lamport counter monotonic if a
        //    log file is ever replaced by an older copy.
        let mut state = local::State::load(&paths.local_state())?;

        // 3. The _sys log. Opening it is what recovers `seq` and `lam` (§4.1, §4.3) and
        //    what applies §4.4 to everything already on disk.
        let (mut sys, recovered) =
            AppLog::open(&paths, sys::SLUG, identity.id(), Durability::Sync, &state)?;

        // Reconcile public identity facts from the log, including roots created before
        // cluster identity existed (spec/protocol.md §2.3). Caches carry no authority.
        let first_run = sys.seq() == 0;
        bootstrap_sys(&mut sys, &identity)?;

        // 5. Anything step 3 found that the owner is entitled to hear about. After the
        //    bootstrap, because an audit row cannot be written to a log that has no node
        //    behind it yet — and on a first run there is nothing to report anyway.
        audit_recovery(&mut sys, sys::SLUG, &recovered)?;

        // 6. Step 4 of §2.6: materialize `_sys`, by spec/protocol.md §5.3's three tiers.
        //    Everything above it had to happen first — the rows this replays are the ones
        //    step 4 just wrote — and step 5, loading `apps/`, is `load_apps`, which the
        //    caller runs once this has returned.
        //
        //    Nothing serves app SQL out of `_sys`: its `app_conn()` is never handed out.
        //    App stores hand out read-only sandboxed connections (`store::sandbox`).
        let previous = state
            .get(sys::SLUG)
            .and_then(|record| record.materialized.restore.clone());
        let mut store = Store::open(&paths, sys::SLUG, store::SYS_DDL).map_err(boxed)?;
        if let Some(record) = state.get(sys::SLUG) {
            store.restore_watermark(record.materialized.clone());
        }
        store.refresh(&store::cutoff_now()).map_err(boxed)?;

        // What §5.3 found, if it is worth an audit row — and if it is, the row is an event
        // in the very log the tables were built from, so they are refreshed once more.
        if let Some(restored) = store.restored().cloned()
            && audit_restore(&mut sys, sys::SLUG, &restored, previous.as_ref(), first_run)?
        {
            store.refresh(&store::cutoff_now()).map_err(boxed)?;
        }
        note_health(&store, &store, sys::SLUG)?;
        store.reset_peers().map_err(boxed)?;

        // 7. Record what we now know.
        sys.save_to(&mut state);
        store.save_to(&mut state);
        state.flush()?;

        let mut node = Self {
            paths,
            config,
            identity,
            sys,
            store,
            state,
            apps: BTreeMap::new(),
            pairing: None,
            discovery: None,
            sync: None,
            inbox: None,
            draining: false,
            sync_wake: None,
            sync_seen: std::collections::BTreeSet::new(),
            sync_expired: std::collections::BTreeSet::new(),
            lock,
        };
        // An expired or revoked node says so once, in the audit table the owner reads
        // (spec/protocol.md §2.3.1, §2.3.4) — after the tables exist, since the check
        // reads them, and refreshed once more so the row is visible.
        if node.audit_standing(jiff::Timestamp::now())? {
            node.refresh()?;
        }
        Ok(node)
    }

    /// This node's standing in its cluster (`spec/protocol.md §2.3.1`, `§2.3.4`):
    /// revoked when `sys_node_revocation` names it, expired when its certificate is,
    /// otherwise a member.
    pub fn standing(&self, now: jiff::Timestamp) -> Result<Standing> {
        if self.node_revoked(self.identity.id().as_str())? {
            return Ok(Standing::Revoked);
        }
        if self.identity.is_expired(now) {
            return Ok(Standing::Expired);
        }
        Ok(Standing::Member)
    }

    /// Whether `sys_node_revocation` holds `node` (`spec/data-dictionary.md §3.1c`).
    pub fn node_revoked(&self, node: &str) -> Result<bool> {
        let sql = format!("SELECT count(*) FROM {} WHERE id = ?", sys::REVOCATION);
        self.store
            .conn()
            .query_row(&sql, rusqlite::params![node], |row| row.get::<_, i64>(0))
            .map(|count| count > 0)
            .map_err(|error| boxed(StoreError::Sql(error)))
    }

    /// Write `cert.expired` or `node.revoked` about this node, once per certificate and
    /// once per revocation rather than once per start: the row is skipped while the
    /// audit table already holds one naming the same expiry. Returns whether a row was
    /// written.
    fn audit_standing(&mut self, now: jiff::Timestamp) -> Result<bool> {
        let id = self.identity.id().as_str().to_owned();
        let (kind, detail, warn) = match self.standing(now)? {
            Standing::Member => return Ok(false),
            Standing::Expired => (
                sys::KIND_CERT_EXPIRED,
                serde_json::json!({ "expires_at": self.identity.certificate().expires_at }),
                true,
            ),
            Standing::Revoked => (
                sys::KIND_NODE_REVOKED,
                serde_json::json!({ "self": true }),
                false,
            ),
        };
        let detail = serde_json::to_string(&detail)?;
        let sql = format!(
            "SELECT count(*) FROM {} WHERE kind = ? AND subject = ? AND detail = ?",
            sys::AUDIT
        );
        let already: i64 = self
            .store
            .conn()
            .query_row(&sql, rusqlite::params![kind, id, detail], |row| row.get(0))
            .map_err(|error| boxed(StoreError::Sql(error)))?;
        if already > 0 {
            return Ok(false);
        }
        let at = log::now();
        let row = if warn {
            sys::AuditRow::warn(&at, kind, Some(&id), &detail)
        } else {
            sys::AuditRow::alert(&at, kind, Some(&id), &detail)
        };
        self.sys.put(sys::AUDIT, &new_ulid(), &row)?;
        Ok(true)
    }

    /// Write `local/state.jsonl` if anything has changed since it was last written.
    ///
    /// Worth calling after a run of appends and not worth worrying about if it is missed:
    /// the file is a cache, and everything in it is recoverable by reading the logs. A node
    /// that never flushes is a node that does a little more work at its next start.
    pub fn flush(&mut self) -> Result<()> {
        self.sys.save_to(&mut self.state);
        self.store.save_to(&mut self.state);
        for app in self.apps.values() {
            app.log().save_to(&mut self.state);
            app.store().save_to(&mut self.state);
        }
        self.state.flush()
    }

    /// Rematerialize `_sys` if its log has grown behind the tables — the check the read
    /// path makes per request. Returns whether it rebuilt.
    pub fn refresh(&mut self) -> Result<bool> {
        let drained = self.drain()?;
        let refreshed = self.store.refresh(&store::cutoff_now()).map_err(boxed)?;
        self.publish_peers()?;
        Ok(drained || refreshed)
    }

    // -----------------------------------------------------------------------------------
    // spec/app-contract.md §6 — `snapshot`, `restore`, `restore_tier`
    // -----------------------------------------------------------------------------------

    /// Write a snapshot of `app` now (`spec/protocol.md §5`), and record it as a
    /// `sys_snapshot` event.
    pub fn snapshot(&mut self, app: &str) -> Result<Snapshot> {
        self.snapshot_at(app, jiff::Timestamp::now())
    }

    /// [`snapshot`](Self::snapshot) at a given instant, which is what names the snapshot.
    ///
    /// The store writes it from the log while the app's read-only connections, if any,
    /// go on reading — no window to open, nothing to reseal. Read, written and recorded
    /// in one call; a caller others are waiting on takes the three steps apart
    /// ([`snapshot_job`](Self::snapshot_job), [`SnapshotJob::write`],
    /// [`record_snapshot`](Self::record_snapshot)) and holds its lock for the first and
    /// the last only.
    pub fn snapshot_at(&mut self, app: &str, now: jiff::Timestamp) -> Result<Snapshot> {
        let snapshot = self.snapshot_job(app, now)?.write().map_err(boxed)?;
        self.record_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Read what a snapshot of `app` at `now` will hold, and hand it back as a job that
    /// writes the files without the node (`Store::snapshot_job`). The reading is a pass
    /// over the log and belongs under the node's lock, which is what keeps the log
    /// still; the writing is the slow part and needs no lock at all.
    pub fn snapshot_job(&self, app: &str, now: jiff::Timestamp) -> Result<SnapshotJob> {
        let dev = self.identity.id().clone();
        self.store_for(app)?.snapshot_job(&dev, now).map_err(boxed)
    }

    /// The snapshot [`maintain`](Self::maintain) would write now under the policy of
    /// `spec/data-dictionary.md §3.6`, if one is due — as a job, for the same reason as
    /// [`snapshot_job`](Self::snapshot_job).
    pub fn snapshot_due(&self, app: &str, now: jiff::Timestamp) -> Result<Option<SnapshotJob>> {
        let policy = self.snapshot_policy()?;
        let snap_dir = self.paths.app_snap_dir(app);
        let newest = snapshot::newest(&snap_dir)
            .map_err(snap_err)?
            .and_then(|id| snapshot::read_manifest(&snap_dir.join(id.to_string())).ok());
        let heads = self.heads_for(app)?;
        if snapshot::due(newest.as_ref(), &heads, now, &policy).map_err(snap_err)? {
            Ok(Some(self.snapshot_job(app, now)?))
        } else {
            Ok(None)
        }
    }

    /// Append the `sys_snapshot` row (`spec/data-dictionary.md §3.9`) and the
    /// `snapshot.created` audit row for a snapshot written through any store — this
    /// node's `_sys`, or an app store the loader holds.
    ///
    /// One batch: the index row and the audit row describe one act.
    pub fn record_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        let id = snapshot.id.to_string();
        let counts = snapshot.manifest.row_counts_json();
        let created_by = self.identity.id().as_str().to_owned();
        let detail = serde_json::to_string(&serde_json::json!({
            "app": snapshot.manifest.app,
            "hi_lam": snapshot.manifest.hi_lam,
            "tables": snapshot.manifest.tables.len(),
            "bytes": snapshot.bytes,
        }))?;
        let at = log::now();
        self.sys.batch(|batch| {
            batch.put(
                sys::SNAPSHOT,
                &id,
                &sys::SnapshotRow::new(snapshot, &counts, &created_by, None),
            )?;
            batch.put(
                sys::AUDIT,
                &new_ulid(),
                &sys::AuditRow::info(&at, sys::KIND_SNAPSHOT_CREATED, Some(&id), &detail),
            )
        })?;
        Ok(())
    }

    /// Rebuild `app`'s cache by `spec/protocol.md §5.3`'s three tiers, reporting which one
    /// succeeded, and audit a fall-through (`spec/data-dictionary.md §3.10`).
    pub fn restore(&mut self, app: &str) -> Result<Restored> {
        let previous = self.restore_record(app);
        let is_app = app != sys::SLUG;
        let restored = self
            .store_for_mut(app)?
            .restore(&store::cutoff_now())
            .map_err(boxed)?;
        if audit_restore(&mut self.sys, app, &restored, previous.as_ref(), false)? && !is_app {
            self.store.refresh(&store::cutoff_now()).map_err(boxed)?;
        }
        note_health(&self.store, self.store_for(app)?, app)?;
        self.flush()?;
        Ok(restored)
    }

    /// Which tier [`restore`](Self::restore) would use for `app`, without writing a table
    /// (`spec/cli.md §7`, `--dry-run`). A prediction, as [`Store::restore_dry_run`] says.
    pub fn restore_dry_run(&self, app: &str) -> Result<Restored> {
        self.store_for(app)?
            .restore_dry_run(&store::cutoff_now())
            .map_err(boxed)
    }

    /// Which tier built `app`'s cache — what `pv.node()` and `/api/node` report.
    ///
    /// `None` for an app this node has never materialized.
    #[must_use]
    pub fn restore_tier(&self, app: &str) -> Option<Tier> {
        self.restore_record(app).map(|record| record.tier)
    }

    /// Recompute a snapshot's checksums (`spec/cli.md §7`, `--verify`), and if every file
    /// matches, re-assert its `sys_snapshot` row with `verified_at` set.
    pub fn verify_snapshot(&mut self, app: &str, id: &SnapshotId) -> Result<Verification> {
        let dir = self.paths.app_snap_dir(app).join(id.to_string());
        let verification = snapshot::verify(&dir).map_err(snap_err)?;
        if verification.ok() {
            let snapshot = Snapshot::read(&dir).map_err(snap_err)?;
            let counts = snapshot.manifest.row_counts_json();
            let created_by = self.identity.id().as_str().to_owned();
            let now = log::now();
            self.sys.put(
                sys::SNAPSHOT,
                &id.to_string(),
                &sys::SnapshotRow::new(&snapshot, &counts, &created_by, Some(&now)),
            )?;
        }
        Ok(verification)
    }

    /// Apply `spec/protocol.md §5.4` to `app`'s snapshots at `now`, tombstoning each
    /// removed `sys_snapshot` row and auditing `snapshot.pruned`.
    ///
    /// Works from the directory alone, so it does not need the app's store to be open.
    /// The deleting is `store::snapshot::prune`, which needs nothing of the node; a
    /// caller others are waiting on runs it with no lock held, between
    /// [`snapshot_retention`](Self::snapshot_retention) and
    /// [`record_pruned`](Self::record_pruned).
    pub fn prune_snapshots(&mut self, app: &str, now: jiff::Timestamp) -> Result<Pruned> {
        let retention = self.snapshot_retention()?;
        let pruned =
            snapshot::prune(&self.paths.app_snap_dir(app), now, &retention).map_err(snap_err)?;
        self.record_pruned(app, &pruned, &retention)?;
        Ok(pruned)
    }

    /// `§5.4`'s retention as the settings stand, for `store::snapshot::prune`.
    pub fn snapshot_retention(&self) -> Result<Retention> {
        Ok(self.snapshot_policy()?.retention())
    }

    /// Record what a prune removed: a tombstone for each `sys_snapshot` row and a
    /// `snapshot.pruned` audit row beside it, one batch per snapshot.
    pub fn record_pruned(
        &mut self,
        app: &str,
        pruned: &Pruned,
        retention: &Retention,
    ) -> Result<()> {
        for id in &pruned.removed {
            let id = id.to_string();
            let detail = serde_json::to_string(&serde_json::json!({
                "app": app,
                "retention_days": retention.snapshot_days,
            }))?;
            let at = log::now();
            self.sys.batch(|batch| {
                batch.del(sys::SNAPSHOT, &id)?;
                batch.put(
                    sys::AUDIT,
                    &new_ulid(),
                    &sys::AuditRow::info(&at, sys::KIND_SNAPSHOT_PRUNED, Some(&id), &detail),
                )
            })?;
        }
        Ok(())
    }

    /// The `snapshot.*` settings of `spec/data-dictionary.md §3.6`, from `sys_setting`,
    /// with the dictionary's defaults for anything unset.
    pub fn snapshot_policy(&self) -> Result<SnapshotPolicy> {
        let mut policy = SnapshotPolicy::default();
        if let Some(days) = self.setting_u64("snapshot.retention_days")? {
            policy.retention_days = u32::try_from(days).unwrap_or(u32::MAX);
        }
        if let Some(days) = self.setting_u64("snapshot.interval_days")? {
            policy.interval_days = u32::try_from(days).unwrap_or(u32::MAX);
        }
        if let Some(events) = self.setting_u64("snapshot.min_events")? {
            policy.min_events = events;
        }
        Ok(policy)
    }

    /// The scheduled maintenance of `spec/protocol.md §5`: a snapshot if one is due under
    /// the policy, then retention. The caller owns the timer — the run loop daily,
    /// `privatium snapshot` on demand — and a caller that holds the node's lock while
    /// requests wait takes the same steps apart ([`snapshot_due`](Self::snapshot_due),
    /// [`SnapshotJob::write`], [`record_snapshot`](Self::record_snapshot),
    /// `store::snapshot::prune`, [`record_pruned`](Self::record_pruned)) so that the
    /// files are written with the lock released.
    pub fn maintain(&mut self, app: &str, now: jiff::Timestamp) -> Result<Maintenance> {
        let snapshot = match self.snapshot_due(app, now)? {
            Some(job) => {
                let snapshot = job.write().map_err(boxed)?;
                self.record_snapshot(&snapshot)?;
                Some(snapshot)
            }
            None => None,
        };
        let pruned = self.prune_snapshots(app, now)?;
        Ok(Maintenance { snapshot, pruned })
    }

    /// `auth_layer` (`spec/app-contract.md §6`): the tower middleware that decides who a
    /// request is from: a loopback caller is this node's own device row, anything
    /// else is 403 (`docs/plans/phase-1.md §2.2`). [`Handler::handle`] applies its own
    /// copy itself, so every adapter gets it; this one is for an embedder to wrap their
    /// own router with (`§2.3`), and it refuses a request whose peer it cannot see — serve
    /// the router with `into_make_service_with_connect_info`, or insert [`Peer`] for a
    /// call made in-process.
    #[must_use]
    pub fn auth_layer(&self) -> AuthLayer {
        AuthLayer::new(self.identity.id().clone())
    }

    /// Run a read-only statement on `app`'s sandboxed connection (`spec/app-contract.md
    /// §6`, `query`; `§7`), and hand back the rows as JSON objects typed as the data API
    /// types them (`spec/data-api.md §1`): a declared `DECIMAL` or `BIGINT` is a string,
    /// a `BOOLEAN` a boolean, a `JSON` column its value, NULL is `null`, and a computed
    /// column arrives by its storage class. `params` bind the statement's positional
    /// `?` placeholders — a string as text, an integer as an integer, another number as
    /// a real, a boolean as 1/0, `null` as NULL — and are never interpolated; a count
    /// that does not match the placeholders is refused, and so is an array or an object
    /// among them. The connection is read-only at the file, `query_only`, behind the
    /// authorizer that refuses every write, `PRAGMA`, `ATTACH` and extension load, has
    /// `sys` attached read-only (`spec/data-dictionary.md §4`), and runs the statement
    /// under `lua.max_seconds`. Opened per call; a program that queries per request does
    /// what the framework's own routes do.
    pub fn query(&self, app: &str, sql: &str, params: &[Value]) -> Result<Vec<Map<String, Value>>> {
        let loaded = self.apps.get(app).ok_or_else(|| Error::AppNotLoaded {
            slug: app.to_owned(),
        })?;
        let refused = |problem: String| Error::Sql {
            app: app.to_owned(),
            problem,
        };
        let bound = params
            .iter()
            .enumerate()
            .map(|(index, value)| store::query::bind(index, value))
            .collect::<std::result::Result<Vec<_>, String>>()
            .map_err(refused)?;
        let conn = loaded.store().app_conn().map_err(boxed)?;
        let deadline = Duration::from_secs(self.config.lua.max_seconds.max(1));
        let rows = store::query::run(&conn, loaded.store().schema(), deadline, sql, bound)
            .map_err(refused)?;
        Ok(rows.rows)
    }

    /// Close the node (`spec/app-contract.md §6`): write `local/state.jsonl` and release
    /// the root's lock. Dropping the node releases the lock too; what `close` adds is
    /// the flush and its result, which a drop cannot report.
    pub fn close(mut self) -> Result<()> {
        self.sync.take();
        self.drain()?;
        self.flush()
    }

    // -----------------------------------------------------------------------------------
    // spec/app-contract.md §6 — the areas later phases fill. Present, never Ok.
    // -----------------------------------------------------------------------------------

    /// Start LAN discovery (`spec/protocol.md §6`, `spec/app-contract.md §6`): mDNS
    /// advertisement and browsing (`§6.1`) and the UDP responder on port 52525 (`§6.4`),
    /// each as `discovery.mdns` and `discovery.udp` in `sys_setting` allow, started at
    /// once and never in sequence (`§6.5`). A mechanism the platform refuses — no
    /// multicast interface, the port taken — is reported in
    /// [`discovery_status`](Self::discovery_status) and does not stop the other; a
    /// setting that is neither `true` nor `false` keeps its mechanism off. Writes one
    /// `discovery.method` audit row naming what started. Both stop when the node drops.
    /// Calling this on a node whose discovery is already running is a no-op.
    ///
    /// Fails only for the node's own trouble: a `sys_setting` that cannot be read, an
    /// audit row that cannot be written.
    pub fn serve_discovery(&mut self) -> Result<()> {
        if self.discovery.is_some() {
            return Ok(());
        }
        let facts = self.discovery_facts()?;
        let discovery = discover::Discovery::start(
            facts,
            discover::Options {
                mdns: self.setting_flag("discovery.mdns")?,
                udp: self.setting_flag("discovery.udp")?,
                udp_port: discover::udp::PORT,
            },
        );
        let detail = serde_json::to_string(&serde_json::json!({
            "mdns": discovery.status().mdns.to_string(),
            "udp": discovery.status().udp.to_string(),
            "port": self.config.node.port,
        }))?;
        let at = log::now();
        self.sys.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::info(&at, sys::KIND_DISCOVERY_METHOD, None, &detail),
        )?;
        self.discovery = Some(discovery);
        Ok(())
    }

    /// What [`serve_discovery`](Self::serve_discovery) started, or `None` before it ran.
    #[must_use]
    pub fn discovery_status(&self) -> Option<&discover::Status> {
        self.discovery.as_ref().map(discover::Discovery::status)
    }

    /// Every node seen on the network so far, keyed by ID (`spec/protocol.md §6.1`);
    /// empty while discovery is not running.
    #[must_use]
    pub fn discovered(&self) -> Vec<discover::Discovered> {
        self.discovery
            .as_ref()
            .map(discover::Discovery::discovered)
            .unwrap_or_default()
    }

    /// Record a node learned by any means — a record another mechanism or an embedder
    /// obtained — under its ID, as discovery's own records are (`spec/protocol.md §6.1`).
    /// Returns `false` while discovery is not running, since there is no table to hold
    /// it.
    pub fn absorb_discovered(&self, seen: discover::Discovered) -> bool {
        match &self.discovery {
            Some(discovery) => {
                discovery.absorb(seen);
                true
            }
            None => false,
        }
    }

    /// The cluster's other nodes discovery has seen (`spec/protocol.md §6.1`): every
    /// record whose `cl` is this node's cluster and whose `id` is not its own, by ID.
    /// The only nodes a sync pass is ever offered to.
    #[must_use]
    pub fn peers(&self) -> Vec<discover::Discovered> {
        let cluster = self.identity.cluster_id().as_str();
        let own = self.identity.id().as_str();
        let mut peers: Vec<_> = self
            .discovered()
            .into_iter()
            .filter(|seen| seen.cluster == cluster && seen.id != own)
            .collect();
        peers.sort_by(|a, b| a.id.cmp(&b.id));
        peers
    }

    /// The nodes discovery has seen that are not this cluster's (`spec/protocol.md
    /// §6.1`), by ID: shown to the owner, never contacted.
    #[must_use]
    pub fn strangers(&self) -> Vec<discover::Discovered> {
        let cluster = self.identity.cluster_id().as_str();
        let own = self.identity.id().as_str();
        let mut strangers: Vec<_> = self
            .discovered()
            .into_iter()
            .filter(|seen| seen.cluster != cluster && seen.id != own)
            .collect();
        strangers.sort_by(|a, b| a.id.cmp(&b.id));
        strangers
    }

    /// The facts this node advertises (`spec/protocol.md §6.1`) as they stand: the IDs,
    /// `sys_node.display_name` or the Node ID, the mounted slugs and the ones eligible
    /// for a subtype, the build, the open pairing window's expiry, the port. The TXT
    /// record, the UDP answer and the manifest's `pair` flag are all read from here.
    pub fn discovery_facts(&self) -> Result<discover::Facts> {
        let (display_name, build) = self.node_row_public_facts()?;
        let mut apps: Vec<String> = self
            .mounts()
            .map(|(_, app)| app.slug().to_owned())
            .collect();
        apps.sort();
        let mut advertised: Vec<String> = self
            .mounts()
            .filter(|(_, app)| {
                app.manifest().nav.advertise && app.slug().len() <= app::MAX_ADVERTISED_SLUG
            })
            .map(|(_, app)| app.slug().to_owned())
            .collect();
        advertised.sort();
        // An expired or revoked node advertises `pair = 0` whatever window it holds
        // (spec/protocol.md §2.3.1): the node re-admitting it dials it by URL.
        let member = self.standing(jiff::Timestamp::now())? == Standing::Member;
        let pair_until = self
            .pairing
            .as_ref()
            .filter(|window| member && window.consumed_by().is_none())
            .map(Pairing::expires_at);
        Ok(discover::Facts {
            id: self.identity.id().as_str().to_owned(),
            cluster: self.identity.cluster_id().as_str().to_owned(),
            name: display_name.unwrap_or_else(|| self.identity.id().as_str().to_owned()),
            apps,
            advertised,
            build,
            pair_until,
            port: self.config.node.port,
        })
    }

    /// Hand the current facts to the running mechanisms — after apps load, when a
    /// pairing window opens or closes, when the port is settled. A no-op while
    /// discovery is not running.
    pub fn publish_facts(&self) -> Result<()> {
        if let Some(discovery) = &self.discovery {
            discovery.update(self.discovery_facts()?);
        }
        Ok(())
    }

    /// This installation's `display_name` (empty is unset) and `build` from `sys_node`.
    pub(crate) fn node_row_public_facts(&self) -> Result<(Option<String>, String)> {
        let sql = format!("SELECT display_name, build FROM {} WHERE id = ?", sys::NODE);
        match self.store.conn().query_row(
            &sql,
            rusqlite::params![self.identity.id().as_str()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        ) {
            Ok((name, build)) => Ok((
                name.filter(|n| !n.trim().is_empty()),
                build.unwrap_or_else(|| "custom".to_owned()),
            )),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, "custom".to_owned())),
            Err(error) => Err(boxed(StoreError::Sql(error))),
        }
    }

    /// A boolean `sys_setting` (`spec/data-dictionary.md §3.6`): on when absent or
    /// `true`, off when `false`, and refused with the reason when it is anything else.
    fn setting_flag(&self, key: &str) -> Result<discover::Switch> {
        let Some(value) = self.setting_value(key)? else {
            return Ok(discover::Switch::On);
        };
        let flag = serde_json::from_str::<serde_json::Value>(&value)
            .ok()
            .and_then(|parsed| {
                parsed
                    .as_bool()
                    .or_else(|| match parsed.as_str().map(str::trim) {
                        Some("true") => Some(true),
                        Some("false") => Some(false),
                        _ => None,
                    })
            });
        Ok(match flag {
            Some(true) => discover::Switch::On,
            Some(false) => discover::Switch::Off,
            None => discover::Switch::Refused(format!("{key} is not true or false")),
        })
    }

    /// Where this node's files are.
    #[must_use]
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The root's lock, held for this node's lifetime (`spec/protocol.md §3.1`).
    #[must_use]
    pub fn lock(&self) -> &DataLock {
        &self.lock
    }

    /// This node's configuration, with defaults filled in.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The configuration, for the overrides a run may carry — `--port` and `--solo` of
    /// `spec/cli.md §2` — which hold for this run and never touch `config.toml`.
    ///
    /// Apply them before [`load_apps`](Self::load_apps): the mode decides where every app
    /// is mounted, and the port is what the handler renders its own origin from.
    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// This node's keypair and ID.
    #[must_use]
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// This node's ID — the `dev` of every event it writes.
    #[must_use]
    pub fn id(&self) -> &NodeId {
        self.identity.id()
    }

    /// The framework's own log (`docs/plans/phase-1.md §2.6`).
    ///
    /// `_sys` is an app and is written through the same log an app would use. It is not
    /// discoverable, not mountable, and not lintable, which is why the app loader will skip
    /// it and why it is reachable here instead.
    #[must_use]
    pub fn sys_log(&self) -> &AppLog {
        &self.sys
    }

    /// The framework's own log, for appending.
    pub fn sys_log_mut(&mut self) -> &mut AppLog {
        &mut self.sys
    }

    /// `_sys` materialized into `cache/_sys.sqlite` (`spec/data-dictionary.md §3`).
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The same, for the framework's own maintenance of it.
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// The store for `app`: `_sys`'s, or a loaded app's.
    fn store_for(&self, app: &str) -> Result<&Store> {
        if app == sys::SLUG {
            Ok(&self.store)
        } else {
            self.apps
                .get(app)
                .map(App::store)
                .ok_or_else(|| Error::AppNotLoaded {
                    slug: app.to_owned(),
                })
        }
    }

    fn store_for_mut(&mut self, app: &str) -> Result<&mut Store> {
        if app == sys::SLUG {
            Ok(&mut self.store)
        } else {
            self.apps
                .get_mut(app)
                .map(App::store_mut)
                .ok_or_else(|| Error::AppNotLoaded {
                    slug: app.to_owned(),
                })
        }
    }

    /// The highest `seq` per device in `app`'s log.
    fn heads_for(&self, app: &str) -> Result<BTreeMap<String, u64>> {
        if app == sys::SLUG {
            Ok(self.sys.heads().clone())
        } else {
            self.apps
                .get(app)
                .map(|loaded| loaded.log().heads().clone())
                .ok_or_else(|| Error::AppNotLoaded {
                    slug: app.to_owned(),
                })
        }
    }

    /// The restore record for `app`: the live store's where one is open, and
    /// `local/state.jsonl`'s for an app this node materialized on some earlier run.
    fn restore_record(&self, app: &str) -> Option<RestoreRecord> {
        if app == sys::SLUG {
            self.store.restore_record().cloned()
        } else if let Some(loaded) = self.apps.get(app) {
            loaded.store().restore_record().cloned()
        } else {
            self.state
                .get(app)
                .and_then(|record| record.materialized.restore.clone())
        }
    }

    /// One `sys_setting` value as an integer, if set and integral.
    ///
    /// `value` is a JSON-encoded scalar (`§3.6`), so `365` and `"365"` both read as 365.
    fn setting_u64(&self, key: &str) -> Result<Option<u64>> {
        let value: Option<String> = match self.store.conn().query_row(
            &format!("SELECT value FROM {} WHERE id = ?", sys::SETTING),
            rusqlite::params![key],
            |row| row.get(0),
        ) {
            Ok(value) => Some(value),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(boxed(StoreError::Sql(error))),
        };
        let Some(value) = value else {
            return Ok(None);
        };
        let parsed: serde_json::Value = match serde_json::from_str(&value) {
            Ok(parsed) => parsed,
            Err(_) => return Ok(None),
        };
        Ok(parsed
            .as_u64()
            .or_else(|| parsed.as_str().and_then(|s| s.trim().parse().ok())))
    }
}

/// `StoreError` is boxed inside [`Error`]; this is the conversion at the call sites.
fn boxed(source: StoreError) -> Error {
    Error::Store(Box::new(source))
}

fn snap_err(source: SnapshotError) -> Error {
    boxed(StoreError::Snapshot(source))
}

/// Write `app`'s restore facts into `sys.v_health` through the `_sys` store.
fn note_health(sys_store: &Store, app_store: &Store, app: &str) -> Result<()> {
    if let Some(record) = app_store.restore_record() {
        let log_bytes = app_store.log_bytes().map_err(boxed)?;
        sys_store
            .note_health(
                app,
                record.tier,
                record.snapshot.as_deref(),
                &record.at,
                log_bytes,
            )
            .map_err(boxed)?;
    }
    Ok(())
}

/// Append changed public identity facts as one batch, preserving owner-set and unknown
/// row fields. Original lines remain untouched (spec/protocol.md §4.2, §4.5).
fn bootstrap_sys(sys_log: &mut AppLog, identity: &Identity) -> Result<()> {
    use base64::Engine as _;
    use serde_json::value::{RawValue, to_raw_value};
    use store::events::{Op, read_log, winners};

    type Row = BTreeMap<String, Box<RawValue>>;
    fn row<T: serde::Serialize>(value: &T) -> Result<Row> {
        Ok(serde_json::from_str(&serde_json::to_string(value)?)?)
    }
    fn changed(previous: Option<&Row>, next: &Row) -> bool {
        previous.is_none_or(|previous| {
            previous.len() != next.len()
                || next.iter().any(|(key, value)| {
                    previous.get(key).is_none_or(|old| old.get() != value.get())
                })
        })
    }

    let id = identity.id().as_str().to_owned();
    let pubkey = identity.public_key_base64();
    let events = read_log(sys_log.log_dir(), sys::SLUG, &store::cutoff_now()).map_err(boxed)?;
    let winners = winners(&events);
    let existing = |table: &str, key: &str| -> Result<Option<Row>> {
        winners
            .get(&(table, key))
            .filter(|event| event.op == Op::Put)
            .and_then(|event| event.d.as_deref())
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| Error::IdentityRow)
    };
    let device = existing(sys::DEVICE, &id)?;
    let node = existing(sys::NODE, &id)?;
    let cluster = existing(sys::CLUSTER, identity.cluster_id().as_str())?;
    let cert = identity.certificate().to_base64()?;
    let renewed = identity.renewed()
        || node.as_ref().is_some_and(|row| {
            row.get("cert")
                .and_then(|value| serde_json::from_str::<String>(value.get()).ok())
                .is_some_and(|old| old != cert)
        });
    sys_log.batch(|batch| {
        let created_at = batch.ts().to_owned();
        let mut next_device = device.clone().unwrap_or(row(&sys::DeviceRow::this_node())?);
        next_device.insert("ed25519_pub".into(), to_raw_value(&pubkey)?);
        next_device.insert(
            "x25519_pub".into(),
            to_raw_value(&identity.x25519_public_base64())?,
        );
        if changed(device.as_ref(), &next_device) {
            batch.put(sys::DEVICE, &id, &next_device)?;
        }
        let mut next_node = node
            .clone()
            .unwrap_or(row(&sys::NodeRow::this_installation(&pubkey, &created_at))?);
        next_node.insert("pubkey".into(), to_raw_value(&pubkey)?);
        next_node.insert(
            "cluster_id".into(),
            to_raw_value(identity.cluster_id().as_str())?,
        );
        next_node.insert("cert".into(), to_raw_value(&cert)?);
        next_node.insert(
            "cert_expires_at".into(),
            to_raw_value(&identity.certificate().expires_at)?,
        );
        if changed(node.as_ref(), &next_node) {
            batch.put(sys::NODE, &id, &next_node)?;
        }
        // The cluster's row is the founder's to write, at founding (spec/data-dictionary.md
        // §3.1b): an admitted node holds none until sync brings the founder's. A first
        // bootstrap — a log with no record of this node at all — is a founding too, which
        // is what a zero-byte log left by a crash before the first append comes back as.
        // An existing current row is corrected from the verified keys, as the node's own
        // rows are.
        let founding = cluster.is_none() && (identity.founded() || node.is_none());
        if founding || cluster.is_some() {
            let mut next_cluster = cluster.clone().unwrap_or(row(&sys::ClusterRow {
                pubkey: base64::engine::general_purpose::STANDARD
                    .encode(identity.cluster_public().as_bytes()),
                pkarr_name: identity::pkarr_name(&identity.cluster_public()),
                created_at: &created_at,
                created_by: &id,
            })?);
            next_cluster.insert(
                "pubkey".into(),
                to_raw_value(
                    &base64::engine::general_purpose::STANDARD
                        .encode(identity.cluster_public().as_bytes()),
                )?,
            );
            next_cluster.insert(
                "pkarr_name".into(),
                to_raw_value(&identity::pkarr_name(&identity.cluster_public()))?,
            );
            if changed(cluster.as_ref(), &next_cluster) {
                batch.put(sys::CLUSTER, identity.cluster_id().as_str(), &next_cluster)?;
            }
        }
        Ok(())
    })?;
    let kind = if cluster.is_none() && (identity.founded() || node.is_none()) {
        Some(sys::KIND_CLUSTER_CREATED)
    } else if renewed {
        Some(sys::KIND_CERT_RENEWED)
    } else {
        None
    };
    if let Some(kind) = kind {
        sys_log.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::info(&log::now(), kind, Some(&id), "{}"),
        )?;
    }
    Ok(())
}

/// Turn what a log scan found into `sys_audit` rows (`spec/protocol.md §4.4`).
///
/// This lives on [`Node`] rather than on [`AppLog`] for one reason of order: `_sys` has to
/// be open before anything can be audited, and a log that reached into another log to
/// report itself would make that dependency impossible to see. Every app's diagnostics come
/// here, which is also why the app is named in the detail rather than assumed.
///
/// Rejections are reported **once**. The scan only offers an event whose `seq` is past the
/// head recorded in `local/state.jsonl`, so a bad line that has already been audited stays
/// in the log — `§3.1` forbids removing it — without producing a fresh row on every start.
fn audit_recovery(sys_log: &mut AppLog, app: &str, recovered: &log::Recovered) -> Result<()> {
    for rejected in &recovered.rejected {
        let detail = serde_json::to_string(&serde_json::json!({
            "app": app,
            "dev": rejected.dev,
            "seq": rejected.seq,
            "ts": rejected.ts,
            "ahead_secs": rejected.ahead_secs,
            // The file name, never the full path. `sys_audit` is replicated (§3.10), and
            // this node's data root is nobody else's business.
            "segment": file_name(&rejected.segment),
            "offset": rejected.offset,
        }))?;
        let at = log::now();
        sys_log.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::warn(
                &at,
                sys::KIND_EVENT_REJECTED,
                Some(rejected.dev.as_str()),
                &detail,
            ),
        )?;
    }

    // A batch that reached the disk short (`spec/protocol.md §4.1`): its lines are
    // skipped by every reader and stay in the file; the owner hears about it once.
    for short in &recovered.incomplete {
        let detail = serde_json::to_string(&serde_json::json!({
            "app": app,
            "dev": short.dev,
            "seq": short.seq,
            "expected": short.expected,
            "found": short.found,
            "segment": file_name(&short.segment),
            "offset": short.offset,
        }))?;
        let at = log::now();
        sys_log.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::warn(
                &at,
                sys::KIND_BATCH_INCOMPLETE,
                Some(short.dev.as_str()),
                &detail,
            ),
        )?;
    }

    if let Some(skew) = &recovered.skew {
        let detail = serde_json::to_string(&serde_json::json!({
            "app": app,
            "tail_ts": skew.tail_ts,
            "behind_secs": skew.behind_secs,
        }))?;
        let at = log::now();
        // No subject. §3.10's subject is a device, app, or snapshot; this is about the
        // node's own clock, and `actor: system` already says whose.
        sys_log.put(
            sys::AUDIT,
            &new_ulid(),
            &sys::AuditRow::warn(&at, sys::KIND_CLOCK_SKEW, None, &detail),
        )?;
    }

    Ok(())
}

/// Turn what a `spec/protocol.md §5.3` restore did into a `sys_audit` row, when it is
/// worth one. Returns whether a row was written.
///
/// Bounded on purpose, because `Store::refresh` runs per request and a row per refresh
/// would append to `sys_audit` forever:
///
/// - `restore.tier2` (warn) when CSV rescued a snapshot whose SQLite file failed, once per
///   `(tier, snapshot)` — `previous` is what `local/state.jsonl` last recorded.
/// - `restore.tier3` (alert, `§3.10`) when a snapshot that applied could not be read at
///   all, once per `(tier, snapshot)` likewise — or when the replay rebuilt a cache that
///   did not exist for a log that has events, which is `docs/backup-and-restore.md §3`'s
///   "I rebuilt from scratch" and happens once per deletion. Not on a first run, when the
///   only events are the ones this very open just wrote.
///
/// An *expected* tier 3 — no snapshot yet, a changed `schema.sql`, a tail that is not
/// causal — is not audited. `sys.v_health` still says what happened.
fn audit_restore(
    sys_log: &mut AppLog,
    app: &str,
    restored: &Restored,
    previous: Option<&RestoreRecord>,
    first_run: bool,
) -> Result<bool> {
    let transition =
        previous.is_none_or(|p| p.tier != restored.tier || p.snapshot != restored.snapshot);
    let (kind, alert) = match restored.tier {
        Tier::Sqlite => return Ok(false),
        Tier::Csv if transition => (sys::KIND_RESTORE_TIER2, false),
        Tier::Replay
            if (restored.unexpected() && transition) || (restored.from_scratch && !first_run) =>
        {
            (sys::KIND_RESTORE_TIER3, true)
        }
        _ => return Ok(false),
    };

    let detail = serde_json::to_string(&serde_json::json!({
        "app": app,
        "tier": restored.tier.as_u8(),
        "snapshot": restored.snapshot,
        "skipped": restored.skipped,
        "from_scratch": restored.from_scratch,
    }))?;
    let at = log::now();
    let subject = restored.snapshot.as_deref().unwrap_or(app);
    let row = if alert {
        sys::AuditRow::alert(&at, kind, Some(subject), &detail)
    } else {
        sys::AuditRow::warn(&at, kind, Some(subject), &detail)
    };
    sys_log.put(sys::AUDIT, &new_ulid(), &row)?;
    Ok(true)
}

/// A fresh ULID, Crockford Base32, 26 characters (`spec/protocol.md §4.1`) — the default
/// row key, minted by whoever writes the row: `pv.ulid()`, the data API, and an
/// embedder's [`Event::put`].
#[must_use]
pub fn new_ulid() -> String {
    ulid::Ulid::generate().to_string()
}

/// The last component of a path, for a message that must not carry a filesystem layout.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Versions of the two foreign engines statically linked into this build.
///
/// Reported by `privatium` so the number CI prints for binary size is the size of a
/// binary that genuinely contains both engines, rather than one where the linker
/// discarded them as unreferenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedEngines {
    /// SQLite's own `sqlite_version()`, e.g. `3.53.2`.
    pub sqlite: String,
    /// Lua's `_VERSION`, which must be `Lua 5.4` — not LuaJIT, not Luau (`AGENTS.md`).
    pub lua: String,
}

/// Failures of the linkage probe.
#[derive(Debug, Error)]
pub enum EngineError {
    /// SQLite linked but did not answer.
    #[error("bundled SQLite failed to answer sqlite_version(): {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Lua linked but did not answer.
    #[error("vendored Lua failed to answer _VERSION: {0}")]
    Lua(#[from] mlua::Error),
}

/// Open an in-memory SQLite database and a fresh Lua state, and ask each its version.
///
/// This exists to fail loudly on any platform where the bundled C build of either engine
/// is broken, rather than deep in the store or the Lua host, where there is real code to
/// blame it on.
pub fn linked_engines() -> std::result::Result<LinkedEngines, EngineError> {
    let conn = rusqlite::Connection::open_in_memory()?;
    let sqlite: String = conn.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;

    let lua = mlua::Lua::new();
    let lua: String = lua.load("return _VERSION").eval()?;

    Ok(LinkedEngines { sqlite, lua })
}
