// This file is part of Privatium
// crates/privatium-core/src/http/apps.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-09-06
// Summary: What answers beneath an app's mount (spec/protocol.md §9.1). Tier 2: web/ served as-is with
//          index.html at the mount point, streamed in 64 KiB frames, under that app's own CSP
//          (spec/app-contract.md §5, §5.4). Tier 1: what a Lua handler answered, as a response
//          with the same headers — a rendered view inside the framework's page frame unless
//          the app supplied the document — the app's static/ served the same way as web/, and
//          the error page with the traceback and the offending line (spec/cli.md §3).
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

use std::path::PathBuf;

use axum::body::Body;
use axum::http::header::{ALLOW, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderValue};
use axum::http::{Request as HttpRequest, StatusCode};
use tower::ServiceExt as _;
use tower_http::services::ServeDir;

use crate::http::{headers, shell};
use crate::lua::{LuaResponse, SourceContext};
use crate::wire::{Request, Response};

/// Serve `rest` — the path beneath `base`, `/` for `base` itself — out of `dir`.
///
/// `ServeDir` does the file work: it percent-decodes, refuses any `..` component, appends
/// `index.html` to a directory, guesses the content type, honours `Range` and
/// `If-Modified-Since`, and reads the file as a stream. The sub-request it sees carries the
/// remainder path only; `redirect_path_prefix` puts `base` back on the one redirect it
/// may issue (a directory without its trailing slash), so no adapter ever rewrites a path.
/// `base` is the mount for a Tier 2 app's `web/`, and `<mount>static/` for a Tier 1
/// app's `static/`.
pub async fn serve_web(
    dir: PathBuf,
    base: &str,
    rest: &str,
    request: Request,
    csp: &str,
    solo: bool,
) -> Response {
    let (parts, body) = request.into_parts();
    let mut uri = rest.to_owned();
    if let Some(query) = parts.uri.query() {
        uri.push('?');
        uri.push_str(query);
    }
    let mut sub = match HttpRequest::builder()
        .method(parts.method)
        .uri(uri)
        .body(body)
    {
        Ok(sub) => sub,
        Err(error) => {
            return headers::text(
                StatusCode::BAD_REQUEST,
                format!("400 Bad Request: {error}\n"),
            );
        }
    };
    *sub.headers_mut() = parts.headers;

    let service = ServeDir::new(dir)
        .append_index_html_on_directories(true)
        .redirect_path_prefix(base.trim_end_matches('/'));
    let mut response = match service.oneshot(sub).await {
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    };

    if response.status() == StatusCode::NOT_FOUND {
        response = headers::html(
            StatusCode::NOT_FOUND,
            shell::not_found(&url_path(base, rest), solo),
        );
    }
    app_headers(&mut response, csp);
    response
}

/// The answer for a Tier 1 route in a build without the Lua host.
#[must_use]
pub fn no_handler(slug: &str, csp: &str, solo: bool) -> Response {
    let mut response = headers::html(
        StatusCode::SERVICE_UNAVAILABLE,
        shell::no_handler(slug, solo),
    );
    app_headers(&mut response, csp);
    response
}

/// What a Lua handler answered, as a response (`spec/lua-api.md §3.1`). A rendered view
/// that the app did not frame itself goes inside the framework's page frame, titled by
/// the app, listing the items the view added to the menu, and carrying the CSRF token for
/// htmx (`§4.1`).
#[must_use]
pub fn lua_response(
    answer: LuaResponse,
    frame: &shell::Frame,
    csrf_token: &str,
    node_label: &str,
    csp: &str,
    solo: bool,
) -> Response {
    let mut response = match answer {
        LuaResponse::Html(body) => headers::with_body(StatusCode::OK, headers::HTML, body),
        LuaResponse::Text(body) => headers::with_body(StatusCode::OK, headers::TEXT, body),
        LuaResponse::Json(body) => headers::with_body(StatusCode::OK, headers::JSON, body),
        LuaResponse::Redirect(location) => headers::redirect(StatusCode::SEE_OTHER, &location),
        LuaResponse::NoContent => {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            response
        }
        LuaResponse::View {
            html,
            complete,
            menu,
        } => {
            if complete {
                headers::with_body(StatusCode::OK, headers::HTML, html)
            } else {
                let body = String::from_utf8_lossy(&html);
                let page_menu: Vec<shell::MenuLink> = menu
                    .into_iter()
                    .map(|item| shell::MenuLink {
                        label: item.label,
                        href: crate::wire::router::url(&frame.mount, &item.path),
                        icon: item.icon,
                    })
                    .collect();
                headers::html(
                    StatusCode::OK,
                    shell::app_frame(frame, &page_menu, solo, csrf_token, &body, node_label),
                )
            }
        }
    };
    app_headers(&mut response, csp);
    response
}

/// A failure beneath a Tier 1 mount — a Lua error, a limit, a refused token, a reload
/// that did not load — as the shell's error page under the app's own headers. `detail`
/// is the error's text with its traceback; `at` the offending line with context, when
/// the traceback named one of the app's files (`spec/cli.md §3`).
#[must_use]
pub fn lua_failure(
    status: StatusCode,
    detail: &str,
    at: Option<&SourceContext>,
    csp: &str,
    solo: bool,
) -> Response {
    let mut response = headers::html(status, shell::lua_error(status, detail, at, solo));
    app_headers(&mut response, csp);
    response
}

/// No route registered at `path` beneath a Tier 1 mount.
#[must_use]
pub fn not_found_under(path: &str, csp: &str, solo: bool) -> Response {
    let mut response = headers::html(StatusCode::NOT_FOUND, shell::not_found(path, solo));
    app_headers(&mut response, csp);
    response
}

/// A route exists at the path but not for this method.
#[must_use]
pub fn method_not_allowed_under(allow: &[String], csp: &str) -> Response {
    let allow = allow.join(", ");
    let mut response = headers::text(
        StatusCode::METHOD_NOT_ALLOWED,
        format!("405 Method Not Allowed — allowed: {allow}\n"),
    );
    if let Ok(value) = HeaderValue::from_str(&allow) {
        response.headers_mut().insert(ALLOW, value);
    }
    app_headers(&mut response, csp);
    response
}

/// The app's own policy — `App::csp().header_for(origin)`, never `header()` — and
/// `no-store`, on every response carrying the app's bytes (`§9.3`).
fn app_headers(response: &mut Response, csp: &str) {
    if let Ok(value) = HeaderValue::from_str(csp) {
        response
            .headers_mut()
            .insert(CONTENT_SECURITY_POLICY, value);
    }
    if !response.headers().contains_key(CONTENT_TYPE) {
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    headers::secure(response, csp);
}

fn url_path(mount: &str, rest: &str) -> String {
    crate::wire::router::url(mount, rest)
}
