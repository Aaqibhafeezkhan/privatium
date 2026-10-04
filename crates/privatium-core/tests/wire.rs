// This file is part of Privatium
// crates/privatium-core/tests/wire.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: core::handle against spec/protocol.md §9 and ADR 0003 — every route reachable with no
//          listener, the headers of §9.3 on every response, nothing leaked unauthenticated
//          (§9.2), solo mode at `/` with the framework prefixes winning (§9.1), Tier 2 served
//          under its own CSP (spec/app-contract.md §5.4), the seed behind a POST (§9), and
//          bodies that stream. Tier 1 routes are tests/lua.rs.
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

use std::collections::BTreeSet;
use std::fs;
use std::net::SocketAddr;
use std::pin::Pin;

use axum::body::{HttpBody as _, to_bytes};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, HOST, IF_NONE_MATCH, LOCATION,
    REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{Method, StatusCode};
use common::a11y::{self, Unit};
use common::{
    APP, event, hand_append, lua_manifest, repo_apps_dir, ts_offset_secs, write_app, write_web_app,
};
use privatium_core::app::Warning;
use privatium_core::http::assets;
use privatium_core::{AppRoot, Body, Handler, LoadReport, Node, Peer, Request, Response};

/// `spec/protocol.md §9.3`, verbatim.
const DEFAULT_CSP: &str = "default-src 'self'; script-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// A node with the three reference apps loaded as bundled, plus whatever `local` holds.
fn open(root: &tempfile::TempDir) -> (Node, LoadReport) {
    let mut node = Node::open(root.path()).unwrap();
    let roots = [
        AppRoot::local(node.paths().apps_dir()),
        AppRoot::bundled(repo_apps_dir()),
    ];
    let report = node.load_apps(&roots).unwrap();
    (node, report)
}

fn handler(root: &tempfile::TempDir) -> Handler {
    let (node, report) = open(root);
    Handler::new(node, report)
}

/// A solo node for `app`, with the reference apps available to it.
fn solo(root: &tempfile::TempDir, app: &str) -> Handler {
    fs::write(
        root.path().join("config.toml"),
        format!("[node]\nmode = \"solo\"\napp = \"{app}\"\n"),
    )
    .unwrap();
    handler(root)
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

fn with_host(mut request: Request, host: &str) -> Request {
    request.headers_mut().insert(HOST, host.parse().unwrap());
    request
}

fn with_peer(mut request: Request, addr: &str) -> Request {
    let addr: SocketAddr = addr.parse().unwrap();
    request.extensions_mut().insert(Peer(addr));
    request
}

async fn body_of(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn header<'a>(response: &'a Response, name: &axum::http::HeaderName) -> &'a str {
    response
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap())
        .unwrap_or_default()
}

/// The paths every test reaches for, with the status a fresh host-mode node answers.
const HOST_ROUTES: &[(&str, u16)] = &[
    ("/api/v1/sync/heads", 403),
    ("/api/v1/sync/pull", 403),
    ("/api/v1/sync/push", 403),
    ("/", 200),
    ("/settings", 200),
    ("/settings/apps", 200),
    ("/settings/data", 200),
    ("/settings/devices", 200),
    ("/api/v1/health", 200),
    ("/api/v1/manifest", 200),
    ("/skills/privatium-overview.md", 200),
    ("/skills/bundle.zip", 200),
    ("/static/shell.css", 200),
    ("/static/htmx.min.js", 200),
    ("/a/sketch/", 200),
    ("/a/sketch/style.css", 200),
    ("/a/hello/", 200),
    ("/a/animals/static/animals.css", 200),
    ("/a/animals/play", 404),
    ("/a/sketch", 308),
    ("/a/nope/", 404),
    ("/a/", 404),
    ("/nope", 404),
    ("/api/nope", 404),
    ("/skills/nope.md", 404),
    ("/static/pv.js", 200),
    ("/a/sketch/api/node", 200),
    ("/a/hello/api/schema", 200),
    ("/a/sketch/api/nope", 404),
    ("/settings/nope", 404),
];

