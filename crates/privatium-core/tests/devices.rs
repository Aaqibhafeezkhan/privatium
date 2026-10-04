// This file is part of Privatium
// crates/privatium-core/tests/devices.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-06
// Last Modified: 2026-10-04
// Summary: The owner's surfaces through core::handle: the pairing API of spec/protocol.md §9.2 and the
//          manifest's pair flag, the code page with the §7.7 disclosure, the devices page with
//          its label and revoke forms, the display-name form of §6.1, the hourly last_seen_at
//          of spec/data-dictionary.md §3.2, and every one of those pages under the PV4xx
//          rules. A refused caller — a LAN peer without a session, a form without its token, a
//          wrong value — is refused with nothing derived.
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

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use axum::body::to_bytes;
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{Method, StatusCode};
use common::a11y::{self, Unit};
use common::{at, audit_rows, sys_row};
use privatium_core::http::pairing::DISCLOSURE;
use privatium_core::{AppRoot, Body, Handler, Node, Peer, Request, Response, sys};
use serde_json::{Value, json};

/// A synthetic browser device, as pairing would have written it (`§3.2`), with one key
/// the dictionary does not know so preservation is observable (`spec/protocol.md §4.2`).
const DEVICE: &str = "b3nn8t2q";

fn device_row(label: &str, user_agent: &str) -> Value {
    json!({
        "label": label,
        "kind": "browser",
        "replica": false,
        "ed25519_pub": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "x25519_pub": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "paired_at": "2026-09-06T10:00:00.000Z",
        "paired_via": "lan",
        "user_agent": user_agent,
        "future_field": "kept verbatim",
    })
}

fn handler(root: &tempfile::TempDir) -> Handler {
    let mut node = Node::open(root.path()).unwrap();
    let report = node
        .load_apps(&[AppRoot::bundled(common::repo_apps_dir())])
        .unwrap();
    Handler::new(node, report)
}

fn with_device(handler: &Handler, label: &str, user_agent: &str) {
    let mut node = handler.node().lock().unwrap();
    node.sys_log_mut()
        .put(sys::DEVICE, DEVICE, &device_row(label, user_agent))
        .unwrap();
    node.refresh().unwrap();
}

