// This file is part of Privatium
// crates/privatium-core/tests/scaffold.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-09-06
// Summary: What `privatium new` writes, loaded and driven through core::handle: the CRUD screens
//          `--scaffold` emits for a typed table (spec/cli.md §4, spec/app-contract.md §4.7)
//          create, show, edit and tombstone a row, refuse a bad value with the schema's own
//          message, and every rendered page meets the PV4xx rules (spec/cli.md §5.1) as the
//          shell's own pages must; the empty app of each tier loads; a copy of hello carries
//          its new slug and title.
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

use std::fs;
use std::path::Path;

use axum::body::{Body, to_bytes};
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{Method, StatusCode};
use common::a11y::{self, Unit};
use common::repo_apps_dir;
use privatium_core::app::manifest::Tier;
use privatium_core::app::scaffold::{self, File};
use privatium_core::{AppRoot, Handler, Node, Request, Response, Schema};

const DDL: &str = "CREATE TABLE fill (
    id     VARCHAR PRIMARY KEY,
    drug   VARCHAR NOT NULL,
    copay  DECIMAL(18,2),
    taken  BOOLEAN,
    on_day DATE,
    tags   VARCHAR[]
);";

fn write_files(dir: &Path, files: &[File]) {
    for file in files {
        let path = dir.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
}

fn handler_for(root: &tempfile::TempDir) -> Handler {
    fs::write(
        root.path().join("config.toml"),
        "[lua]\npool_size = 2\nmax_seconds = 20\n",
    )
    .unwrap();
    let mut node = Node::open(root.path()).unwrap();
    let roots = [
        AppRoot::local(node.paths().apps_dir()),
        AppRoot::bundled(repo_apps_dir()),
    ];
    let report = node.load_apps(&roots).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    Handler::new(node, report)
}

fn get(path: &str) -> Request {
    axum::http::Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn posted(handler: &Handler, mount: &str, path: &str, form: &str) -> Request {
    let token = handler.csrf().token(mount);
    axum::http::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("{form}&_csrf={token}")))
        .unwrap()
}