// ---------------------------------------------------------------------------------------
// §9.1 — every prefix, through handle, with no socket
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §9.1`, ADR 0003 — every namespace answers through `handle`, an
/// unknown slug is a 404 and not a panic, and a Tier 1 view renders inside the
/// framework's page frame (`spec/lua-api.md §4.1`).
#[tokio::test]
async fn test_spec_9_1_every_prefix_reachable_through_handle() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    for (path, status) in HOST_ROUTES {
        let response = handler.handle(get(path)).await;
        assert_eq!(response.status().as_u16(), *status, "{path}");
    }
    let redirect = handler.handle(get("/a/sketch")).await;
    assert_eq!(header(&redirect, &LOCATION), "/a/sketch/");
    let tier1 = handler.handle(get("/a/hello/")).await;
    let text = body_of(tier1).await;
    assert!(text.starts_with("<!doctype html>"), "{text}");
    assert!(text.contains("<title>Hello — Privatium</title>"), "{text}");
    assert!(text.contains("We haven't met yet."), "{text}");
    assert!(text.contains("href=\"/a/hello/edit\""), "{text}");

    // HEAD answers like GET without a body; the wrong method says which would work.
    let head = handler.handle(request(Method::HEAD, "/settings")).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(header(&head, &CONTENT_TYPE), "text/html; charset=utf-8");
    assert!(body_of(head).await.is_empty());
    let post = handler.handle(request(Method::POST, "/")).await;
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header(&post, &axum::http::header::ALLOW), "GET, HEAD");
    let delete = handler
        .handle(request(Method::DELETE, "/api/v1/health"))
        .await;
    assert_eq!(delete.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ---------------------------------------------------------------------------------------
// §9.2 — unauthenticated endpoints leak nothing
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §9.2` — health is `{"v":1,"id":"..."}` only; the manifest is the ID,
/// the name, the app index and the pair flag with no counts, timestamps or content; and a
/// caller that is not this machine gets a 403 from every route that says nothing at all
/// (`docs/plans/phase-1.md §2.2`).
#[tokio::test]
async fn test_spec_9_2_unauthenticated_leaks_nothing() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let id = handler.node().lock().unwrap().id().as_str().to_owned();

    let health = handler.handle(get("/api/v1/health")).await;
    assert_eq!(header(&health, &CONTENT_TYPE), "application/json");
    let health: serde_json::Value = serde_json::from_str(&body_of(health).await).unwrap();
    assert_eq!(health, serde_json::json!({ "v": 1, "id": id }));

    let manifest = handler.handle(get("/api/v1/manifest")).await;
    let manifest: serde_json::Value = serde_json::from_str(&body_of(manifest).await).unwrap();
    let keys: BTreeSet<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["v", "id", "name", "apps", "pair"].into_iter().collect()
    );
    assert_eq!(manifest["v"], 1);
    assert_eq!(manifest["id"], id);
    // No display name is set, so the Node ID stands in for it.
    assert_eq!(manifest["name"], id);
    assert_eq!(manifest["pair"], false);
    let apps = manifest["apps"].as_array().unwrap();
    let slugs: Vec<&str> = apps.iter().map(|a| a["slug"].as_str().unwrap()).collect();
    assert_eq!(slugs, ["animals", "hello", "pantry", "sketch"]);
    for app in apps {
        let keys: BTreeSet<&str> = app
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.is_subset(&["slug", "title", "icon"].into_iter().collect()),
            "{keys:?}"
        );
    }
    assert_eq!(apps[3]["title"], "Sketch");
    assert_eq!(apps[3]["icon"], "pencil-square");
    let text = manifest.to_string();
    for forbidden in ["count", "_at", "seq", "lam", "display_name", "profile"] {
        assert!(!text.contains(forbidden), "{forbidden} in {text}");
    }

    // Outside the bootstrap set, an unauthenticated peer sees a pairing refusal (§8.4).
    for (path, _) in HOST_ROUTES {
        if path.starts_with("/static/")
            || matches!(
                *path,
                "/api/v1/health"
                    | "/api/v1/manifest"
                    | "/a/sketch/"
                    | "/a/sketch/style.css"
                    | "/a/animals/static/animals.css"
            )
        {
            continue;
        }
        let response = handler
            .handle(with_peer(get(path), "192.168.1.5:40000"))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        let text = body_of(response).await;
        assert!(text.contains("pair this device"), "{path}: {text}");
        assert!(!text.contains(&id), "{path}: {text}");
        assert!(!text.contains("Sketch"), "{path}: {text}");
    }
    // Loopback with a Host that is not this machine — DNS rebinding — is refused too.
    let rebound = handler
        .handle(with_host(
            with_peer(get("/settings"), "127.0.0.1:40000"),
            "evil.example:8420",
        ))
        .await;
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);
    // Loopback with a loopback Host, or no peer at all (in-process), is this space.
    for request in [
        with_host(
            with_peer(get("/settings"), "127.0.0.1:40000"),
            "localhost:8420",
        ),
        with_peer(get("/settings"), "127.0.0.1:40000"),
        get("/settings"),
    ] {
        assert_eq!(handler.handle(request).await.status(), StatusCode::OK);
    }
}

// ---------------------------------------------------------------------------------------
// §9.3 — headers on every response
// ---------------------------------------------------------------------------------------

/// `spec/protocol.md §9.3` — the CSP, `nosniff` and `no-referrer` on every response, the
/// shell's being the default verbatim; `no-store` on everything but the embedded assets
/// and the skill documents; the shell never relaxes the default for itself.
#[tokio::test]
async fn test_spec_9_3_headers_present() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let mut requests: Vec<Request> = HOST_ROUTES.iter().map(|(path, _)| get(path)).collect();
    requests.push(request(Method::POST, "/"));
    requests.push(request(Method::GET, "/settings/apps/hello/seed"));
    requests.push(with_peer(get("/"), "10.0.0.1:1"));
    for request in requests {
        let path = request.uri().path().to_owned();
        let response = handler.handle(request).await;
        assert_eq!(
            header(&response, &X_CONTENT_TYPE_OPTIONS),
            "nosniff",
            "{path}"
        );
        assert_eq!(header(&response, &REFERRER_POLICY), "no-referrer", "{path}");
        let csp = header(&response, &CONTENT_SECURITY_POLICY).to_owned();
        assert!(!csp.is_empty(), "{path}: no CSP");
        let cacheable = path.starts_with("/static/") || path.starts_with("/skills/");
        let cache = header(&response, &CACHE_CONTROL);
        if cacheable && response.status() == StatusCode::OK {
            assert_eq!(cache, "no-cache", "{path}");
        } else {
            assert_eq!(cache, "no-store", "{path}");
        }
        if !path.starts_with("/a/") {
            assert_eq!(csp, DEFAULT_CSP, "{path}");
        }
    }

    // The shell's own pages carry no inline script or style, which is what lets §9.3
    // apply to them exactly as written.
    for path in [
        "/",
        "/settings",
        "/settings/apps",
        "/settings/data",
        "/settings/devices",
    ] {
        let html = body_of(handler.handle(get(path)).await).await;
        assert!(!html.contains("<script>"), "{path}: inline script");
        assert!(!html.contains(" style=\""), "{path}: inline style");
        assert!(!html.contains(" onclick="), "{path}: inline handler");
        assert!(html.contains(&assets::versioned("htmx.min.js")), "{path}");
        assert!(html.contains(&assets::versioned("shell.css")), "{path}");
        assert!(html.contains("<html lang=\"en\">"), "{path}");
    }
}

/// `spec/app-contract.md §5.4` — an app response carries exactly
/// `App::csp().header_for(origin)`, rendered against the request's `Host`, and never the
/// load-time `header()`.
#[tokio::test]
async fn test_app_response_carries_header_for_origin() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let (expected_host, expected_default, load_time) = {
        let node = handler.node().lock().unwrap();
        let sketch = node.app("sketch").unwrap();
        (
            sketch.csp().header_for("http://localhost:9999"),
            sketch.csp().header_for("http://127.0.0.1:8420"),
            sketch.csp().header().to_owned(),
        )
    };
    let with = handler
        .handle(with_host(get("/a/sketch/"), "localhost:9999"))
        .await;
    assert_eq!(header(&with, &CONTENT_SECURITY_POLICY), expected_host);
    assert!(
        expected_host
            .contains("script-src http://localhost:9999/a/sketch/ http://localhost:9999/static/")
    );
    assert_ne!(expected_host, load_time);

    // No Host, or one that could not go into a header: the node's own loopback origin.
    let without = handler.handle(get("/a/sketch/")).await;
    assert_eq!(header(&without, &CONTENT_SECURITY_POLICY), expected_default);
    let injected = handler
        .handle(with_host(get("/a/sketch/"), "x; script-src *"))
        .await;
    assert_eq!(
        header(&injected, &CONTENT_SECURITY_POLICY),
        expected_default
    );

    // Every response beneath the mount, including a 404 and a Tier 1 page, and no-store.
    for path in ["/a/sketch/style.css", "/a/sketch/missing.js", "/a/hello/"] {
        let response = handler.handle(get(path)).await;
        let csp = header(&response, &CONTENT_SECURITY_POLICY).to_owned();
        assert!(
            csp.contains("script-src http://127.0.0.1:8420/a/"),
            "{path}: {csp}"
        );
        assert!(
            csp.contains("http://127.0.0.1:8420/static/"),
            "{path}: {csp}"
        );
        assert_eq!(header(&response, &CACHE_CONTROL), "no-store", "{path}");
    }
}