/// An in-process call: no peer, so the node's own standing (`spec/protocol.md §8.4`).
fn owner(method: Method, path: &str) -> Request {
    axum::http::Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// A form POST carrying the token `csrf()` emits for `path`.
fn form(handler: &Handler, path: &str, fields: &str) -> Request {
    let token = handler.csrf().token(path);
    let body = if fields.is_empty() {
        format!("_csrf={token}")
    } else {
        format!("{fields}&_csrf={token}")
    };
    axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

fn json_post(path: &str, content_type: &str, body: &str) -> Request {
    axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// A LAN peer with no session (TEST-NET, never loopback).
fn lan(method: Method, path: &str) -> Request {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "192.0.2.1:8420")
        .header("accept", "application/json")
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(Peer("192.0.2.10:4000".parse().unwrap()));
    request
}

async fn body_of(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn json_of(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let text = body_of(response).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn location(response: &Response) -> &str {
    response.headers()[LOCATION].to_str().unwrap()
}

fn assert_clean(what: &str, html: &str) {
    let findings = a11y::check(html, Unit::Document);
    assert!(findings.is_empty(), "{what}: {findings:?}\n{html}");
}

// ---------------------------------------------------------------------------------------
// §9.2 — /api/v1/pair and the manifest's pair flag
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §9.2` — `POST /api/v1/pair` opens a window and answers the code in
/// both renderings, the URL and the expiry; the manifest's `pair` is true while it is
/// open and false once it is closed; `GET` answers the window or `null`. A body that is
/// not JSON, an unknown key, and a `ttl` outside 1–120 are refused before anything
/// opens; a LAN peer with no session is refused on both methods.
#[tokio::test]
async fn test_spec_9_2_manifest_pair_flag_is_true_while_open() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let (status, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(manifest["pair"], false);
    let (status, none) = json_of(h.handle(owner(Method::GET, "/api/v1/pair")).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(none.is_null(), "{none}");

    // Refusals open nothing.
    for (content_type, body, expected) in [
        (
            "application/x-www-form-urlencoded",
            "ttl=120",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        ("application/json", "{\"ttl\":0}", StatusCode::BAD_REQUEST),
        ("application/json", "{\"ttl\":121}", StatusCode::BAD_REQUEST),
        (
            "application/json",
            "{\"code\":\"amber otter\"}",
            StatusCode::BAD_REQUEST,
        ),
        ("application/json", "not json", StatusCode::BAD_REQUEST),
    ] {
        let response = h
            .handle(json_post("/api/v1/pair", content_type, body))
            .await;
        assert_eq!(response.status(), expected, "{content_type} {body}");
        assert!(
            !h.node()
                .lock()
                .unwrap()
                .pairing_open(jiff::Timestamp::now())
        );
    }
    for method in [Method::GET, Method::POST] {
        let response = h.handle(lan(method.clone(), "/api/v1/pair")).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} from the LAN"
        );
    }
    assert!(
        !h.node()
            .lock()
            .unwrap()
            .pairing_open(jiff::Timestamp::now())
    );
    assert_eq!(
        h.handle(owner(Method::DELETE, "/api/v1/pair"))
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    let (status, window) = json_of(
        h.handle(json_post(
            "/api/v1/pair",
            "application/json",
            "{\"ttl\": 90}",
        ))
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{window}");
    assert_eq!(window["emoji"].as_array().unwrap().len(), 4);
    assert_eq!(window["labels"].as_array().unwrap().len(), 4);
    assert_eq!(window["words"].as_array().unwrap().len(), 2);
    assert!(window["url"].as_str().unwrap().starts_with("http://"));
    assert!(window["expires_at"].as_str().unwrap().ends_with('Z'));
    assert!(window.get("consumed_by").is_none());
    let (_, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(manifest["pair"], true);
    let (_, again) = json_of(h.handle(owner(Method::GET, "/api/v1/pair")).await).await;
    assert_eq!(again["id"], window["id"], "GET reports the same window");
    // An empty POST body opens nothing new: the open window is answered again.
    let (_, third) = json_of(
        h.handle(json_post("/api/v1/pair", "application/json", ""))
            .await,
    )
    .await;
    assert_eq!(third["id"], window["id"]);

    let close = h
        .handle(form(&h, "/settings/devices/pairing/close", ""))
        .await;
    assert_eq!(close.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&close), "/settings/devices");
    let (_, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(manifest["pair"], false);
    let (_, none) = json_of(h.handle(owner(Method::GET, "/api/v1/pair")).await).await;
    assert!(none.is_null());
    let expired = audit_rows(&h.node().lock().unwrap(), "pair.expired");
    assert_eq!(expired.len(), 1);
    assert!(
        expired[0]["detail"]
            .as_str()
            .unwrap()
            .contains("closed by owner")
    );
}

// ---------------------------------------------------------------------------------------
// §7.2, §7.7 — the code page
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §7.7` — the plain-HTTP pairing screens carry the disclosure: the
/// code page on the node, and the bootstrap document a phone loads. The code page shows
/// the four glyphs with their labels beneath, the two words, the QR code as an image
/// with the URL as text beside it, the seconds remaining and the close button; a form
/// opening pairing leads to it, and a closed window leaves a card that says so.
#[tokio::test]
async fn test_spec_7_7_plain_http_pairing_page_discloses_the_gap() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    // The bootstrap a phone loads while pairing is closed, to compare against the one
    // it loads while a window is open: the two are the same bytes, because no window
    // state reaches that document.
    let before = body_of(h.handle(lan_html("/a/hello/")).await).await;
    let closed = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    assert!(closed.contains("Pairing is closed"), "{closed}");
    assert!(closed.contains("id=\"pv-pairing\""), "{closed}");
    assert!(
        !closed.contains("hx-trigger"),
        "a closed card does not poll"
    );

    let opened = h.handle(form(&h, "/settings/devices/pair", "")).await;
    assert_eq!(opened.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&opened), "/settings/devices/pairing");
    let window = h.node().lock().unwrap().pairing().unwrap().code();
    let page = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    assert!(page.contains(DISCLOSURE), "{page}");
    for glyph in window.glyphs() {
        assert!(page.contains(glyph.glyph), "glyph {}", glyph.label);
        assert!(page.contains(&format!(
            "<span class=\"pv-glyph-label\">{}</span>",
            glyph.label
        )));
    }
    let words = window.words();
    assert!(page.contains(&format!(
        "<p class=\"pv-words\">{} {}</p>",
        words[0], words[1]
    )));
    let url = h.node().lock().unwrap().listen_url();
    assert!(
        page.contains("<svg class=\"pv-qr-image\" role=\"img\""),
        "{page}"
    );
    assert!(page.contains(&format!("<figcaption><code>{url}</code></figcaption>")));
    assert!(page.contains("second"), "{page}");
    assert!(page.contains("hx-trigger=\"every 5s\""), "{page}");
    assert!(
        page.contains("action=\"/settings/devices/pairing/close\""),
        "{page}"
    );
    assert!(
        !page.contains("_csrf\" value=\"\""),
        "every form carries a token"
    );

    // The bootstrap a phone loads carries the sentence and the pairing screen's markup:
    // sixteen labelled keys, the word field, the status region. The sentence sits inside
    // the pairing section, which stays hidden on a device that is already paired, and the
    // bootstrap's own heading and status carry the class that keeps them out of sight
    // for a moment, so a paired device sees no interstitial between pages (§7.7).
    let bootstrap = body_of(h.handle(lan_html("/a/hello/")).await).await;
    assert!(bootstrap.contains(DISCLOSURE));
    let section_start = bootstrap.find("<section id=\"pv-pair\" hidden").unwrap();
    let section_end = bootstrap[section_start..].find("</section>").unwrap() + section_start;
    let disclosure_at = bootstrap.find(DISCLOSURE).unwrap();
    assert!(
        section_start < disclosure_at && disclosure_at < section_end,
        "the disclosure belongs to the pairing screen\n{bootstrap}"
    );
    assert_eq!(bootstrap.matches(DISCLOSURE).count(), 1);
    assert!(bootstrap.contains("<h1 class=\"pv-quiet\">"), "{bootstrap}");
    assert!(
        bootstrap.contains("<p id=\"pv-connecting\" class=\"pv-quiet\" role=\"status\">"),
        "{bootstrap}"
    );
    assert_eq!(bootstrap.matches("class=\"pv-pad-key\"").count(), 16);
    assert!(bootstrap.contains("<label for=\"pv-words\">"));
    assert!(bootstrap.contains("id=\"pv-pair-status\" role=\"status\""));
    // The code never reaches the bootstrap. A single word of the list can appear in
    // the screen's own prose ("device" is one), so the check is that the document is
    // unchanged by the open window, and that neither rendering of the code is in it.
    assert_eq!(bootstrap, before, "the bootstrap carries no window state");
    assert!(!bootstrap.contains(&format!("{} {}", words[0], words[1])));
    assert!(!bootstrap.contains(&window.glyphs().map(|g| g.glyph).concat()));
}

fn lan_html(path: &str) -> Request {
    let mut request = lan(Method::GET, path);
    request
        .headers_mut()
        .insert("accept", "text/html".parse().unwrap());
    request
}

// ---------------------------------------------------------------------------------------
// §3.2 — the devices page, labels, revocation, last_seen_at
// ---------------------------------------------------------------------------------------

/// The devices page lists every active device with its label and user agent escaped,
/// a label form and a revoke button for each device but this node; the revoke form
/// revokes it, closes its access and drops it from the list; the label form relabels
/// it. Revoking this node, an unknown device, or a device with the token missing is
/// refused with nothing written.
#[tokio::test]
async fn test_settings_devices_lists_paired_devices_and_revokes_one() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    with_device(
        &h,
        "<b>Pixel</b> 9",
        "Mozilla/5.0 <script>alert(1)</script>",
    );
    let id = h.node().lock().unwrap().id().as_str().to_owned();

    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(page.contains(DEVICE), "{page}");
    assert!(page.contains("&lt;b&gt;Pixel&lt;/b&gt; 9"), "{page}");
    assert!(
        page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{page}"
    );
    assert!(!page.contains("<b>Pixel</b>") && !page.contains("<script>alert"));
    assert!(page.contains(&format!("action=\"/settings/devices/{DEVICE}/revoke\"")));
    assert!(page.contains(&format!("action=\"/settings/devices/{DEVICE}/label\"")));
    assert!(
        !page.contains(&format!("action=\"/settings/devices/{id}/revoke\"")),
        "this node has no revoke form"
    );
    assert!(
        page.contains("action=\"/settings/devices/pair\""),
        "the owner sees Open pairing"
    );
    assert_clean("devices page", &page);

    // Refusals write nothing: no token, this node's own row, an unknown device.
    let no_token = axum::http::Request::builder()
        .method(Method::POST)
        .uri(format!("/settings/devices/{DEVICE}/revoke"))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("_csrf=stale"))
        .unwrap();
    assert_eq!(h.handle(no_token).await.status(), StatusCode::FORBIDDEN);
    let own = h
        .handle(form(&h, &format!("/settings/devices/{id}/revoke"), ""))
        .await;
    assert_eq!(own.status(), StatusCode::BAD_REQUEST);
    let unknown = h
        .handle(form(&h, "/settings/devices/zzzzzzzz/revoke", ""))
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(audit_rows(&h.node().lock().unwrap(), "device.revoked").is_empty());
    assert!(sys_row(&h.node().lock().unwrap(), sys::DEVICE, &id).unwrap()["revoked_at"].is_null());

    // The label form.
    let labelled = h
        .handle(form(
            &h,
            &format!("/settings/devices/{DEVICE}/label"),
            "label=Kitchen+tablet",
        ))
        .await;
    assert_eq!(labelled.status(), StatusCode::SEE_OTHER);
    let row = sys_row(&h.node().lock().unwrap(), sys::DEVICE, DEVICE).unwrap();
    assert_eq!(row["label"], "Kitchen tablet");
    assert_eq!(row["user_agent"], "Mozilla/5.0 <script>alert(1)</script>");
    let too_long = h
        .handle(form(
            &h,
            &format!("/settings/devices/{DEVICE}/label"),
            &format!("label={}", "x".repeat(81)),
        ))
        .await;
    assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    assert!(body_of(too_long).await.contains("at most 80 characters"));

    // Revocation.
    let revoked = h
        .handle(form(&h, &format!("/settings/devices/{DEVICE}/revoke"), ""))
        .await;
    assert_eq!(revoked.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&revoked), "/settings/devices");
    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(
        !page.contains(DEVICE),
        "a revoked device leaves the active list:\n{page}"
    );
    let row = sys_row(&h.node().lock().unwrap(), sys::DEVICE, DEVICE).unwrap();
    assert!(row["revoked_at"].as_str().unwrap().ends_with('Z'), "{row}");
    assert_eq!(
        row["label"], "Kitchen tablet",
        "every other column survives"
    );
    let audit = audit_rows(&h.node().lock().unwrap(), "device.revoked");
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["subject"], DEVICE);
    // Revoking again is a no-op, not a second row.
    h.handle(form(&h, &format!("/settings/devices/{DEVICE}/revoke"), ""))
        .await;
    assert_eq!(
        audit_rows(&h.node().lock().unwrap(), "device.revoked").len(),
        1
    );
}

