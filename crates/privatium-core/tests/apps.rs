// This file is part of Privatium
// crates/privatium-core/tests/apps.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-02
// Last Modified: 2026-10-04
// Summary: The app loader against spec/app-contract.md §3, §3.1, §5.4, §8 and §9, spec/protocol.md §1.1
//          and §12, and spec/data-dictionary.md §3.4 — refusal per app and loud, the index as
//          events, the sandboxed cache, the store the node-level snapshot and restore reopen,
//          and the seed that never loads itself.
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

// AGENTS.md, Style: unwrap() is permitted in tests, and a test that hides a failure
// behind `?` is worse than one that panics with a line number.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use common::{
    APP, HELLO_DDL, audit_rows, digest_via, event, files_in, flip_byte, hand_append, log_lines,
    lua_manifest, repo_apps_dir, sha256_hex, sys_app_row, sys_lines, tree, ts_offset_secs,
    write_app, write_lua_app,
};
use privatium_core::app::{RESERVED_SLUGS, SUPPORTED_API, StreamEvent, Widening};
use privatium_core::local::State;
use privatium_core::store::Tier;
use privatium_core::{AppRoot, Csp, Error, Event, Node, Permissions, Source, Stage, Warning};

/// `spec/protocol.md §9.3`, verbatim.
const DEFAULT_CSP: &str = "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

fn open(root: &tempfile::TempDir) -> Node {
    Node::open(root.path()).unwrap()
}

fn local(node: &Node) -> AppRoot {
    AppRoot::local(node.paths().apps_dir())
}