// ---------------------------------------------------------------------------------------
// §5 — Tier 2 served as-is
// ---------------------------------------------------------------------------------------

/// `spec/app-contract.md §5` — `web/index.html` at the mount point, everything under
/// `web/` as-is with its content type, and nothing outside `web/` however the path is
/// spelled. The body of a large file arrives in frames, not as one buffer.
#[tokio::test]
async fn test_tier2_index_at_mount_and_nothing_outside_web() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let web = repo_apps_dir().join("sketch").join("web");

    let index = handler.handle(get("/a/sketch/")).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert!(header(&index, &CONTENT_TYPE).starts_with("text/html"));
    assert_eq!(
        body_of(index).await,
        fs::read_to_string(web.join("index.html")).unwrap()
    );
    let css = handler.handle(get("/a/sketch/style.css")).await;
    assert!(
        header(&css, &CONTENT_TYPE).starts_with("text/css"),
        "{}",
        header(&css, &CONTENT_TYPE)
    );
    assert_eq!(
        body_of(css).await,
        fs::read_to_string(web.join("style.css")).unwrap()
    );
    let js = handler.handle(get("/a/sketch/app.js")).await;
    assert!(header(&js, &CONTENT_TYPE).contains("javascript"));

    for outside in [
        "/a/sketch/app.toml",
        "/a/sketch/../app.toml",
        "/a/sketch/%2e%2e/app.toml",
        "/a/sketch/..%2fapp.toml",
        "/a/sketch/README.md",
        "/a/sketch/../../Cargo.toml",
    ] {
        let response = handler.handle(get(outside)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{outside}");
        let text = body_of(response).await;
        assert!(!text.contains("[app]"), "{outside} leaked the manifest");
        assert!(!text.contains("[workspace]"), "{outside}");
    }
}

/// ADR 0003 — `Response` bodies are streams. A 1 MiB file comes back as many frames of at
/// most 64 KiB, the first of them before the rest was read.
#[tokio::test]
async fn test_response_body_streams_without_buffering() {
    let root = tempfile::tempdir().unwrap();
    let apps = Node::open(root.path()).unwrap().paths().apps_dir();
    let big = vec![b'x'; 1024 * 1024];
    let dir = write_web_app(&apps, "big", &[]);
    fs::write(dir.join("web").join("big.bin"), &big).unwrap();
    let handler = handler(&root);

    let response = handler.handle(get("/a/big/big.bin")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut sizes = Vec::new();
    loop {
        let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await;
        match frame {
            None => break,
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    sizes.push(data.len());
                }
            }
            Some(Err(error)) => panic!("{error}"),
        }
    }
    assert_eq!(sizes.iter().sum::<usize>(), big.len());
    assert!(sizes.len() >= 16, "{} frames", sizes.len());
    assert!(sizes.iter().all(|s| *s <= 64 * 1024), "{sizes:?}");
}

// ---------------------------------------------------------------------------------------
// §9.1 — solo mode
// ---------------------------------------------------------------------------------------