/// `spec/data-dictionary.md §3.2`, `spec/protocol.md §4.2` — the devices page trusts
/// nothing a `sys_device` row carries, its key included: a row whose `id` is not shaped
/// as a Node ID — a hand-edited log, a restored backup — is listed with the ID escaped,
/// gets no label form and no revoke form, and the page still meets the PV4xx rules and
/// the document checks. An empty ID and one that is only whitespace are listed the same
/// way; the router answers nothing for either.
#[tokio::test]
async fn test_spec_3_2_devices_page_never_trusts_a_device_id_from_the_log() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let hostile = "x\" onmouseover=\"1";
    for id in [hostile, "", " "] {
        let mut node = h.node().lock().unwrap();
        node.sys_log_mut()
            .put(sys::DEVICE, id, &device_row("Stray", "synthetic agent"))
            .unwrap();
        node.refresh().unwrap();
    }
    with_device(&h, "Pixel 9", "synthetic agent");
    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(page.contains("x&quot; onmouseover=&quot;1"), "{page}");
    assert!(!page.contains(hostile), "{page}");
    assert!(!page.contains("onmouseover=\"1\""), "{page}");
    assert!(
        !page.contains("action=\"/settings/devices/x"),
        "no form for an ID the router would refuse:\n{page}"
    );
    assert!(
        page.contains(&format!("action=\"/settings/devices/{DEVICE}/revoke\"")),
        "a shaped ID keeps its forms"
    );
    assert_clean("devices page with a stray row", &page);
    let attempted = h
        .handle(form(
            &h,
            "/settings/devices/x%22%20onmouseover=%221/revoke",
            "",
        ))
        .await;
    assert_eq!(attempted.status(), StatusCode::NOT_FOUND);
}

