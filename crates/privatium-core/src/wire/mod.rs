// This file is part of Privatium
// crates/privatium-core/src/wire/mod.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-10-04
// Summary: core::handle(Request) -> Response (ADR 0003): the one entry point for application traffic.
//          Bodies are streams in both directions. The router is built from Node::mounts(); the
//          auth layer runs here so every adapter gets it; the §9.3 headers go on every
//          response on the way out. How the Node is shared is decided here too — one mutex,
//          taken for the synchronous part of a request and released before anything awaits; a
//          Lua handler runs on a blocking thread outside it.
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
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::body::to_bytes;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{Method, StatusCode};
use tower::{Layer as _, ServiceExt as _};

use crate::app::manifest::Chrome;
use crate::app::{Appended, LoadReport, Tier};
use crate::config::Mode;
use crate::http::shell::{Context, Notice, NoticeKind};
use crate::http::{self, AuthLayer, Csrf, api, apps, assets, headers, shell, skills};
use crate::lua::{
    Host, LuaRequest, LuaResponse, NodeFacts, RequestCtx, Resolved, RunError, UiSettings,
    context_in,
};
use crate::{Error, Node};

pub mod channel;
mod data;
mod handoff;
mod owner;
pub mod router;

pub use data::{ApiSettings, ApiState, PING as STREAM_PING};
pub use router::{FRAMEWORK_PREFIXES, Route, Router, SettingsPage, url};

/// A request or response body: a boxed stream of byte frames, never a `Vec<u8>`
/// (`AGENTS.md`). This is `axum`'s body type, taken for its shape rather than for axum — a
/// wrapper over any `http_body::Body` — so an adapter with a stream of its own wraps it and
/// converts nothing.
pub type Body = axum::body::Body;

/// A request, as the `http` crate spells one, with a streaming body.
pub type Request = axum::http::Request<Body>;

/// A response, likewise.
pub type Response = axum::http::Response<Body>;

pub use crate::http::auth::{Device, Peer};

/// The node behind `handle`, and everything a request needs that the node itself does not
/// hold: the load report, the CSRF issuer, the auth layer, the origin to fall back on.
///
/// **How the node is shared.** `Node` is `Send` and not `Sync` — its SQLite connections and
/// its log writers are single-threaded things — so it lives behind one `Mutex`. A request
/// takes the lock for its synchronous part: resolving the route, the stat-based
/// `refresh_app`/`refresh`, the `_sys` reads and the HTML they render, and releases it
/// before anything is awaited, so file streaming for a Tier 2 app never holds it. Every
/// `app_conn()` is taken and dropped inside one locked section, which is what keeps the
/// rule — never hold one across a privileged window — true without further machinery. An
/// actor would give the same serialization with a channel in between; the mutex is the
/// same guarantee with less to read, and `refresh_app`/`load_seed` need `&mut Node` anyway.
/// A settings page holds the lock for the milliseconds its reads take.
///
/// **A Lua handler runs outside the lock.** The lock is taken once before it — to refresh
/// the app, open its read-only connection and take the facts `pv.node()` reports — and
/// released; the VM then runs on a blocking thread with that connection, so a handler
/// that takes `lua.max_seconds` blocks neither the shell nor another app. Only
/// `pv.append`, `pv.batch` and `pv.setting` take the lock, for the milliseconds a batch
/// write and its incremental apply take. The connection is opened after the refresh and
/// closed when the run ends; no VM keeps one across requests, which keeps that same rule
/// without machinery. The data API (`api`) takes the same shape: a query runs on a
/// blocking thread with a connection taken under the lock, an append takes the lock for
/// the write, and a stream subscribes under the lock and is pumped by a task after it.
#[derive(Clone)]
pub struct Handler {
    node: Arc<Mutex<Node>>,
    report: LoadReport,
    csrf: Arc<Csrf>,
    auth: AuthLayer,
    mode: Mode,
    /// `http://127.0.0.1:<port>` — what `App::csp().header_for` is rendered against when a
    /// request carries no usable `Host`, which an in-process adapter's may not.
    default_origin: String,
    /// The data API's per-device state: SQL rate buckets and open streams.
    api: ApiState,
    handoffs: handoff::Handoffs,
    /// The ID of each device the owner revokes, as it happens: every open channel
    /// listens and closes its own device's socket at once (`spec/data-dictionary.md
    /// §3.2`, "access denied immediately"), rather than at its next frame.
    revoked: tokio::sync::broadcast::Sender<String>,
    /// How long a `/ws` or `/ws/pair` peer may take over its handshake.
    timeouts: channel::ChannelTimeouts,
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handler")
            .field("mode", &self.mode)
            .field("default_origin", &self.default_origin)
            .finish_non_exhaustive()
    }
}