/// `spec/app-contract.md §2.2`, `spec/protocol.md §9.1` — the solo app owns `/`, with no
/// `/a/<slug>/` prefix, and in solo mode `header_for` is the default policy because the
/// app is the origin.
#[tokio::test]
async fn test_solo_mode_mounts_at_root() {
    let root = tempfile::tempdir().unwrap();
    let handler = solo(&root, "sketch");
    let web = repo_apps_dir().join("sketch").join("web");

    let index = handler.handle(with_host(get("/"), "127.0.0.1:8420")).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert_eq!(
        body_of(index).await,
        fs::read_to_string(web.join("index.html")).unwrap()
    );
    let css = handler.handle(get("/style.css")).await;
    assert_eq!(css.status(), StatusCode::OK);
    assert_eq!(header(&css, &CONTENT_SECURITY_POLICY), DEFAULT_CSP);
    {
        let node = handler.node().lock().unwrap();
        let sketch = node.app("sketch").unwrap();
        assert_eq!(sketch.mount(), Some("/"));
        assert_eq!(
            sketch.csp().header_for("http://127.0.0.1:8420"),
            DEFAULT_CSP
        );
        assert!(node.app("hello").unwrap().mount().is_none());
    }
    // The manifest indexes the mounted app only.
    let manifest: serde_json::Value =
        serde_json::from_str(&body_of(handler.handle(get("/api/v1/manifest")).await).await)
            .unwrap();
    assert_eq!(manifest["apps"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["apps"][0]["slug"], "sketch");
}

/// `spec/app-contract.md §5.4`, `spec/protocol.md §9.3` — the solo app's
/// `permissions.cross_origin_isolated` is honoured, not merely accepted: every response
/// of the origin carries `Cross-Origin-Opener-Policy: same-origin` and
/// `Cross-Origin-Embedder-Policy: require-corp`, the framework's own routes included,
/// since both are document-level and the solo app owns the origin. Without the
/// permission neither header appears.
#[tokio::test]
async fn test_spec_app_contract_5_4_cross_origin_isolated_headers_in_solo_mode() {
    let manifest = format!(
        "{}\n[permissions]\ncross_origin_isolated = true\n",
        common::web_manifest("iso")
    );
    let root = tempfile::tempdir().unwrap();
    write_app(
        &root.path().join("apps"),
        "iso",
        Some(&manifest),
        &[("web/index.html", "<!doctype html><title>iso</title>\n")],
    );
    let handler = solo(&root, "iso");
    for path in [
        "/",
        "/api/node",
        "/api/v1/health",
        "/settings",
        "/static/pv.js",
        "/nope",
    ] {
        let response = handler.handle(get(path)).await;
        let headers = response.headers();
        assert_eq!(
            headers
                .get("cross-origin-opener-policy")
                .map(|v| v.to_str().unwrap()),
            Some("same-origin"),
            "{path}: {headers:?}"
        );
        assert_eq!(
            headers
                .get("cross-origin-embedder-policy")
                .map(|v| v.to_str().unwrap()),
            Some("require-corp"),
            "{path}"
        );
    }

    // The same app without the permission: nothing isolates.
    let plain = tempfile::tempdir().unwrap();
    write_web_app(&plain.path().join("apps"), "iso", &[]);
    let handler = solo(&plain, "iso");
    for path in ["/", "/api/node"] {
        let response = handler.handle(get(path)).await;
        assert!(
            response
                .headers()
                .get("cross-origin-opener-policy")
                .is_none(),
            "{path}"
        );
        assert!(
            response
                .headers()
                .get("cross-origin-embedder-policy")
                .is_none(),
            "{path}"
        );
    }
}

/// `spec/app-contract.md §2.2` — no launcher and no `/a/<slug>/` in solo mode; the shell's
/// own pages drop the launcher link.
#[tokio::test]
async fn test_launcher_absent_in_solo_mode() {
    let root = tempfile::tempdir().unwrap();
    let handler = solo(&root, "sketch");
    let index = body_of(handler.handle(get("/")).await).await;
    assert!(!index.contains("pv-launcher"), "{index}");
    assert!(
        !index.contains("pv-header"),
        "the shell leaked into the app: {index}"
    );
    // `/a/sketch/` is a path inside the solo app's `web/`, where nothing lives.
    let prefixed = handler.handle(get("/a/sketch/")).await;
    assert_eq!(prefixed.status(), StatusCode::NOT_FOUND);
    let settings = body_of(handler.handle(get("/settings")).await).await;
    assert!(!settings.contains("</svg> Apps</a>"), "{settings}");
    assert!(settings.contains("solo"), "{settings}");
}

/// `spec/protocol.md §9.1` — framework prefixes take precedence in solo mode, and each
/// shadowed route is a load-time warning naming the route and the prefix.
#[tokio::test]
async fn test_solo_mode_framework_prefix_wins() {
    let root = tempfile::tempdir().unwrap();
    let apps = Node::open(root.path()).unwrap().paths().apps_dir();
    write_web_app(
        &apps,
        "solo",
        &[
            ("web/settings/index.html", "<p>the app's settings</p>"),
            ("web/static/x.js", "alert(1)"),
            ("web/api/v1/health", "{\"fake\":true}"),
            ("web/play/index.html", "<p>play</p>"),
        ],
    );
    let handler = solo(&root, "solo");

    let settings = handler.handle(get("/settings")).await;
    assert_eq!(settings.status(), StatusCode::OK);
    let text = body_of(settings).await;
    assert!(text.contains("Settings"), "{text}");
    assert!(!text.contains("the app's settings"), "{text}");
    assert_eq!(
        handler.handle(get("/static/x.js")).await.status(),
        StatusCode::NOT_FOUND
    );
    let health = body_of(handler.handle(get("/api/v1/health")).await).await;
    let health: serde_json::Value = serde_json::from_str(&health).unwrap();
    assert_eq!(health["v"], 1, "{health}");
    assert!(health.get("fake").is_none(), "{health}");
    // The app's own routes are its.
    assert_eq!(handler.handle(get("/")).await.status(), StatusCode::OK);
    assert_eq!(handler.handle(get("/play/")).await.status(), StatusCode::OK);

    let shadowed: Vec<(String, &str)> = handler
        .report()
        .warnings
        .iter()
        .filter_map(|w| match w {
            Warning::RouteShadowed {
                slug,
                route,
                prefix,
            } if slug == "solo" => Some((route.clone(), *prefix)),
            _ => None,
        })
        .collect();
    assert_eq!(
        shadowed,
        [
            ("/api".to_owned(), "/api"),
            ("/settings".to_owned(), "/settings"),
            ("/static".to_owned(), "/static"),
        ]
    );
    let text = handler.report().warnings[0].to_string();
    assert!(text.contains("shadowed by the framework prefix"), "{text}");
    // The settings page surfaces them.
    let apps_page = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(
        apps_page.contains("shadowed by the framework prefix /settings"),
        "{apps_page}"
    );
}

// ---------------------------------------------------------------------------------------
// The read path — `echo >>` and the seed
// ---------------------------------------------------------------------------------------

/// `apps/hello/README.md` — a line appended by hand is visible on the next request, with
/// no restart: `refresh_app` runs per request — a stat, then a rebuild when stale.
#[tokio::test]
async fn test_hand_appended_line_visible_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let before = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(before.contains("<code>profile</code>: 0 rows"), "{before}");

    let (path, dev) = {
        let node = handler.node().lock().unwrap();
        (
            node.paths().app_log(APP, node.id()),
            node.id().as_str().to_owned(),
        )
    };
    let ts = ts_offset_secs(-1);
    hand_append(
        &path,
        &event(
            1,
            1,
            &ts,
            &dev,
            "profile",
            "a",
            Some(r#"{"display_name":"Someone Else"}"#),
        ),
        "\n",
    );

    let after = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(after.contains("<code>profile</code>: 1 row "), "{after}");
    assert!(!after.contains("<code>profile</code>: 0 rows"), "{after}");
}

/// `spec/app-contract.md §9` — the seed offer is shown for a loaded app with an empty log,
/// and only a POST carrying `csrf()` loads it; a GET does nothing, a POST without the token
/// does nothing, and a second POST is refused because the log now holds events.
#[tokio::test]
async fn test_seed_offer_shown_and_only_a_post_loads_it() {
    let root = tempfile::tempdir().unwrap();
    let apps = Node::open(root.path()).unwrap().paths().apps_dir();
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
                "{\"op\":\"put\",\"tbl\":\"profile\",\"id\":\"a\",\"d\":{\"display_name\":\"Ada\"}}\n\
                 {\"op\":\"put\",\"tbl\":\"profile\",\"id\":\"b\",\"d\":{\"display_name\":\"Grace\"}}\n",
            ),
        ],
    );
    let handler = handler(&root);
    let action = "/settings/apps/seeded/seed";
    let log = {
        let node = handler.node().lock().unwrap();
        node.paths().app_log("seeded", node.id())
    };

    let page = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(page.contains(&format!("action=\"{action}\"")), "{page}");
    assert!(page.contains("name=\"_csrf\""), "{page}");
    assert!(page.contains("Load sample data"), "{page}");
    // Of the reference apps animals and pantry ship a seed, so exactly three offers
    // appear — this app's and theirs — and nothing is offered for hello or sketch.
    assert_eq!(page.matches("Load sample data").count(), 3, "{page}");
    assert!(
        page.contains("action=\"/settings/apps/animals/seed\""),
        "{page}"
    );
    assert!(
        page.contains("action=\"/settings/apps/pantry/seed\""),
        "{page}"
    );
    assert!(!page.contains("/settings/apps/hello/seed"), "{page}");
    assert!(!page.contains("/settings/apps/sketch/seed"), "{page}");

    // A GET is not the act.
    assert_eq!(
        handler.handle(get(action)).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "");

    // A POST without the token is not the act either.
    let post = |body: String| {
        axum::http::Request::builder()
            .method(Method::POST)
            .uri(action)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap()
    };
    let forbidden = handler.handle(post("_csrf=nope".into())).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(fs::read_to_string(&log).unwrap_or_default(), "");
    let other = handler.csrf().token("/settings/apps/other/seed");
    let forbidden = handler.handle(post(format!("_csrf={other}"))).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // The act.
    let token = handler.csrf().token(action);
    let seeded = handler.handle(post(format!("_csrf={token}"))).await;
    assert_eq!(seeded.status(), StatusCode::SEE_OTHER);
    assert_eq!(header(&seeded, &LOCATION), "/settings/apps#app-seeded");
    let lines: Vec<serde_json::Value> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["d"]["display_name"], "Ada");
    let page = body_of(handler.handle(get("/settings/apps")).await).await;
    assert!(page.contains("<code>profile</code>: 2 rows"), "{page}");
    // This app's offer is gone; animals' and pantry's, whose logs are still empty, remain.
    assert!(!page.contains(&format!("action=\"{action}\"")), "{page}");
    assert_eq!(page.matches("Load sample data").count(), 2, "{page}");

    // Never over existing events.
    let refused = handler.handle(post(format!("_csrf={token}"))).await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let text = body_of(refused).await;
    assert!(text.contains("already holds 2 event"), "{text}");
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
}