/// `spec/data-dictionary.md §3.2` — revocation is a `put` with `revoked_at` set, never a
/// `del`: the log holds no tombstone for the device, the revoking line carries every
/// column the row had — a key the dictionary does not know included
/// (`spec/protocol.md §4.2`) — and the historical record of the pairing survives.
#[tokio::test]
async fn test_spec_3_2_revocation_is_a_put_never_a_del() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    with_device(&h, "Pixel 9", "synthetic agent");
    h.handle(form(&h, &format!("/settings/devices/{DEVICE}/revoke"), ""))
        .await;
    let node = h.node().lock().unwrap();
    let log = std::fs::read_to_string(node.paths().app_log(sys::SLUG, node.id())).unwrap();
    let lines: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|event: &Value| event["tbl"] == "sys_device" && event["id"] == DEVICE)
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "the pairing's put and the revocation's put:\n{log}"
    );
    assert!(lines.iter().all(|event| event["op"] == "put"));
    let revocation = &lines[1]["d"];
    assert!(revocation["revoked_at"].is_string());
    assert_eq!(revocation["future_field"], "kept verbatim");
    assert_eq!(revocation["label"], "Pixel 9");
    assert_eq!(revocation["user_agent"], "synthetic agent");
    assert_eq!(revocation["paired_at"], "2026-09-06T10:00:00.000Z");
    assert!(
        revocation.get("revoked_reason").is_none(),
        "no reason was given"
    );
}

