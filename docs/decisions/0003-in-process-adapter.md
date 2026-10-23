<!--
This file is part of Privatium
docs/decisions/0003-in-process-adapter.md
Author(s): Gabriel Mongefranco
Created: 2026-08-31
Last Modified: 2026-08-31
Summary: Decision record. One request/response interface in the core, three transports behind it, and
         why this is what makes offline work without a certificate. Status: DECIDED.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# ADR 0003 — One core interface, three transports

**Status: DECIDED.**

## Decision

`privatium-core` exposes exactly one entry point for application traffic:

```rust
core::handle(Request) -> Response
```

Everything that serves an app — the daemon, the desktop shell, the mobile shells, an
embedded Tier 3 host — is an adapter over that one function. There is no second routing
table, no parallel "native API", and no per-platform request path.

| Deployment | Adapter | What the front end calls |
|---|---|---|
| Node daemon | `axum` over TCP | `http://host:8420/a/<slug>/…` |
| Desktop shell (Tauri v2) | custom URI scheme handler → direct call | `fetch('/a/<slug>/…')` |
| Mobile shells (Tauri v2) | custom URI scheme handler → direct call | `fetch('/a/<slug>/…')` |
| Browser, remote node | HTTP over the wire | `fetch('/a/<slug>/…')` |
| Tier 3 embedded | `core::handle` directly | n/a |

The front end never learns which one it got.

## Reasons

### 1. It is how offline works without a certificate

This is the load-bearing reason and it is not obvious.

Service workers require a **secure context**. The "potentially trustworthy origin" list is
`https:`, `wss:`, `file:`, `localhost`, `127.0.0.1/8`, `::1`, and `*.localhost`. **A LAN IP
address is not on that list.** `http://192.168.1.5:8420` cannot register a service worker
in any browser, under any flag a non-technical owner will ever set.

IndexedDB *is* available on plain HTTP, so a page can store data offline. But without a
service worker there is nothing to serve the page shell from cache, so when the node is
unreachable the browser shows a connection error and the local data sits behind it,
unreachable. Storage without a cached shell buys nothing.

| Origin | Service worker | Works offline |
|---|---|---|
| `http://192.168.1.5:8420` | ✘ | ✘ — the page will not even load |
| `https://you.duckdns.org` | ✔ | ✔ — needs a domain and a certificate |
| `https://x.ts.net` | ✔ | ✔ — needs a third-party account |
| **custom scheme in a native shell** | ✔ | ✔ — **no third party at all** |

In a native shell the webview runs on a scheme the browser treats as trustworthy — and,
more importantly, the core is *in the process*. There is no service worker to register, no
replica to synchronise, and no cache to invalidate, because there is no network hop to
survive. **Offline is the default state, not a feature.**

So the constraint that keeps the browser online-only is the **origin**, not the rendering
model. Fixing the origin is the whole fix. See `docs/architecture.md §2.5`.

### 2. One front end, one test surface

Tier 1 LSP output and Tier 2 `web/` directories become byte-identical across daemon,
desktop, mobile, and remote browser. `pv.js` holds the only conditional, and an app author
never sees it.

Every route becomes testable as `Request -> Response` with no socket. The `AGENTS.md` rule
that every normative MUST maps to a named test gets substantially cheaper.

### 3. It is nearly free now and expensive later

Introducing this abstraction after three adapters exist is a rewrite. Introducing it before
any exist is an afternoon.

## Constraints this imposes

- **`Request` and `Response` bodies MUST be streams, not `Vec<u8>`.** Both directions.
  Response streaming is required by `/api/stream`; request streaming is required by file
  uploads and long POSTs. Designing this in is cheap; retrofitting it is not.
- **No adapter may add routes.** If a platform needs behaviour the others lack, it belongs
  in the core behind a capability flag, not in the adapter.
- **No adapter may rewrite paths.** `pv.url()` is the only URL construction point
  (`spec/data-api.md §6`).

## Streaming on custom schemes: spec both, build one

Custom-scheme streaming in a platform webview — particularly WKWebView on iOS — is the
least-proven part of this design. Rather than gate the first working node on it:

- `spec/data-api.md §3` defines **SSE as the required transport** and **long-poll as a
  conformant fallback** a client MAY negotiate.
- The node ships SSE over HTTP, which is uncontroversial and covers the daemon and browser.
- The custom-scheme spike belongs to the mobile shell repositories as a **documented open
  risk**, not a blocker for the node.

Because `Response` is stream-shaped in the core, the fallback is a transport swap rather
than a refactor. That is the entire point of paying the design cost early.

## Consequences

- The core gains the `Request`/`Response` types and the axum adapter. The native shell
  adapters add no core work when they arrive.
- The desktop shell gets offline for free — no service worker, no PWA manifest, no
  certificate, no domain, no account.
- A PWA client replica remains possible for people on a real HTTPS origin, but it is now
  clearly the *second* path rather than the only one. Build the native shell first.
- Tier 3 embedders call `core::handle` directly and get the same routes with no HTTP stack.

## Would reopen if

Platform webviews turn out to forbid custom-scheme request interception broadly enough that
mobile shells must bind a local port after all — in which case the adapter boundary stays
and only the mobile adapter changes, which is the design working as intended.

---

Copyright © 2026 Gabriel Mongefranco