// ---------------------------------------------------------------------------------------
// The shell
// ---------------------------------------------------------------------------------------

/// `spec/data-dictionary.md §3.4` — an app whose folder is gone stays in the launcher as
/// unavailable, with the reason, rather than pretending it is gone.
#[tokio::test]
async fn test_launcher_shows_a_missing_folder_as_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let apps = Node::open(root.path()).unwrap().paths().apps_dir();
    let dir = write_web_app(&apps, "gone", &[]);
    {
        let (node, _) = open(&root);
        assert!(node.app("gone").is_some());
    }
    fs::remove_dir_all(&dir).unwrap();
    let handler = handler(&root);
    assert!(handler.report().missing.contains(&"gone".to_owned()));

    let launcher = body_of(handler.handle(get("/")).await).await;
    assert!(launcher.contains("pv-launcher"), "{launcher}");
    assert!(launcher.contains("href=\"/a/sketch/\""), "{launcher}");
    assert!(
        launcher.contains("gone — unavailable: folder missing"),
        "{launcher}"
    );
    assert!(!launcher.contains("href=\"/a/gone/\""), "{launcher}");
    assert_eq!(
        handler.handle(get("/a/gone/")).await.status(),
        StatusCode::NOT_FOUND
    );
    // Icons are inlined SVG with the attributes docs/icons.md requires, never a font.
    assert!(launcher.contains("<svg class=\"pv-icon\""), "{launcher}");
    assert!(launcher.contains("focusable=\"false\""));
    assert!(
        !launcher.contains("bi-"),
        "a vendored class leaked: {launcher}"
    );
}

/// The four settings pages: identity, the installed apps with their warnings and
/// errors, the data directory with backup instructions, and this space's own device row
/// beside the offer to pair another.
#[tokio::test]
async fn test_settings_pages_render_the_node() {
    let root = tempfile::tempdir().unwrap();
    let apps = Node::open(root.path()).unwrap().paths().apps_dir();
    // A permission widening and an icon the set lacks, both surfaced on the apps page.
    write_app(
        &apps,
        "wide",
        Some(&format!(
            "{}icon = \"no-such-icon\"\n[permissions]\nsql = true\n",
            lua_manifest("wide")
        )),
        &[("app.lua", "")],
    );
    // A broken folder, refused loudly.
    write_app(&apps, "broken", Some("not toml at all ["), &[]);
    let handler = handler(&root);
    let (id, data_dir) = {
        let node = handler.node().lock().unwrap();
        (
            node.id().as_str().to_owned(),
            node.paths().data_dir().display().to_string(),
        )
    };

    let node_page = body_of(handler.handle(get("/settings")).await).await;
    assert!(node_page.contains(&id), "{node_page}");
    assert!(node_page.contains("pv/1"), "{node_page}");
    assert!(node_page.contains(">host<"), "{node_page}");
    assert!(node_page.contains("No alerts"), "{node_page}");

    let apps_page = body_of(handler.handle(get("/settings/apps")).await).await;
    for expected in [
        "id=\"app-hello\"",
        "id=\"app-sketch\"",
        "href=\"/a/sketch/\"",
        "ad-hoc read-only SQL",
        "not in the vendored Bootstrap Icons set",
        "Not loaded at startup",
        "<code>broken</code>",
        "web (web app)",
        "no schema.sql — the event log is the store",
    ] {
        assert!(apps_page.contains(expected), "{expected}\n{apps_page}");
    }

    let data_page = body_of(handler.handle(get("/settings/data")).await).await;
    assert!(
        data_page.contains(&privatium_core::icons::escape(&data_dir)),
        "{data_page}"
    );
    assert!(
        data_page.contains("simply copy the <a href=\"file:///"),
        "the data folder is a link: {data_page}"
    );
    assert!(data_page.contains(">Backup and Restore</a>"), "{data_page}");

    let devices = body_of(handler.handle(get("/settings/devices")).await).await;
    assert!(devices.contains(&id), "{devices}");
    assert!(devices.contains("this space"), "{devices}");
    assert!(devices.contains("Pair a device"), "{devices}");
}