async fn body_of(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn page(handler: &Handler, path: &str) -> String {
    let response = handler.handle(get(path)).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let html = body_of(response).await;
    let findings = a11y::check(&html, Unit::Document);
    assert!(
        findings.is_empty(),
        "{path}:\n{}\n\n{html}",
        findings.join("\n")
    );
    html
}

fn location(response: &Response) -> String {
    response
        .headers()
        .get(LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}

/// `spec/cli.md §4` — the generated app is a working, lint-clean CRUD over the table:
/// list, detail, create, edit and delete screens, each rendered page held to `PV401`–
/// `PV407`, a refused value shown as the schema's message, a deletion a tombstone.
#[tokio::test(flavor = "multi_thread")]
async fn test_scaffold_output_passes_lint() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("apps").join("meds");
    let mut files = scaffold::fresh("meds", "Meds", Tier::Lua);
    files.retain(|f| f.path != "app.lua" && !f.path.starts_with("views/"));
    files.push(File {
        path: "schema.sql".into(),
        contents: DDL.as_bytes().to_vec(),
    });
    files.extend(scaffold::crud(&Schema::parse(DDL).unwrap(), "fill").unwrap());
    write_files(&dir, &files);
    for name in [
        "app.toml",
        "app.lua",
        "schema.sql",
        "views/fill_index.lsp",
        "views/fill_show.lsp",
        "views/fill_form.lsp",
    ] {
        assert!(dir.join(name).is_file(), "{name}");
    }

    let handler = handler_for(&root);
    let mount = "/a/meds/";

    // Empty list, and the form.
    let list = page(&handler, mount).await;
    assert!(list.contains("<h1>Fill</h1>"), "{list}");
    assert!(list.contains("Nothing here yet."), "{list}");
    let form = page(&handler, "/a/meds/new").await;
    assert!(form.contains("<h1>New Fill</h1>"), "{form}");
    assert!(
        form.contains("<label for=\"f-drug\">Drug</label>"),
        "{form}"
    );
    assert!(
        form.contains("name=\"drug\" type=\"text\" value=\"\" required"),
        "{form}"
    );
    assert!(form.contains("inputmode=\"decimal\""), "{form}");
    assert!(form.contains("type=\"checkbox\""), "{form}");
    assert!(form.contains("type=\"date\""), "{form}");
    assert!(
        !form.contains("name=\"tags\""),
        "a structured column is not a field: {form}"
    );
    assert!(form.contains("name=\"_csrf\""), "PV204: {form}");

    // A refused value: NOT NULL drug, shown as the schema's own message.
    let response = handler
        .handle(posted(&handler, mount, "/a/meds/new", "drug=&copay=12.50"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_of(response).await;
    assert!(html.contains("role=\"alert\""), "{html}");
    assert!(
        html.contains("drug"),
        "the message names the column: {html}"
    );
    assert!(
        html.contains("value=\"12.50\""),
        "the form keeps what was typed: {html}"
    );
    assert!(
        a11y::check(&html, Unit::Document).is_empty(),
        "{:?}",
        a11y::check(&html, Unit::Document)
    );

    // Create, follow the redirect to the detail page.
    let response = handler
        .handle(posted(
            &handler,
            mount,
            "/a/meds/new",
            "drug=Amoxicillin&copay=12.5&taken=true&on_day=2026-09-04",
        ))
        .await;
    assert_eq!(
        response.status(),
        StatusCode::SEE_OTHER,
        "{}",
        body_of(response).await
    );
    let detail = location(&response);
    assert!(detail.starts_with("/a/meds/"), "{detail}");
    let id = detail.trim_start_matches("/a/meds/").to_owned();
    assert_eq!(id.len(), 26, "{id}");
    let show = page(&handler, &detail).await;
    assert!(show.contains("Amoxicillin"), "{show}");
    assert!(show.contains("<dd>12.50</dd>"), "typed at scale 2: {show}");
    assert!(show.contains("<dd>Yes</dd>"), "{show}");
    assert!(show.contains("<dd>2026-09-04</dd>"), "{show}");
    let list = page(&handler, mount).await;
    assert!(
        list.contains("<th scope=\"col\">Drug</th>"),
        "PV407: {list}"
    );
    assert!(list.contains("Amoxicillin"), "{list}");

    // Edit: the form is filled, the amendment reuses the id.
    let edit = page(&handler, &format!("{detail}/edit")).await;
    assert!(edit.contains("<h1>Edit Fill</h1>"), "{edit}");
    assert!(edit.contains("value=\"Amoxicillin\""), "{edit}");
    assert!(edit.contains("checked"), "{edit}");
    let response = handler
        .handle(posted(
            &handler,
            mount,
            &format!("{detail}/edit"),
            "drug=Amoxicillin+500mg&copay=12.5&on_day=2026-09-04",
        ))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), detail);
    let show = page(&handler, &detail).await;
    assert!(show.contains("Amoxicillin 500mg"), "{show}");
    assert!(
        show.contains("<dd>No</dd>"),
        "an unchecked box is false: {show}"
    );

    // Delete: a tombstone, so the detail page is gone and the list is empty again.
    let response = handler
        .handle(posted(&handler, mount, &format!("{detail}/delete"), ""))
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&response), mount);
    let response = handler.handle(get(&detail)).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let list = page(&handler, mount).await;
    assert!(list.contains("Nothing here yet."), "{list}");

    // The log holds the whole history: put, put, del under one id.
    let node = handler.node().lock().unwrap();
    let log = fs::read_to_string(node.paths().app_log("meds", node.id())).unwrap();
    let ops: Vec<(String, String)> = log
        .lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            (
                v["op"].as_str().unwrap().to_owned(),
                v["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        ops,
        [
            ("put".to_owned(), id.clone()),
            ("put".to_owned(), id.clone()),
            ("del".to_owned(), id)
        ]
    );
}