/// `spec/data-dictionary.md §3.2` — `last_seen_at` is written when the row holds none,
/// then at most hourly: a second mark thirty minutes later writes nothing, one an hour
/// later writes again. An unknown device and a revoked one write nothing. The instant
/// is passed in, never read from the clock.
#[test]
fn test_spec_3_2_last_seen_at_is_written_at_most_hourly() {
    let root = tempfile::tempdir().unwrap();
    let mut node = Node::open(root.path()).unwrap();
    node.sys_log_mut()
        .put(
            sys::DEVICE,
            DEVICE,
            &device_row("Pixel 9", "synthetic agent"),
        )
        .unwrap();
    node.refresh().unwrap();
    let t0 = at("2026-09-06T12:00:00.000Z");
    let minutes = |m: i64| t0.checked_add(jiff::SignedDuration::from_mins(m)).unwrap();
    let events = |node: &Node| -> usize {
        std::fs::read_to_string(node.paths().app_log(sys::SLUG, node.id()))
            .unwrap()
            .lines()
            .filter(|line| line.contains("\"tbl\":\"sys_device\"") && line.contains(DEVICE))
            .count()
    };
    assert_eq!(events(&node), 1);
    assert!(
        node.note_device_seen(DEVICE, t0).unwrap(),
        "no mark yet: written"
    );
    assert_eq!(
        sys_row(&node, sys::DEVICE, DEVICE).unwrap()["last_seen_at"],
        "2026-09-06T12:00:00.000Z"
    );
    assert!(
        !node.note_device_seen(DEVICE, minutes(30)).unwrap(),
        "within the hour: nothing"
    );
    assert!(!node.note_device_seen(DEVICE, minutes(59)).unwrap());
    assert_eq!(events(&node), 2);
    assert!(
        node.note_device_seen(DEVICE, minutes(60)).unwrap(),
        "the boundary: written"
    );
    assert_eq!(
        sys_row(&node, sys::DEVICE, DEVICE).unwrap()["last_seen_at"],
        "2026-09-06T13:00:00.000Z"
    );
    assert_eq!(events(&node), 3);
    assert!(
        !node.note_device_seen("zzzzzzzz", minutes(120)).unwrap(),
        "unknown: nothing"
    );
    // A mark from the future — a clock that was wrong — is corrected on the next mark,
    // not honoured for however long it claims.
    let mut row = sys_row(&node, sys::DEVICE, DEVICE).unwrap();
    row["last_seen_at"] = Value::String("2027-01-01T00:00:00.000Z".to_owned());
    node.sys_log_mut().put(sys::DEVICE, DEVICE, &row).unwrap();
    node.refresh().unwrap();
    assert!(
        node.note_device_seen(DEVICE, minutes(90)).unwrap(),
        "a future mark is written over"
    );
    assert_eq!(
        sys_row(&node, sys::DEVICE, DEVICE).unwrap()["last_seen_at"],
        "2026-09-06T13:30:00.000Z"
    );
    // An unreadable mark is written over too.
    let mut row = sys_row(&node, sys::DEVICE, DEVICE).unwrap();
    row["last_seen_at"] = Value::String("yesterday".to_owned());
    node.sys_log_mut().put(sys::DEVICE, DEVICE, &row).unwrap();
    node.refresh().unwrap();
    assert!(node.note_device_seen(DEVICE, minutes(100)).unwrap());
    // The rows above spell `revoked_at` out as null, which is the same value as an
    // absent key (`spec/data-dictionary.md §2.1`): the device is still revocable.
    assert!(sys_row(&node, sys::DEVICE, DEVICE).unwrap()["revoked_at"].is_null());
    node.revoke_device(DEVICE, Some("lost"), minutes(120))
        .unwrap();
    assert!(
        sys_row(&node, sys::DEVICE, DEVICE).unwrap()["revoked_at"].is_string(),
        "an explicit null was not mistaken for a revocation"
    );
    assert!(
        !node.note_device_seen(DEVICE, minutes(240)).unwrap(),
        "revoked: nothing"
    );
    assert_eq!(
        sys_row(&node, sys::DEVICE, DEVICE).unwrap()["revoked_reason"],
        "lost"
    );
    // The registry refuses this node's own row and text past the bound.
    let own = node.id().as_str().to_owned();
    assert!(matches!(
        node.revoke_device(&own, None, t0),
        Err(privatium_core::Error::OwnDevice)
    ));
    assert!(matches!(
        node.label_device(&own, "x"),
        Err(privatium_core::Error::OwnDevice)
    ));
    assert!(matches!(
        node.label_device(DEVICE, &"x".repeat(81)),
        Err(privatium_core::Error::InvalidText { .. })
    ));
}