/// `spec/protocol.md §8.4` — a request from this machine's own interface address is the
/// owner's, exactly as loopback is: a browser opened on the node's LAN address from the
/// node's own keyboard sees the settings page and no pairing screen. The `Host` rule
/// still holds — a name that is not this machine's is refused — and a peer on another
/// machine still gets the bootstrap set alone.
#[tokio::test]
async fn test_spec_8_4_a_request_from_this_machines_own_address_is_the_owner() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let own: Vec<std::net::IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|interface| interface.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .collect();
    let page = |peer: std::net::IpAddr, host: &str| {
        let mut request = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/settings")
            .header("host", host)
            .header("accept", "text/html")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(Peer(SocketAddr::new(peer, 4000)));
        request
    };
    for ip in &own {
        let host = match ip {
            std::net::IpAddr::V4(v4) => format!("{v4}:8420"),
            std::net::IpAddr::V6(v6) => format!("[{v6}]:8420"),
        };
        let response = handler.handle(page(*ip, &host)).await;
        assert_eq!(response.status(), StatusCode::OK, "{ip}");
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(
            body.contains("<h2>Settings</h2>"),
            "{ip}: the page, not the bootstrap"
        );
        assert!(!body.contains("id=\"pv-pair\""), "{ip}");
        // The same address with a Host that is not this machine's: DNS rebinding.
        let rebound = handler.handle(page(*ip, "attacker.example:8420")).await;
        assert_eq!(rebound.status(), StatusCode::FORBIDDEN, "{ip}");
        let named = handler.handle(page(*ip, "localhost:8420")).await;
        assert_eq!(
            named.status(),
            StatusCode::OK,
            "{ip}: loopback names are this machine"
        );
    }
    // A peer that is not this machine — TEST-NET-1 — gets the bootstrap, whatever it
    // sends as Host.
    let stranger = handler
        .handle(page("192.0.2.10".parse().unwrap(), "192.0.2.1:8420"))
        .await;
    let body = String::from_utf8(
        to_bytes(stranger.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("id=\"pv-pair\""),
        "a stranger gets the bootstrap"
    );
    assert!(!body.contains("<h2>Settings</h2>"));
    assert!(
        privatium_core::http::auth::is_this_machine("127.0.0.1".parse().unwrap())
            && !privatium_core::http::auth::is_this_machine("192.0.2.10".parse().unwrap())
    );
    assert!(privatium_core::http::auth::host_names_this_machine(
        "localhost:8420"
    ));
    assert!(privatium_core::http::auth::host_names_this_machine("[::1]"));
    assert!(!privatium_core::http::auth::host_names_this_machine(
        "192.0.2.10:8420"
    ));
    assert!(!privatium_core::http::auth::host_names_this_machine(
        "attacker.example"
    ));
}

/// `spec/protocol.md §9.3`: an embedded asset carries a strong ETag and answers 304 to a
/// matching `If-None-Match` on its fixed path, and the content-addressed path the
/// framework's own pages use is fresh for a day. A prefix that is not this build's is
/// not an asset at all.
#[tokio::test]
async fn test_spec_9_3_assets_revalidate_by_etag_and_the_addressed_path_caches_for_a_day() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);

    let fixed = handler.handle(get("/static/pv.js")).await;
    assert_eq!(fixed.status(), StatusCode::OK);
    assert_eq!(header(&fixed, &CACHE_CONTROL), "no-cache");
    let etag = header(&fixed, &ETAG).to_owned();
    assert_eq!(
        etag,
        format!("\"{}\"", assets::integrity("pv.js")),
        "the ETag is the integrity hash in quotes"
    );

    for value in [
        etag.clone(),
        format!("W/{etag}"),
        format!("\"other\", {etag}"),
        "*".into(),
    ] {
        let mut request = get("/static/pv.js");
        request
            .headers_mut()
            .insert(IF_NONE_MATCH, value.parse().unwrap());
        let response = handler.handle(request).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{value}");
        assert_eq!(header(&response, &ETAG), etag, "{value}");
        assert_eq!(header(&response, &CACHE_CONTROL), "no-cache", "{value}");
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty(),
            "{value}"
        );
    }
    let mut stale = get("/static/pv.js");
    stale
        .headers_mut()
        .insert(IF_NONE_MATCH, "\"sha256-stale\"".parse().unwrap());
    assert_eq!(handler.handle(stale).await.status(), StatusCode::OK);

    let addressed = handler.handle(get(&assets::versioned("pv.js"))).await;
    assert_eq!(addressed.status(), StatusCode::OK);
    assert_eq!(
        header(&addressed, &CACHE_CONTROL),
        "public, max-age=86400, immutable"
    );
    assert_eq!(header(&addressed, &ETAG), etag);
    assert_eq!(
        header(&addressed, &CONTENT_TYPE),
        header(&fixed, &CONTENT_TYPE)
    );
    let logo = handler
        .handle(get(&assets::versioned("privatium-logo-light.svg")))
        .await;
    assert_eq!(logo.status(), StatusCode::OK);
    assert_eq!(
        header(&logo, &CACHE_CONTROL),
        "public, max-age=86400, immutable"
    );

    // The skill documents and the bundle revalidate the same way.
    for path in ["/skills/privatium-overview.md", "/skills/bundle.zip"] {
        let first = handler.handle(get(path)).await;
        assert_eq!(first.status(), StatusCode::OK, "{path}");
        assert_eq!(header(&first, &CACHE_CONTROL), "no-cache", "{path}");
        let tag = header(&first, &ETAG).to_owned();
        assert!(tag.starts_with("\"sha256-"), "{path}: {tag}");
        let mut again = get(path);
        again
            .headers_mut()
            .insert(IF_NONE_MATCH, tag.parse().unwrap());
        let response = handler.handle(again).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{path}");
        assert_eq!(header(&response, &ETAG), tag, "{path}");
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }

    // Another build's prefix is refused, never answered from this build's bytes.
    let other = handler.handle(get("/static/0123456789abcdef/pv.js")).await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    assert_eq!(header(&other, &CACHE_CONTROL), "no-store");

    // The framework's own pages name assets under this build's prefix with integrity.
    let launcher = body_of(handler.handle(get("/")).await).await;
    assert!(
        launcher.contains(&format!("href=\"{}\"", assets::versioned("shell.css"))),
        "{launcher}"
    );
    assert!(
        launcher.contains(&format!(
            "src=\"{}\"",
            assets::versioned("privatium-logo-light.svg")
        )),
        "{launcher}"
    );
}