impl Handler {
    /// Wrap a node whose apps are loaded, with what `load_apps` reported.
    #[must_use]
    pub fn new(node: Node, report: LoadReport) -> Self {
        let csrf = Csrf::new(node.identity());
        // The adapter's layer, not the embedder's: `handle`'s in-process callers carry no
        // peer, and the framework's own socket adapter always inserts one.
        let auth = AuthLayer::for_adapter(node.id().clone());
        let mode = node.config().node.mode;
        let default_origin = format!("http://{}:{}", Ipv4Addr::LOCALHOST, node.config().node.port);
        Self {
            node: Arc::new(Mutex::new(node)),
            report,
            csrf: Arc::new(csrf),
            auth,
            mode,
            default_origin,
            api: ApiState::default(),
            handoffs: handoff::Handoffs::default(),
            revoked: tokio::sync::broadcast::channel(64).0,
            timeouts: channel::ChannelTimeouts::default(),
        }
    }

    /// The handshake bounds of `/ws` and `/ws/pair` — to shorten them in a test, or for
    /// an embedder whose link is slower than a LAN.
    pub fn channel_timeouts_mut(&mut self) -> &mut channel::ChannelTimeouts {
        &mut self.timeouts
    }

    /// The shared node. Lock it briefly; never across an `await`.
    #[must_use]
    pub fn node(&self) -> &Arc<Mutex<Node>> {
        &self.node
    }

    /// The data API's state — to shorten the stream's keep-alive in a test, or for an
    /// embedder whose link drops idle connections sooner than 30 seconds.
    pub fn api_mut(&mut self) -> &mut ApiState {
        &mut self.api
    }

    /// What `load_apps` reported when this handler was built.
    #[must_use]
    pub fn report(&self) -> &LoadReport {
        &self.report
    }

    /// The CSRF issuer — for a test that needs a valid token, or a template helper.
    #[must_use]
    pub fn csrf(&self) -> &Csrf {
        &self.csrf
    }

    /// The origin `App::csp().header_for` is rendered against for `request`: its `Host`
    /// header when it names an authority, otherwise this node's loopback origin.
    ///
    /// The value goes into a header verbatim, so it is accepted only when it is a bare
    /// authority — no `;`, no spaces — and `http` is the only scheme this origin is served on.
    #[must_use]
    pub fn origin_of(&self, request: &Request) -> String {
        request
            .headers()
            .get(HOST)
            .and_then(|host| host.to_str().ok())
            .filter(|host| is_authority(host))
            .map_or_else(
                || self.default_origin.clone(),
                |host| format!("http://{host}"),
            )
    }

    /// `core::handle` (ADR 0003). Infallible: anything that goes wrong is a response.
    pub fn handle(
        &self,
        mut request: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send + '_>> {
        Box::pin(async move {
            if !self.classify(&mut request) {
                let mut response = headers::text(
                    StatusCode::FORBIDDEN,
                    "403 Forbidden — this session is no longer authorized; pair again.\n",
                );
                headers::secure(&mut response, headers::CSP_DEFAULT);
                if self.isolated() {
                    headers::isolate(&mut response);
                }
                return response;
            }
            let inner = tower::service_fn(|request: Request| async move {
                Ok::<Response, std::convert::Infallible>(self.dispatch(request).await)
            });
            let mut response = match self.auth.layer(inner).oneshot(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            };
            headers::secure(&mut response, headers::CSP_DEFAULT);
            if self.isolated() {
                headers::isolate(&mut response);
            }
            response
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Node> {
        self.node.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn classify(&self, request: &mut Request) -> bool {
        use http::auth::{Public, Session};
        request.extensions_mut().remove::<Public>();
        request.extensions_mut().remove::<http::auth::Bootstrap>();
        let mut node = self.lock();
        if let Some(session) = request.extensions().get::<Session>() {
            if node.refresh().is_err()
                || session.node != *node.id()
                || !channel::active(&node, session)
            {
                return false;
            }
            // A node session belongs to a peer copying logs, not to a person browsing:
            // it reaches the sync routes and the two public facts, and answers 403
            // everywhere else (`spec/protocol.md §8.4`).
            let route = Router::new(
                self.mode,
                node.mounts().map(|(mount, app)| (mount, app.slug())),
            )
            .resolve(request.uri().path());
            return !session.is_node()
                || matches!(
                    route,
                    Route::SyncHeads
                        | Route::SyncPull
                        | Route::SyncPush
                        | Route::Health
                        | Route::Manifest
                );
        }
        let peer = request.extensions().get::<Peer>().map(|p| p.0).or_else(|| {
            request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|p| p.0)
        });
        // This machine's own request — loopback, or the node's LAN address opened from
        // its own keyboard — is the owner's (spec/protocol.md §8.4); the layer holds it
        // to the Host rule below.
        if peer.is_none_or(|p| http::auth::is_this_machine(p.ip())) {
            return true;
        }
        if request.method() != Method::GET && request.method() != Method::HEAD {
            return true;
        }
        let route = Router::new(
            self.mode,
            node.mounts().map(|(mount, app)| (mount, app.slug())),
        )
        .resolve(request.uri().path());
        let html = request
            .headers()
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',').any(|t| {
                    t.split(';')
                        .next()
                        .is_some_and(|t| t.trim().eq_ignore_ascii_case("text/html"))
                })
            });
        let public = match &route {
            Route::SyncHeads | Route::SyncPull | Route::SyncPush => None,
            Route::Ws | Route::WsPair | Route::Health | Route::Manifest | Route::Static { .. } => {
                Some(Public::Asset)
            }
            _ if html => Some(Public::Page),
            Route::App { slug, rest, .. } if rest != "/api" && !rest.starts_with("/api/") => node
                .app(slug)
                .and_then(|app| match app.manifest().app.tier {
                    Tier::Web => Some(Public::Asset),
                    Tier::Lua if rest.starts_with("/static/") => Some(Public::Asset),
                    _ => None,
                }),
            _ => None,
        };
        if let Some(public) = public {
            request.extensions_mut().insert(public);
        }
        true
    }

