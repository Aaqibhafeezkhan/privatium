// This file is part of Privatium
// crates/privatium-core/tests/channel.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: LAN bootstrap isolation, channel framing and authenticated routing.
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

use axum::body::to_bytes;
use privatium_core::{AppRoot, Body, Handler, Node, Peer, Request};

fn handler(root: &tempfile::TempDir) -> Handler {
    let mut node = Node::open(root.path()).unwrap();
    let report = node
        .load_apps(&[AppRoot::bundled(common::repo_apps_dir())])
        .unwrap();
    Handler::new(node, report)
}

fn lan(method: &str, path: &str, accept: &str) -> Request {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "192.0.2.1:8420")
        .header("accept", accept)
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(Peer("192.0.2.10:4000".parse().unwrap()));
    request
}

#[tokio::test]
async fn test_spec_8_3_1_bootstrap_uses_destination_app_permissions() {
    let root = tempfile::tempdir().unwrap();
    let apps = tempfile::tempdir().unwrap();
    let app = apps.path().join("synthetic");
    std::fs::create_dir_all(app.join("web")).unwrap();
    std::fs::write(app.join("app.toml"), "[app]\ntitle = \"Synthetic\"\nslug = \"synthetic\"\nversion = \"1.0.0\"\napi = 1\ntier = \"web\"\n[permissions]\nwasm = true\nremote = [\"https://example.invalid\"]\n").unwrap();
    std::fs::write(
        app.join("web/index.html"),
        "<!doctype html><title>Synthetic</title><h1>Synthetic</h1>",
    )
    .unwrap();
    let mut node = Node::open(root.path()).unwrap();
    let report = node.load_apps(&[AppRoot::bundled(apps.path())]).unwrap();
    assert!(node.app("synthetic").is_some(), "{report:?}");
    let h = Handler::new(node, report);
    for path in ["/a/synthetic/", "/a/synthetic/post"] {
        let response = h.handle(lan("GET", path, "text/html")).await;
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("'wasm-unsafe-eval'"), "{csp}");
        assert!(csp.contains("https://example.invalid"), "{csp}");
        assert!(!csp.contains("'unsafe-eval'"));
    }
    let response = h.handle(lan("GET", "/", "text/html")).await;
    assert!(
        !response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("wasm")
    );
}

#[tokio::test]
async fn test_spec_8_4_plain_http_on_the_lan_serves_only_the_bootstrap_set() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    for path in [
        "/",
        "/settings",
        "/settings/apps",
        "/settings/data",
        "/settings/devices",
        "/skills/privatium-overview.md",
        "/skills/bundle.zip",
        "/a/hello/",
        "/a/animals/play",
        "/a/sketch",
        "/a/nope/",
        "/a/",
        "/nope",
        "/api/nope",
        "/a/hello/api/schema",
        "/a/hello/api/events",
        "/a/hello/api/stream",
        "/api/v1/pair",
        "/settings/devices/pairing",
    ] {
        assert_eq!(
            h.handle(lan("GET", path, "application/json"))
                .await
                .status(),
            403,
            "{path}"
        );
        let r = h.handle(lan("GET", path, "text/html")).await;
        assert_eq!(r.status(), 200, "{path}");
        let body = to_bytes(r.into_body(), 65536).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("/static/client.js"));
        assert_eq!(h.handle(lan("POST", path, "text/html")).await.status(), 403);
    }
    for path in [
        "/api/v1/health",
        "/api/v1/manifest",
        "/static/shell.css",
        "/static/htmx.min.js",
        "/a/sketch/style.css",
        "/a/animals/static/animals.css",
    ] {
        assert_eq!(
            h.handle(lan("GET", path, "*/*")).await.status(),
            200,
            "{path}"
        );
    }
    for path in ["/ws", "/ws/pair"] {
        assert_eq!(h.handle(lan("GET", path, "*/*")).await.status(), 426);
    }
}

#[tokio::test]
async fn test_spec_9_2_bootstrap_page_carries_no_app_data() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let response = h
        .handle(lan("GET", "/a/hello/?q=%22%3E", "text/html"))
        .await;
    assert_eq!(response.headers()["cache-control"], "no-store");
    let html = String::from_utf8(
        to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("<noscript>"));
    assert!(html.contains("integrity=\"sha256-"));
    assert!(!html.contains("_csrf"));
    assert!(!html.contains("cache/"));
    assert!(!html.contains("identity/"));
    // The §7.7 notice, in the owner's words: the exposure is every visit, and a replaced
    // page reads the owner's data.
    assert!(
        html.contains("each time a device opens Privatium this way"),
        "{html}"
    );
    assert!(
        html.contains("swap the page for a fake one and read your data"),
        "{html}"
    );
    let findings = common::a11y::check(&html, common::a11y::Unit::Document);
    assert!(findings.is_empty(), "{findings:?}");
}

#[tokio::test]
async fn test_loopback_keeps_phase_1_semantics() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    let mut req = lan("GET", "/a/hello/", "text/html");
    req.headers_mut()
        .insert("host", "127.0.0.1:8420".parse().unwrap());
    req.extensions_mut()
        .insert(Peer("127.0.0.1:4000".parse().unwrap()));
    let html = String::from_utf8(
        to_bytes(h.handle(req).await.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("<h1>"));
    assert!(!html.contains("data-pv-bootstrap"));
}

#[tokio::test]
async fn test_spec_8_3_page_frame_scripts_carry_integrity() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    for path in ["/", "/a/hello/", "/settings/devices"] {
        let req = axum::http::Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap();
        let html = String::from_utf8(
            to_bytes(h.handle(req).await.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("href=\"/static/shell.css\""));
        assert!(html.contains("src=\"/static/htmx.min.js\""));
        // The frame pins framework assets; the client hashes app assets received
        // through the authenticated channel (spec/protocol.md §8.3).
        for tag in html
            .split('<')
            .filter(|tag| tag.starts_with("script ") || tag.starts_with("link rel=\"stylesheet\""))
        {
            let tag = tag.split('>').next().unwrap();
            if !tag.contains("href=\"/static/") && !tag.contains("src=\"/static/") {
                continue;
            }
            assert!(tag.contains("integrity=\"sha256-"), "{tag}");
        }
    }
}

#[tokio::test]
async fn test_spec_8_4_cross_site_bootstrap_and_forged_device_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let h = handler(&root);
    for path in ["/", "/ws", "/ws/pair", "/api/v1/manifest"] {
        let mut req = lan("GET", path, "text/html");
        req.headers_mut()
            .insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert_eq!(h.handle(req).await.status(), 403);
    }
    let mut req = lan("GET", "/a/hello/api/events", "application/json");
    req.extensions_mut().insert(privatium_core::Device(
        h.node().lock().unwrap().id().clone(),
    ));
    assert_eq!(h.handle(req).await.status(), 403);
}