// ---------------------------------------------------------------------------------------
// spec/lua-api.md §4.1, spec/app-contract.md §3 — the standard chrome around a view
// ---------------------------------------------------------------------------------------

/// `spec/lua-api.md §4.1` — the frame's header is three zones: the brand linking to `/`,
/// the app's title with its icon linking to the mount, and the controls; the title is a
/// paragraph so the view keeps the one `<h1>`; the body names the mount; the head loads
/// both stylesheets and the chrome script at addressed paths with their hashes. The
/// shell's own pages keep the brand as their `<h1>` and show no title zone.
#[tokio::test]
async fn test_spec_4_1_frame_renders_three_zones_and_the_title_links_to_the_mount() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    let page = body_of(handler.handle(get("/a/hello/")).await).await;
    assert!(page.contains("<title>Hello — Privatium</title>"), "{page}");
    assert!(
        page.contains("<body data-pv-mount=\"/a/hello/\" hx-headers="),
        "{page}"
    );
    let header = page
        .split("<header class=\"pv-header\">")
        .nth(1)
        .and_then(|rest| rest.split("</header>").next())
        .unwrap();
    let brand = header.find("<p class=\"pv-brand\"><a href=\"/\">").unwrap();
    let title = header
        .find("<p class=\"pv-app-title\"><a href=\"/a/hello/\">")
        .unwrap();
    let controls = header
        .find("<nav class=\"pv-controls\" aria-label=\"Framework\">")
        .unwrap();
    assert!(brand < title && title < controls, "{header}");
    let title_zone = &header[title..controls];
    assert!(
        title_zone.contains("<svg"),
        "the manifest's icon: {title_zone}"
    );
    assert!(
        title_zone.contains("<span>Hello</span></a></p>"),
        "{title_zone}"
    );
    assert!(
        !title_zone.contains("<h"),
        "the title is not a heading: {title_zone}"
    );
    assert!(
        header.contains("<picture><source media=\"(max-width: 30rem)\""),
        "{header}"
    );
    assert!(header.contains("<a href=\"/\">"), "{header}");
    assert!(header.contains("Apps</a>"), "{header}");
    for asset in ["chrome.css", "shell.css", "chrome.js"] {
        assert!(
            page.contains(&format!(
                "\"{}\" integrity=\"{}\"",
                assets::versioned(asset),
                assets::integrity(asset)
            )),
            "{asset}: {page}"
        );
    }
    assert!(
        page.contains("<p id=\"pv-status\" class=\"pv-status\" role=\"status\"></p>"),
        "{page}"
    );
    let findings = a11y::check(&page, Unit::Document);
    assert!(findings.is_empty(), "{findings:?}");

    let launcher = body_of(handler.handle(get("/")).await).await;
    assert!(
        launcher.contains("<h1 class=\"pv-brand\"><a href=\"/\">"),
        "{launcher}"
    );
    assert!(!launcher.contains("pv-app-title"), "{launcher}");
    assert!(!launcher.contains("data-pv-mount"), "{launcher}");
    assert!(
        launcher.contains("<p id=\"pv-status\" class=\"pv-status\" role=\"status\"></p>"),
        "{launcher}"
    );
}

/// `spec/lua-api.md §4.1`, `spec/app-contract.md §3` — the one menu lists the manifest's
/// `[[ui.menu]]` items, then what the view added with `menu()`, then a rule, then the
/// framework's four pages in order; the launcher is not among them, because the header
/// carries the Apps link beside the menu. Paths resolve through `url()`.
#[tokio::test]
async fn test_spec_4_1_menu_lists_app_items_then_a_separator_then_system_pages_without_apps() {
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("apps");
    write_app(
        &apps,
        "menued",
        Some(&format!(
            "{}[[ui.menu]]\nlabel = \"Setup\"\npath = \"/setup\"\nicon = \"gear\"\n\
             [[ui.menu]]\nlabel = \" About \"\npath = \"/about?x=1\"\n",
            lua_manifest("menued")
        )),
        &[
            (
                "app.lua",
                "local pv = require 'privatium'\npv.get('/', function() return pv.render('index') end)\n\
                 pv.get('/plain', function() return pv.render('plain') end)\n",
            ),
            (
                "views/index.lsp",
                "<? menu('Print list', '/print') ?><h1>Home</h1>",
            ),
            ("views/plain.lsp", "<h1>Plain</h1>"),
        ],
    );
    let handler = handler(&root);
    let page = body_of(handler.handle(get("/a/menued/")).await).await;
    let menu = page
        .split("<ul id=\"pv-app-menu\" class=\"pv-menu-app\">")
        .nth(1)
        .and_then(|rest| rest.split("</details>").next())
        .unwrap();
    let positions: Vec<usize> = [
        "<li><a href=\"/a/menued/setup\"><svg",
        "Setup</a></li>",
        "<li><a href=\"/a/menued/about?x=1\">About</a></li>",
        "<li><a href=\"/a/menued/print\">Print list</a></li></ul>",
        "<hr class=\"pv-menu-rule\">",
        "<ul class=\"pv-menu-system\"><li><a href=\"/settings\">Space settings</a></li>",
        "<li><a href=\"/settings/apps\">App settings</a></li>",
        "<li><a href=\"/settings/data\">Data settings</a></li>",
        "<li><a href=\"/settings/devices\">Devices</a></li></ul>",
    ]
    .iter()
    .map(|needle| {
        menu.find(needle)
            .unwrap_or_else(|| panic!("{needle}\n{menu}"))
    })
    .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{menu}");
    assert!(
        !menu.contains("href=\"/\""),
        "no Apps entry in the menu: {menu}"
    );
    assert_eq!(
        page.matches("Apps</a>").count(),
        1,
        "the header's Apps link alone"
    );
    let findings = a11y::check(&page, Unit::Document);
    assert!(findings.is_empty(), "{findings:?}");

    // The page item belongs to its page.
    let plain = body_of(handler.handle(get("/a/menued/plain")).await).await;
    assert!(plain.contains("About</a></li></ul>"), "{plain}");
    assert!(!plain.contains("Print list"), "{plain}");
    // The launcher has the slot, empty, so a script may still fill it.
    let launcher = body_of(handler.handle(get("/")).await).await;
    assert!(
        launcher.contains("<ul id=\"pv-app-menu\" class=\"pv-menu-app\"></ul>"),
        "{launcher}"
    );
}