// ---------------------------------------------------------------------------------------
// §6.1 — the display name
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §6.1`, `§9.2` — the node page's form sets `sys_node.display_name`;
/// the manifest, the discovery facts and the page carry it, escaped; an empty name
/// unsets it so the Node ID stands in again; a name past 63 characters is refused on
/// the page with the row untouched; a form without its token is refused.
#[tokio::test]
async fn test_settings_node_display_name_is_set_by_the_owner_and_reaches_the_manifest() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let id = h.node().lock().unwrap().id().as_str().to_owned();
    let page = body_of(h.handle(owner(Method::GET, "/settings")).await).await;
    assert!(page.contains("<label for=\"display-name\" "), "{page}");
    assert!(page.contains("Your other spaces on this network"), "{page}");
    assert!(page.contains("<label for=\"join-url\">"), "{page}");
    assert!(page.contains("http://"), "the LAN URL is on the page");
    assert_clean("node page", &page);

    let named = h
        .handle(form(
            &h,
            "/settings/name",
            "display_name=Study+%3Cb%3Ehome%3C%2Fb%3E",
        ))
        .await;
    assert_eq!(named.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&named), "/settings");
    let (_, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(manifest["name"], "Study <b>home</b>");
    assert_eq!(
        h.node().lock().unwrap().discovery_facts().unwrap().name,
        "Study <b>home</b>"
    );
    let page = body_of(h.handle(owner(Method::GET, "/settings")).await).await;
    assert!(
        page.contains("value=\"Study &lt;b&gt;home&lt;/b&gt;\""),
        "{page}"
    );
    assert!(!page.contains("Study <b>home</b>"));
    let audit = audit_rows(&h.node().lock().unwrap(), "config.changed");
    assert_eq!(audit.len(), 1);
    assert!(
        !audit[0]["detail"].as_str().unwrap().contains("Study"),
        "never the name"
    );

    let too_long = h
        .handle(form(
            &h,
            "/settings/name",
            &format!("display_name={}", "n".repeat(64)),
        ))
        .await;
    assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    assert!(body_of(too_long).await.contains("at most 63 characters"));
    let (_, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(manifest["name"], "Study <b>home</b>", "refused: unchanged");

    let stale = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/settings/name")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("display_name=Hijack&_csrf=nope"))
        .unwrap();
    assert_eq!(h.handle(stale).await.status(), StatusCode::FORBIDDEN);

    let cleared = h.handle(form(&h, "/settings/name", "display_name=+")).await;
    assert_eq!(cleared.status(), StatusCode::SEE_OTHER);
    let (_, manifest) = json_of(h.handle(owner(Method::GET, "/api/v1/manifest")).await).await;
    assert_eq!(manifest["name"], id, "unset again, so the Node ID");
    let node = h.node().lock().unwrap();
    let row = sys_row(&node, sys::NODE, &id).unwrap();
    assert!(row["display_name"].is_null());
    assert_eq!(
        row["pubkey"],
        node.identity().public_key_base64(),
        "the rest of the row survives"
    );
}

// ---------------------------------------------------------------------------------------
// The settings page's own wording
// ---------------------------------------------------------------------------------------

/// The Display name cell falls back to the machine's name where the owner has set none,
/// and says `(not set)` where the platform hands no name over. The stored name always
/// wins, and every one of the three is escaped.
#[test]
fn test_settings_display_name_falls_back_to_the_machine_name() {
    use privatium_core::http::devices::{display_name_value, machine_name};

    assert_eq!(display_name_value(Some("Study"), Some("laptop")), "Study");
    assert_eq!(
        display_name_value(Some("<b>x</b>"), None),
        "&lt;b&gt;x&lt;/b&gt;"
    );
    let fallback = display_name_value(None, Some("kitchen-<pc>"));
    assert!(fallback.starts_with("kitchen-&lt;pc&gt; "), "{fallback}");
    assert!(fallback.contains("this computer's name"), "{fallback}");
    assert_eq!(
        display_name_value(None, None),
        "<span class=\"pv-muted\">(not set)</span>"
    );

    // Whatever this machine answers, it is one non-empty line with no control characters.
    if let Some(name) = machine_name() {
        assert!(!name.is_empty());
        assert!(!name.chars().any(char::is_control), "{name}");
        assert_eq!(name.trim(), name, "{name}");
    }
}

/// The node page renders the Display name cell with the owner's pencil beside it and the
/// naming form inside the disclosure, and shows the fallback rather than the Space ID
/// while no name is set. A session that is not the owner gets the value and no form.
#[tokio::test]
async fn test_settings_display_name_is_edited_in_place() {
    use privatium_core::http::devices::{display_name_value, machine_name};

    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let page = body_of(h.handle(owner(Method::GET, "/settings")).await).await;
    assert!(
        page.contains("<summary aria-label=\"Edit display name\""),
        "{page}"
    );
    assert!(page.contains("class=\"pv-name-edit\""), "{page}");
    assert!(
        page.contains(&display_name_value(None, machine_name().as_deref())),
        "{page}"
    );
    assert!(
        !page.contains("the Space ID stands in for it"),
        "the old wording is gone: {page}"
    );
    assert_clean("node page, unnamed", &page);
}

/// The Devices table names a space by its ID and its display name, in the shape the
/// discovered-space lists use — and prints the ID once where there is no display name,
/// which is what a space with no name advertises as its own (`spec/protocol.md §6.1`).
#[tokio::test]
async fn test_settings_devices_show_the_id_and_the_display_name() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    with_device(&h, "Pixel 9", "synthetic agent");

    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    let cell = format!("<code>{DEVICE}</code>");
    assert_eq!(
        page.matches(&cell).count(),
        1,
        "the ID is printed once while the space has no display name: {page}"
    );

    {
        let mut node = h.node().lock().unwrap();
        node.sys_log_mut()
            .put(sys::NODE, DEVICE, &json!({ "display_name": "Kitchen" }))
            .unwrap();
        node.refresh().unwrap();
    }
    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(page.contains(&format!("{cell} Kitchen")), "{page}");
    assert_clean("devices page, named space", &page);
}

/// `spec/protocol.md §7.2` — both renderings are shown, and the words come first: on the
/// space's own code page, and on the connect screen where they are typed and tapped.
#[tokio::test]
async fn test_spec_7_2_both_renderings_with_the_words_first() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    assert_eq!(
        h.handle(form(&h, "/settings/devices/pair", ""))
            .await
            .status(),
        StatusCode::SEE_OTHER
    );
    let window = h.node().lock().unwrap().pairing().unwrap().code();
    let page = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    let words = page.find("Type these two words").expect("the words");
    let emoji = page.find("Or tap these four emoji").expect("the emoji");
    assert!(words < emoji, "words before emoji: {page}");
    for word in window.words() {
        assert!(page.contains(word), "{word} is shown");
    }
    for glyph in window.glyphs() {
        assert!(page.contains(glyph.glyph), "glyph {}", glyph.label);
    }
    assert_clean("pairing page, open", &page);

    let connect = body_of(h.handle(lan_html("/")).await).await;
    let words = connect.find("id=\"pv-words\"").expect("the word field");
    let pad = connect.find("class=\"pv-pad\"").expect("the pad");
    assert!(words < pad, "the word field before the pad: {connect}");
    assert_clean("connect screen", &connect);
}

// ---------------------------------------------------------------------------------------
// spec/cli.md §5.4 — the PV4xx rules over every page in this file
// ---------------------------------------------------------------------------------------

/// `spec/cli.md §5.4` — the devices page with a device and its forms, the code page with
/// an open window, the closed card, the node page with its form, and the bootstrap with
/// the pairing screen's markup all meet the PV4xx rules and the document checks.
#[tokio::test]
async fn test_spec_cli_5_pv4xx_pairing_and_devices_pages() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    with_device(&h, "Pixel 9", "synthetic agent");
    let closed = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    assert_clean("pairing page, closed", &closed);
    h.node()
        .lock()
        .unwrap()
        .pair(Duration::from_secs(120))
        .unwrap();
    for path in [
        "/settings",
        "/settings/devices",
        "/settings/devices/pairing",
    ] {
        let html = body_of(h.handle(owner(Method::GET, path)).await).await;
        assert_clean(path, &html);
    }
    let bootstrap = body_of(h.handle(lan_html("/a/hello/")).await).await;
    assert_clean("bootstrap with the pairing screen", &bootstrap);
    // The screen's pad keys carry their label as text, so a screen reader says each once.
    let tree = a11y::parse(&bootstrap);
    let keys: Vec<_> = tree
        .descendants()
        .into_iter()
        .filter(|e| e.has_class("pv-pad-key"))
        .collect();
    assert_eq!(keys.len(), 16);
    for key in keys {
        assert!(key.attr("aria-label").is_none());
        assert!(!key.all_text().trim().is_empty());
    }
}

// ---------------------------------------------------------------------------------------
// §9.2, §7.1 — a window for a node, and /api/v1/join, for the owner alone
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §9.2`, `§7.1` — `POST /api/v1/pair` with `"node": true` opens a
/// window for a node, reported as such; `POST /api/v1/join` and the two forms are the
/// owner's alone; a session is refused on every one of them; an oversized body is
/// refused before it is read; a malformed URL, an empty code and a `node` that is not a
/// boolean are refused by name and dial nothing.
#[tokio::test]
async fn test_spec_9_2_join_and_node_windows_are_the_owners_alone_and_bound_their_bodies() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    // A node window, reported as one, and the code page that describes it.
    let (status, window) = json_of(
        h.handle(json_post(
            "/api/v1/pair",
            "application/json",
            "{\"ttl\": 90, \"node\": true}",
        ))
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{window}");
    assert_eq!(window["node"], true);
    let (_, again) = json_of(h.handle(owner(Method::GET, "/api/v1/pair")).await).await;
    assert_eq!(again["node"], true);
    let page = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    assert!(page.contains("Admit a space"), "{page}");
    assert!(page.contains("privatium pair --join"), "{page}");
    assert_clean("code page for a node window", &page);
    let devices = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(devices.contains("for another space"), "{devices}");
    let close = h
        .handle(form(&h, "/settings/devices/pairing/close", ""))
        .await;
    assert_eq!(close.status(), StatusCode::SEE_OTHER);

    // `node` that is not a boolean, and a body past the bound, are refused by name.
    for (body, expected) in [
        ("{\"ttl\": 90, \"node\": \"yes\"}", StatusCode::BAD_REQUEST),
        ("{\"ttl\": 90, \"node\": 1}", StatusCode::BAD_REQUEST),
    ] {
        let response = h
            .handle(json_post("/api/v1/pair", "application/json", body))
            .await;
        assert_eq!(response.status(), expected, "{body}");
        assert!(
            !h.node()
                .lock()
                .unwrap()
                .pairing_open(jiff::Timestamp::now())
        );
    }
    let mut oversized = json_post("/api/v1/pair", "application/json", "{\"ttl\": 90}");
    oversized
        .headers_mut()
        .insert("content-length", "4096".parse().unwrap());
    assert_eq!(
        h.handle(oversized).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );

    // The join route: the owner's alone, JSON alone, bounded, and checked by shape
    // before anything is dialed.
    for (content_type, body, expected, needle) in [
        (
            "application/x-www-form-urlencoded",
            "url=http://192.0.2.5:8420&code=amber+otter",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "application/json",
        ),
        (
            "application/json",
            "not json",
            StatusCode::BAD_REQUEST,
            "url",
        ),
        (
            "application/json",
            "{\"url\": \"http://192.0.2.5:8420\"}",
            StatusCode::BAD_REQUEST,
            "code",
        ),
        (
            "application/json",
            "{\"url\": \"http://192.0.2.5:8420\", \"code\": \"\"}",
            StatusCode::BAD_REQUEST,
            "code",
        ),
        (
            "application/json",
            "{\"url\": \"http://192.0.2.5:8420\", \"code\": \"not a code at all\"}",
            StatusCode::BAD_REQUEST,
            "code",
        ),
        (
            "application/json",
            "{\"url\": \"https://192.0.2.5:8420\", \"code\": \"abyss acid\"}",
            StatusCode::BAD_REQUEST,
            "http://",
        ),
        (
            "application/json",
            "{\"url\": \"http://192.0.2.5:8420/settings\", \"code\": \"abyss acid\"}",
            StatusCode::BAD_REQUEST,
            "host and a port",
        ),
        (
            "application/json",
            "{\"url\": \"http://192.0.2.5:8420\", \"code\": \"abyss acid\", \"ttl\": 5}",
            StatusCode::BAD_REQUEST,
            "nothing else",
        ),
    ] {
        let (status, body_text) = json_of(
            h.handle(json_post("/api/v1/join", content_type, body))
                .await,
        )
        .await;
        assert_eq!(status, expected, "{content_type} {body}: {body_text}");
        assert!(
            body_text.as_str().unwrap_or_default().contains(needle),
            "{body}: {body_text}"
        );
    }
    let mut oversized = json_post(
        "/api/v1/join",
        "application/json",
        "{\"url\": \"http://192.0.2.5:8420\", \"code\": \"abyss acid\"}",
    );
    oversized
        .headers_mut()
        .insert("content-length", "4096".parse().unwrap());
    assert_eq!(
        h.handle(oversized).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    for method in [Method::GET, Method::POST] {
        let response = h.handle(lan(method.clone(), "/api/v1/join")).await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} from the LAN"
        );
    }
    assert_eq!(
        h.handle(owner(Method::GET, "/api/v1/join")).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    // The two forms carry a token and are the owner's; a LAN caller is refused.
    for path in ["/settings/devices/admit", "/settings/join"] {
        let response = h.handle(lan(Method::POST, path)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let admit = h.handle(form(&h, "/settings/devices/admit", "")).await;
    assert_eq!(admit.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&admit), "/settings/devices/pairing");
    assert!(h.node().lock().unwrap().pairing().unwrap().is_for_node());
    // A join form with a code that is not one is refused on the node page, by name.
    let refused = h
        .handle(form(
            &h,
            "/settings/join",
            "url=http%3A%2F%2F192.0.2.5%3A8420&code=nonsense",
        ))
        .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    let page = body_of(refused).await;
    assert!(page.contains("code"), "{page}");
    assert_clean("node page with a refusal", &page);
    // The identity is untouched by every refusal above.
    assert!(h.node().lock().unwrap().is_disposable().unwrap());
}

/// `spec/protocol.md §2.3.4` — the devices page's *Revoke* on a node's row revokes it as
/// a node: the device row, the `sys_node_revocation` row and `node.revoked`.
#[tokio::test]
async fn test_spec_2_3_4_the_devices_page_revokes_a_node_as_a_node() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let node_id = "k7m2q9xf";
    {
        let mut node = h.node().lock().unwrap();
        node.sys_log_mut()
            .put(
                sys::DEVICE,
                node_id,
                &json!({
                    "kind": "node",
                    "replica": true,
                    "ed25519_pub": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                    "x25519_pub": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                    "paired_at": "2026-09-06T10:00:00.000Z",
                    "paired_via": "lan",
                }),
            )
            .unwrap();
        node.refresh().unwrap();
    }
    let page = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(page.contains("Revoking a space"), "{page}");
    assert_clean("devices page with a node row", &page);
    let revoked = h
        .handle(form(&h, &format!("/settings/devices/{node_id}/revoke"), ""))
        .await;
    assert_eq!(revoked.status(), StatusCode::SEE_OTHER);
    let node = h.node().lock().unwrap();
    assert!(node.node_revoked(node_id).unwrap());
    assert!(sys_row(&node, "sys_node_revocation", node_id).is_some());
    assert!(sys_row(&node, "sys_device", node_id).unwrap()["revoked_at"].is_string());
    assert_eq!(audit_rows(&node, "node.revoked").len(), 1);
}