/// Three events for `hello`'s `profile` table, hand-appended before the app is loaded —
/// what a log restored from a backup looks like to a first load.
fn seed_hello_log(node: &Node) {
    let path = node.paths().app_log(APP, node.id());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let dev = node.id().as_str().to_owned();
    let ts = ts_offset_secs(-60);
    hand_append(
        &path,
        &event(
            1,
            1,
            &ts,
            &dev,
            "profile",
            "a",
            Some(r#"{"display_name":"Gabriel"}"#),
        ),
        "\n",
    );
    hand_append(
        &path,
        &event(
            2,
            2,
            &ts,
            &dev,
            "profile",
            "b",
            Some(r#"{"display_name":"Ada"}"#),
        ),
        "\n",
    );
    hand_append(&path, &event(3, 3, &ts, &dev, "profile", "b", None), "\n");
}

// ---------------------------------------------------------------------------------------
// §3.1 — refusal, per app and loud
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §1.1` — all ten reserved slugs, one assertion each, refused for that
/// reason and none other.
#[test]
fn test_spec_1_1_reserved_slug_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();

    // `_sys` cannot be tried from a folder of its own name — `_` folders are skipped — so
    // it is declared from a folder that would otherwise load.
    let folder_for = |slug: &str| {
        if slug == "_sys" {
            "sysx".to_owned()
        } else {
            slug.to_owned()
        }
    };
    for slug in RESERVED_SLUGS {
        write_app(
            &apps,
            &folder_for(slug),
            Some(&lua_manifest(slug)),
            &[("app.lua", "")],
        );
    }

    let report = node.load_apps(&[local(&node)]).unwrap();
    assert!(report.loaded.is_empty(), "{report:?}");
    assert_eq!(report.failed.len(), 10, "{report:?}");
    for slug in RESERVED_SLUGS {
        let folder = folder_for(slug);
        let failure = report
            .failed
            .iter()
            .find(|f| f.folder == folder)
            .unwrap_or_else(|| panic!("{slug}: not refused"));
        assert_eq!(failure.stage, Stage::Validate, "{slug}");
        assert!(
            failure.reason.contains("reserved") && failure.reason.contains("§1.1"),
            "{slug}: {}",
            failure.reason
        );
        assert!(node.app(slug).is_none(), "{slug}");
        assert!(node.app(&folder).is_none(), "{slug}");
        // Through the folder: `_sys` itself is the framework's store and would answer.
        assert!(
            matches!(node.snapshot(&folder), Err(Error::AppNotLoaded { .. })),
            "{slug}"
        );
        assert!(
            !node.paths().data_dir().join(&folder).exists(),
            "{slug}: data/<slug>/ appeared for a refused app"
        );
    }

    // Loud, once: an `app.load_failed` per folder, and none again on the next start.
    assert_eq!(audit_rows(&node, "app.load_failed").len(), 10);
    node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(
        audit_rows(&node, "app.load_failed").len(),
        10,
        "the same refusal was audited twice"
    );

    // A reserved folder name is not a row key; nothing reserved reaches the index.
    for slug in RESERVED_SLUGS {
        assert!(sys_app_row(&node, slug).is_none(), "{slug} is in sys_app");
    }
}

/// `spec/app-contract.md §3.1` (and `spec/cli.md` PV104) — the slug is the folder.
#[test]
fn test_spec_3_1_slug_dir_mismatch_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_app(
        &node.paths().apps_dir(),
        "bar",
        Some(&lua_manifest("foo")),
        &[("app.lua", "")],
    );

    let report = node.load_apps(&[local(&node)]).unwrap();
    assert!(report.loaded.is_empty());
    assert_eq!(report.failed.len(), 1);
    let failure = &report.failed[0];
    assert_eq!(
        (failure.folder.as_str(), failure.stage),
        ("bar", Stage::Validate)
    );
    assert!(
        failure.reason.contains("does not match its folder"),
        "{}",
        failure.reason
    );
    assert!(node.app("foo").is_none() && node.app("bar").is_none());

    // The folder is a valid slug, so its row exists and says why (§3.4).
    let row = sys_app_row(&node, "bar").unwrap();
    assert_eq!(row["title"], "foo");
    assert!(
        row["last_error"]
            .as_str()
            .unwrap()
            .contains("does not match"),
        "{row}"
    );
    assert!(row["installed_at"].is_null(), "never loaded cleanly: {row}");
    assert!(sys_app_row(&node, "foo").is_none());
    assert!(!node.paths().data_dir().join("bar").exists());
    assert!(!node.paths().data_dir().join("foo").exists());
}

/// `spec/protocol.md §12` — an app declaring a higher `api` is refused; the row records
/// it; fixing the manifest clears the row and installs the app.
#[test]
fn test_spec_12_higher_api_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();
    let later = lua_manifest("later").replace("api = 1", "api = 2");
    write_app(&apps, "later", Some(&later), &[("app.lua", "")]);

    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.failed.len(), 1, "{report:?}");
    let failure = &report.failed[0];
    assert_eq!(failure.stage, Stage::Validate);
    assert!(
        failure.reason.contains("api 2")
            && failure.reason.contains(&format!("api {SUPPORTED_API}"))
            && failure.reason.contains("§12"),
        "{}",
        failure.reason
    );
    assert!(node.app("later").is_none());

    let row = sys_app_row(&node, "later").unwrap();
    assert_eq!(row["api"], 2);
    assert_eq!(row["source"], "local");
    assert_eq!(row["enabled"], true);
    assert!(row["installed_at"].is_null());
    assert!(row["last_error"].as_str().unwrap().contains("§12"));
    let audits = audit_rows(&node, "app.load_failed");
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["subject"], "later");
    assert_eq!(audits[0]["severity"], "warn");
    let detail: serde_json::Value =
        serde_json::from_str(audits[0]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(detail["stage"], "validate");
    assert_eq!(detail["source"], "local");
    assert!(audit_rows(&node, "app.installed").is_empty());

    // The owner fixes it: the row clears, `installed_at` is set, `app.installed` once.
    write_app(&apps, "later", Some(&lua_manifest("later")), &[]);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec!["later"]);
    let row = sys_app_row(&node, "later").unwrap();
    assert_eq!(row["api"], 1);
    assert!(row["last_error"].is_null(), "{row}");
    assert!(!row["installed_at"].is_null(), "{row}");
    assert_eq!(audit_rows(&node, "app.installed").len(), 1);
    assert_eq!(audit_rows(&node, "app.load_failed").len(), 1);
}

/// `spec/app-contract.md §3.1` — one broken app MUST NOT prevent the node from starting.
/// Four ways to be broken, one good app, and the node serves the good one.
#[test]
fn test_broken_app_does_not_stop_node() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();

    write_app(
        &apps,
        "broken",
        Some("[app]\nslug = \"broken\"\nthis is not toml\n"),
        &[],
    );
    fs::create_dir_all(apps.join("empty")).unwrap();
    write_app(&apps, "no-lua", Some(&lua_manifest("no-lua")), &[]);
    write_lua_app(
        &apps,
        "bad-schema",
        &[("schema.sql", "CREATE TABLE t (name VARCHAR);")],
    );
    write_lua_app(&apps, "good", &[("schema.sql", HELLO_DDL)]);

    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec!["good"], "{report:?}");
    let failed: BTreeMap<&str, Stage> = report
        .failed
        .iter()
        .map(|f| (f.folder.as_str(), f.stage))
        .collect();
    assert_eq!(
        failed,
        BTreeMap::from([
            ("broken", Stage::Parse),
            ("empty", Stage::Parse),
            ("no-lua", Stage::Tier),
            ("bad-schema", Stage::Schema),
        ])
    );
    for failure in &report.failed {
        let row = sys_app_row(&node, &failure.folder)
            .unwrap_or_else(|| panic!("{}: no row", failure.folder));
        assert_eq!(row["last_error"], failure.reason, "{}", failure.folder);
    }
    assert!(sys_app_row(&node, "empty").unwrap()["title"].is_null());
    assert_eq!(audit_rows(&node, "app.load_failed").len(), 4);

    // The good app is fully a node's app.
    let good = node.app("good").unwrap();
    assert_eq!(good.mount(), Some("/a/good/"));
    node.snapshot("good").unwrap();
    assert_eq!(node.restore("good").unwrap().tier, Tier::Sqlite);

    // A second start finds the same four broken and says nothing new about them.
    let again = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(again.loaded, vec!["good"]);
    assert_eq!(again.failed.len(), 4);
    assert_eq!(audit_rows(&node, "app.load_failed").len(), 4);
}

/// `spec/app-contract.md §3.1` — two folders, one slug: the second discovered is refused,
/// and local is discovered before bundled, so the owner's copy wins.
#[test]
fn test_spec_3_1_collision_refuses_the_second_folder() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), "dup", &[]);
    write_app(
        elsewhere.path(),
        "dup",
        Some(&lua_manifest("dup").replace("title = \"dup\"", "title = \"Bundled copy\"")),
        &[("app.lua", "")],
    );

    let report = node
        .load_apps(&[local(&node), AppRoot::bundled(elsewhere.path())])
        .unwrap();
    assert_eq!(report.loaded, vec!["dup"]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].stage, Stage::Collision);
    assert_eq!(report.failed[0].source, Source::Bundled);
    assert_eq!(node.app("dup").unwrap().source(), Source::Local);
    let row = sys_app_row(&node, "dup").unwrap();
    assert_eq!(row["title"], "dup");
    assert_eq!(row["source"], "local");
    assert!(row["last_error"].is_null(), "{row}");
}

// ---------------------------------------------------------------------------------------
// §5.4 — permissions and the CSP
// ---------------------------------------------------------------------------------------

/// `spec/app-contract.md §5.4` and `spec/protocol.md §9.3` — the default forbids inline
/// script, inline handlers included; each permission widens exactly its own token and is
/// surfaced; nothing relaxes the default to make anything work (`AGENTS.md`).
#[test]
fn test_csp_default_blocks_inline_handlers() {
    let csp = Csp::for_app(Some("/a/hello/"), &Permissions::default());
    assert_eq!(csp.header(), DEFAULT_CSP);
    assert!(csp.is_default());
    for forbidden in [
        "'unsafe-inline'",
        "'unsafe-eval'",
        "'unsafe-hashes'",
        "nonce-",
        "'strict-dynamic'",
        "*",
        "data:",
    ] {
        assert!(!csp.header().contains(forbidden), "{forbidden}");
    }

    // Through a loaded app: `sketch` spells every default explicitly and gets exactly the
    // default, with nothing to surface.
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    node.load_apps(&[AppRoot::bundled(repo_apps_dir())])
        .unwrap();
    let sketch = node.app("sketch").unwrap();
    assert_eq!(sketch.csp().header(), DEFAULT_CSP);
    assert!(sketch.warnings().is_empty());
    assert_eq!(sketch.mount(), Some("/a/sketch/"));
    assert_eq!(
        sketch.csp().header_for("http://127.0.0.1:8420"),
        DEFAULT_CSP.replace(
            "script-src 'self'",
            "script-src http://127.0.0.1:8420/a/sketch/ http://127.0.0.1:8420/static/"
        )
    );

    // Each permission widens exactly its token and is surfaced to the owner.
    let apps = node.paths().apps_dir();
    write_app(
        &apps,
        "wide",
        Some(&format!(
            "{}[permissions]\ninline_script = true\nwasm = true\neval = true\n\
             remote = [\"https://cdn.example\"]\nsql = true\n",
            lua_manifest("wide")
        )),
        &[("app.lua", "")],
    );
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec!["wide"], "{report:?}");
    let wide = node.app("wide").unwrap();
    assert_eq!(
        wide.csp().header(),
        "default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval' 'unsafe-eval' \
         https://cdn.example; object-src 'none'; base-uri 'none'; form-action 'self'; \
         frame-ancestors 'none'; img-src 'self' https://cdn.example; connect-src 'self' \
         https://cdn.example"
    );
    let widenings: Vec<&Widening> = wide
        .warnings()
        .iter()
        .filter_map(|w| match w {
            Warning::Permission { widening, .. } => Some(widening),
            _ => None,
        })
        .collect();
    assert_eq!(
        widenings,
        vec![
            &Widening::InlineScript,
            &Widening::Wasm,
            &Widening::Eval,
            &Widening::Remote(vec!["https://cdn.example".into()]),
            &Widening::Sql,
        ]
    );
    assert_eq!(report.warnings.len(), 5);
    assert_eq!(
        report.warnings[3].to_string(),
        "wide: this app uses the following online services: https://cdn.example"
    );
    assert_eq!(report.warnings[3].label(), "Privacy warning");
    assert_eq!(
        report.warnings[3].detail(),
        "this app uses the following online services: https://cdn.example"
    );
    assert_eq!(
        sys_app_row(&node, "wide").unwrap()["permissions"],
        r#"{"eval":true,"inline_script":true,"remote":["https://cdn.example"],"sql":true,"wasm":true}"#
    );

    // A `remote` that is not an origin never reaches a header.
    write_app(
        &apps,
        "leak",
        Some(&format!(
            "{}[permissions]\nremote = [\"https://x; script-src *\"]\n",
            lua_manifest("leak")
        )),
        &[("app.lua", "")],
    );
    let report = node.load_apps(&[local(&node)]).unwrap();
    let leak = report.failed.iter().find(|f| f.folder == "leak").unwrap();
    assert_eq!(leak.stage, Stage::Validate);
    assert!(leak.reason.contains("not an origin"), "{}", leak.reason);
}

/// `docs/frameworks.md §5.4` — `cross_origin_isolated` MUST fail to load in host mode,
/// and is allowed for the solo app; solo mode mounts one app at `/` and nothing else.
#[test]
fn test_cross_origin_isolated_refused_in_host_mode() {
    let root = tempfile::tempdir().unwrap();
    let iso = format!(
        "{}[permissions]\ncross_origin_isolated = true\n",
        lua_manifest("iso")
    );
    {
        let mut node = open(&root);
        let apps = node.paths().apps_dir();
        write_app(&apps, "iso", Some(&iso), &[("app.lua", "")]);
        write_lua_app(&apps, "other", &[]);
        let report = node.load_apps(&[local(&node)]).unwrap();
        assert_eq!(report.loaded, vec!["other"]);
        assert_eq!(report.failed[0].folder, "iso");
        assert!(
            report.failed[0].reason.contains("solo mode only"),
            "{}",
            report.failed[0].reason
        );
    }

    fs::write(
        root.path().join("config.toml"),
        "[node]\nmode = \"solo\"\napp = \"iso\"\n",
    )
    .unwrap();
    let mut node = open(&root);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec!["iso", "other"], "{report:?}");
    assert_eq!(node.app("iso").unwrap().mount(), Some("/"));
    assert_eq!(node.app("other").unwrap().mount(), None);
    assert_eq!(node.mounts().count(), 1);
    assert!(report.warnings.contains(&Warning::Permission {
        slug: "iso".into(),
        widening: Widening::CrossOriginIsolated,
    }));
    assert_eq!(node.app("iso").unwrap().csp().header(), DEFAULT_CSP);
    assert_eq!(
        node.app("iso")
            .unwrap()
            .csp()
            .header_for("http://127.0.0.1:8420"),
        DEFAULT_CSP,
        "the solo app is the origin"
    );

    // A solo app that is not there is said out loud. The previous node goes first: it holds
    // the cache exclusively, and a shadowing `let` does not drop it.
    drop(node);
    fs::write(
        root.path().join("config.toml"),
        "[node]\nmode = \"solo\"\napp = \"absent\"\n",
    )
    .unwrap();
    let mut node = open(&root);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert!(report.warnings.contains(&Warning::SoloAppNotLoaded {
        slug: "absent".into()
    }));
    assert_eq!(node.mounts().count(), 0);
}

// ---------------------------------------------------------------------------------------
// §9 — the seed
// ---------------------------------------------------------------------------------------

/// `spec/app-contract.md §9` — a seed is offered, never loaded silently, refused over an
/// app that has events, and appended as this node's events when it does load.
#[test]
fn test_seed_not_loaded_over_existing_events() {
    let seed = concat!(
        r#"{"seq":1,"lam":1,"ts":"2020-01-01T00:00:00.000Z","dev":"zzzzzzzz","app":"other","op":"put","tbl":"profile","id":"p1","d":{"display_name":"Sample"}}"#,
        "\n",
        r#"{"op":"put","tbl":"profile","id":"p2","d":{"display_name":"Two","extra":true}}"#,
        "\n\n",
        r#"{"op":"del","tbl":"profile","id":"p2"}"#,
        "\n",
    );
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();
    let dev = node.id().as_str().to_owned();

    // (a) An app whose log already has an event — `hello`, with a hand-written line.
    let dir = write_lua_app(
        &apps,
        APP,
        &[("schema.sql", HELLO_DDL), ("sample/seed.jsonl", seed)],
    );
    let log = node.paths().app_log(APP, node.id());
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    hand_append(
        &log,
        &event(
            1,
            1,
            &ts_offset_secs(-60),
            &dev,
            "profile",
            "x",
            Some(r#"{"display_name":"Existing"}"#),
        ),
        "\n",
    );
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec![APP]);
    assert_eq!(
        node.seed_available(APP),
        Some(dir.join("sample").join("seed.jsonl"))
    );
    assert_eq!(log_lines(&log).len(), 1, "load_apps seeded something");
    let error = node.load_seed(APP).unwrap_err();
    assert!(
        matches!(error, Error::SeedRefused { events: 1, .. }),
        "{error}"
    );
    assert_eq!(log_lines(&log).len(), 1, "a refused seed appended");

    // (b) A fresh app: the seed loads once, as this node's events, in one batch.
    write_lua_app(
        &apps,
        "fresh",
        &[("schema.sql", HELLO_DDL), ("sample/seed.jsonl", seed)],
    );
    node.load_apps(&[local(&node)]).unwrap();
    let fresh_log = node.paths().app_log("fresh", node.id());
    assert_eq!(
        fs::read_to_string(&fresh_log).unwrap(),
        "",
        "a load must not seed"
    );
    let seeded = node.load_seed("fresh").unwrap();
    assert_eq!(seeded.events, 3);
    let lines = log_lines(&fresh_log);
    assert_eq!(lines.len(), 3);
    for (index, line) in lines.iter().enumerate() {
        let n = u64::try_from(index).unwrap() + 1;
        assert_eq!(line["seq"], n, "gapless from 1");
        assert_eq!(line["lam"], n);
        assert_eq!(
            line["dev"], dev,
            "this node's events, not the seed's device"
        );
        assert_eq!(line["app"], "fresh");
        assert_ne!(line["ts"], "2020-01-01T00:00:00.000Z");
        assert_eq!(line["ts"], lines[0]["ts"], "one batch, one instant");
    }
    assert_eq!(lines[0]["d"]["display_name"], "Sample");
    assert_eq!(lines[1]["d"]["extra"], true, "keys inside d survive");
    assert_eq!(lines[2]["op"], "del");
    assert!(lines[2].get("d").is_none());
    assert_eq!(
        files_in(fresh_log.parent().unwrap()),
        BTreeSet::from([format!("{dev}.jsonl")]),
        "no other device's log was written"
    );

    // Materialized incrementally, visible through the sandboxed connection.
    let conn = node.app("fresh").unwrap().store().app_conn().unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM profile", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1, "p2 was put and deleted");
    let name: String = conn
        .query_row(
            "SELECT display_name FROM profile WHERE id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Sample");
    drop(conn);
    assert!(matches!(
        node.load_seed("fresh"),
        Err(Error::SeedRefused { events: 3, .. })
    ));

    // No seed, and not loaded, are their own answers.
    write_lua_app(&apps, "plain", &[]);
    node.load_apps(&[local(&node)]).unwrap();
    assert!(node.seed_available("plain").is_none());
    assert!(matches!(node.load_seed("plain"), Err(Error::NoSeed { .. })));
    assert!(matches!(
        node.load_seed("nowhere"),
        Err(Error::AppNotLoaded { .. })
    ));

    // The incremental result is the replay's, across a restart with the cache gone.
    drop(node);
    fs::remove_file(root.path().join("cache").join("fresh.sqlite")).unwrap();
    let mut node = open(&root);
    node.load_apps(&[local(&node)]).unwrap();
    let conn = node.app("fresh").unwrap().store().app_conn().unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM profile", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

/// `docs/plans/phase-1.md §2.3` at the node: the batch is durable before the cache is
/// touched, so when the incremental apply cannot update the cache the caller is not told
/// a landed write failed. The cache is rebuilt from the log before the call returns, a
/// reader sees the row, and the stream is told the tables changed underneath it rather
/// than handed lines that were never applied.
#[test]
fn test_spec_4_5_append_heals_a_cache_the_apply_could_not_update() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();
    write_lua_app(&apps, APP, &[("schema.sql", HELLO_DDL)]);
    node.load_apps(&[local(&node)]).unwrap();
    let mut stream = node.subscribe(APP).unwrap();

    // The cache table is gone underneath the store.
    node.app(APP)
        .unwrap()
        .store()
        .conn()
        .execute_batch("DROP TABLE main.\"profile\"")
        .unwrap();

    let id = "01J9YQ2W7C8XKF3M0N5RTVB6ZP";
    let appended = node
        .append(
            APP,
            Event::put(
                "profile",
                id,
                serde_json::json!({ "display_name": "Gabriel" }),
            ),
        )
        .unwrap();
    assert_eq!(appended.events.len(), 1);
    assert_eq!(
        log_lines(&node.paths().app_log(APP, node.id())).len(),
        1,
        "the write is durable whatever the cache did"
    );

    // Rebuilt before the call returned: a reader on the sandboxed connection sees the row.
    let conn = node.app(APP).unwrap().store().app_conn().unwrap();
    let name: String = conn
        .query_row(
            "SELECT display_name FROM profile WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Gabriel");
    drop(conn);
    assert_eq!(
        node.app(APP).unwrap().store().restore_tier(),
        Some(Tier::Replay)
    );

    // The stream heard that the tables changed, not a line to apply to tables it has.
    match stream.try_recv().unwrap() {
        StreamEvent::Resync { reason, lam } => {
            assert_eq!(reason, "rematerialized");
            assert_eq!(lam, appended.lam);
        }
        other => panic!("expected a resync, got {other:?}"),
    }
    assert!(stream.try_recv().is_err(), "nothing else was broadcast");
}

// ---------------------------------------------------------------------------------------
// §8 — the lifecycle, on the reference apps
// ---------------------------------------------------------------------------------------

/// The four reference apps load as `bundled`, and their `sys_app` rows say exactly what
/// their `app.toml` says (`spec/data-dictionary.md §3.4`, every column). A second load
/// appends nothing.
#[test]
fn test_reference_apps_load_and_index_rows_match() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let report = node
        .load_apps(&[AppRoot::bundled(repo_apps_dir())])
        .unwrap();
    assert_eq!(
        report.loaded,
        vec!["animals", "hello", "pantry", "sketch"],
        "{report:?}"
    );
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.warnings.is_empty(), "{report:?}");
    assert!(report.missing.is_empty());
    assert_eq!(node.mounts().count(), 4);

    for (slug, title, tier, icon, order, tables) in [
        ("hello", "Hello", "lua", "chat-heart", 10, 1),
        ("animals", "Animals", "lua", "diagram-3", 20, 2),
        ("sketch", "Sketch", "web", "pencil-square", 30, 0),
        ("pantry", "Pantry", "web", "box-seam", 40, 3),
    ] {
        let app = node
            .app(slug)
            .unwrap_or_else(|| panic!("{slug} not loaded"));
        assert_eq!(app.source(), Source::Bundled);
        assert_eq!(app.mount(), Some(format!("/a/{slug}/").as_str()));
        assert_eq!(app.store().schema().tables.len(), tables, "{slug}");
        assert_eq!(app.manifest().app.tier.as_str(), tier);

        let dir = repo_apps_dir().join(slug);
        let manifest_text = fs::read_to_string(dir.join("app.toml")).unwrap();
        let schema_text = fs::read_to_string(dir.join("schema.sql")).unwrap_or_default();

        let row = sys_app_row(&node, slug).unwrap_or_else(|| panic!("{slug}: no row"));
        assert_eq!(row["title"], title, "{slug}");
        assert_eq!(row["version"], "1.0.0");
        assert_eq!(row["api"], 1);
        assert_eq!(row["tier"], tier);
        assert_eq!(row["icon"], icon);
        assert_eq!(row["source"], "bundled");
        assert_eq!(row["enabled"], true);
        assert_eq!(row["nav_order"], order);
        assert_eq!(row["advertise"], true);
        assert_eq!(row["permissions"], "{}");
        assert!(row["last_error"].is_null(), "{row}");
        assert!(row["installed_at"].is_string(), "{row}");
        assert_eq!(row["installed_at"], row["updated_at"], "{row}");
        assert_eq!(row["manifest_hash"], sha256_hex(&manifest_text), "{slug}");
        assert_eq!(app.manifest_hash(), sha256_hex(&manifest_text));
        assert_eq!(row["schema_hash"], sha256_hex(&schema_text), "{slug}");
        assert_eq!(app.store().schema().hash, sha256_hex(&schema_text));

        for path in [
            node.paths().app_log_dir(slug),
            node.paths().app_snap_dir(slug),
            node.paths().app_cache_db(slug),
        ] {
            assert!(path.exists(), "{}", path.display());
        }
    }
    let nav: Vec<String> = {
        let mut statement = node
            .store()
            .conn()
            .prepare("SELECT id FROM v_app_nav")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(
        nav,
        vec!["hello", "animals", "sketch", "pantry"],
        "nav_order, then title"
    );

    // The identity rows and founding audit, then one row and one `app.installed` per app, and nothing more
    // on a second load: the upsert is idempotent.
    assert_eq!(sys_lines(&node).len(), 4 + 4 * 2);
    assert_eq!(audit_rows(&node, "app.installed").len(), 4);
    let again = node
        .load_apps(&[AppRoot::bundled(repo_apps_dir())])
        .unwrap();
    assert_eq!(again.loaded, report.loaded);
    assert_eq!(sys_lines(&node).len(), 12, "a second load appended to _sys");
    assert!(audit_rows(&node, "app.load_failed").is_empty());
}

/// `docs/plans/phase-1.md §2.6` — `_sys` and the lint corpus are not apps; nor is a file.
#[test]
fn test_underscore_folders_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();
    write_app(
        &apps,
        "_sys",
        Some(&lua_manifest("_sys")),
        &[("app.lua", "")],
    );
    write_app(
        &apps,
        "_lint/pass/PV101",
        Some(&lua_manifest("pv101")),
        &[("app.lua", "")],
    );
    write_app(
        &apps,
        "_lint/fail/PV101",
        Some("this is deliberately not a manifest"),
        &[],
    );
    fs::write(apps.join("README.md"), "not an app").unwrap();

    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report, privatium_core::LoadReport::default(), "{report:?}");
    assert_eq!(node.apps().count(), 0);
    assert!(audit_rows(&node, "app.load_failed").is_empty());
    assert_eq!(sys_lines(&node).len(), 4, "something was indexed");
}

/// `spec/protocol.md §3` — loading one app adds exactly its four paths to the tree, and
/// `test_spec_3_layout_created`'s set is untouched until then.
#[test]
fn test_spec_3_app_layout() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let before = tree(root.path());
    write_lua_app(&node.paths().apps_dir(), APP, &[]);
    let with_folder = tree(root.path());
    assert_eq!(
        with_folder.difference(&before).cloned().collect::<Vec<_>>(),
        vec![
            "apps/hello/".to_owned(),
            "apps/hello/app.lua".to_owned(),
            "apps/hello/app.toml".to_owned(),
        ]
    );

    node.load_apps(&[local(&node)]).unwrap();
    let dev = node.id().as_str();
    let added: BTreeSet<String> = tree(root.path())
        .difference(&with_folder)
        .cloned()
        .collect();
    assert_eq!(
        added,
        BTreeSet::from([
            "cache/hello.sqlite".to_owned(),
            "data/hello/".to_owned(),
            "data/hello/log/".to_owned(),
            format!("data/hello/log/{dev}.jsonl"),
            "data/hello/snap/".to_owned(),
        ])
    );
}

/// `spec/data-dictionary.md §3.4` — the row is an event in `data/_sys/`, carrying every
/// column the loader knows, with `app.installed` in the same batch.
#[test]
fn test_sys_app_upsert_is_an_event() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), APP, &[]);
    node.load_apps(&[local(&node)]).unwrap();

    let lines = sys_lines(&node);
    assert_eq!(lines.len(), 6);
    let row = &lines[4];
    assert_eq!(row["tbl"], "sys_app");
    assert_eq!(row["id"], APP, "keyed by slug, not a ULID");
    assert_eq!(row["op"], "put");
    let keys: BTreeSet<&str> = row["d"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "title",
            "version",
            "api",
            "tier",
            "source",
            "enabled",
            "installed_at",
            "updated_at",
            "schema_hash",
            "manifest_hash",
            "advertise",
            "permissions",
        ]
        .into_iter()
        .collect(),
        "icon, nav_order and last_error are NULL, so absent (§2.1)"
    );
    assert_eq!(row["d"]["api"], 1);
    assert_eq!(row["d"]["source"], "local");
    assert_eq!(row["d"]["schema_hash"], sha256_hex(""), "no schema.sql");
    assert_eq!(lines[5]["tbl"], "sys_audit");
    assert_eq!(lines[5]["d"]["kind"], "app.installed");
    assert_eq!(lines[5]["d"]["subject"], APP);
    assert_eq!(lines[4]["ts"], lines[5]["ts"], "one batch");
}

/// `spec/data-dictionary.md §3.4` — removing a folder MUST NOT delete the row or the
/// data; it sets `last_error = "folder missing"`, and the folder's return clears it.
#[test]
fn test_removed_folder_sets_last_error_and_keeps_row() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    let apps = node.paths().apps_dir();
    write_lua_app(&apps, "gone", &[]);
    node.load_apps(&[local(&node)]).unwrap();
    drop(node);

    fs::remove_dir_all(apps.join("gone")).unwrap();
    let mut node = open(&root);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.missing, vec!["gone"]);
    assert!(node.app("gone").is_none());
    let row = sys_app_row(&node, "gone").unwrap();
    assert_eq!(row["last_error"], "folder missing");
    assert_eq!(row["title"], "gone", "the rest of the row survives");
    assert!(row["installed_at"].is_string());
    assert!(
        node.paths().app_log_dir("gone").exists(),
        "the data survives"
    );
    let again = node.load_apps(&[local(&node)]).unwrap();
    assert!(again.missing.is_empty(), "marked again");

    write_lua_app(&apps, "gone", &[]);
    let back = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(back.loaded, vec!["gone"]);
    assert!(sys_app_row(&node, "gone").unwrap()["last_error"].is_null());
}

/// `spec/data-dictionary.md §3.4` — `enabled = false` is the owner's, survives a reload,
/// and keeps the app out of the mounts while its row and data stay.
#[test]
fn test_disabled_app_keeps_row_and_is_not_mounted() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), "quiet", &[]);
    node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(sys_app_row(&node, "quiet").unwrap()["enabled"], true);
    let dev = node.id().as_str().to_owned();
    let sys_log = node.paths().app_log("_sys", node.id());
    let next = u64::try_from(sys_lines(&node).len()).unwrap() + 1;
    drop(node);

    // The owner's action, as the settings page will write it: a put amending the row.
    let ts = ts_offset_secs(-1);
    hand_append(
        &sys_log,
        &format!(
            r#"{{"seq":{next},"lam":{next},"ts":"{ts}","dev":"{dev}","app":"_sys","op":"put","tbl":"sys_app","id":"quiet","d":{{"source":"local","enabled":false}}}}"#
        ),
        "\n",
    );
    let mut node = open(&root);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.disabled, vec!["quiet"]);
    assert!(report.loaded.is_empty());
    assert!(node.app("quiet").is_none());
    assert_eq!(node.mounts().count(), 0);
    let row = sys_app_row(&node, "quiet").unwrap();
    assert_eq!(
        row["enabled"], false,
        "the owner's choice survived the reload"
    );
    assert_eq!(
        row["title"], "quiet",
        "the manifest's facts were re-asserted"
    );
    assert!(row["last_error"].is_null());
}

// ---------------------------------------------------------------------------------------
// §7 — the sandboxed cache
// ---------------------------------------------------------------------------------------

/// `spec/app-contract.md §7` — after load an app's `app_conn` can read and do nothing
/// else: no writes, no `ATTACH`, no `PRAGMA`, `identity/node.key` out of reach.
#[test]
fn test_app_store_is_sandboxed_after_load() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    node.load_apps(&[AppRoot::bundled(repo_apps_dir())])
        .unwrap();
    let key = node
        .paths()
        .node_key()
        .display()
        .to_string()
        .replace('\\', "/");

    for slug in ["hello", "animals", "sketch"] {
        let store = node.app(slug).unwrap().store();
        let conn = store.app_conn().unwrap();
        for sql in [
            format!("ATTACH '{key}' AS leak"),
            format!("VACUUM INTO '{key}'"),
            "CREATE TABLE leak (id TEXT)".to_owned(),
            "PRAGMA query_only = 0".to_owned(),
        ] {
            assert!(conn.execute_batch(&sql).is_err(), "{slug}: {sql} ran");
        }
    }
    let conn = node.app("hello").unwrap().store().app_conn().unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM profile", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    // Through the app's handle the tombstone set is readable — a derived fact, not a
    // secret — and not writable.
    assert!(
        conn.execute_batch("DELETE FROM pv_tombstone").is_err(),
        "the tombstone set was writable"
    );
}

/// `spec/protocol.md §5.3` for an app — the tier reaches `v_health`, `Node::restore_tier`,
/// `local/state.jsonl`, and the bounded audits.
#[test]
fn test_app_restore_tier_reaches_health_and_node() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), APP, &[("schema.sql", HELLO_DDL)]);
    seed_hello_log(&node);
    node.load_apps(&[local(&node)]).unwrap();

    // A log with events and no cache is `docs/backup-and-restore.md §3`'s "I rebuilt from
    // scratch" — the case a folder restored from a backup is — and is alerted once.
    assert_eq!(node.restore_tier(APP), Some(Tier::Replay));
    assert_eq!(audit_rows(&node, "restore.tier3").len(), 1);

    let snapshot = node.snapshot(APP).unwrap();
    assert_eq!(snapshot.manifest.tables[0].rows, 1);

    let restored = node.restore(APP).unwrap();
    assert_eq!(restored.tier, Tier::Sqlite, "{restored:?}");
    assert_eq!(node.restore_tier(APP), Some(Tier::Sqlite));
    let state = State::load(&node.paths().local_state()).unwrap();
    let record = state
        .get(APP)
        .unwrap()
        .materialized
        .restore
        .clone()
        .unwrap();
    assert_eq!(record.tier, Tier::Sqlite);
    assert_eq!(
        record.snapshot.as_deref(),
        Some(snapshot.id.to_string().as_str())
    );
    let (tier, snapshot_id, log_bytes): (i32, Option<String>, i64) = node
        .store()
        .conn()
        .query_row(
            "SELECT restore_tier, snapshot_id, log_bytes FROM v_health WHERE app_id = ?",
            rusqlite::params![APP],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(tier, 1);
    assert_eq!(
        snapshot_id.as_deref(),
        Some(snapshot.id.to_string().as_str())
    );
    assert!(log_bytes > 0);

    // Tier 2, audited once however often it recurs; the app is still served.
    flip_byte(&snapshot.dir.join("profile.sqlite"));
    assert_eq!(node.restore(APP).unwrap().tier, Tier::Csv);
    assert_eq!(node.restore(APP).unwrap().tier, Tier::Csv);
    node.refresh().unwrap();
    assert_eq!(audit_rows(&node, "restore.tier2").len(), 1);
    assert_eq!(node.restore_tier(APP), Some(Tier::Csv));
    let conn = node.app(APP).unwrap().store().app_conn().unwrap();
    let name: String = conn
        .query_row(
            "SELECT display_name FROM profile WHERE id = 'a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Gabriel");

    // Maintenance goes through the same functions for an app as for `_sys`.
    let maintenance = node.maintain(APP, jiff::Timestamp::now()).unwrap();
    assert!(maintenance.snapshot.is_none(), "nothing due a moment later");
    assert!(node.verify_snapshot(APP, &snapshot.id).is_ok());
}

/// A loaded app snapshotted through `Node::snapshot` restores from tier 1 across a
/// restart with `cache/` gone, and the tables come back identical.
#[test]
fn test_app_snapshot_restores_at_tier1_across_restart() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), APP, &[("schema.sql", HELLO_DDL)]);
    seed_hello_log(&node);
    node.load_apps(&[local(&node)]).unwrap();
    let snapshot = node.snapshot(APP).unwrap();
    let before = digest_via(
        &node.app(APP).unwrap().store().app_conn().unwrap(),
        "profile",
    );
    assert_ne!(before, "empty");
    drop(node);

    fs::remove_dir_all(root.path().join("cache")).unwrap();
    let mut node = open(&root);
    let report = node.load_apps(&[local(&node)]).unwrap();
    assert_eq!(report.loaded, vec![APP]);
    let app = node.app(APP).unwrap();
    let restored = app.store().restored().unwrap();
    assert_eq!(restored.tier, Tier::Sqlite, "{restored:?}");
    assert_eq!(
        restored.snapshot.as_deref(),
        Some(snapshot.id.to_string().as_str())
    );
    assert_eq!(node.restore_tier(APP), Some(Tier::Sqlite));
    assert_eq!(
        digest_via(&app.store().app_conn().unwrap(), "profile"),
        before
    );
    let tier: i32 = node
        .store()
        .conn()
        .query_row(
            "SELECT restore_tier FROM v_health WHERE app_id = ?",
            rusqlite::params![APP],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tier, 1);
    assert_eq!(
        files_in(&root.path().join("cache")),
        BTreeSet::from(["_sys.sqlite".to_owned(), "hello.sqlite".to_owned()])
    );
    // `hello` was alerted once, at its first load over a restored log with no cache; the
    // restart found a snapshot that applied and added nothing. `_sys` has its own.
    let hello_alerts = audit_rows(&node, "restore.tier3")
        .iter()
        .filter(|row| {
            row["detail"]
                .as_str()
                .unwrap()
                .contains("\"app\":\"hello\"")
        })
        .count();
    assert_eq!(
        hello_alerts, 1,
        "a snapshot applied; the restart rebuilt nothing from scratch"
    );
}

/// `apps/hello/README.md` — a line appended by `echo` appears on the next reload, through
/// the store, and a reload with nothing new does nothing.
#[test]
fn test_hand_appended_line_appears_through_refresh_app() {
    let root = tempfile::tempdir().unwrap();
    let mut node = open(&root);
    write_lua_app(&node.paths().apps_dir(), APP, &[("schema.sql", HELLO_DDL)]);
    node.load_apps(&[local(&node)]).unwrap();
    assert!(!node.refresh_app(APP).unwrap(), "nothing to do yet");

    let dev = node.id().as_str().to_owned();
    hand_append(
        &node.paths().app_log(APP, node.id()),
        &event(
            1,
            1,
            &ts_offset_secs(-30),
            &dev,
            "profile",
            "the-ulid",
            Some(r#"{"display_name":"Gabriel"}"#),
        ),
        "\n",
    );
    assert!(node.refresh_app(APP).unwrap(), "the log grew unnoticed");
    let app = node.app(APP).unwrap();
    let name: String = app
        .store()
        .app_conn()
        .unwrap()
        .query_row(
            "SELECT display_name FROM profile WHERE id = 'the-ulid'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(name, "Gabriel");
    assert!(!node.refresh_app(APP).unwrap());
    assert!(matches!(
        node.refresh_app("nowhere"),
        Err(Error::AppNotLoaded { .. })
    ));
}