/// `spec/lua-api.md §4.1` — in solo mode there is no launcher: the bar has no Apps link,
/// the title links to `/`, and the menu holds the framework's pages alone. The rule
/// between the two lists is present but hidden by the stylesheet while the app's list
/// is empty, so an item a script appends later shows it without a page change.
#[tokio::test]
async fn test_spec_4_1_solo_mode_frame_has_no_apps_link_and_no_separator_without_app_items() {
    let root = tempfile::tempdir().unwrap();
    let handler = solo(&root, "hello");
    let page = body_of(handler.handle(get("/")).await).await;
    assert!(
        page.contains("<body data-pv-mount=\"/\" hx-headers="),
        "{page}"
    );
    assert!(
        page.contains("<p class=\"pv-app-title\"><a href=\"/\">"),
        "{page}"
    );
    assert!(!page.contains("Apps</a>"), "{page}");
    assert!(
        page.contains(
            "<ul id=\"pv-app-menu\" class=\"pv-menu-app\"></ul>\n<hr class=\"pv-menu-rule\">\n<ul class=\"pv-menu-system\">"
        ),
        "{page}"
    );
    assert_eq!(page.matches("<li><a href=\"/settings").count(), 4, "{page}");
    let css = std::str::from_utf8(assets::get("chrome.css").unwrap().bytes).unwrap();
    assert!(
        css.contains("#pv-app-menu:empty + .pv-menu-rule { display: none; }"),
        "the rule hides itself while the app's list is empty"
    );
    let findings = a11y::check(&page, Unit::Document);
    assert!(findings.is_empty(), "{findings:?}");
}

/// `spec/app-contract.md §3` — `ui.styles` and `ui.scripts` load in the frame's head on
/// every page, the scripts deferred, each with the hash of the file as it is on disk, so
/// an edit is served with its new hash on the next request; a fragment request gets no
/// head at all.
#[tokio::test]
async fn test_spec_3_ui_scripts_and_styles_load_in_the_frame_head_with_defer_and_integrity() {
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("apps");
    let dir = write_app(
        &apps,
        "assetful",
        Some(&format!(
            "{}[ui]\nscripts = [\"static/app.js\"]\nstyles = [\"static/app.css\"]\n",
            lua_manifest("assetful")
        )),
        &[
            (
                "app.lua",
                "local pv = require 'privatium'\npv.get('/', function() return pv.render('index') end)\n",
            ),
            ("views/index.lsp", "<h1>Home</h1>"),
            ("static/app.js", "console.log('one');\n"),
            ("static/app.css", "h1 { margin: 0; }\n"),
        ],
    );
    let handler = handler(&root);
    let page = body_of(handler.handle(get("/a/assetful/")).await).await;
    let head = page.split("</head>").next().unwrap();
    let css_hash = assets::integrity_of(b"h1 { margin: 0; }\n");
    let js_hash = assets::integrity_of(b"console.log('one');\n");
    assert!(
        head.contains(&format!(
            "<link rel=\"stylesheet\" href=\"/a/assetful/static/app.css\" integrity=\"{css_hash}\">"
        )),
        "{head}"
    );
    assert!(
        head.contains(&format!(
            "<script src=\"/a/assetful/static/app.js\" integrity=\"{js_hash}\" defer></script>"
        )),
        "{head}"
    );
    assert!(
        head.find("shell.css").unwrap() < head.find("static/app.css").unwrap(),
        "the app's sheet comes after the framework's, so its rules win: {head}"
    );

    // Edited on disk: the next page names the new bytes.
    fs::write(dir.join("static/app.js"), "console.log('two');\n").unwrap();
    let again = body_of(handler.handle(get("/a/assetful/")).await).await;
    assert!(
        again.contains(&assets::integrity_of(b"console.log('two');\n")),
        "{again}"
    );
    assert!(!again.contains(&js_hash), "{again}");

    // A fragment is the view alone.
    let mut fragment = get("/a/assetful/");
    fragment
        .headers_mut()
        .insert("hx-request", "true".parse().unwrap());
    let fragment = body_of(handler.handle(fragment).await).await;
    assert_eq!(fragment.trim(), "<h1>Home</h1>");
}

/// `spec/protocol.md §9.3` — the chrome's stylesheet, script and the brand mark are
/// embedded assets like the rest: an `ETag` at the fixed path, a day's cache at the
/// addressed one, and the frame names the addressed paths.
#[tokio::test]
async fn test_spec_9_3_chrome_assets_are_addressable_by_build_and_carry_an_etag() {
    let root = tempfile::tempdir().unwrap();
    let handler = handler(&root);
    for (name, kind) in [
        ("chrome.css", "text/css"),
        ("chrome.js", "text/javascript"),
        ("privatium-mark-white.svg", "image/svg+xml"),
    ] {
        let fixed = handler.handle(get(&format!("/static/{name}"))).await;
        assert_eq!(fixed.status(), StatusCode::OK, "{name}");
        assert!(header(&fixed, &CONTENT_TYPE).starts_with(kind), "{name}");
        assert_eq!(header(&fixed, &CACHE_CONTROL), "no-cache", "{name}");
        assert_eq!(
            header(&fixed, &ETAG),
            format!("\"{}\"", assets::integrity(name)),
            "{name}"
        );
        let addressed = handler.handle(get(&assets::versioned(name))).await;
        assert_eq!(addressed.status(), StatusCode::OK, "{name}");
        assert_eq!(
            header(&addressed, &CACHE_CONTROL),
            "public, max-age=86400, immutable",
            "{name}"
        );
    }
    let page = body_of(handler.handle(get("/a/hello/")).await).await;
    for name in ["chrome.css", "chrome.js", "privatium-mark-white.svg"] {
        assert!(page.contains(&assets::versioned(name)), "{name}: {page}");
        assert!(
            !page.contains(&format!("\"/static/{name}\"")),
            "{name}: {page}"
        );
    }
}