    fn solo(&self) -> bool {
        self.mode == Mode::Solo
    }

    /// Whether the solo app has `permissions.cross_origin_isolated`
    /// (`spec/app-contract.md §5.4`): then every response of the origin carries the two
    /// headers, the framework's own included, since the app owns the origin. Read under
    /// the lock each time, because an `app.toml` edit is reloaded on the next request.
    fn isolated(&self) -> bool {
        self.solo()
            && self.lock().mounts().any(|(mount, app)| {
                mount == "/" && app.manifest().permissions.cross_origin_isolated
            })
    }

    async fn dispatch(&self, request: Request) -> Response {
        if request
            .extensions()
            .get::<http::auth::Bootstrap>()
            .is_some()
        {
            let origin = self.origin_of(&request);
            let mut node = self.lock();
            let route = Router::new(
                self.mode,
                node.mounts().map(|(mount, app)| (mount, app.slug())),
            )
            .resolve(request.uri().path());
            let name = api::display_name(&node)
                .ok()
                .flatten()
                .unwrap_or_else(|| node.id().as_str().to_owned());
            let mut response = http::pairing::bootstrap(request.uri(), node.id().as_str(), &name);
            if let Route::App { slug, .. } = route {
                if node.refresh_app(&slug).is_err() {
                    return headers::text(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Cannot load this app. Ask the owner to check its configuration.",
                    );
                }
                if let Some(app) = node.app(&slug) {
                    headers::secure(&mut response, &app.csp().header_for(&origin));
                }
            }
            return if request.method() == Method::HEAD {
                headers::strip_body(response)
            } else {
                response
            };
        }
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        let head = method == Method::HEAD;
        let get = method == Method::GET || head;
        let owner = Self::is_owner(&request);

        let route = {
            let node = self.lock();
            Router::new(
                self.mode,
                node.mounts().map(|(mount, app)| (mount, app.slug())),
            )
            .resolve(&path)
        };

        let response = match route {
            Route::SyncHeads => return self.sync_heads(request).await,
            Route::SyncPull => return self.sync_pull(request).await,
            Route::SyncPush => return self.sync_push(request).await,
            Route::Ws | Route::WsPair => {
                return channel::upgrade(self.clone(), request, route == Route::WsPair).await;
            }
            Route::Launcher => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                self.render(owner, shell::launcher)
            }
            Route::Settings(page) => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                if page == SettingsPage::Apps {
                    self.refresh_all_apps();
                }
                self.render(owner, |cx| shell::settings(cx, page, None))
            }
            Route::Seed { slug } => {
                if method != Method::POST {
                    return headers::method_not_allowed("POST");
                }
                return self.seed(&slug, &path, request).await;
            }
            Route::NodeName => {
                return self
                    .owner_action(owner::OwnerAction::NodeName, &path, request)
                    .await;
            }
            Route::PairOpen => {
                return self
                    .owner_action(owner::OwnerAction::PairOpen, &path, request)
                    .await;
            }
            Route::PairClose => {
                return self
                    .owner_action(owner::OwnerAction::PairClose, &path, request)
                    .await;
            }
            Route::DeviceLabel { id } => {
                return self
                    .owner_action(owner::OwnerAction::DeviceLabel(id), &path, request)
                    .await;
            }
            Route::DeviceRevoke { id } => {
                return self
                    .owner_action(owner::OwnerAction::DeviceRevoke(id), &path, request)
                    .await;
            }
            Route::AdmitNode => {
                return self
                    .owner_action(owner::OwnerAction::AdmitNode, &path, request)
                    .await;
            }
            Route::Join => {
                return self
                    .owner_action(owner::OwnerAction::Join, &path, request)
                    .await;
            }
            Route::PairPage => return self.pairing_page(&request),
            Route::PairApi => return self.pair_api(request).await,
            Route::JoinApi => return self.join_api(request).await,
            Route::Health => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                let node = self.lock();
                headers::with_body(StatusCode::OK, headers::JSON, api::health(&node))
            }
            Route::Manifest => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                let mut node = self.lock();
                match node.refresh().and_then(|_| api::manifest(&node)) {
                    Ok(value) => headers::json(StatusCode::OK, &value),
                    Err(error) => self.failure(&error),
                }
            }
            Route::Skill { name } => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                match skills::skill(&name).zip(skills::etag(&name)) {
                    Some((text, etag)) => {
                        let mut response = if headers::matches_etag(request.headers(), etag) {
                            headers::with_body(
                                StatusCode::NOT_MODIFIED,
                                headers::MARKDOWN,
                                Body::empty(),
                            )
                        } else {
                            headers::with_body(StatusCode::OK, headers::MARKDOWN, text)
                        };
                        headers::cache_asset(&mut response, false, etag);
                        response
                    }
                    None => self.not_found(&path),
                }
            }
            Route::SkillBundle => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                let etag = skills::bundle_etag();
                let mut response = if headers::matches_etag(request.headers(), etag) {
                    headers::with_body(StatusCode::NOT_MODIFIED, headers::ZIP, Body::empty())
                } else {
                    headers::with_body(StatusCode::OK, headers::ZIP, skills::bundle())
                };
                headers::cache_asset(&mut response, false, etag);
                response
            }
            Route::Static { rest } => {
                if !get {
                    return headers::method_not_allowed("GET, HEAD");
                }
                match assets::get(&rest) {
                    // A browser revalidating the fixed path gets 304 and no bytes; the
                    // content-addressed path is fresh for a day (`spec/protocol.md §9.3`).
                    Some(asset) => {
                        let mut response = if headers::matches_etag(request.headers(), asset.etag) {
                            headers::with_body(
                                StatusCode::NOT_MODIFIED,
                                asset.content_type,
                                Body::empty(),
                            )
                        } else {
                            headers::with_body(StatusCode::OK, asset.content_type, asset.bytes)
                        };
                        headers::cache_asset(&mut response, asset.addressed, asset.etag);
                        response
                    }
                    None => return self.solo_static(&rest, request).await,
                }
            }
            Route::Redirect { to } => headers::redirect(StatusCode::PERMANENT_REDIRECT, &to),
            Route::NotFound => self.not_found(&path),
            Route::App { slug, mount, rest } => {
                return self.app(&slug, &mount, &rest, request).await;
            }
        };

        if head {
            headers::strip_body(response)
        } else {
            response
        }
    }

    /// `/static/<rest>` the framework has no asset for. In solo mode the mounted Tier 1
    /// app's `static/` answers — `url('/static/animals.css')` has to reach the app's own
    /// stylesheet when the mount is `/` (`spec/protocol.md §9.1`); the framework's names
    /// still win, since they were tried first. Anywhere else it is a 404.
    async fn solo_static(&self, rest: &str, request: Request) -> Response {
        let path = request.uri().path().to_owned();
        if !self.solo() {
            return self.not_found(&path);
        }
        let origin = self.origin_of(&request);
        let plan = {
            let node = self.lock();
            node.mounts()
                .find(|(mount, app)| *mount == "/" && app.manifest().app.tier == Tier::Lua)
                .and_then(|(_, app)| {
                    app.dir()
                        .map(|dir| (dir.join("static"), app.csp().header_for(&origin)))
                })
        };
        match plan {
            Some((dir, csp)) if dir.is_dir() => {
                apps::serve_web(
                    dir,
                    "/static/",
                    &format!("/{rest}"),
                    request,
                    &csp,
                    true,
                    None,
                )
                .await
            }
            _ => self.not_found(&path),
        }
    }

    /// A route beneath an app's mount. The read path refreshes the app first — the
    /// `echo >>` reload of `apps/hello/README.md`, and the edit loop of `spec/cli.md §3`
    /// — then serves it by tier. An edit that did not load is the error page, with the
    /// traceback and the offending line, until the next edit does. `api/` is the
    /// framework's beneath every mount (`spec/protocol.md §9.1`) and is resolved before
    /// a Tier 1 route table or a Tier 2 `web/` is consulted (`data`).
    async fn app(&self, slug: &str, mount: &str, rest: &str, request: Request) -> Response {
        let origin = self.origin_of(&request);
        let is_api = rest == "/api" || rest.starts_with("/api/");
        let plan = {
            let mut node = self.lock();
            if let Err(error) = node.refresh_app(slug) {
                if let Error::AppReloadFailed { reason, .. } = &error
                    && let Some(app) = node.app(slug)
                {
                    let at = app.dir().and_then(|dir| context_in(dir, reason));
                    eprintln!("privatium: {slug}: not reloaded — {reason}");
                    if let Some(at) = &at {
                        eprint!("{}", at.render_text());
                    }
                    return apps::lua_failure(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &format!("{slug}: not reloaded — {reason}"),
                        at.as_ref(),
                        &app.csp().header_for(&origin),
                        self.solo(),
                    );
                }
                return self.failure(&error);
            }
            let Some(app) = node.app(slug) else {
                return self.not_found(request.uri().path());
            };
            // A mounted app has a folder; an embedder's (`Node::open_app`) has neither a
            // folder nor a mount, so this is never reached for one — said in the type.
            let Some(dir) = app.dir() else {
                return self.not_found(request.uri().path());
            };
            let csp = app.csp().header_for(&origin);
            if is_api {
                Plan::Api
            } else {
                match app.manifest().app.tier {
                    Tier::Web => {
                        let chrome = match app.manifest().ui.chrome {
                            Chrome::None => None,
                            Chrome::Standard => {
                                let frame = shell::Frame::for_app(app.manifest(), mount, None);
                                let label = match api::display_name(&node) {
                                    Ok(name) => {
                                        name.unwrap_or_else(|| node.id().as_str().to_owned())
                                    }
                                    Err(error) => return self.failure(&error),
                                };
                                Some(shell::chrome_pieces(&frame, self.solo(), &label))
                            }
                        };
                        Plan::Web {
                            web_dir: dir.join("web"),
                            csp,
                            chrome,
                        }
                    }
                    // `static/` beneath a Tier 1 mount is the app's directory of that name
                    // (`spec/lua-api.md §2`), served as a Tier 2 app's `web/` is.
                    Tier::Lua if rest == "/static" || rest.starts_with("/static/") => {
                        Plan::Static {
                            dir: dir.join("static"),
                            csp,
                        }
                    }
                    Tier::Lua => match app.lua_host() {
                        Some(host) => {
                            let conn = match app.store().app_conn() {
                                Ok(conn) => conn,
                                Err(error) => return self.failure(&Error::Store(Box::new(error))),
                            };
                            Plan::Lua(Box::new(LuaPlan {
                                host: Arc::clone(host),
                                frame: shell::Frame::for_app(
                                    app.manifest(),
                                    mount,
                                    app.dir().map(|dir| dir.join("static")).as_deref(),
                                ),
                                node_label: match api::display_name(&node) {
                                    Ok(name) => {
                                        name.unwrap_or_else(|| node.id().as_str().to_owned())
                                    }
                                    Err(error) => return self.failure(&error),
                                },
                                csp,
                                conn,
                                facts: node_facts(&node, slug),
                                ui: ui_settings(&node),
                                csrf_token: self.csrf.token(mount),
                            }))
                        }
                        None => Plan::Done(apps::no_handler(slug, &csp, self.solo())),
                    },
                    // A tier 3 entry is never mounted (`App::mount`), so the router never gets
                    // here; if it ever did, nothing is served.
                    Tier::Rust => return self.not_found(request.uri().path()),
                }
            }
        };
        self.fire_pending(slug).await;
        match plan {
            Plan::Api => self.data_api(slug, rest, request).await,
            Plan::Web {
                web_dir,
                csp,
                chrome,
            } => {
                apps::serve_web(
                    web_dir,
                    mount,
                    rest,
                    request,
                    &csp,
                    self.solo(),
                    chrome.as_ref(),
                )
                .await
            }
            Plan::Static { dir, csp } => {
                let base = format!("{mount}static/");
                let file = rest.strip_prefix("/static").unwrap_or("/");
                let file = if file.is_empty() { "/" } else { file };
                apps::serve_web(dir, &base, file, request, &csp, self.solo(), None).await
            }
            Plan::Done(response) => response,
            Plan::Lua(plan) => self.lua(slug, mount, rest, request, *plan).await,
        }
    }

    /// A Tier 1 route: resolve it against the app's route table, read the body — bounded,
    /// Tier 1 does not stream (`docs/plans/phase-1.md §8`, R6) — and run the handler on a
    /// blocking thread with the connection and facts taken under the lock.
    async fn lua(
        &self,
        slug: &str,
        mount: &str,
        rest: &str,
        request: Request,
        plan: LuaPlan,
    ) -> Response {
        let solo = self.solo();
        let method = request.method().clone();
        let (index, params) = match plan.host.resolve(method.as_str(), rest) {
            Resolved::Match { index, params } => (index, params),
            Resolved::MethodNotAllowed { allow } => {
                return apps::method_not_allowed_under(&allow, &plan.csp);
            }
            Resolved::NotFound => {
                return apps::not_found_under(&url(mount, rest), &plan.csp, solo);
            }
        };

        let declared = request
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok());
        if declared.is_some_and(|length| length > http::FORM_LIMIT) {
            return apps::lua_failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                TOO_LARGE,
                None,
                &plan.csp,
                solo,
            );
        }
        let (parts, body) = request.into_parts();
        let body = match to_bytes(body, http::FORM_LIMIT).await {
            Ok(body) => body,
            Err(_) => {
                return apps::lua_failure(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    TOO_LARGE,
                    None,
                    &plan.csp,
                    solo,
                );
            }
        };
        let query = parts
            .uri
            .query()
            .map(|q| http::parse_form(q.as_bytes()))
            .unwrap_or_default();
        let content_type = parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let form = if content_type.starts_with("application/x-www-form-urlencoded")
            || (content_type.is_empty() && !body.is_empty())
        {
            http::parse_form(&body)
        } else {
            BTreeMap::new()
        };
        // `spec/lua-api.md §4.1`: every non-GET request beneath the mount carries the
        // mount's token — as the `_csrf` field `csrf()` emitted, or as the header the
        // page frame gives htmx — or it is refused before any handler runs.
        if method != Method::GET && method != Method::HEAD {
            let presented = form
                .get(http::csrf::FIELD)
                .cloned()
                .or_else(|| {
                    parts
                        .headers
                        .get(http::csrf::HEADER)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            if !self.csrf.verify(mount, &presented) {
                return apps::lua_failure(
                    StatusCode::FORBIDDEN,
                    CSRF_REFUSED,
                    None,
                    &plan.csp,
                    solo,
                );
            }
        }
        let headers = parts
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let is_htmx = parts.headers.contains_key("hx-request");
        let device = parts
            .extensions
            .get::<Device>()
            .map(|device| device.0.as_str().to_owned())
            .unwrap_or_else(|| plan.facts.id.clone());
        let lua_request = LuaRequest {
            method: method.as_str().to_owned(),
            path: rest.to_owned(),
            params,
            query,
            form,
            body: body.to_vec(),
            headers,
            device: device.clone(),
            is_htmx,
        };
        let route = format!(
            "{method} {}",
            plan.host
                .routes()
                .get(index)
                .map_or(rest, |route| route.pattern.as_str())
        );
        let ctx = RequestCtx {
            conn: plan.conn,
            node: Arc::clone(&self.node),
            facts: plan.facts,
            device,
            ui: plan.ui,
            csrf_token: plan.csrf_token.clone(),
        };
        let host = Arc::clone(&plan.host);
        let mut frame = plan.frame;
        // The frame's asset hashes are read from disk on the same blocking thread as the
        // handler, and only when the answer is a view the frame will wrap.
        let outcome = tokio::task::spawn_blocking(move || {
            let answer = host.run(index, lua_request, ctx);
            if matches!(
                answer,
                Ok(LuaResponse::View {
                    complete: false,
                    ..
                })
            ) {
                frame.hash_assets();
            }
            (answer, frame)
        })
        .await;

        let response = match outcome {
            Ok((Ok(answer), frame)) => apps::lua_response(
                answer,
                &frame,
                &plan.csrf_token,
                &plan.node_label,
                &plan.csp,
                solo,
            ),
            Ok((Err(RunError::Limit { kind, detail }), _)) => {
                let audit = serde_json::json!({
                    "app": slug,
                    "route": route,
                    "limit": kind.as_str(),
                    "detail": detail,
                })
                .to_string();
                if let Err(error) = self.lock().audit_lua_limit(slug, &audit) {
                    eprintln!("privatium: {slug}: could not write lua.limit_exceeded: {error}");
                }
                eprintln!("privatium: {slug}: {route}: {kind} limit exceeded: {detail}");
                apps::lua_failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!(
                        "{slug}: {route}: the {kind} limit was exceeded (spec/lua-api.md §5). \
                         The request was abandoned; the node and the next request are \
                         unaffected."
                    ),
                    None,
                    &plan.csp,
                    solo,
                )
            }
            // The browser and the terminal see the same thing: the traceback, and the
            // offending line with its neighbours (`spec/cli.md §3`).
            Ok((Err(RunError::Lua { message, at }), _)) => {
                eprintln!("privatium: {slug}: {route}: {message}");
                if let Some(at) = &at {
                    eprint!("{}", at.render_text());
                }
                apps::lua_failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("{slug}: {route}: {message}"),
                    at.as_ref(),
                    &plan.csp,
                    solo,
                )
            }
            Ok((Err(error), _)) => {
                eprintln!("privatium: {slug}: {route}: {error}");
                apps::lua_failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("{slug}: {route}: {error}"),
                    None,
                    &plan.csp,
                    solo,
                )
            }
            Err(join) => {
                eprintln!("privatium: {slug}: {route}: the handler's thread panicked: {join}");
                apps::lua_failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("{slug}: {route}: the handler's thread panicked: {join}"),
                    None,
                    &plan.csp,
                    solo,
                )
            }
        };
        if method == Method::HEAD {
            headers::strip_body(response)
        } else {
            response
        }
    }

    /// Fire a Tier 1 app's `pv.on('append')` handlers for a batch written outside a
    /// handler — the seed. A failing handler is the app's bug and is logged, not the
    /// owner's act undone: the batch is already durable.
    async fn fire_append(&self, slug: &str, appended: Appended, device: Option<String>) {
        let (host, ctx) = {
            let node = self.lock();
            let Some(app) = node.app(slug) else {
                return;
            };
            let Some(host) = app.lua_host() else {
                return;
            };
            let conn = match app.store().app_conn() {
                Ok(conn) => conn,
                Err(error) => {
                    eprintln!("privatium: {slug}: pv.on('append') not fired: {error}");
                    return;
                }
            };
            let facts = node_facts(&node, slug);
            let mount = app
                .mount()
                .map_or_else(|| format!("/a/{slug}/"), str::to_owned);
            let ctx = RequestCtx {
                conn,
                node: Arc::clone(&self.node),
                device: device.unwrap_or_else(|| facts.id.clone()),
                facts,
                ui: ui_settings(&node),
                csrf_token: self.csrf.token(&mount),
            };
            (Arc::clone(host), ctx)
        };
        let outcome = tokio::task::spawn_blocking(move || host.fire(&appended, ctx)).await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(RunError::Limit { kind, detail })) => {
                let audit = serde_json::json!({
                    "app": slug,
                    "route": "pv.on('append')",
                    "limit": kind.as_str(),
                    "detail": detail,
                })
                .to_string();
                if let Err(error) = self.lock().audit_lua_limit(slug, &audit) {
                    eprintln!("privatium: {slug}: could not write lua.limit_exceeded: {error}");
                }
                eprintln!("privatium: {slug}: pv.on('append'): {kind} limit exceeded: {detail}");
            }
            Ok(Err(error)) => eprintln!("privatium: {slug}: pv.on('append'): {error}"),
            Err(join) => eprintln!("privatium: {slug}: pv.on('append'): thread panicked: {join}"),
        }
    }

    /// Deliver received `pv.on('append')` callbacks for `slug`, with the origin device.
    /// Takes pending batches under the node lock and releases it before running Lua;
    /// callbacks may append. Callback failures follow the normal Lua audit path.
    pub async fn fire_pending(&self, slug: &str) {
        let pending = self.lock().take_unfired(slug);
        for (appended, origin) in pending {
            self.fire_append(slug, appended, Some(origin)).await;
        }
    }

    /// Drain synchronized logs for every loaded app, then run Lua callbacks outside
    /// the node lock. Returns storage failures without exposing received row contents.
    pub async fn drain_sync(&self) -> crate::Result<()> {
        let slugs = {
            let mut node = self.lock();
            node.refresh()?;
            let slugs = node
                .apps()
                .map(|app| app.slug().to_owned())
                .collect::<Vec<_>>();
            for slug in &slugs {
                node.refresh_app(slug)?;
            }
            slugs
        };
        for slug in slugs {
            self.fire_pending(&slug).await;
        }
        Ok(())
    }

    /// `POST /settings/apps/<slug>/seed`: the owner's explicit act
    /// (`spec/app-contract.md §9`), behind `csrf()`.
    async fn seed(&self, slug: &str, path: &str, request: Request) -> Response {
        let device = request
            .extensions()
            .get::<Device>()
            .map(|device| device.0.as_str().to_owned());
        let owner = Self::is_owner(&request);
        if let Err(refusal) = self.form_with_token(path, request).await {
            return refusal;
        }

        let outcome = self.lock().load_seed(slug);
        match outcome {
            Ok(seeded) => {
                let mut response = headers::redirect(
                    StatusCode::SEE_OTHER,
                    &format!("{}#app-{slug}", SettingsPage::Apps.path()),
                );
                // htmx follows `HX-Redirect` client-side; a plain form follows `Location`.
                if let Ok(value) = axum::http::HeaderValue::from_str(SettingsPage::Apps.path()) {
                    response.headers_mut().insert("hx-redirect", value);
                }
                // The seed is this node's own append, so a Tier 1 app's `pv.on('append')`
                // sees it (`spec/lua-api.md §3.4`) — after the lock is released, since a
                // handler may append in turn.
                if let Some(appended) = seeded.appended.filter(|a| !a.events.is_empty()) {
                    self.fire_append(slug, appended, device).await;
                }
                response
            }
            Err(error) => {
                let status = match error {
                    Error::SeedRefused { .. } => StatusCode::CONFLICT,
                    Error::AppNotLoaded { .. } | Error::NoSeed { .. } => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let notice = Notice {
                    kind: NoticeKind::Warn,
                    text: error.to_string(),
                };
                let page = {
                    let node = self.lock();
                    let cx = Context {
                        node: &node,
                        report: &self.report,
                        csrf: &self.csrf,
                        owner,
                    };
                    shell::settings(&cx, SettingsPage::Apps, Some(&notice))
                };
                match page {
                    Ok(html) => headers::html(status, html),
                    Err(error) => self.failure(&error),
                }
            }
        }
    }

    /// Render a shell page under the lock, after refreshing `_sys` (per request: the read
    /// path notices a `_sys` log that grew behind it).
    fn render(
        &self,
        owner: bool,
        page: impl FnOnce(&Context<'_>) -> crate::Result<String>,
    ) -> Response {
        let mut node = self.lock();
        if let Err(error) = node.refresh() {
            return self.failure(&error);
        }
        let cx = Context {
            node: &node,
            report: &self.report,
            csrf: &self.csrf,
            owner,
        };
        match page(&cx) {
            Ok(html) => headers::html(StatusCode::OK, html),
            Err(error) => self.failure(&error),
        }
    }

    /// The apps page shows each app's tables, so each loaded app is refreshed first. A
    /// refresh that fails is not fatal to the page; the app's row will say what it can.
    fn refresh_all_apps(&self) {
        let mut node = self.lock();
        let slugs: Vec<String> = node.apps().map(|app| app.slug().to_owned()).collect();
        for slug in slugs {
            let _ = node.refresh_app(&slug);
        }
    }

    fn not_found(&self, path: &str) -> Response {
        headers::html(StatusCode::NOT_FOUND, shell::not_found(path, self.solo()))
    }

    fn failure(&self, error: &Error) -> Response {
        let status = match error {
            Error::AppNotLoaded { .. } => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        headers::html(
            status,
            shell::error(status, &error.to_string(), self.solo()),
        )
    }
}

/// What `app` decided under the lock, to be served after it.
enum Plan {
    /// The data API beneath the mount, either tier (`api`).
    Api,
    /// Tier 2: stream `web/`; an HTML document receives `chrome` when the manifest did not
    /// decline it (`spec/app-contract.md §5`).
    Web {
        web_dir: PathBuf,
        csp: String,
        chrome: Option<shell::ChromePieces>,
    },
    /// Tier 1: stream the app's `static/`.
    Static { dir: PathBuf, csp: String },
    /// Tier 1: run a handler. Boxed: the plan carries a connection, the node facts and
    /// three strings, many times the size of the other variants.
    Lua(Box<LuaPlan>),
    /// Already answered.
    Done(Response),
}

/// Everything a Tier 1 request takes from under the lock.
struct LuaPlan {
    host: Arc<Host>,
    /// What the page frame carries for this app: title, menu items, declared assets.
    frame: shell::Frame,
    node_label: String,
    csp: String,
    conn: rusqlite::Connection,
    facts: NodeFacts,
    ui: UiSettings,
    /// The mount's token: what `csrf()` emits and the frame hands htmx.
    csrf_token: String,
}

/// The 413 a Tier 1 request gets for a body past `http::FORM_LIMIT`.
const TOO_LARGE: &str = "413 Payload Too Large: a Tier 1 request body is at most 64 KiB — \
                         Tier 1 does not stream (docs/plans/phase-1.md §8, R6)";

/// The 403 a non-GET Tier 1 request gets without the mount's token.
const CSRF_REFUSED: &str = "403 Forbidden: the request carries no valid token — a form needs \
                            <?= csrf() ?>, a request without a form the X-CSRF-Token header; \
                            a token from before the node restarted is stale, so reload the \
                            page and try again (spec/lua-api.md §4.1)";

/// What `pv.node()` reports, taken while the lock is held.
fn node_facts(node: &Node, slug: &str) -> NodeFacts {
    let id = node.id().as_str().to_owned();
    let name = api::display_name(node)
        .ok()
        .flatten()
        .unwrap_or_else(|| id.clone());
    NodeFacts {
        id,
        name,
        solo: node.config().node.mode == Mode::Solo,
        peers: node.paired_node_count().unwrap_or(0),
        restore_tier: node.restore_tier(slug).map(|tier| tier.as_u8()),
    }
}

/// The `ui.*` settings `fmt.*` reads, taken while the lock is held.
fn ui_settings(node: &Node) -> UiSettings {
    let read = |key: &str| {
        node.setting_value(key)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<String>(&text).ok())
    };
    let mut ui = UiSettings::default();
    if let Some(locale) = read("ui.locale") {
        ui.locale = locale;
    }
    if let Some(format) = read("ui.date_format") {
        ui.date_format = format;
    }
    ui
}

/// `host[:port]` and nothing else: letters, digits, `.`, `-`, `:`, and brackets for IPv6.
/// What may follow `http://` in a CSP `host-source`.
fn is_authority(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorities() {
        assert!(is_authority("127.0.0.1:8420"));
        assert!(is_authority("localhost"));
        assert!(is_authority("[::1]:8420"));
        assert!(!is_authority("a; script-src *"));
        assert!(!is_authority("host/path"));
        assert!(!is_authority(""));
    }

    /// The whole sharing story rests on this: a `Mutex<Node>` is `Sync` only if `Node` is
    /// `Send`.
    #[test]
    fn the_node_can_be_shared_across_threads() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<Node>();
        assert_sync::<Handler>();
    }
}
