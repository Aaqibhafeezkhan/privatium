// This file is part of Privatium
// crates/privatium-core/src/http/apps.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: What answers beneath an app's mount (spec/protocol.md §9.1). Tier 2: web/ served with
//          index.html at the mount point, streamed in 64 KiB frames, under that app's own CSP
//          (spec/app-contract.md §5, §5.4); an HTML document is buffered and receives the
//          standard chrome at its three anchors unless the manifest declines it. Tier 1: what
//          a Lua handler answered, as a response with the same headers — a rendered view
//          inside the framework's page frame, or the document the view owns with the chrome
//          inserted the same way — the app's static/ served the same way as web/, and the
//          error page with the traceback and the offending line (spec/cli.md §3).
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

use axum::body::{Body, to_bytes};
use axum::http::header::{
    ALLOW, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderValue,
};
use axum::http::{Method, Request as HttpRequest, StatusCode};
use tower::ServiceExt as _;
use tower_http::services::ServeDir;

use crate::http::{headers, shell};
use crate::lua::{LuaResponse, SourceContext};
use crate::wire::{Request, Response};

/// The largest HTML document the chrome is inserted into. A longer one is served as it
/// is: a document of that size is not a page an owner reads but a file being downloaded.
pub const CHROME_DOCUMENT_LIMIT: usize = 4 * 1024 * 1024;

/// The three anchors a document needs for the chrome (`spec/app-contract.md §5`), as byte
/// offsets into the text: just before `</head>`, just after the first `<body …>` tag, and
/// just before the last `</body>`. Matching is case-insensitive and asks for nothing more
/// than those three tags: no parse of the markup between them is attempted. `Err` names
/// the anchor that is missing, as the load warning and the lint report it.
pub fn chrome_anchors(document: &str) -> Result<(usize, usize, usize), &'static str> {
    let lower = document.to_ascii_lowercase();
    let head_end = lower.find("</head>").ok_or("</head>")?;
    let body_open = body_tag_end(&lower).ok_or("<body>")?;
    let body_end = lower
        .rfind("</body>")
        .filter(|at| *at >= body_open)
        .ok_or("</body>")?;
    if head_end > body_open {
        return Err("</head>");
    }
    Ok((head_end, body_open, body_end))
}

/// The offset just past the `>` of the first `<body …>` tag, or `None`.
fn body_tag_end(lower: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = lower[from..].find("<body") {
        let start = from + at;
        let after = start + "<body".len();
        let next = lower.as_bytes().get(after).copied();
        if next.is_none_or(|byte| byte == b'>' || byte.is_ascii_whitespace()) {
            return lower[after..].find('>').map(|close| after + close + 1);
        }
        from = after;
    }
    None
}

/// `document` with the chrome inserted at its three anchors, or the anchor that is
/// missing. Nothing else in the document changes.
pub fn insert_chrome(document: &str, pieces: &shell::ChromePieces) -> Result<String, &'static str> {
    let (head_end, body_open, body_end) = chrome_anchors(document)?;
    let mut out = String::with_capacity(
        document.len() + pieces.head.len() + pieces.body_start.len() + pieces.body_end.len() + 3,
    );
    out.push_str(&document[..head_end]);
    out.push_str(&pieces.head);
    out.push_str(&document[head_end..body_open]);
    out.push('\n');
    out.push_str(&pieces.body_start);
    out.push_str(&document[body_open..body_end]);
    out.push_str(&pieces.body_end);
    out.push_str(&document[body_end..]);
    Ok(out)
}

/// Serve `rest` — the path beneath `base`, `/` for `base` itself — out of `dir`.
///
/// `ServeDir` does the file work: it percent-decodes, refuses any `..` component, appends
/// `index.html` to a directory, guesses the content type, honours `Range` and
/// `If-Modified-Since`, and reads the file as a stream. The sub-request it sees carries the
/// remainder path only; `redirect_path_prefix` puts `base` back on the one redirect it
/// may issue (a directory without its trailing slash), so no adapter ever rewrites a path.
/// `base` is the mount for a Tier 2 app's `web/`, and `<mount>static/` for a Tier 1
/// app's `static/`.
///
/// `chrome` is what a Tier 2 app's HTML documents receive (`spec/app-contract.md §5`):
/// a complete `text/html` answer to a GET is buffered and served with the pieces at its
/// anchors, the file's own validators dropped since the bytes are no longer the file's.
/// A document without an anchor, one too large, or one that is not UTF-8 is served
/// untouched; the load warning named the missing anchor already. `None` — a Tier 1
/// app's `static/`, or an app that declined the chrome — streams every file as it is.
pub async fn serve_web(
    dir: PathBuf,
    base: &str,
    rest: &str,
    request: Request,
    csp: &str,
    solo: bool,
    chrome: Option<&shell::ChromePieces>,
) -> Response {
    let (parts, body) = request.into_parts();
    let is_get = parts.method == Method::GET;
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
    } else if let Some(pieces) = chrome
        && is_get
        && is_html_document(&response)
    {
        response = with_chrome(response, pieces).await;
    }
    app_headers(&mut response, csp);
    response
}

/// A complete `text/html` answer small enough to buffer: a 200, not a range or a
/// not-modified, with a `Content-Length` within [`CHROME_DOCUMENT_LIMIT`].
fn is_html_document(response: &Response) -> bool {
    if response.status() != StatusCode::OK {
        return false;
    }
    let html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("text/html")
        });
    let small = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_none_or(|length| length <= CHROME_DOCUMENT_LIMIT);
    html && small
}

/// `response` with its document read in full and the chrome inserted. When the document
/// cannot take the chrome — an anchor is missing, or the bytes are not text — it is
/// served as it was read.
async fn with_chrome(response: Response, pieces: &shell::ChromePieces) -> Response {
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, CHROME_DOCUMENT_LIMIT).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return headers::text(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("500 Internal Server Error: the document could not be read: {error}\n"),
            );
        }
    };
    let inserted = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|document| insert_chrome(document, pieces).ok());
    let body = match inserted {
        Some(document) => document.into_bytes(),
        None => bytes.to_vec(),
    };
    // The bytes are no longer the file's, so the file's validators and its range offer
    // go; `no-store` is applied on the way out with the app's other headers.
    for name in [
        axum::http::header::ETAG,
        axum::http::header::LAST_MODIFIED,
        axum::http::header::ACCEPT_RANGES,
    ] {
        parts.headers.remove(name);
    }
    parts
        .headers
        .insert(CONTENT_TYPE, HeaderValue::from_static(headers::HTML));
    parts
        .headers
        .insert(CONTENT_LENGTH, HeaderValue::from(body.len()));
    Response::from_parts(parts, Body::from(body))
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
/// htmx (`§4.1`). A document the view owns with `layout()` receives the chrome at its
/// anchors unless the manifest declines it (`spec/app-contract.md §5`); a fragment
/// answering htmx is served as it is.
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
            owned,
            menu,
        } => {
            if complete {
                let body = match (owned, frame.chrome) {
                    (true, crate::app::manifest::Chrome::Standard) => {
                        let pieces = shell::chrome_pieces(frame, solo, node_label);
                        match std::str::from_utf8(&html)
                            .ok()
                            .and_then(|document| insert_chrome(document, &pieces).ok())
                        {
                            Some(document) => document.into_bytes(),
                            None => html,
                        }
                    }
                    _ => html,
                };
                headers::with_body(StatusCode::OK, headers::HTML, body)
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
