// This file is part of Privatium
// crates/privatium-core/tests/reference.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-10-04
// Summary: The three reference apps end to end through core::handle with no listener, exactly as their
//          READMEs describe them: hello (write, amend, break the cache, and the README's own
//          `echo >>` line run for real), animals (the seed, a round over htmx and over plain
//          form posts, the three-event teach, the recursive knowledge page, reset as
//          tombstones, and the CSP the Alpine build runs under), sketch (every call app.js
//          makes, against the log), pantry (the seed through its seven views, a batch added as
//          one batch of the log, and the merge rule's own cases: a move, a double undo, a
//          balance below zero). Then the accessibility baseline: the PV4xx rules of
//          spec/cli.md §5 held over the shell's own pages, the Tier 1 page frame and the
//          reference views (§5.4), and the declared colour tokens at their contrast floors
//          (PV406).
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
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use axum::body::{Body, HttpBody as _, to_bytes};
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, LOCATION};
use axum::http::{Method, StatusCode};
use common::a11y::{self, Unit};
use common::{hand_append, log_lines, lua_manifest, repo_apps_dir, write_app, write_web_app};
use privatium_core::http::shell;
use privatium_core::{AppRoot, Error, Handler, LoadReport, Node, Request, Response};
use serde_json::{Value, json};

/// A small VM pool, as tests/lua.rs uses.
const LUA_CONFIG: &str =
    "[lua]\npool_size = 2\nmax_instructions = 5000000\nmax_memory_mb = 16\nmax_seconds = 20\n";

fn configure(root: &tempfile::TempDir, config: &str) {
    fs::write(root.path().join("config.toml"), config).unwrap();
}

/// A node with the owner's `apps/` and the repository's reference apps as `bundled`.
fn open(root: &tempfile::TempDir) -> (Node, LoadReport) {
    let mut node = Node::open(root.path()).unwrap();
    let roots = [
        AppRoot::local(node.paths().apps_dir()),
        AppRoot::bundled(repo_apps_dir()),
    ];
    let report = node.load_apps(&roots).unwrap();
    (node, report)
}

fn handler_for(root: &tempfile::TempDir) -> Handler {
    let (node, report) = open(root);
    Handler::new(node, report)
}

fn request(method: Method, path: &str) -> Request {
    axum::http::Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn get(path: &str) -> Request {
    request(Method::GET, path)
}

fn post(path: &str, form: &str) -> Request {
    axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form.to_owned()))
        .unwrap()
}

fn post_json(path: &str, value: &Value) -> Request {
    axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}

/// The mount a host-mode path lies beneath: `/a/<slug>/`.
fn mount_of(path: &str) -> String {
    let slug = path.trim_start_matches("/a/").split('/').next().unwrap();
    format!("/a/{slug}/")
}

/// The mount's token — what `csrf()` emits (`spec/lua-api.md §4.1`).
fn token(handler: &Handler, mount: &str) -> String {
    handler.csrf().token(mount)
}

/// A form POST carrying the mount's token, as a form with `<?= csrf() ?>` would.
fn posted(handler: &Handler, path: &str, form: &str) -> Request {
    let token = token(handler, &mount_of(path));
    let body = if form.is_empty() {
        format!("_csrf={token}")
    } else {
        format!("{form}&_csrf={token}")
    };
    post(path, &body)
}

fn with_htmx(mut request: Request) -> Request {
    request
        .headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    request
}

/// `request` as htmx sends a boosted one: a navigation, answered with the whole page.
fn with_boost(mut request: Request) -> Request {
    request
        .headers_mut()
        .insert("hx-boosted", "true".parse().unwrap());
    request
}

async fn body_of(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn json_of(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let text = body_of(response).await;
    let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, value)
}

fn header<'a>(response: &'a Response, name: &axum::http::HeaderName) -> &'a str {
    response
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap())
        .unwrap_or_default()
}

fn log_path(handler: &Handler, slug: &str) -> PathBuf {
    let node = handler.node().lock().unwrap();
    node.paths().app_log(slug, node.id())
}

fn node_id(handler: &Handler) -> String {
    handler.node().lock().unwrap().id().as_str().to_owned()
}

/// The next SSE frame of a stream body, within `secs`, as `(event, data)`.
async fn next_frame(body: &mut Body, secs: u64) -> Option<(String, String)> {
    let frame = tokio::time::timeout(
        Duration::from_secs(secs),
        std::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)),
    )
    .await
    .ok()??
    .ok()?;
    let text = String::from_utf8_lossy(&frame.into_data().ok()?).into_owned();
    let mut event = String::new();
    let mut data = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event = rest.to_owned();
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data = rest.to_owned();
        }
    }
    Some((event, data))
}

/// A ULID the way `pv.js` mints one: ten characters of time, sixteen of randomness.
fn ulid() -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut out = vec![b'0'; 26];
    for slot in (0..10).rev() {
        out[slot] = ALPHABET[(millis % 32) as usize];
        millis /= 32;
    }
    let mut seed = std::ptr::addr_of!(out) as u64 ^ millis as u64;
    for slot in out.iter_mut().skip(10) {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *slot = ALPHABET[(seed % 32) as usize];
    }
    String::from_utf8(out).unwrap()
}

fn is_ulid(text: &str) -> bool {
    text.len() == 26
        && text
            .bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

/// The `x-data` values a rendered page carries, and the `Alpine.data('…')` names an app's
/// script registers — under the CSP build the former must be a subset of the latter.
fn x_data_values(html: &str) -> BTreeSet<String> {
    a11y::parse(html)
        .descendants()
        .iter()
        .filter_map(|e| e.attr("x-data").map(str::to_owned))
        .collect()
}

fn alpine_components(script: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = script;
    while let Some(at) = rest.find("Alpine.data('") {
        let after = &rest[at + "Alpine.data('".len()..];
        let end = after.find('\'').unwrap();
        out.insert(after[..end].to_owned());
        rest = &after[end..];
    }
    out
}

fn assert_clean(what: &str, html: &str, unit: Unit) {
    let findings = a11y::check(html, unit);
    assert!(
        findings.is_empty(),
        "{what}: {} finding(s):\n  {}\n\n{html}",
        findings.len(),
        findings.join("\n  ")
    );
}

// ---------------------------------------------------------------------------------------
// hello
// ---------------------------------------------------------------------------------------

/// `apps/hello/README.md` — load, the greeting inside the frame, the form with its label
/// and token, a name written as one line of the log checked field by field, an edit that
/// amends the same row, and "break the cache; nothing is lost".
#[tokio::test]
async fn test_hello_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let (node, report) = open(&root);
    assert!(report.loaded.contains(&"hello".to_owned()), "{report:?}");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(node.app("hello").unwrap().mount(), Some("/a/hello/"));
    let dev = node.id().as_str().to_owned();
    let cache_dir = node.paths().cache_dir();
    let snap_dir = node.paths().app_snap_dir("hello");
    let log = node.paths().app_log("hello", node.id());
    let handler = Handler::new(node, report);
    let mount_token = token(&handler, "/a/hello/");

    // The empty state, inside the frame, with the page's one h1 supplied by the view.
    let home = handler.handle(get("/a/hello/")).await;
    assert_eq!(home.status(), StatusCode::OK);
    assert_eq!(header(&home, &CONTENT_TYPE), "text/html; charset=utf-8");
    let text = body_of(home).await;
    assert!(text.starts_with("<!doctype html>"), "{text}");
    assert!(text.contains("<html lang=\"en\">"), "{text}");
    assert!(text.contains("<title>Hello — Privatium</title>"), "{text}");
    assert!(text.contains("<main id=\"main\">"), "{text}");
    assert!(text.contains("<h1>We haven't met yet.</h1>"), "{text}");
    assert!(text.contains("href=\"/a/hello/edit\""), "{text}");
    assert_eq!(text.matches("<h1").count(), 1, "{text}");

    // The form: one field, labelled by `for`, and csrf() in it.
    let edit = body_of(handler.handle(get("/a/hello/edit")).await).await;
    assert!(edit.contains("<label for=\"display_name\">"), "{edit}");
    assert!(edit.contains("id=\"display_name\""), "{edit}");
    assert!(
        edit.contains(&format!("name=\"_csrf\" value=\"{mount_token}\"")),
        "{edit}"
    );
    assert!(
        edit.contains("<form method=\"post\" action=\"/a/hello/name\">"),
        "{edit}"
    );

    // A name: 303 home, greeted, and one line in the log with every field as §4.1 has it.
    let named = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=Ada"))
        .await;
    assert_eq!(named.status(), StatusCode::SEE_OTHER);
    assert_eq!(header(&named, &LOCATION), "/a/hello/");
    let greeting = body_of(handler.handle(get("/a/hello/")).await).await;
    assert!(greeting.contains("Good "), "{greeting}");
    assert!(greeting.contains("Ada."), "{greeting}");
    assert!(greeting.contains("Change my name"), "{greeting}");
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    let keys: BTreeSet<&str> = line
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["seq", "lam", "ts", "dev", "app", "op", "tbl", "id", "d"]
            .into_iter()
            .collect()
    );
    assert_eq!(line["seq"], 1);
    assert_eq!(line["lam"], 1);
    assert_eq!(line["dev"], dev);
    assert_eq!(line["app"], "hello");
    assert_eq!(line["op"], "put");
    assert_eq!(line["tbl"], "profile");
    assert_eq!(line["d"], json!({ "display_name": "Ada" }));
    assert!(is_ulid(line["id"].as_str().unwrap()), "{line}");
    assert!(line["ts"].as_str().unwrap().ends_with('Z'), "{line}");
    let profile_id = line["id"].as_str().unwrap().to_owned();

    // An edit reuses the id: an amendment, one row in the table, two lines in the log.
    let renamed = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=Grace"))
        .await;
    assert_eq!(renamed.status(), StatusCode::SEE_OTHER);
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["id"], profile_id);
    assert_eq!(lines[1]["seq"], 2);
    assert_eq!(lines[1]["lam"], 2);
    {
        let node = handler.node().lock().unwrap();
        let conn = node.app("hello").unwrap().store().app_conn().unwrap();
        let mut statement = conn
            .prepare("SELECT id, display_name FROM profile")
            .unwrap();
        let rows: Vec<(String, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows, vec![(profile_id.clone(), "Grace".to_owned())]);
    }

    // "Break the cache; nothing is lost": the cache and the snapshots gone, the node
    // reopened, the greeting still there and the next edit still an amendment.
    drop(handler);
    fs::remove_dir_all(&cache_dir).unwrap();
    let _ = fs::remove_dir_all(&snap_dir);
    let handler = handler_for(&root);
    let greeting = body_of(handler.handle(get("/a/hello/")).await).await;
    assert!(greeting.contains("Grace."), "{greeting}");
    let again = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=Linus"))
        .await;
    assert_eq!(again.status(), StatusCode::SEE_OTHER);
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[2]["id"], profile_id);
    assert_eq!(lines[2]["seq"], 3);
    assert_eq!(lines[2]["lam"], 3);
}