/// `spec/cli.md §4` — the empty app of every tier loads and, where it has a page, renders
/// one that meets the PV4xx rules.
#[tokio::test(flavor = "multi_thread")]
async fn test_spec_cli_4_every_tier_loads() {
    let root = tempfile::tempdir().unwrap();
    let apps = root.path().join("apps");
    for (slug, tier) in [
        ("my-lua", Tier::Lua),
        ("my-web", Tier::Web),
        ("my-rust", Tier::Rust),
    ] {
        write_files(
            &apps.join(slug),
            &scaffold::fresh(slug, &scaffold::title_from_slug(slug), tier),
        );
    }
    let handler = handler_for(&root);
    {
        let node = handler.node().lock().unwrap();
        for slug in ["my-lua", "my-web", "my-rust"] {
            assert!(node.app(slug).is_some(), "{slug} loaded");
        }
        assert_eq!(node.app("my-lua").unwrap().title(), "My Lua");
    }

    let html = page(&handler, "/a/my-lua/").await;
    assert!(html.contains("<h1>My Lua</h1>"), "{html}");
    let response = handler.handle(get("/a/my-web/")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_of(response).await;
    assert!(html.contains("<h1>My Web</h1>"), "{html}");
    let findings = a11y::check(&html, Unit::Document);
    assert!(findings.is_empty(), "{findings:?}");
}

/// `spec/cli.md §4` — `--from hello` copies the reference app and rewrites what names it:
/// the manifest's slug and title, the path in every file header, the skill name, the
/// README heading. Prose keeps its words.
#[tokio::test(flavor = "multi_thread")]
async fn test_spec_cli_4_from_copy_rewrites_slug_and_title() {
    let root = tempfile::tempdir().unwrap();
    let files = scaffold::copy(&repo_apps_dir().join("hello"), "greeter", "Greeter").unwrap();
    let text = |name: &str| {
        String::from_utf8(
            files
                .iter()
                .find(|f| f.path == name)
                .unwrap()
                .contents
                .clone(),
        )
        .unwrap()
    };
    let manifest = text("app.toml");
    assert!(manifest.contains("slug        = \"greeter\""), "{manifest}");
    assert!(manifest.contains("title       = \"Greeter\""), "{manifest}");
    assert!(
        manifest.contains("File: apps/greeter/app.toml"),
        "{manifest}"
    );
    assert!(text("app.lua").contains("-- apps/greeter/app.lua"));
    assert!(text("views/index.lsp").contains("apps/greeter/views/index.lsp"));
    let skill = text("SKILL.md");
    assert!(skill.contains("name: privatium-app-greeter"), "{skill}");
    assert!(
        skill.lines().any(|l| l.trim_end() == "# greeter"),
        "{skill}"
    );
    assert!(skill.contains("privatium lint apps/greeter"), "{skill}");
    let readme = text("README.md");
    assert!(
        readme.starts_with("# greeter — the reference Tier 1 app"),
        "{readme}"
    );
    assert!(
        readme.contains("cat data/hello/log/*.jsonl"),
        "prose is not rewritten: {readme}"
    );
    assert!(!files.iter().any(|f| f.contents.is_empty()));

    write_files(&root.path().join("apps").join("greeter"), &files);
    let handler = handler_for(&root);
    {
        let node = handler.node().lock().unwrap();
        let app = node.app("greeter").unwrap();
        assert_eq!(app.title(), "Greeter");
        assert!(
            node.app("hello").is_some(),
            "the original still loads beside it"
        );
    }
    let html = page(&handler, "/a/greeter/").await;
    assert!(html.contains("We haven't met yet."), "{html}");
    assert!(html.contains("<title>Greeter"), "{html}");
}

/// `spec/cli.md §2`, `§4` — the example apps the binary carries are the repository's
/// `apps/hello`, `apps/animals`, `apps/sketch` and `apps/pantry`, every file byte for byte and nothing
/// else, so a first run or `new --examples` writes exactly what a checkout mounts.
#[test]
fn test_spec_cli_4_embedded_examples_match_the_repository_apps() {
    use privatium_core::app::examples;

    assert_eq!(examples::SLUGS, ["hello", "animals", "sketch", "pantry"]);
    assert!(examples::files("_lint").is_none());
    assert!(examples::files("nope").is_none());
    for (slug, files) in examples::all() {
        let dir = repo_apps_dir().join(slug);
        let mut on_disk = Vec::new();
        walk_relative(&dir, &dir, &mut on_disk);
        on_disk.sort();
        let embedded: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(embedded, on_disk, "{slug}: the embedded file set");
        assert!(files.iter().any(|f| f.path == "app.toml"), "{slug}");
        for file in &files {
            let bytes = fs::read(dir.join(&file.path)).unwrap();
            assert_eq!(file.contents, bytes, "{slug}/{}", file.path);
        }
    }

    // A copy of an embedded example rewrites what names the app, as `--from` promises.
    let copied = scaffold::copy_files(examples::files("hello").unwrap(), "greeter", "Greeter");
    let manifest = copied.iter().find(|f| f.path == "app.toml").unwrap();
    let manifest = std::str::from_utf8(&manifest.contents).unwrap();
    assert!(manifest.contains("slug        = \"greeter\""), "{manifest}");
    assert!(manifest.contains("apps/greeter/app.toml"), "{manifest}");
}

fn walk_relative(base: &Path, dir: &Path, into: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk_relative(base, &path, into);
        } else {
            into.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}