/// `spec/cli.md §5.1` PV401–PV407, over the join form, the *Admit a space* button and
/// the node page's two lists (`tests/common/a11y.rs`): the node page for the owner and
/// for a session, the devices page with the button, and the code page for a node window.
#[tokio::test]
async fn test_spec_cli_5_pv4xx_join_form_admit_button_and_node_page() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    with_device(&h, "Pixel 9", "synthetic agent");
    let page = body_of(h.handle(owner(Method::GET, "/settings")).await).await;
    assert!(page.contains("<label for=\"join-url\">"), "{page}");
    assert!(page.contains("<label for=\"join-code\">"), "{page}");
    assert!(page.contains("Your other spaces on this network"), "{page}");
    assert!(page.contains("Other spaces on this network"), "{page}");
    assert!(page.contains("Cluster ID"), "{page}");
    assert_clean("node page, owner", &page);
    let devices = body_of(h.handle(owner(Method::GET, "/settings/devices")).await).await;
    assert!(devices.contains("Admit a space"), "{devices}");
    assert_clean("devices page with the admit button", &devices);
    h.node()
        .lock()
        .unwrap()
        .pair_node(Duration::from_secs(120))
        .unwrap();
    for path in [
        "/settings",
        "/settings/devices",
        "/settings/devices/pairing",
    ] {
        let html = body_of(h.handle(owner(Method::GET, path)).await).await;
        assert_clean(&format!("{path} with a node window"), &html);
    }
    // The pad keys and the QR code are the device window's; a node window shows the
    // command and the labels to type instead, and stays polling.
    let code_page = body_of(
        h.handle(owner(Method::GET, "/settings/devices/pairing"))
            .await,
    )
    .await;
    assert!(code_page.contains("id=\"pv-pairing\""), "{code_page}");
    assert!(code_page.contains("hx-trigger=\"every 5s\""), "{code_page}");
}