/// `apps/hello/README.md`, "Try this" — the README's own `echo '…' >> …` command, parsed
/// out of the file and run against a log that holds the three lines it describes: the
/// path is this device's log, `seq` 4 keeps the file gapless (`spec/protocol.md §4.1`),
/// `lam` 4 beats the row it amends (`§4.5`), the page shows it, and the next write the
/// node makes takes `seq` 5 and `lam` 5 (`§4.3`, after the log is rescanned).
#[tokio::test]
async fn test_hello_readme_echo_example_is_valid() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let handler = handler_for(&root);
    let dev = node_id(&handler);
    let log = log_path(&handler, "hello");
    for name in ["Ada", "Grace", "Linus"] {
        let response = handler
            .handle(posted(
                &handler,
                "/a/hello/name",
                &format!("display_name={name}"),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
    let lines = log_lines(&log);
    assert_eq!(
        lines.len(),
        3,
        "the README says three name changes are three lines"
    );
    let profile_id = lines[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(lines[2]["lam"], 3);

    // The command, as written: join the `\` continuation, take the single-quoted JSON
    // and the `>>` path.
    let readme = fs::read_to_string(repo_apps_dir().join("hello").join("README.md")).unwrap();
    let mut joined = String::new();
    for line in readme.lines() {
        match line.strip_suffix('\\') {
            Some(head) => joined.push_str(head),
            None => {
                joined.push_str(line);
                joined.push('\n');
            }
        }
    }
    let command = joined
        .lines()
        .find(|line| line.trim_start().starts_with("echo '"))
        .expect("the README's echo example")
        .trim();
    let quoted_start = command.find('\'').unwrap() + 1;
    let quoted_end = command.rfind('\'').unwrap();
    let json_template = &command[quoted_start..quoted_end];
    let path_template = command[quoted_end + 1..]
        .trim()
        .strip_prefix(">>")
        .expect("appends with >>")
        .trim();
    assert_eq!(path_template, "data/hello/log/<your-id>.jsonl");
    let path = path_template.replace("<your-id>", &dev);
    assert_eq!(
        root.path().join(&path),
        log,
        "the README's path is this device's log"
    );
    let line = json_template
        .replace("<your-id>", &dev)
        .replace("<the-ulid>", &profile_id);
    let value: Value = serde_json::from_str(&line).expect("the README's JSON parses");
    assert_eq!(value["seq"], 4, "the next seq after three lines");
    assert_eq!(
        value["lam"], 4,
        "above the log's highest lam, or it loses the merge"
    );
    assert_eq!(value["dev"], dev);
    assert_eq!(value["app"], "hello");
    assert_eq!(value["op"], "put");
    assert_eq!(value["tbl"], "profile");
    assert_eq!(value["id"], profile_id);
    assert_eq!(value["d"]["display_name"], "Someone Else");

    hand_append(&log, &line, "\n");
    let lines = log_lines(&log);
    let seqs: Vec<u64> = lines.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, [1, 2, 3, 4], "gapless");

    // Reload the page: the hand-written line won.
    let greeting = body_of(handler.handle(get("/a/hello/")).await).await;
    assert!(greeting.contains("Someone Else."), "{greeting}");

    // The node picked both counters up from the file: the next write is seq 5, lam 5,
    // and still an amendment of the same row.
    let fifth = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=Fifth"))
        .await;
    assert_eq!(fifth.status(), StatusCode::SEE_OTHER);
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[4]["seq"], 5);
    assert_eq!(lines[4]["lam"], 5);
    assert_eq!(lines[4]["id"], profile_id);
    let greeting = body_of(handler.handle(get("/a/hello/")).await).await;
    assert!(greeting.contains("Fifth."), "{greeting}");
}

// ---------------------------------------------------------------------------------------
// animals
// ---------------------------------------------------------------------------------------

/// The seed's events as the file spells them, in order.
fn animals_seed() -> Vec<Value> {
    fs::read_to_string(
        repo_apps_dir()
            .join("animals")
            .join("sample")
            .join("seed.jsonl"),
    )
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect()
}

/// Load `sample/seed.jsonl` through the settings page, as the owner does.
async fn seed_animals(handler: &Handler) {
    let action = "/settings/apps/animals/seed";
    let page = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(page.contains(&format!("action=\"{action}\"")), "{page}");
    let token = handler.csrf().token(action);
    let seeded = handler
        .handle(post(action, &format!("_csrf={token}")))
        .await;
    assert_eq!(seeded.status(), StatusCode::SEE_OTHER);
}

/// `apps/animals/README.md` — the seed through the settings page as this node's events,
/// a round over htmx (the `_board` fragment, the cursor row, the same question on a
/// reload), the three-event teach that turns the leaf into the question in one batch, the
/// recursive knowledge page, reset as tombstones for every node, and the Alpine CSP build
/// under the default policy: no `unsafe-*`, every `x-data` a registered component, no
/// inline handler anywhere.
#[tokio::test]
async fn test_animals_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let handler = handler_for(&root);
    let dev = node_id(&handler);
    let log = log_path(&handler, "animals");
    let seed = animals_seed();
    assert_eq!(seed.len(), 7);

    seed_animals(&handler).await;
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 7);
    for (index, (line, wanted)) in lines.iter().zip(&seed).enumerate() {
        assert_eq!(line["seq"], index as u64 + 1, "{line}");
        assert_eq!(line["dev"], dev, "the seed is appended as this node's");
        assert_eq!(line["app"], "animals");
        assert_eq!(line["op"], "put");
        assert_eq!(line["tbl"], "node");
        assert_eq!(line["id"], wanted["id"]);
        assert_eq!(line["d"], wanted["d"]);
        assert!(is_ulid(line["id"].as_str().unwrap()), "{line}");
    }
    let by_text = |text: &str| -> String {
        seed.iter().find(|e| e["d"]["text"] == text).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let page = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(!page.contains("/settings/apps/animals/seed"), "{page}");
    assert!(page.contains("<code>node</code>: 7 rows"), "{page}");

    // Play starts at the root: no cursor row, the question nobody points at.
    let play = handler.handle(get("/a/animals/")).await;
    assert_eq!(play.status(), StatusCode::OK);
    let csp = header(&play, &CONTENT_SECURITY_POLICY).to_owned();
    assert!(!csp.contains("unsafe-"), "{csp}");
    assert!(csp.contains("script-src http://"), "{csp}");
    assert!(
        csp.contains("/a/animals/ "),
        "scoped to the app's path: {csp}"
    );
    let text = body_of(play).await;
    assert!(
        text.contains("<h1>Does it live in the water?</h1>"),
        "{text}"
    );
    assert!(text.contains("4 animals, 3 questions"), "{text}");
    // Swap navigation: the main region is boosted, and the app's stylesheet and scripts
    // load once in the head, from [ui], so the view itself carries none.
    assert!(
        text.contains("<main id=\"main\" hx-boost=\"true\">"),
        "{text}"
    );
    let (head, rest) = text.split_once("</head>").unwrap();
    assert!(
        head.contains(
            "<link rel=\"stylesheet\" href=\"/a/animals/static/animals.css\" integrity=\"sha256-"
        ),
        "{head}"
    );
    let main = rest
        .split("<main")
        .nth(1)
        .unwrap()
        .split("</main>")
        .next()
        .unwrap();
    assert!(!main.contains("<script"), "{main}");
    assert!(!main.contains("<link"), "{main}");

    // A boosted request for another page is a navigation: the whole page, whose main
    // region holds the view, for the frame to swap in.
    let boosted = body_of(
        handler
            .handle(with_boost(with_htmx(get("/a/animals/knowledge"))))
            .await,
    )
    .await;
    assert!(boosted.starts_with("<!doctype html>"), "{boosted}");
    let boosted_main = boosted
        .split("<main id=\"main\" hx-boost=\"true\">")
        .nth(1)
        .and_then(|rest| rest.split("</main>").next())
        .unwrap_or_else(|| panic!("{boosted}"));
    assert!(
        boosted_main.contains("<h1>What I know</h1>"),
        "{boosted_main}"
    );
    assert!(
        boosted.contains("<ul id=\"pv-app-menu\" class=\"pv-menu-app\" hx-swap-oob=\"true\">"),
        "{boosted}"
    );

    // An answer over htmx: the fragment alone, its forms carrying the token, the cursor
    // written, and the same question on a fresh GET.
    let answered = handler
        .handle(with_htmx(posted(
            &handler,
            "/a/animals/answer",
            "choice=yes",
        )))
        .await;
    assert_eq!(answered.status(), StatusCode::OK);
    let fragment = body_of(answered).await;
    assert!(!fragment.contains("<!doctype"), "{fragment}");
    assert!(!fragment.contains("pv-header"), "{fragment}");
    assert!(
        fragment.contains("<h1>Does it have fins?</h1>"),
        "{fragment}"
    );
    assert!(fragment.contains("name=\"_csrf\""), "{fragment}");
    for form in a11y::parse(&fragment).find_all("form") {
        assert_eq!(form.attr("method"), Some("post"), "{fragment}");
        assert_eq!(form.attr("action"), form.attr("hx-post"), "{fragment}");
        assert!(
            form.descendants()
                .iter()
                .any(|e| e.name == "input" && e.attr("name") == Some("_csrf")),
            "{fragment}"
        );
    }
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 8);
    assert_eq!(lines[7]["tbl"], "cursor");
    assert_eq!(lines[7]["id"], "cursor");
    assert_eq!(lines[7]["d"]["node_id"], by_text("Does it have fins?"));
    assert!(lines[7]["d"]["started"].is_string(), "{}", lines[7]);
    let reloaded = body_of(handler.handle(get("/a/animals/")).await).await;
    assert!(
        reloaded.contains("<h1>Does it have fins?</h1>"),
        "{reloaded}"
    );

    // Down to a leaf.
    let leaf = body_of(
        handler
            .handle(with_htmx(posted(
                &handler,
                "/a/animals/answer",
                "choice=yes",
            )))
            .await,
    )
    .await;
    assert!(leaf.contains("<h1>Is it a shark?</h1>"), "{leaf}");
    assert_eq!(log_lines(&log).len(), 9);

    // Teach: the leaf becomes the question, keeping its id; three puts and the cursor's
    // tombstone in one batch — one ts, contiguous seqs.
    let teach = body_of(handler.handle(get("/a/animals/teach")).await).await;
    assert!(teach.contains("tells it apart from shark"), "{teach}");
    assert!(teach.contains("<label for=\"answer-yes\">"), "{teach}");
    assert!(teach.contains("id=\"answer-yes\""), "{teach}");
    let taught = handler
        .handle(posted(
            &handler,
            "/a/animals/teach",
            "animal=dolphin&question=Is+it+a+mammal%3F&answer=yes",
        ))
        .await;
    assert_eq!(taught.status(), StatusCode::SEE_OTHER);
    assert_eq!(header(&taught, &LOCATION), "/a/animals/");
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 13);
    let batch = &lines[9..13];
    let ts: BTreeSet<&str> = batch.iter().map(|l| l["ts"].as_str().unwrap()).collect();
    assert_eq!(ts.len(), 1, "one batch, one ts: {batch:?}");
    let seqs: Vec<u64> = batch.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, [10, 11, 12, 13]);
    assert_eq!(batch[0]["d"], json!({ "kind": "a", "text": "dolphin" }));
    assert_eq!(batch[1]["d"], json!({ "kind": "a", "text": "shark" }));
    assert_eq!(
        batch[2]["id"],
        by_text("shark"),
        "the leaf became the question"
    );
    assert_eq!(batch[2]["d"]["kind"], "q");
    assert_eq!(batch[2]["d"]["text"], "Is it a mammal?");
    assert_eq!(batch[2]["d"]["yes_id"], batch[0]["id"]);
    assert_eq!(batch[2]["d"]["no_id"], batch[1]["id"]);
    assert_eq!(batch[3]["op"], "del");
    assert_eq!(batch[3]["tbl"], "cursor");
    assert!(batch[3].get("d").is_none(), "{}", batch[3]);
    let dolphin_id = batch[0]["id"].as_str().unwrap().to_owned();
    let shark_id = batch[1]["id"].as_str().unwrap().to_owned();
    assert!(is_ulid(&dolphin_id) && is_ulid(&shark_id));

    // The knowledge page: the recursive walk, `->` between the questions.
    let knowledge = body_of(handler.handle(get("/a/animals/knowledge")).await).await;
    assert!(
        knowledge.contains("<th scope=\"col\">Animal</th>"),
        "{knowledge}"
    );
    for animal in ["shark", "otter", "parrot", "wombat", "dolphin"] {
        assert!(
            knowledge.contains(&format!("<td>{animal}</td>")),
            "{animal}: {knowledge}"
        );
    }
    assert!(
        knowledge.contains(
            "Does it live in the water?: yes -&gt; Does it have fins?: yes -&gt; Is it a mammal?: yes"
        ),
        "{knowledge}"
    );
    assert!(
        knowledge.contains(&format!("aria-controls=\"path-{dolphin_id}\"")),
        "{knowledge}"
    );
    assert!(
        knowledge.contains(&format!("id=\"path-{dolphin_id}\"")),
        "{knowledge}"
    );

    // The Alpine CSP build: every x-data names a registered component, and no page
    // carries an inline handler the policy would drop.
    let script = fs::read_to_string(
        repo_apps_dir()
            .join("animals")
            .join("static")
            .join("animals.js"),
    )
    .unwrap();
    let registered = alpine_components(&script);
    assert_eq!(
        registered,
        ["confirmable", "disclosure"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    for (name, html) in [
        ("play", &reloaded),
        ("teach", &teach),
        ("knowledge", &knowledge),
    ] {
        let used = x_data_values(html);
        assert!(
            used.is_subset(&registered),
            "{name}: x-data {used:?} not all registered {registered:?}"
        );
        let handlers: Vec<String> = a11y::check(html, Unit::Document)
            .into_iter()
            .filter(|f| f.starts_with("inline event handler"))
            .collect();
        assert!(handlers.is_empty(), "{name}: {handlers:?}");
    }
    assert!(!x_data_values(&knowledge).is_empty(), "{knowledge}");
    // Alpine's CDN builds call Alpine.start() in a microtask as soon as their script
    // runs, and start() dispatches `alpine:init` right then. A component registered from
    // an `alpine:init` listener in a script loaded *after* Alpine is registered too late
    // and every x-data is an "Undefined variable", as seen in a browser. So animals.js
    // has to come before alpine-csp.min.js, and both are `defer`, which keeps that order.
    for (name, html) in [
        ("play", &reloaded),
        ("teach", &teach),
        ("knowledge", &knowledge),
    ] {
        let components = html
            .find("/static/animals.js")
            .unwrap_or_else(|| panic!("{name}: animals.js not loaded"));
        let alpine = html
            .find("/static/alpine-csp.min.js")
            .unwrap_or_else(|| panic!("{name}: alpine not loaded"));
        assert!(
            components < alpine,
            "{name}: animals.js must be loaded before Alpine, or alpine:init fires first"
        );
        let tree = a11y::parse(html);
        for script in tree.find_all("script") {
            assert_eq!(
                script.attr("defer"),
                Some(""),
                "{name}: every script is defer, so document order is execution order: {}",
                script.describe()
            );
        }
    }
    assert!(
        script.contains("alpine:init"),
        "animals.js registers its components on alpine:init"
    );

    // The app's own assets, beneath its mount.
    let alpine = handler
        .handle(get("/a/animals/static/alpine-csp.min.js"))
        .await;
    assert_eq!(alpine.status(), StatusCode::OK);
    assert!(header(&alpine, &CONTENT_TYPE).contains("javascript"));
    // With JavaScript off, what Alpine hides is shown and its toggles are dropped, by a
    // media query in the one stylesheet: a <noscript> link would arrive live in a page
    // swapped in with scripting on.
    let sheet = handler.handle(get("/a/animals/static/animals.css")).await;
    assert_eq!(sheet.status(), StatusCode::OK);
    assert!(header(&sheet, &CONTENT_TYPE).starts_with("text/css"));
    let sheet = body_of(sheet).await;
    let nojs = sheet
        .split("@media (scripting: none) {")
        .nth(1)
        .unwrap_or_else(|| panic!("{sheet}"));
    assert!(
        nojs.contains("[x-cloak] { display: revert !important; }"),
        "{nojs}"
    );
    assert!(
        nojs.contains(".pv-js-only { display: none !important; }"),
        "{nojs}"
    );
    assert_eq!(
        handler
            .handle(get("/a/animals/static/nojs.css"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // Reset: a tombstone for every node and the cursor, then the starter tree planted
    // again, all in one batch; the log keeps every round played.
    let reset = handler
        .handle(posted(&handler, "/a/animals/reset", ""))
        .await;
    assert_eq!(reset.status(), StatusCode::SEE_OTHER);
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 26);
    let batch = &lines[13..];
    let ts: BTreeSet<&str> = batch.iter().map(|l| l["ts"].as_str().unwrap()).collect();
    assert_eq!(ts.len(), 1, "one batch: {batch:?}");
    let (tombstones, planted) = batch.split_at(10);
    assert!(
        tombstones.iter().all(|l| l["op"] == "del"),
        "{tombstones:?}"
    );
    let deleted: BTreeSet<String> = tombstones
        .iter()
        .filter(|l| l["tbl"] == "node")
        .map(|l| l["id"].as_str().unwrap().to_owned())
        .collect();
    let mut every_node: BTreeSet<String> = seed
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect();
    every_node.insert(dolphin_id);
    every_node.insert(shark_id);
    assert_eq!(deleted, every_node);
    assert_eq!(tombstones.last().unwrap()["tbl"], "cursor");
    assert_starter_tree(planted);
    let fresh = body_of(handler.handle(get("/a/animals/")).await).await;
    assert!(fresh.contains("<h1>Does it have legs?</h1>"), "{fresh}");
    assert!(fresh.contains("2 animals, 1 question<"), "{fresh}");
    assert_eq!(
        log_lines(&log).len(),
        26,
        "a tree already there is not planted twice"
    );
}

/// The three `put` events of the starter tree, in the order `lib/tree.lua` writes them:
/// the two leaves, then the root question pointing at their minted ids.
fn assert_starter_tree(events: &[Value]) {
    assert_eq!(events.len(), 3, "{events:?}");
    for event in events {
        assert_eq!(event["op"], "put", "{event}");
        assert_eq!(event["tbl"], "node", "{event}");
        assert!(is_ulid(event["id"].as_str().unwrap()), "{event}");
    }
    assert_eq!(events[0]["d"], json!({ "kind": "a", "text": "dog" }));
    assert_eq!(events[1]["d"], json!({ "kind": "a", "text": "fish" }));
    assert_eq!(events[2]["d"]["kind"], "q");
    assert_eq!(events[2]["d"]["text"], "Does it have legs?");
    assert_eq!(events[2]["d"]["yes_id"], events[0]["id"]);
    assert_eq!(events[2]["d"]["no_id"], events[1]["id"]);
}

/// Every write in animals as a plain form post with no
/// `HX-Request`: the first GET plants the starter tree, each write answers 303 to the
/// board and the next GET shows the state; the error branch renders the page with
/// `role="alert"`; every board form carries `method` and `action` beside `hx-post`; the
/// reset form is a real form with `csrf()`; the `<noscript>` sheet is linked and reveals
/// what Alpine hides.
#[tokio::test]
async fn test_animals_works_with_javascript_disabled() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let handler = handler_for(&root);
    let log = log_path(&handler, "animals");

    let plain = |path: &str, form: &str| posted(&handler, path, form);
    let redirected = |response: &Response| {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(header(response, &LOCATION), "/a/animals/");
    };
    let board = || async { body_of(handler.handle(get("/a/animals/")).await).await };

    // The first visit plants the starter tree: three puts, then a question on screen.
    assert!(board().await.contains("<h1>Does it have legs?</h1>"));
    assert_starter_tree(&log_lines(&log));
    let started = handler.handle(plain("/a/animals/start", "")).await;
    redirected(&started);
    assert!(board().await.contains("<h1>Does it have legs?</h1>"));
    let answered = handler
        .handle(plain("/a/animals/answer", "choice=yes"))
        .await;
    redirected(&answered);
    assert!(board().await.contains("<h1>Is it a dog?</h1>"));
    let taught = handler
        .handle(plain(
            "/a/animals/teach",
            "animal=penguin&question=Does+it+fly%3F&answer=yes",
        ))
        .await;
    redirected(&taught);
    assert!(
        board().await.contains("<h1>Does it have legs?</h1>"),
        "teaching ends the round at the root"
    );
    let answered = handler
        .handle(plain("/a/animals/answer", "choice=yes"))
        .await;
    redirected(&answered);
    assert!(board().await.contains("<h1>Does it fly?</h1>"));
    let answered = handler
        .handle(plain("/a/animals/answer", "choice=yes"))
        .await;
    redirected(&answered);
    assert!(board().await.contains("<h1>Is it a penguin?</h1>"));
    let answered = handler
        .handle(plain("/a/animals/answer", "choice=no"))
        .await;
    redirected(&answered);
    assert!(
        board().await.contains("<h1>Is it a penguin?</h1>"),
        "a leaf stays a leaf"
    );

    // Writes retain POST and CSRF; the read-only win page uses GET in both paths.
    let page = board().await;
    let forms = a11y::parse(&page).find_all("form").len();
    assert!(forms >= 1, "{page}");
    for form in a11y::parse(&page).find_all("form") {
        if form.attr("action") == Some("/a/animals/won") {
            assert_eq!(form.attr("method"), Some("get"));
            assert_eq!(form.attr("hx-get"), form.attr("action"));
            continue;
        }
        assert_eq!(form.attr("method"), Some("post"), "{page}");
        let action = form.attr("action").unwrap();
        assert!(action.starts_with("/a/animals/"), "{page}");
        assert_eq!(form.attr("hx-post"), Some(action), "{page}");
        assert!(
            form.descendants()
                .iter()
                .any(|e| e.name == "input" && e.attr("name") == Some("_csrf")),
            "{page}"
        );
    }
    assert!(
        page.contains("<link rel=\"stylesheet\" href=\"/a/animals/static/animals.css\""),
        "the stylesheet whose scripting: none block applies without JavaScript: {page}"
    );

    // What Alpine hides is reachable: the reset form is a real form with the token, the
    // toggles are marked JS-only, the hidden parts carry x-cloak for the stylesheet's
    // scripting: none block to revert.
    let knowledge = body_of(handler.handle(get("/a/animals/knowledge")).await).await;
    assert!(
        knowledge.contains("<form method=\"post\" action=\"/a/animals/reset\">"),
        "{knowledge}"
    );
    let tree = a11y::parse(&knowledge);
    let reset_form = tree
        .find_all("form")
        .into_iter()
        .find(|f| f.attr("action") == Some("/a/animals/reset"))
        .unwrap();
    assert!(
        reset_form
            .descendants()
            .iter()
            .any(|e| e.name == "input" && e.attr("name") == Some("_csrf")),
        "{knowledge}"
    );
    let animals = tree.find_all("tbody")[0].find_all("tr").len();
    assert_eq!(animals, 3, "dog, fish and penguin: {knowledge}");
    let toggles: Vec<&a11y::Element> = tree
        .descendants()
        .into_iter()
        .filter(|e| e.name == "button" && e.attr("x-on:click").is_some())
        .collect();
    assert_eq!(
        toggles.len(),
        animals + 2,
        "a show-path per animal, forget, keep: {knowledge}"
    );
    for toggle in toggles {
        assert!(toggle.has_class("pv-js-only"), "{}", toggle.describe());
    }
    let cloaked = tree
        .descendants()
        .into_iter()
        .filter(|e| e.attr("x-cloak").is_some())
        .count();
    assert_eq!(
        cloaked,
        animals + 1,
        "a path per animal and the confirmation: {knowledge}"
    );
    let teach = body_of(handler.handle(get("/a/animals/teach")).await).await;
    let tree = a11y::parse(&teach);
    let toggle = tree
        .descendants()
        .into_iter()
        .find(|e| e.name == "button" && e.attr("x-on:click") == Some("toggle"))
        .unwrap();
    assert!(toggle.has_class("pv-js-only"), "{teach}");
    assert!(teach.contains("/a/animals/static/animals.css"), "{teach}");
    let sheet = body_of(handler.handle(get("/a/animals/static/animals.css")).await).await;
    let nojs = sheet
        .split_once("@media (scripting: none) {")
        .and_then(|(_, block)| block.split_once("\n}"))
        .map(|(block, _)| block.to_owned())
        .unwrap_or_else(|| panic!("{sheet}"));
    let rules: BTreeMap<String, BTreeMap<String, String>> =
        a11y::rules(&nojs).into_iter().collect();
    assert!(
        rules["[x-cloak]"]["display"].starts_with("revert"),
        "{nojs}"
    );
    assert!(
        rules[".pv-js-only"]["display"].starts_with("none"),
        "{nojs}"
    );

    // Forget, as a plain post: back to the first question, the taught animal gone. Then
    // the error branch, a page with the alert in it: an answer outside the allowlist is
    // refused without a write.
    let reset = handler.handle(plain("/a/animals/reset", "")).await;
    redirected(&reset);
    assert!(board().await.contains("<h1>Does it have legs?</h1>"));
    let knowledge = body_of(handler.handle(get("/a/animals/knowledge")).await).await;
    assert!(!knowledge.contains("penguin"), "{knowledge}");
    assert!(knowledge.contains("<td>dog</td>"), "{knowledge}");
    let before = log_lines(&log).len();
    let refused = handler
        .handle(plain("/a/animals/answer", "choice=maybe"))
        .await;
    assert_eq!(refused.status(), StatusCode::OK);
    let text = body_of(refused).await;
    assert!(text.starts_with("<!doctype html>"), "{text}");
    assert!(text.contains("role=\"alert\""), "{text}");
    assert!(text.contains("Answer yes or no."), "{text}");
    assert!(text.contains("<h1>Does it have legs?</h1>"), "{text}");
    assert_eq!(log_lines(&log).len(), before, "nothing written");
}

// ---------------------------------------------------------------------------------------
// sketch
// ---------------------------------------------------------------------------------------

/// `apps/sketch` — exactly the calls `app.js` makes, against the log: the page, every
/// file it loads and `pv.js`; the boot read of `/api/events?tbl=stroke`; the stream; a
/// `pv.put` whose frame is the log line; a clear as a `del` batch; the boot read again,
/// which applies the put and then the del; a line appended by hand, which the next read
/// returns and the stream reports as a resync; `/api/node`. And the page itself: no
/// inline script or style, a zoomable viewport, a labelled canvas.
#[tokio::test]
async fn test_sketch_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = handler_for(&root);
    handler.api_mut().set_ping(Duration::from_millis(200));
    let dev = node_id(&handler);
    let log = log_path(&handler, "sketch");
    let web = repo_apps_dir().join("sketch").join("web");

    let index = handler.handle(get("/a/sketch/")).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert!(header(&index, &CONTENT_TYPE).starts_with("text/html"));
    let index = body_of(index).await;
    // Sketch declines the chrome (`[ui] chrome = "none"`), so the document is the file,
    // and the Privatium mark in its rail is the link out instead.
    assert_eq!(index, fs::read_to_string(web.join("index.html")).unwrap());
    assert!(!index.contains("pv-header"), "{index}");
    assert!(
        index.contains("<a id=\"mark\" class=\"mark\">"),
        "the mark is a link: {index}"
    );
    assert!(index.contains("<html lang=\"en\">"), "{index}");
    assert!(index.contains("<canvas id=\"pad\""), "{index}");
    assert!(
        index.contains("aria-label=\"Drawing sheet"),
        "the sheet carries its own name"
    );
    assert!(!index.contains("user-scalable=no"), "{index}");
    assert!(index.contains("<meta name=\"viewport\""), "{index}");
    assert!(
        index.contains("<script type=\"module\" src=\"app.js\">"),
        "{index}"
    );
    assert_clean("sketch index.html", &index, Unit::Document);
    for (file, kind) in [
        ("app.js", "javascript"),
        ("sheet.js", "javascript"),
        ("strokes.js", "javascript"),
        ("paint.js", "javascript"),
        ("clip.js", "javascript"),
        ("tools.js", "javascript"),
        ("history.js", "javascript"),
        ("exit.js", "javascript"),
        ("style.css", "text/css"),
        ("mark.svg", "svg"),
        ("mark-light.svg", "svg"),
    ] {
        let response = handler.handle(get(&format!("/a/sketch/{file}"))).await;
        assert_eq!(response.status(), StatusCode::OK, "{file}");
        assert!(header(&response, &CONTENT_TYPE).contains(kind), "{file}");
        assert_eq!(
            body_of(response).await,
            fs::read_to_string(web.join(file)).unwrap(),
            "{file}"
        );
    }
    let pv = handler.handle(get("/static/pv.js")).await;
    assert_eq!(pv.status(), StatusCode::OK);
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    for call in [
        "pv.events({ tbl: 'stroke' })",
        "pv.append(",
        "pv.subscribe(",
        "pv.on('resync'",
        "pv.ulid()",
    ] {
        assert!(app_js.contains(call), "app.js no longer calls {call}");
    }
    // The sheet is a fixed 1600 x 1200 coordinate space; the canvas's own pixel size
    // still follows its CSS box at the device ratio, which is where sheet.js earns its
    // place as the Tier 2 worked example.
    let sheet_js = fs::read_to_string(web.join("sheet.js")).unwrap();
    assert!(
        sheet_js.contains("clientWidth") && sheet_js.contains("devicePixelRatio"),
        "the backing store follows the CSS size"
    );
    assert!(
        !sheet_js.contains("window.innerWidth") && !app_js.contains("window.innerWidth"),
        "the sheet is never sized from the viewport"
    );
    assert!(
        app_js.contains("aria-pressed"),
        "the current colour is announced"
    );
    // A batch over `api.max_batch` is refused whole (spec/data-api.md §3), so clearing a
    // full sheet and pasting a large selection are written in ceiling-sized chunks.
    let history_js = fs::read_to_string(web.join("history.js")).unwrap();
    assert!(
        history_js.contains("export const MAX_BATCH = 1000;"),
        "the batch ceiling no longer matches spec/data-api.md §3"
    );
    assert!(
        app_js.contains("batches(groups)"),
        "app.js no longer chunks a write at the ceiling"
    );
    // Each quick tool is a split button: the tool, and a caret that opens its options.
    // The caret is a control of its own, so the menu never depends on a long press
    // (`docs/sketch-app-design.md`, and the accessibility skill's rule on gestures).
    assert_eq!(
        index.matches("class=\"quick-wrap\"").count(),
        4,
        "the four quick tools are split buttons"
    );
    assert_eq!(
        index.matches("class=\"more\" data-more=").count(),
        4,
        "each carries its own caret"
    );
    assert_eq!(
        index
            .matches("aria-haspopup=\"menu\" aria-expanded=\"false\"")
            .count(),
        4,
        "and announces the menu it opens"
    );
    assert!(
        index.contains("<div id=\"quick-menu\" role=\"menu\" hidden>"),
        "{index}"
    );
    // <body> carries data-tool for the CSS, so a delegated click handler has to name the
    // control itself or every click on the sheet re-chooses the current tool.
    assert!(
        app_js.contains("closest('button[data-tool]"),
        "the tool-choosing click handler is not scoped to a button"
    );
    // A tab's own write comes back on the stream; counting it twice leaves the second
    // undo refusing, because the next entry then describes a mark that is already gone.
    assert!(
        history_js.contains("this.inflight"),
        "history.js no longer recognises the echo of its own write"
    );

    // Every glyph is a vendored Bootstrap Icon, inlined into the page's sprite.
    let index_html = &index;
    let icons = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons");
    for (symbol, icon) in [
        ("i-select", "cursor"),
        ("i-brush", "brush"),
        ("i-eraser", "eraser"),
        ("i-pick", "eyedropper"),
        ("i-fill", "paint-bucket"),
        ("i-undo", "arrow-90deg-left"),
        ("i-redo", "arrow-90deg-right"),
        ("i-apps", "grid-3x3-gap"),
        ("i-delete", "trash"),
        ("i-caret", "caret-down-fill"),
    ] {
        let source = fs::read_to_string(icons.join(format!("{icon}.svg"))).unwrap();
        let body: String = source
            .split_once('>')
            .unwrap()
            .1
            .rsplit_once("</svg>")
            .unwrap()
            .0
            .lines()
            .map(str::trim)
            .collect();
        assert!(
            index_html.contains(&format!(
                "<symbol id=\"{symbol}\" viewBox=\"0 0 16 16\">{body}</symbol>"
            )),
            "sprite symbol {symbol} is not the vendored {icon}.svg"
        );
    }

    // Boot on an empty log.
    let boot = body_of(handler.handle(get("/a/sketch/api/events?tbl=stroke")).await).await;
    assert_eq!(boot, "");

    // The stream, then a stroke: `pv.put('stroke', id, drawing)`.
    let mut stream = handler
        .handle(get("/a/sketch/api/stream"))
        .await
        .into_body();
    let id = ulid();
    let drawing = json!({ "points": [[10, 10], [40, 90]], "color": "#00274C", "width": 3 });
    let (status, body) = json_of(
        handler
            .handle(post_json(
                "/a/sketch/api/events",
                &json!({ "events": [{ "op": "put", "tbl": "stroke", "id": id, "d": drawing }] }),
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ids"][0], id);
    assert_eq!(body["lam"], 1);
    let (event, data) = next_frame(&mut stream, 5).await.unwrap();
    assert_eq!(event, "append");
    let raw = fs::read_to_string(&log).unwrap();
    assert_eq!(
        data,
        raw.lines().next().unwrap(),
        "the frame is the log line"
    );
    let lines = log_lines(&log);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["seq"], 1);
    assert_eq!(lines[0]["lam"], 1);
    assert_eq!(lines[0]["dev"], dev);
    assert_eq!(lines[0]["app"], "sketch");
    assert_eq!(lines[0]["tbl"], "stroke");
    assert_eq!(
        lines[0]["d"], drawing,
        "stored as given: no schema, no validation"
    );

    // Clear: one batch of dels.
    let (status, _) = json_of(
        handler
            .handle(post_json(
                "/a/sketch/api/events",
                &json!({ "events": [{ "op": "del", "tbl": "stroke", "id": id }] }),
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (event, data) = next_frame(&mut stream, 5).await.unwrap();
    assert_eq!(event, "append");
    let del: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(del["op"], "del");
    assert_eq!(del["id"], id);
    assert!(del.get("d").is_none(), "{del}");

    // Boot again: the log in order, a put then a del, so nothing is drawn.
    let boot = body_of(handler.handle(get("/a/sketch/api/events?tbl=stroke")).await).await;
    let mut strokes: BTreeMap<String, Value> = BTreeMap::new();
    for line in boot.lines() {
        let ev: Value = serde_json::from_str(line).unwrap();
        let ev_id = ev["id"].as_str().unwrap().to_owned();
        if ev["op"] == "del" {
            strokes.remove(&ev_id);
        } else {
            strokes.insert(ev_id, ev["d"].clone());
        }
    }
    assert_eq!(boot.lines().count(), 2, "{boot}");
    assert!(strokes.is_empty());

    // A line appended by hand: the next boot read returns it, and the stream says resync.
    let by_hand = ulid();
    let ts = privatium_core::log::format_ts(jiff::Timestamp::now());
    let line = format!(
        r##"{{"seq":3,"lam":3,"ts":"{ts}","dev":"{dev}","app":"sketch","op":"put","tbl":"stroke","id":"{by_hand}","d":{{"points":[[0,0],[5,5]],"color":"#FFCB05","width":2}}}}"##
    );
    hand_append(&log, &line, "\n");
    let boot = body_of(handler.handle(get("/a/sketch/api/events?tbl=stroke")).await).await;
    assert_eq!(boot.lines().count(), 3, "{boot}");
    assert_eq!(boot.lines().last().unwrap(), line);
    let mut seen = Vec::new();
    for _ in 0..6 {
        match next_frame(&mut stream, 5).await {
            Some(frame) => {
                let stop = frame.0 == "resync";
                seen.push(frame);
                if stop {
                    break;
                }
            }
            None => break,
        }
    }
    assert!(
        seen.iter()
            .any(|(event, data)| event == "resync" && data.contains("\"lam\":3")),
        "{seen:?}"
    );

    // `pv.node()`.
    let (status, node) = json_of(handler.handle(get("/a/sketch/api/node")).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(node["id"], dev, "{node}");
}

// ---------------------------------------------------------------------------------------
// pantry
// ---------------------------------------------------------------------------------------

/// A node with the reference apps, `pantry` seeded from `sample/seed.jsonl`.
fn pantry(root: &tempfile::TempDir) -> Handler {
    let (mut node, report) = open(root);
    node.load_seed("pantry").unwrap();
    Handler::new(node, report)
}

/// One view, through `/api/q`, as `pv.query` reads it.
async fn view(handler: &mut Handler, path: &str) -> Vec<Value> {
    let (status, body) = json_of(handler.handle(get(path)).await).await;
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    body["rows"].as_array().cloned().unwrap_or_default()
}

/// One batch of events, through `/api/events`, as `pv.append` writes it.
async fn pantry_append(handler: &mut Handler, events: Value) -> (StatusCode, Value) {
    json_of(
        handler
            .handle(post_json(
                "/a/pantry/api/events",
                &json!({ "events": events }),
            ))
            .await,
    )
    .await
}

/// `sql` without its `--` comments, so a rule about the SQL is not answered by the prose
/// that explains the rule.
fn sql_only(sql: &str) -> String {
    sql.lines()
        .map(|line| line.split("--").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// A day, in UTC, as `date('now')` reads it — the clock the views compare against.
fn utc_day(offset: i64) -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .checked_add(jiff::Span::new().days(offset))
        .unwrap()
        .to_string()
}

/// A batch on `shelf`, with the stock it arrived with, in one batch of the log — exactly
/// what `writes.js` sends. Returns the batch's id.
async fn stock(handler: &mut Handler, shelf: &str, name: &str, amount: &str) -> String {
    let id = ulid();
    let (status, body) = pantry_append(
        handler,
        json!([
            { "op": "put", "tbl": "batch", "id": id, "d": {
                "name": name, "icon": "snow", "unit": "bags", "shelf_id": shelf,
                "stored_on": utc_day(0), "expires_on": null } },
            { "op": "put", "tbl": "quantity_change", "id": ulid(), "d": {
                "batch_id": id, "amount": amount, "reason": "stocked", "of_id": null,
                "at": "2026-09-07T10:00:00Z" } },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

/// One recorded change against `batch`. Returns its id.
async fn change(
    handler: &mut Handler,
    batch: &str,
    amount: &str,
    reason: &str,
    of_id: Option<&str>,
) -> String {
    let id = ulid();
    let (status, body) = pantry_append(
        handler,
        json!([{ "op": "put", "tbl": "quantity_change", "id": id, "d": {
            "batch_id": batch, "amount": amount, "reason": reason, "of_id": of_id,
            "at": "2026-09-07T11:00:00Z" } }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

/// What `v_batch` says one batch's balance is, or `None` when it is not on the shelf.
async fn balance_on(handler: &mut Handler, shelf: &str, id: &str) -> Option<String> {
    view(handler, &format!("/a/pantry/api/q/v_batch?shelf={shelf}"))
        .await
        .iter()
        .find(|row| row["id"] == id)
        .map(|row| row["balance"].as_str().unwrap().to_owned())
}

/// The id of the seeded shelf whose name starts with `prefix`.
async fn seeded_shelf(handler: &mut Handler, prefix: &str) -> String {
    view(handler, "/a/pantry/api/q/v_shelf")
        .await
        .into_iter()
        .find(|row| row["name"].as_str().unwrap_or_default().starts_with(prefix))
        .unwrap_or_else(|| panic!("no seeded shelf named {prefix}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// `apps/pantry/README.md` — the page and every file it loads; the seed through the views;
/// `/api/schema` naming the three tables and the seven views with their placeholders; a
/// batch added as one batch of the log; a withdrawal, a return and an undo, each of them
/// one event; and the balance the views report after every one of them.
#[tokio::test]
async fn test_pantry_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let web = repo_apps_dir().join("pantry").join("web");

    let index = handler.handle(get("/a/pantry/")).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert!(header(&index, &CONTENT_TYPE).starts_with("text/html"));
    let index = body_of(index).await;
    // Pantry wears the standard chrome (`spec/app-contract.md §5`): the served document
    // is the file with the bar after its <body> and the footer before its </body>, the
    // app's own title in the bar's centre, and its main region the skip link's target.
    let file = fs::read_to_string(web.join("index.html")).unwrap();
    assert_ne!(index, file);
    assert!(
        index.contains("<body>\n<a class=\"pv-skip\" href=\"#main\">"),
        "{index}"
    );
    // The file's header comment names the three tags; the chrome goes at the real ones,
    // after the comment, where a browser renders it, and the file's comment is unchanged.
    let (comment, document) = index.split_once("-->").unwrap();
    assert!(!comment.contains("pv-header"), "{comment}");
    assert_eq!(
        format!("{comment}-->"),
        file.split_once("-->")
            .map(|(c, _)| format!("{c}-->"))
            .unwrap()
    );
    let title = document.find("<title>").unwrap();
    let sheet = document
        .find(&privatium_core::http::assets::versioned("chrome.css"))
        .unwrap();
    let head_end = document.find("</head>").unwrap();
    assert!(title < sheet && sheet < head_end, "{document}");
    assert!(
        document.find("<header class=\"pv-header\">").unwrap() > head_end,
        "{document}"
    );
    assert!(
        index.contains("<p class=\"pv-app-title\"><a href=\"/a/pantry/\">"),
        "{index}"
    );
    assert!(index.contains("<span>Pantry</span></a></p>"), "{index}");
    assert!(index.contains("<main id=\"main\">"), "{index}");
    assert!(
        index.contains("<p id=\"pv-status\" class=\"pv-status\" role=\"status\"></p>"),
        "{index}"
    );
    assert!(
        file.contains("</head>") && file.contains("</body>"),
        "{file}"
    );
    assert!(index.contains("<html lang=\"en\">"), "{index}");
    assert!(!index.contains("user-scalable=no"), "{index}");
    assert!(
        index.contains("<script type=\"module\" src=\"app.js\">"),
        "{index}"
    );
    assert_eq!(index.matches("<h1").count(), 1, "{index}");
    assert_clean("pantry index.html", &index, Unit::Document);
    let app_js = fs::read_to_string(web.join("app.js")).unwrap();
    // The work and the rail are separate stacks. Laying both out as rows of one grid ties
    // each heading on the right to the bottom of a box on the left, which is what made the
    // two halves of this page drift out of line.
    assert_eq!(index.matches("class=\"work\"").count(), 1, "{index}");
    assert_eq!(index.matches("class=\"rail\"").count(), 1, "{index}");
    // A batch has to go on a shelf, so the batch half is not served until there is one:
    // an empty batch table over an empty shelf list is a dead end, and its Shelf field
    // would have nothing to offer.
    assert!(
        index.contains(
            "<section class=\"pane\" id=\"batches\" aria-labelledby=\"h-batches\" hidden>"
        ),
        "{index}"
    );
    assert!(
        app_js.contains("$('batches').toggleAttribute('hidden', bare)"),
        "app.js no longer hides the batch half until a shelf exists"
    );
    // The bar is the way back and the footer's slot is the status line, so the app draws
    // neither of its own (`PV408`) and says what happened through `pv.status`.
    assert!(!index.contains("class=\"exit\""), "{index}");
    assert!(!index.contains("id=\"status\""), "{index}");
    assert!(
        app_js.contains("pv.status(message)"),
        "app.js no longer writes its status through the chrome's slot"
    );
    assert!(
        !app_js.contains("'../../'"),
        "app.js draws a second way back"
    );

    for (file, kind) in [
        ("app.js", "javascript"),
        ("views.js", "javascript"),
        ("forms.js", "javascript"),
        ("writes.js", "javascript"),
        ("style.css", "text/css"),
    ] {
        let response = handler.handle(get(&format!("/a/pantry/{file}"))).await;
        assert_eq!(response.status(), StatusCode::OK, "{file}");
        assert!(header(&response, &CONTENT_TYPE).contains(kind), "{file}");
        assert_eq!(
            body_of(response).await,
            fs::read_to_string(web.join(file)).unwrap(),
            "{file}"
        );
    }
    // The two readings this app is built to show: the views say what is true now, the log
    // says what happened.
    for call in [
        "pv.query('v_shelf')",
        "pv.query('v_batch', { shelf: state.open })",
        "pv.events({ tbl, limit: page, offset })",
        "pv.subscribe(onStream)",
        "pv.on('resync'",
    ] {
        assert!(app_js.contains(call), "app.js no longer calls {call}");
    }
    let writes_js = fs::read_to_string(web.join("writes.js")).unwrap();
    assert_eq!(
        writes_js.matches("pv.append(").count(),
        6,
        "one append per flow, and every flow in one file"
    );
    assert!(
        writes_js.contains("pv.ulid()"),
        "ids are minted client-side"
    );

    // `/api/schema`: what a client that has never seen this app would read.
    let (status, schema) = json_of(handler.handle(get("/a/pantry/api/schema")).await).await;
    assert_eq!(status, StatusCode::OK);
    let tables: Vec<&str> = schema["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tables, vec!["batch", "quantity_change", "shelf"]);
    let views: BTreeMap<String, Value> = schema["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["name"].as_str().unwrap().to_owned(), v["params"].clone()))
        .collect();
    assert_eq!(
        views.keys().cloned().collect::<Vec<_>>(),
        vec![
            "v_batch",
            "v_check_batch",
            "v_check_out",
            "v_expiring",
            "v_out",
            "v_shelf",
            "v_stock_by_unit"
        ]
    );
    assert_eq!(views["v_batch"], json!(["shelf"]));
    assert_eq!(views["v_out"], json!(["since"]));
    assert_eq!(views["v_expiring"], json!(["days"]));
    assert!(
        schema["tables"].as_array().unwrap().iter().all(|table| {
            table["columns"]
                .as_array()
                .unwrap()
                .iter()
                .all(|column| column["name"] != "balance")
        }),
        "a balance is never a column: {schema}"
    );

    // The seed, through the views.
    let shelves = view(&mut handler, "/a/pantry/api/q/v_shelf").await;
    assert_eq!(shelves.len(), 4, "{shelves:?}");
    assert_eq!(shelves[0]["name"], "Freezer drawer 1");
    assert_eq!(shelves[0]["batches"], 2);
    let baking = shelves
        .iter()
        .find(|row| row["name"] == "Baking shelf")
        .unwrap();
    assert_eq!(
        baking["batches"], 0,
        "the flour reached zero and left the shelf, and its history stayed"
    );

    let drawer = shelves[0]["id"].as_str().unwrap().to_owned();
    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={drawer}"),
    )
    .await;
    let balances: BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap(),
                row["balance"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        balances,
        BTreeMap::from([("Chicken soup", "5.000"), ("Waffles", "8.000")]),
        "6 - 2 + 1 stocked, and 12 - 4, added up from the log"
    );

    let out = view(
        &mut handler,
        "/a/pantry/api/q/v_out?since=2026-01-01T00:00:00Z",
    )
    .await;
    let soup = out
        .iter()
        .find(|row| row["batch_name"] == "Chicken soup")
        .unwrap();
    assert_eq!(soup["taken"], "2.000");
    assert_eq!(soup["returned"], "1.000");
    assert_eq!(soup["still_out"], "1.000");
    let stew = out
        .iter()
        .find(|row| row["batch_name"] == "Beef stew")
        .unwrap();
    assert_eq!(
        stew["still_out"], "0.000",
        "fully returned, so out of the tray"
    );

    let summary = view(&mut handler, "/a/pantry/api/q/v_stock_by_unit").await;
    let by_unit: BTreeMap<&str, &str> = summary
        .iter()
        .map(|row| {
            (
                row["unit"].as_str().unwrap(),
                row["balance"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        by_unit,
        BTreeMap::from([
            ("bags", "3.000"),
            ("containers", "9.000"),
            ("kg", "2.500"),
            ("pieces", "8.000"),
        ])
    );
    assert!(
        view(&mut handler, "/a/pantry/api/q/v_check_batch")
            .await
            .is_empty(),
        "nothing to check in a seeded app"
    );

    // Adding a batch is two events and one batch of the log: a batch never exists with no
    // stock, because decimal_sum over no rows is NULL, not zero.
    let log = log_path(&handler, "pantry");
    let before = log_lines(&log).len();
    let id = stock(&mut handler, &drawer, "Green beans", "4").await;
    let lines = log_lines(&log);
    assert_eq!(lines.len(), before + 2);
    let (first, second) = (&lines[before], &lines[before + 1]);
    assert_eq!(first["ts"], second["ts"], "one batch, one ts");
    assert_eq!(
        second["seq"].as_u64().unwrap(),
        first["seq"].as_u64().unwrap() + 1
    );
    assert_eq!(first["tbl"], "batch");
    assert_eq!(second["tbl"], "quantity_change");
    assert_eq!(second["d"]["amount"], "4.000", "at the column's scale");

    // Take some out, put some back, and read the balance after each.
    let balance_of = |rows: &[Value], id: &str| -> Option<String> {
        rows.iter()
            .find(|row| row["id"] == id)
            .map(|row| row["balance"].as_str().unwrap().to_owned())
    };
    let taken = change(&mut handler, &id, "-1.5", "taken", None).await;
    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={drawer}"),
    )
    .await;
    assert_eq!(balance_of(&rows, &id).as_deref(), Some("2.500"));

    change(&mut handler, &id, "0.5", "returned", Some(&taken)).await;
    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={drawer}"),
    )
    .await;
    assert_eq!(balance_of(&rows, &id).as_deref(), Some("3.000"));
    let out = view(
        &mut handler,
        "/a/pantry/api/q/v_out?since=2026-01-01T00:00:00Z",
    )
    .await;
    let card = out.iter().find(|row| row["id"] == taken.as_str()).unwrap();
    assert_eq!(card["taken"], "1.500");
    assert_eq!(card["returned"], "0.500");
    assert_eq!(card["still_out"], "1.000");

    // Undo the withdrawal: the row is gone from the tables and both lines are in the log.
    let (status, body) = pantry_append(
        &mut handler,
        json!([{ "op": "del", "tbl": "quantity_change", "id": taken }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={drawer}"),
    )
    .await;
    assert_eq!(
        balance_of(&rows, &id).as_deref(),
        Some("4.500"),
        "the withdrawal is gone; the return is not"
    );
    let events = body_of(
        handler
            .handle(get(&format!(
                "/a/pantry/api/events?tbl=quantity_change&id={taken}"
            )))
            .await,
    )
    .await;
    let ops: Vec<String> = events
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(ops, vec!["put", "del"], "the log kept both");
}

/// `spec/app-contract.md §4.5`, `spec/data-dictionary.md §2.1` — a balance is nowhere in
/// the schema. What `v_batch` reports is `decimal_sum` over the batch's own changes, and
/// it equals the sum of the amounts the log holds, added up here in thousandths.
#[tokio::test]
async fn test_pantry_balance_is_never_stored() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let declarations =
        sql_only(&fs::read_to_string(repo_apps_dir().join("pantry").join("schema.sql")).unwrap());
    let tables = declarations
        .split("CREATE VIEW")
        .next()
        .expect("the tables come first");
    assert!(
        !tables.contains("balance"),
        "no table declares a balance: {tables}"
    );

    let shelf = seeded_shelf(&mut handler, "Freezer drawer 1").await;
    let id = stock(&mut handler, &shelf, "Stock parcels", "7.25").await;
    change(&mut handler, &id, "-2.125", "taken", None).await;
    change(&mut handler, &id, "-0.125", "taken", None).await;

    // The log, added up in thousandths — the same arithmetic decimal_sum does exactly.
    let thousandths = |amount: &str| -> i64 {
        let (whole, fraction) = amount.split_once('.').unwrap_or((amount, "0"));
        let sign = if amount.starts_with('-') { -1 } else { 1 };
        let fraction = format!("{fraction:0<3}");
        whole.parse::<i64>().unwrap() * 1000 + sign * fraction[..3].parse::<i64>().unwrap()
    };
    let total: i64 = log_lines(&log_path(&handler, "pantry"))
        .iter()
        .filter(|line| line["tbl"] == "quantity_change" && line["d"]["batch_id"] == id.as_str())
        .map(|line| thousandths(line["d"]["amount"].as_str().unwrap()))
        .sum();
    assert_eq!(total, 5000);

    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={shelf}"),
    )
    .await;
    let balance = rows.iter().find(|row| row["id"] == id.as_str()).unwrap()["balance"]
        .as_str()
        .unwrap();
    assert_eq!(balance, "5.000");
    assert_eq!(thousandths(balance), total);
}

/// `spec/data-dictionary.md §2.1` — a DECIMAL keeps the scale its column declares, whoever
/// wrote it: a JSON number through the API, a string through the API, and the seed's own
/// `"2.5"`. Every one of them arrives back as a string, because a JSON number is a double.
#[tokio::test]
async fn test_pantry_decimal_scale_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Pantry shelf").await;

    let id = stock(&mut handler, &shelf, "Typed as text", "12.5").await;
    let numeric = ulid();
    let (status, body) = pantry_append(
        &mut handler,
        json!([{ "op": "put", "tbl": "quantity_change", "id": numeric, "d": {
            "batch_id": id, "amount": 0.5, "reason": "returned", "of_id": numeric,
            "at": "2026-09-07T12:00:00Z" } }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let stored: Vec<String> = log_lines(&log_path(&handler, "pantry"))
        .iter()
        .filter(|line| line["tbl"] == "quantity_change" && line["d"]["batch_id"] == id.as_str())
        .map(|line| line["d"]["amount"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(stored, vec!["12.500".to_owned(), "0.500".to_owned()]);

    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={shelf}"),
    )
    .await;
    let batch = rows.iter().find(|row| row["id"] == id.as_str()).unwrap();
    assert_eq!(batch["balance"], "13.000");
    assert!(batch["balance"].is_string(), "a decimal is never a number");
    let rice = rows
        .iter()
        .find(|row| row["name"] == "Basmati rice")
        .unwrap();
    assert_eq!(
        rice["balance"], "2.500",
        "the seed's own scale, from \"2.5\""
    );
}

/// `spec/data-api.md §2` — the author's own `CHECK` refuses a reason the app does not have,
/// the refusal names the event's `index`, and nothing of the batch is appended.
#[tokio::test]
async fn test_pantry_schema_rejects_bad_reason() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Freezer drawer 2").await;
    let id = stock(&mut handler, &shelf, "Sorbet", "2").await;
    let log = log_path(&handler, "pantry");
    let before = log_lines(&log).len();

    let (status, body) = pantry_append(
        &mut handler,
        json!([
            { "op": "put", "tbl": "quantity_change", "id": ulid(), "d": {
                "batch_id": id, "amount": "-1", "reason": "taken", "of_id": null,
                "at": "2026-09-07T12:00:00Z" } },
            { "op": "put", "tbl": "quantity_change", "id": ulid(), "d": {
                "batch_id": id, "amount": "-1", "reason": "eaten", "of_id": null,
                "at": "2026-09-07T12:00:01Z" } },
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["index"], 1, "{body}");
    assert_eq!(
        log_lines(&log).len(),
        before,
        "all of a batch or none of it"
    );

    // A return with no withdrawal to point at is refused by the second CHECK.
    let (status, body) = pantry_append(
        &mut handler,
        json!([{ "op": "put", "tbl": "quantity_change", "id": ulid(), "d": {
            "batch_id": id, "amount": "1", "reason": "returned", "of_id": null,
            "at": "2026-09-07T12:00:02Z" } }]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["index"], 0, "{body}");
    assert_eq!(log_lines(&log).len(), before);
}

/// `spec/app-contract.md §9` — a seed populates an empty app or nothing. The second load
/// is refused, and it is refused after any event of any device.
#[test]
fn test_pantry_seed_loads_into_empty_app_only() {
    let root = tempfile::tempdir().unwrap();
    let (mut node, _report) = open(&root);
    let seeded = node.load_seed("pantry").unwrap();
    assert_eq!(
        seeded.events, 22,
        "four shelves, six batches, twelve changes"
    );
    assert!(
        matches!(node.load_seed("pantry"), Err(Error::SeedRefused { .. })),
        "a seed populates an empty app or nothing"
    );
}

/// `spec/protocol.md §4.5` — two devices move one batch to different shelves. The rows
/// have one id, so one row survives, and it is the one that ranks last by `(lam, ts, dev)`.
/// `§4.6` — the surviving row keeps the id, so every change still points at a batch.
#[tokio::test]
async fn test_spec_4_5_pantry_move_is_last_write_wins() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let drawer = seeded_shelf(&mut handler, "Freezer drawer 1").await;
    let rack = seeded_shelf(&mut handler, "Pantry shelf").await;
    let id = stock(&mut handler, &drawer, "Pastry", "3").await;

    // Another device moved the same batch to the pantry shelf, later in the order §4.5
    // ranks by, and its own log file is what carries it.
    let lam = log_lines(&log_path(&handler, "pantry")).last().unwrap()["lam"]
        .as_u64()
        .unwrap()
        + 10;
    let elsewhere = handler
        .node()
        .lock()
        .unwrap()
        .paths()
        .app_log_dir("pantry")
        .join("zzzzzzzz.jsonl");
    // Its rank is higher because its `lam` is; a line stamped in the future would be kept
    // in the log and left out of the tables (`spec/protocol.md §4.4`), which is a different
    // rule from this one.
    let ts = privatium_core::log::format_ts(jiff::Timestamp::now());
    let stored_on = utc_day(0);
    let line = format!(
        r##"{{"seq":1,"lam":{lam},"ts":"{ts}","dev":"zzzzzzzz","app":"pantry","op":"put","tbl":"batch","id":"{id}","d":{{"name":"Pastry","icon":"snow","unit":"bags","shelf_id":"{rack}","stored_on":"{stored_on}","expires_on":null}}}}"##
    );
    hand_append(&elsewhere, &line, "\n");

    let here = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={drawer}"),
    )
    .await;
    assert!(
        !here.iter().any(|row| row["id"] == id.as_str()),
        "the batch is not on two shelves: {here:?}"
    );
    let there = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={rack}"),
    )
    .await;
    let moved: Vec<&Value> = there
        .iter()
        .filter(|row| row["id"] == id.as_str())
        .collect();
    assert_eq!(moved.len(), 1, "one row, not two: {there:?}");
    assert_eq!(
        moved[0]["balance"], "3.000",
        "§4.6: the id survived the move, so the change still points at the batch"
    );
}

/// `spec/protocol.md §4.6` — a move is a put on the live id, never a tombstone and a new
/// one: the schema's own `batch_id` values all resolve after it, and the app's flows write
/// no such pair.
#[tokio::test]
async fn test_spec_4_6_pantry_move_does_not_orphan_changes() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let drawer = seeded_shelf(&mut handler, "Freezer drawer 1").await;
    let rack = seeded_shelf(&mut handler, "Baking shelf").await;
    let id = stock(&mut handler, &drawer, "Puff pastry", "6").await;
    change(&mut handler, &id, "-2", "taken", None).await;

    let (status, body) = pantry_append(
        &mut handler,
        json!([{ "op": "put", "tbl": "batch", "id": id, "d": {
            "name": "Puff pastry", "icon": "snow", "unit": "bags", "shelf_id": rack,
            "stored_on": utc_day(0), "expires_on": null } }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={rack}"),
    )
    .await;
    let moved = rows.iter().find(|row| row["id"] == id.as_str()).unwrap();
    assert_eq!(moved["balance"], "4.000", "6 - 2, kept across the move");

    // Every batch a change names is a batch that is there.
    let batches: BTreeSet<String> = log_lines(&log_path(&handler, "pantry"))
        .iter()
        .filter(|line| line["tbl"] == "batch")
        .map(|line| line["id"].as_str().unwrap().to_owned())
        .collect();
    for line in log_lines(&log_path(&handler, "pantry"))
        .iter()
        .filter(|line| line["tbl"] == "quantity_change")
    {
        let names = line["d"]["batch_id"].as_str().unwrap();
        assert!(batches.contains(names), "{names} names no batch");
    }
    assert!(
        !fs::read_to_string(repo_apps_dir().join("pantry").join("web").join("writes.js"))
            .unwrap()
            .contains("'del', tbl: 'batch'"),
        "a move never tombstones the batch"
    );
}

/// `spec/protocol.md §4.5` — two devices undo the same change. Both write the tombstone,
/// the group's last event is a tombstone either way, and undoing twice leaves what undoing
/// once leaves. No counter and no guard anywhere in the app.
#[tokio::test]
async fn test_spec_4_5_pantry_double_undo_is_one_undo() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Freezer drawer 2").await;
    let id = stock(&mut handler, &shelf, "Berries", "5").await;
    let taken = change(&mut handler, &id, "-2", "taken", None).await;

    assert_eq!(
        balance_on(&mut handler, &shelf, &id).await.as_deref(),
        Some("3.000")
    );

    for _ in 0..2 {
        let (status, body) = pantry_append(
            &mut handler,
            json!([{ "op": "del", "tbl": "quantity_change", "id": taken }]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "a repeated del is a replay: {body}");
    }
    assert_eq!(
        balance_on(&mut handler, &shelf, &id).await.as_deref(),
        Some("5.000"),
        "undone once, undone twice, the same state"
    );
    let events = body_of(
        handler
            .handle(get(&format!(
                "/a/pantry/api/events?tbl=quantity_change&id={taken}"
            )))
            .await,
    )
    .await;
    assert_eq!(events.lines().count(), 3, "put, del, del — all kept");
    let (status, _) = json_of(
        handler
            .handle(get(&format!("/a/pantry/api/row/quantity_change/{taken}")))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the row is absent, once");
}

/// `spec/protocol.md §4.5` — two devices each take the last portion while apart. Both
/// withdrawals are their own rows, both are kept, and the balance the log adds up to is
/// below zero. The app reports it and clamps nothing.
#[tokio::test]
async fn test_pantry_negative_balance_is_reported_not_clamped() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Freezer drawer 1").await;
    let id = stock(&mut handler, &shelf, "Last portion", "1").await;
    change(&mut handler, &id, "-1", "taken", None).await;
    change(&mut handler, &id, "-1", "taken", None).await;

    let checks = view(&mut handler, "/a/pantry/api/q/v_check_batch").await;
    let row = checks.iter().find(|row| row["id"] == id.as_str()).unwrap();
    assert_eq!(row["balance"], "-1.000", "the recorded number, not a floor");
    assert_eq!(row["name"], "Last portion");
    assert_eq!(row["shelf_name"], "Freezer drawer 1");

    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={shelf}"),
    )
    .await;
    assert_eq!(
        rows.iter().find(|row| row["id"] == id.as_str()).unwrap()["balance"],
        "-1.000",
        "a batch below zero is not hidden either"
    );
}

/// `spec/protocol.md §4.5` — two devices put the same withdrawal back. Both returns are
/// rows of their own, both are kept, and `v_check_out` says the withdrawal has had more
/// back than went out.
#[tokio::test]
async fn test_pantry_over_return_is_reported() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Freezer drawer 1").await;
    let id = stock(&mut handler, &shelf, "Waffle box", "8").await;
    let taken = change(&mut handler, &id, "-4", "taken", None).await;
    change(&mut handler, &id, "4", "returned", Some(&taken)).await;
    change(&mut handler, &id, "4", "returned", Some(&taken)).await;

    let checks = view(&mut handler, "/a/pantry/api/q/v_check_out").await;
    let row = checks
        .iter()
        .find(|row| row["id"] == taken.as_str())
        .unwrap();
    assert_eq!(row["taken"], "4.000");
    assert_eq!(row["returned"], "8.000");
    assert_eq!(row["batch_name"], "Waffle box");
}

/// `spec/cli.md §5.1` (`PV308`), `spec/data-dictionary.md §2` — a date is compared with
/// SQLite's own modifier, never by adding to the column. `v_expiring` binds `$days` as
/// text into `date('now', '+' || $days || ' days')`, and it answers for a batch already
/// past its date, one inside the window, one far off, and one with no date at all.
#[tokio::test]
async fn test_pantry_expiry_uses_the_date_modifier() {
    let root = tempfile::tempdir().unwrap();
    let mut handler = pantry(&root);
    let shelf = seeded_shelf(&mut handler, "Baking shelf").await;

    let schema =
        sql_only(&fs::read_to_string(repo_apps_dir().join("pantry").join("schema.sql")).unwrap());
    assert!(
        schema.contains("date('now', '+' || $days || ' days')"),
        "the modifier spelling is what the view runs"
    );
    assert!(
        !schema.contains("expires_on +") && !schema.contains("expires_on -"),
        "a DATE is never an integer to add to"
    );

    let mut ids = BTreeMap::new();
    for (name, expires) in [
        ("Past", Some(utc_day(-10))),
        ("Soon", Some(utc_day(3))),
        ("Fresh", Some(utc_day(400))),
        ("Undated", None),
    ] {
        let id = ulid();
        let (status, body) = pantry_append(
            &mut handler,
            json!([
                { "op": "put", "tbl": "batch", "id": id, "d": {
                    "name": name, "icon": "snow", "unit": "bags", "shelf_id": shelf,
                    "stored_on": utc_day(-20), "expires_on": expires } },
                { "op": "put", "tbl": "quantity_change", "id": ulid(), "d": {
                    "batch_id": id, "amount": "1", "reason": "stocked", "of_id": null,
                    "at": "2026-09-07T10:00:00Z" } },
            ]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        ids.insert(name, id);
    }

    let rows = view(
        &mut handler,
        &format!("/a/pantry/api/q/v_batch?shelf={shelf}"),
    )
    .await;
    let state: BTreeMap<&str, &str> = rows
        .iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap(),
                row["expiry"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(state["Past"], "past");
    assert_eq!(state["Soon"], "soon");
    assert_eq!(state["Fresh"], "fresh");
    assert_eq!(
        state["Undated"], "unknown",
        "no date is honestly its own state, not fresh"
    );

    let expiring = view(&mut handler, "/a/pantry/api/q/v_expiring?days=30").await;
    let named: BTreeSet<&str> = expiring
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert!(named.contains("Past"), "{named:?}");
    assert!(named.contains("Soon"), "{named:?}");
    assert!(!named.contains("Fresh"), "{named:?}");
    assert!(!named.contains("Undated"), "{named:?}");
    assert_eq!(
        expiring.iter().find(|row| row["name"] == "Past").unwrap()["expiry"],
        "past"
    );

    // The window is the placeholder's, bound as text: one day ahead names neither.
    let tight = view(&mut handler, "/a/pantry/api/q/v_expiring?days=1").await;
    let named: BTreeSet<&str> = tight
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert!(named.contains("Past"), "{named:?}");
    assert!(!named.contains("Soon"), "{named:?}");
}

// ---------------------------------------------------------------------------------------
// The accessibility baseline (spec/cli.md §5.4)
// ---------------------------------------------------------------------------------------

/// `spec/cli.md §5.4` — the framework's own pages meet `PV401`–`PV407`: the launcher with
/// a mounted, an unavailable and a missing app; the four settings pages with a permission
/// widening, an unknown icon, a broken folder, a seed offer and an alert so the alerts
/// table renders; the 404; the error page; a Lua error page with source context; the 503
/// for a build without a Lua host. `lang`, one `<main>`, labelled `<nav>`s, the skip
/// target, headings in order with one h1, labels, named controls, `<th scope>`, status
/// regions carrying text, no `on*=`, no `style=`, no inline script.
#[tokio::test]
async fn test_spec_cli_5_pv4xx_shell_pages() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let apps = privatium_core::Paths::rooted(root.path()).apps_dir();
    write_app(
        &apps,
        "wide",
        Some(&format!(
            "{}icon = \"no-such-icon\"\n[permissions]\nsql = true\n",
            lua_manifest("wide")
        )),
        &[("app.lua", "")],
    );
    write_app(&apps, "broken", Some("not toml at all ["), &[]);
    write_app(
        &apps,
        "seeded",
        Some(&lua_manifest("seeded")),
        &[
            ("app.lua", ""),
            (
                "schema.sql",
                "CREATE TABLE profile (id VARCHAR PRIMARY KEY, display_name VARCHAR);",
            ),
            (
                "sample/seed.jsonl",
                "{\"op\":\"put\",\"tbl\":\"profile\",\"id\":\"a\",\"d\":{\"display_name\":\"Ada\"}}\n",
            ),
        ],
    );
    write_app(
        &apps,
        "boom",
        Some(&lua_manifest("boom")),
        &[(
            "app.lua",
            "local pv = require 'privatium'\npv.get('/', function()\n  error('boom')\nend)\n",
        )],
    );
    let gone = write_web_app(&apps, "gone", &[]);
    {
        let (node, _) = open(&root);
        assert!(node.app("gone").is_some());
    }
    fs::remove_dir_all(&gone).unwrap();
    let handler = handler_for(&root);
    assert!(handler.report().missing.contains(&"gone".to_owned()));
    {
        let mut node = handler.node().lock().unwrap();
        let at = privatium_core::log::now();
        node.sys_log_mut()
            .put(
                privatium_core::sys::AUDIT,
                "01J6ZK2Q000000000000000ART",
                &json!({
                    "at": at,
                    "kind": "restore.tier3",
                    "actor": "system",
                    "subject": "hello",
                    "detail": "{\"reason\":\"a test alert\"}",
                    "severity": "alert",
                }),
            )
            .unwrap();
        node.refresh().unwrap();
    }

    let mut pages: Vec<(String, String)> = Vec::new();
    for (path, status) in [
        ("/", StatusCode::OK),
        ("/settings", StatusCode::OK),
        ("/settings/apps", StatusCode::OK),
        ("/settings/data", StatusCode::OK),
        ("/settings/devices", StatusCode::OK),
        ("/nope", StatusCode::NOT_FOUND),
        ("/a/boom/", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let response = handler.handle(get(path)).await;
        assert_eq!(response.status(), status, "{path}");
        pages.push((path.to_owned(), body_of(response).await));
    }
    pages.push((
        "shell::error".into(),
        shell::error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the detail <b>escaped</b>",
            false,
        ),
    ));
    pages.push(("shell::no_handler".into(), shell::no_handler("hello", true)));
    pages.push(("shell::not_found solo".into(), shell::not_found("/x", true)));
    for (path, html) in &pages {
        assert_clean(path, html, Unit::Document);
    }
    let page = |path: &str| -> &str { &pages.iter().find(|(p, _)| p == path).unwrap().1 };
    // What the fixtures were for: every branch of every page really rendered.
    let launcher = page("/");
    assert!(launcher.contains("href=\"/a/hello/\""), "{launcher}");
    assert!(launcher.contains("pv-unavailable"), "{launcher}");
    assert!(
        launcher.contains("gone — unavailable: folder missing"),
        "{launcher}"
    );
    assert!(
        launcher.contains("includeIndicatorStyles\":false"),
        "{launcher}"
    );
    let node_page = page("/settings");
    assert!(
        node_page.contains("<nav aria-label=\"Settings\">"),
        "{node_page}"
    );
    assert!(
        node_page.contains("<th scope=\"col\">When</th>"),
        "{node_page}"
    );
    assert!(node_page.contains("restore.tier3"), "{node_page}");
    let apps_page = page("/settings/apps");
    for expected in [
        "Load sample data",
        "Privacy warning",
        "Load warning",
        "ad-hoc read-only SQL",
        "not in the vendored Bootstrap Icons set",
        "Not loaded at startup",
        "<code>broken</code>",
    ] {
        assert!(apps_page.contains(expected), "{expected}\n{apps_page}");
    }
    let devices = page("/settings/devices");
    assert!(
        devices.contains("<th scope=\"col\">Device</th>"),
        "{devices}"
    );
    let boom = page("/a/boom/");
    assert!(boom.contains("<mark aria-current=\"true\">"), "{boom}");
    assert!(boom.contains("pv-source"), "{boom}");
    assert!(boom.contains("boom"), "{boom}");
    assert!(page("/nope").contains("<code>/nope</code>"));
}

/// `spec/app-contract.md §3` — optional descriptions are text, including markup from
/// an untrusted manifest; absent and blank descriptions fall back to the app slug.
#[tokio::test]
async fn test_spec_app_3_launcher_description_escaped_and_optional() {
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("apps");
    for (slug, extra) in [
        (
            "described",
            "description = '<img src=x onerror=alert(1)>'\n",
        ),
        ("blank", "description = '   '\n"),
        ("absent", ""),
    ] {
        write_app(
            &apps,
            slug,
            Some(&format!("{}{extra}", lua_manifest(slug))),
            &[("app.lua", "")],
        );
    }
    let handler = handler_for(&root);
    let page = body_of(handler.handle(get("/")).await).await;
    assert!(
        page.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "{page}"
    );
    assert!(!page.contains("<img src=x"), "{page}");
    for slug in ["blank", "absent"] {
        assert!(page.contains(&format!("<small>{slug}</small>")), "{page}");
    }
    assert_clean("launcher descriptions", &page, Unit::Document);
}

/// `spec/cli.md §5.4` — the Tier 1 page frame and the reference views meet the same
/// rules: hello's greeting, form and error re-render; animals' board, teach and knowledge
/// pages with the seed loaded; the `_board` fragment on its own (element checks only —
/// the page it lands in supplies the document); sketch's `index.html` as served.
#[tokio::test]
async fn test_spec_cli_5_pv4xx_app_frame_and_reference_views() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let handler = handler_for(&root);
    seed_animals(&handler).await;

    let mut documents: Vec<(&str, String)> = Vec::new();
    for path in [
        "/a/hello/",
        "/a/hello/edit",
        "/a/animals/",
        "/a/animals/teach",
        "/a/animals/knowledge",
    ] {
        let response = handler.handle(get(path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        documents.push((path, body_of(response).await));
    }
    let error = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=+"))
        .await;
    assert_eq!(error.status(), StatusCode::OK);
    documents.push(("/a/hello/name (error re-render)", body_of(error).await));
    let named = handler
        .handle(posted(&handler, "/a/hello/name", "display_name=Ada"))
        .await;
    assert_eq!(named.status(), StatusCode::SEE_OTHER);
    documents.push((
        "/a/hello/ (greeting)",
        body_of(handler.handle(get("/a/hello/")).await).await,
    ));
    documents.push((
        "sketch web/index.html",
        fs::read_to_string(
            repo_apps_dir()
                .join("sketch")
                .join("web")
                .join("index.html"),
        )
        .unwrap(),
    ));
    for (name, html) in &documents {
        assert_clean(name, html, Unit::Document);
    }
    assert!(
        documents[4].1.contains("aria-controls=\"path-"),
        "{}",
        documents[4].1
    );
    assert!(
        documents[5].1.contains("role=\"alert\""),
        "{}",
        documents[5].1
    );

    let fragment = body_of(
        handler
            .handle(with_htmx(posted(&handler, "/a/animals/start", "")))
            .await,
    )
    .await;
    assert!(
        fragment.contains("<h1>Does it live in the water?</h1>"),
        "{fragment}"
    );
    assert_clean("_board fragment", &fragment, Unit::Fragment);
}

/// `PV406` — the declared colour tokens of the chrome's stylesheet, which the shell's
/// own sheet uses and declares none of, meet 4.5:1 for text and 3:1 for focus and control
/// boundaries, in both colour schemes; the bar's fixed colours meet the same floors;
/// `:focus-visible` draws an outline and nothing removes one without a replacement;
/// sketch's own colours likewise.
#[test]
fn test_spec_cli_5_pv406_declared_tokens_meet_contrast() {
    let shell_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("shell");
    let css = fs::read_to_string(shell_dir.join("shell.css")).unwrap();
    let chrome = fs::read_to_string(shell_dir.join("chrome.css")).unwrap();
    assert!(
        a11y::root_tokens(&css).is_empty(),
        "shell.css declares no tokens of its own; chrome.css is the one source"
    );
    let schemes = a11y::root_tokens(&chrome);
    assert_eq!(schemes.len(), 2, "a light and a dark :root");
    // The bar is the same dark panel in both schemes: text, the ring and the borders
    // against it, and the current-page pill, all from the header's own declarations.
    let bar = a11y::rules(&chrome)
        .into_iter()
        .find(|(selector, _)| selector == ".pv-header")
        .expect("the header rule")
        .1;
    for (fg, bg, floor) in [
        ("--pv-bar-fg", "--pv-bar-bg", 4.5),
        ("--pv-bar-bg", "--pv-bar-accent", 4.5),
        ("--pv-bar-accent", "--pv-bar-bg", 3.0),
        ("--pv-bar-rule", "--pv-bar-bg", 3.0),
    ] {
        let ratio = a11y::contrast(&bar[fg], &bar[bg]);
        assert!(
            ratio >= floor,
            "bar: {fg} on {bg} is {ratio:.2}:1, want {floor}"
        );
    }
    let text_pairs = [
        ("--pv-fg", "--pv-bg"),
        ("--pv-fg", "--pv-panel"),
        ("--pv-fg", "--pv-line"),
        ("--pv-muted", "--pv-bg"),
        ("--pv-muted", "--pv-panel"),
        ("--pv-accent-fg", "--pv-accent"),
        ("--pv-warn-fg", "--pv-warn-bg"),
        ("--pv-alert-fg", "--pv-alert-bg"),
        ("--pv-ok-fg", "--pv-ok-bg"),
    ];
    let ui_pairs = [
        ("--pv-focus", "--pv-bg"),
        ("--pv-focus", "--pv-panel"),
        ("--pv-muted", "--pv-bg"),
        ("--pv-muted", "--pv-panel"),
        ("--pv-accent", "--pv-bg"),
        ("--pv-accent", "--pv-panel"),
        ("--pv-alert-fg", "--pv-bg"),
    ];
    for (name, tokens) in [("light", &schemes[0]), ("dark", &schemes[1])] {
        for (fg, bg) in text_pairs {
            let ratio = a11y::contrast(&tokens[fg], &tokens[bg]);
            assert!(
                ratio >= 4.5,
                "{name}: {fg} on {bg} is {ratio:.2}:1, want 4.5"
            );
        }
        for (fg, bg) in ui_pairs {
            let ratio = a11y::contrast(&tokens[fg], &tokens[bg]);
            assert!(ratio >= 3.0, "{name}: {fg} on {bg} is {ratio:.2}:1, want 3");
        }
    }
    let rules = a11y::rules(&css);
    let focus = rules
        .iter()
        .find(|(selector, _)| selector.contains(":focus-visible"))
        .expect(":focus-visible rule");
    let outline = focus.1.get("outline").expect("an outline");
    assert!(
        !outline.starts_with("none") && !outline.starts_with('0'),
        "{outline}"
    );
    assert!(outline.contains("var(--pv-focus)"), "{outline}");
    for (selector, declarations) in &rules {
        if let Some(outline) = declarations.get("outline")
            && (outline.starts_with("none") || outline.trim() == "0")
        {
            assert!(
                declarations.contains_key("box-shadow") || declarations.contains_key("border"),
                "{selector} removes the outline with no replacement"
            );
        }
    }
    let inputs = rules
        .iter()
        .find(|(selector, _)| selector.starts_with("main form input"))
        .expect("the text input rule");
    assert!(
        inputs.1["border"].contains("var(--pv-muted)"),
        "{}",
        inputs.1["border"]
    );
    assert!(
        !rules.iter().any(|(_, d)| d.contains_key("opacity"))
            && !a11y::rules(&chrome)
                .iter()
                .any(|(_, d)| d.contains_key("opacity")),
        "no rule dims text below its token's contrast"
    );
    assert!(css.contains("prefers-reduced-motion"), "the motion guard");
    for (selector, declarations) in a11y::rules(&chrome) {
        if let Some(outline) = declarations.get("outline")
            && (outline.starts_with("none") || outline.trim() == "0")
        {
            panic!("{selector} removes the outline in chrome.css");
        }
    }

    // sketch: its own sheet, its own colours, in both schemes. The sheet itself is always
    // white, so --ink is held against white as well as against the app's own surfaces.
    let sketch =
        fs::read_to_string(repo_apps_dir().join("sketch").join("web").join("style.css")).unwrap();
    let schemes = a11y::root_tokens(&sketch);
    assert_eq!(schemes.len(), 2, "a light and a dark :root");
    for tokens in &schemes {
        for surface in ["--bg", "--panel"] {
            assert!(
                a11y::contrast(&tokens["--ink"], &tokens[surface]) >= 7.0,
                "--ink on {surface} is {:.2}:1",
                a11y::contrast(&tokens["--ink"], &tokens[surface])
            );
            assert!(
                a11y::contrast(&tokens["--muted"], &tokens[surface]) >= 4.5,
                "--muted on {surface} is {:.2}:1",
                a11y::contrast(&tokens["--muted"], &tokens[surface])
            );
            // --muted is also what draws the boundary of anything a person operates, so
            // it carries WCAG 1.4.11 as well as its text duty; --line only ever separates
            // regions, which are not components.
            assert!(
                a11y::contrast(&tokens["--muted"], &tokens[surface]) >= 3.0,
                "control boundaries on {surface}"
            );
        }
        assert!(
            a11y::contrast(&tokens["--accent-ink"], &tokens["--accent"]) >= 4.5,
            "the pressed control's own label"
        );
        assert!(
            a11y::contrast(&tokens["--muted"], &tokens["--accent"]) >= 3.0,
            "a control's boundary survives the pressed fill"
        );
    }
    assert!(
        a11y::contrast(&schemes[0]["--ink"], "#ffffff") >= 7.0,
        "the keyboard crosshair and the selection outline on the white sheet"
    );
    let rules = a11y::rules(&sketch);
    let focus = rules
        .iter()
        .find(|(selector, _)| selector.contains(":focus-visible"))
        .expect(":focus-visible rule");
    assert!(focus.1["outline"].contains("var(--ink)"), "{:?}", focus.1);
    assert!(
        rules
            .iter()
            .any(|(selector, _)| selector.contains("[aria-pressed=\"true\"]")),
        "the current swatch is drawn from aria-pressed"
    );
    // Every icon is a <use> of the page's sprite and inherits its control's colour;
    // without this each one paints black, which vanishes on the dark panel.
    let icon = rules
        .iter()
        .find(|(selector, _)| selector.trim() == ".ic")
        .expect(".ic rule");
    assert!(icon.1["fill"].contains("currentColor"), "{:?}", icon.1);
    assert!(!sketch.contains("user-scalable"), "{sketch}");
}

/// A correct guess renders a win in both HTML paths without resetting or writing data.
#[tokio::test]
async fn test_animals_win_and_restart() {
    let root = tempfile::tempdir().unwrap();
    configure(&root, LUA_CONFIG);
    let handler = handler_for(&root);
    assert_eq!(
        handler.handle(get("/a/animals/won")).await.status(),
        StatusCode::SEE_OTHER,
        "no tree yet, so nothing to have guessed"
    );
    handler.handle(get("/a/animals/")).await;
    handler
        .handle(posted(&handler, "/a/animals/answer", "choice=yes"))
        .await;
    let log = log_path(&handler, "animals");
    let before = log_lines(&log);
    assert_eq!(before.len(), 4, "the starter tree and the cursor on dog");
    let page = body_of(handler.handle(get("/a/animals/won")).await).await;
    assert!(page.contains("<h1>I guessed it!</h1>"));
    assert!(page.contains("Start over"));
    assert_clean("win page", &page, Unit::Document);
    let fragment = body_of(handler.handle(with_htmx(get("/a/animals/won"))).await).await;
    assert!(fragment.contains("<h1>I guessed it!</h1>"));
    assert!(!fragment.contains("pv-header"));
    assert_eq!(log_lines(&log), before);
    let restarted = body_of(
        handler
            .handle(with_htmx(posted(&handler, "/a/animals/start", "")))
            .await,
    )
    .await;
    assert!(restarted.contains("<h1>Does it have legs?</h1>"));
    assert!(!restarted.contains("I guessed it!"));
    assert_eq!(log_lines(&log).len(), before.len() + 1);
}

/// The footer's node label, the app's title and a menu item's label are text, whatever
/// the manifest or the node's name holds; the footer links to the repository and to
/// Devices.
#[test]
fn test_footer_node_label_is_escaped() {
    let frame = shell::Frame {
        title: "Example <b>bold</b>".into(),
        icon: None,
        mount: "/a/example/".into(),
        menu: vec![shell::MenuLink {
            label: "<script>alert(1)</script>".into(),
            href: "/a/example/x".into(),
            icon: None,
        }],
        styles: Vec::new(),
        scripts: Vec::new(),
        static_dir: None,
        chrome: privatium_core::app::manifest::Chrome::Standard,
        navigation: privatium_core::app::manifest::Navigation::Page,
    };
    let page = shell::app_frame(
        &frame,
        &[],
        false,
        "token",
        "<h1>Example</h1>",
        "<img src=x onerror=alert(1)>",
    );
    assert!(page.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!page.contains("<img src=x"));
    assert!(page.contains("<title>Example &lt;b&gt;bold&lt;/b&gt; — Privatium</title>"));
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;</a></li>"));
    assert!(!page.contains("<script>alert"));
    assert!(page.contains("https://github.com/gabrielmongefranco/privatium"));
    assert!(page.contains("Connect a device"));
    assert_clean("footer", &page, Unit::Document);
}
