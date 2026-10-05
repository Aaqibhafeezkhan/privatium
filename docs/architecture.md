<!--
This file is part of Privatium
docs/architecture.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-10-04
Summary: Explanatory architecture overview. Non-normative; see spec/ for the contract.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Architecture

Non-normative. Explains *why* the system looks the way it does. For *what implementations
must do*, read `spec/protocol.md` and `spec/app-contract.md`.

## 1. The problem

Someone wants a small app for exactly one purpose — tracking medication fills, logging
migraines, cataloguing a workshop, whatever. They want it on their phone. They do not want
it on somebody's server. They cannot administer a server themselves, and if their laptop
dies they need a restore procedure they can explain to a relative over the phone.

Every existing answer fails at least one of those. Cloud SaaS fails privacy. Self-hosted
web apps fail administration. Local-only mobile apps fail multi-device. Anything with a
binary database fails the restore-over-the-phone test.

## 2. The six decisions everything else follows from

### 2.1 Plain text is the truth

All state is append-only JSONL. One file per device, never modified after writing. A
person with Notepad can read it. A person with any file sync tool can back it up. Two
devices can never conflict on a file because two devices never write the same file.

Everything else — the database, the snapshots, the CSVs — is a cache that can be deleted
and rebuilt.

This is the constraint that makes the restore drill possible: *copy the folder back*.

### 2.2 The query engine is SQLite, and it is disposable

The framework replays JSONL into a SQLite database, then runs the app's SQL views against
it. SQLite holds the slot (`docs/decisions/0006`) because it compiles in a minute, adds a
few megabytes to the binary, is already on every phone and in every browser, and lets the
framework write while an app reads on a separate, read-only connection — which is the
whole sandbox.

What SQLite lacks, the framework supplies: it is the only writer of the cache, so it types
every value from the declared column on the way in. `DECIMAL` is exact text under a
numeric collation with `decimal_sum()` and friends registered on every connection; dates
and timestamps are ISO 8601 text, which SQLite's date functions understand and which
compares as time. The materializer is a Rust reader over the log and a bound `INSERT`,
not a subsystem.

The database file lives in `cache/` and is rebuilt on demand. Its format compatibility
across engine versions is therefore irrelevant — though SQLite's file format has not
changed since 2004.

### 2.3 The framework is not an application model

The framework's job is storage, sync, discovery, pairing, encryption, and reachability.
It is deliberately **not** a prescription for how to build an application.

Three tiers, chosen per app, mixable on one node:

| Tier | Language | Fits |
|---|---|---|
| **1 — Lua** | Lua 5.4 + LSP templates | Records, lists, forms, reports, trackers |
| **2 — Web** | Your own HTML/JS/WASM against a data API | Games, canvas, charts, animation |
| **3 — Rust** | Rust against `privatium-core`, your own `main()` | Hardware, jobs, custom protocols |

Tiers differ by **language**, not by how much freedom you surrender. None has a ceiling.
Every tier gets the same storage, sync, discovery, pairing, encryption, snapshots, and
backup.

An earlier draft defined a declarative tier — an app was `schema.sql` plus `views.sql` plus
`forms.toml`. It was removed. It had a hard expressiveness ceiling, and it imposed an
application model the framework has no business imposing. What survives is a **scaffold
generator**: `privatium new --scaffold <table>` emits Lua and templates as ordinary source
files you then edit. A starting point you escape from, not a runtime you are trapped in.

Similarly there are three deployment modes: **host** (many apps at `/a/<slug>/`), **solo**
(one app at `/`, indistinguishable from a purpose-built application), and **embedded**
(your binary, the core as a library).

The failure mode to guard against is any tier quietly becoming mandatory because it is the
best-documented path. None of them is the product; the transport and the log are.

### 2.4 Instant feedback is a requirement, not a nicety

Tier 1 has no build step and no restart. LSP templates compile to cached Lua chunks
invalidated on file mtime; `app.lua` reloads in place. Save, refresh, done.

This is load-bearing rather than cosmetic. The people writing these apps are not
professional developers and are frequently working with an AI assistant. A loop measured in
seconds is the difference between iterating and giving up. If a change requires a restart,
that is a bug in the host.

### 2.5 The browser is a client, not the app

The framework's own UI — the shell, the launcher, settings — and every Tier 1 app render as
server HTML with HTMX. Application logic evaluates on the node, once, in one language, with
no second implementation to drift out of sync with the first.

The framework ships **no client-side framework**: it would need a build step, it would
duplicate state, and it would send hundreds of kilobytes to a phone to render one table.
HTMX is roughly 14KB.

This is a decision about the framework, not a rule imposed on apps. A Tier 2 app serves its
own `web/` directory and may use Canvas, WebGL, WASM, Three.js, Chart.js, or React if it
wants — at its own weight cost, which is the app author's call.

#### The chrome every page shares

The node renders the same bar and footer around the launcher, the settings pages and
every Tier 1 view that does not own its document. The bar has three zones: the Privatium
mark linking to the launcher, the app's title and icon linking to the app's first page,
and the controls, an Apps link and one Menu. The menu lists the app's own items first,
from `[ui]` in its manifest and from the `menu()` template helper, then a rule, then the
framework's settings pages. The footer holds a link to the project, a status line, and
the space's name with the way to connect a device.

A document the app owns gets the same bar and footer by insertion rather than by
rendering around it. That is every HTML document a Tier 2 app serves from `web/`, and a
Tier 1 view that calls `layout()`. The node buffers the document — the entry page, not
an asset; everything else under `web/` streams — finds three anchors by tag, skipping
any written inside an HTML comment, and puts
three pieces at them: the chrome's stylesheet and script before `</head>`, the skip link
and the header after the opening `<body>` tag, the footer before the last `</body>`.
The header is the one the frame would draw for that app, with its title and its
manifest's menu items, in host or solo form. Nothing else changes: the body keeps its
attributes, the app's own links and scripts stay where they were, and the response
carries the app's own policy and headers. A document that lacks an anchor is served as
written, and the loader records a warning naming the file and the tag, so the owner
reads why the bar is missing. `[ui] chrome = "none"` in the manifest turns the insertion
off and serves the document byte for byte; `apps/sketch` does that, `apps/pantry` takes
the chrome. The lint holds a document under the chrome to its anchors and its
`<main id="main">` (`PV109`) and to drawing no second way back (`PV408`). The contract is
`spec/app-contract.md §5`; the code is `http::apps::insert_chrome` and
`http::shell::chrome_pieces`.

Two embedded assets carry it. `chrome.css` holds the colour tokens and the rules for the
header, menu, footer and skip link, scoped to their own classes so a document an app
owns keeps its own styles; `shell.css` holds the framework's page styles and declares no
tokens of its own. `chrome.js` writes the footer's status line when the connection
changes or when `pv.js` reports queued changes, in the framework's words and once per
change; it listens for the browser's own events and for the events `pv.js` dispatches
on the document, and imports nothing, so an app's own copy of `pv.js` and the frame
never disagree. An app writes its own task wording into the same line with
`pv.status()`. The contract is `spec/lua-api.md §4.1` and `spec/app-contract.md §3` and
`§5.2`.

#### Moving between pages

By default every link and form in a Tier 1 app loads a fresh page, bar and all. An app
that sets `[ui] navigation = "swap"` asks for something smoother: the frame's main region
carries `hx-boost`, so a link or form inside it fetches the next page with htmx, and the
frame swaps only that page's `<main>` into the current one. The bar and footer never
move, the screen does not flash, the window title and address bar follow the page, the
menu's app items arrive with it, and focus lands on the new page's heading. The node
renders exactly what it would for a fresh load; a boosted request is a page request, and
`req.is_htmx` is false for it. The back button reloads the page you return to, because
htmx's history cache is off and no page is copied into browser storage.

The line a swap may not cross is the app's mount. A document carries one app's security
policy and one set of loaded scripts, and swapping HTML into it cannot change either, so a
page from another app, from the launcher, or from the framework's own `/settings`,
`/api`, `/skills`, `/static` or `/ws` always gets a fresh document under its own policy.
One function, `inScope` in `chrome.js`, draws that line. The frame applies it on loopback
and plain HTTPS; on the encrypted channel `client.js` imports the same function and
applies it before it bridges a request, sending an in-scope request over the channel like
any htmx fragment and following its redirect as a boosted request. Scripts are the other
constraint: htmx never runs a script that arrives in swapped content, so an app with swap
navigation lists its scripts and stylesheets under `[ui]`, the frame's head loads them
once, and the lint refuses one in a view (`PV111`). The contract is `spec/lua-api.md §4.1`
and `spec/protocol.md §8.3.1`; `apps/animals` is the worked example.

Icons are Bootstrap Icons, vendored as raw SVGs and inlined at render (`docs/icons.md`).
No icon font, no CDN, no runtime sprite fetch — which also means no additions to the
Content Security Policy and no broken glyphs offline.

#### Offline capability follows from secure context, not from rendering

It is tempting to read the above and conclude that server rendering is what makes the
browser client online-only. It is not. **The constraint is the origin.**

Service workers require a secure context, and the "potentially trustworthy origin" list is
`https:`, `wss:`, `file:`, `localhost`, `127.0.0.1/8`, `::1`, and `*.localhost`. **A LAN IP
address is not on that list.** `http://192.168.1.5:8420` cannot register a service worker
in any browser, under any flag a non-technical owner will ever set.

IndexedDB *is* available on plain HTTP, so a page can store data offline. But with no
service worker there is nothing to serve the shell from cache, so when the node is
unreachable the browser shows a connection error and the local data sits behind it. Storage
without a cached shell buys nothing.

| Origin | Service worker | Works offline |
|---|---|---|
| `http://192.168.1.5:8420` | ✘ | ✘ — the page will not even load |
| `https://you.duckdns.org` | ✔ | ✔ — needs a domain and a certificate |
| `https://x.ts.net` | ✔ | ✔ — needs a third-party account |
| **Native shell (custom scheme)** | ✔ | ✔ — **no third party at all** |

**The native shell is the answer, and it is a better one than a PWA.** Its webview runs on a
scheme the browser treats as trustworthy, and the core runs *in the same process* — so
there is no service worker to register, no replica to synchronise, and no cache to
invalidate, because there is no network hop to survive. Offline is the default state rather
than a feature. See `docs/decisions/0003-in-process-adapter.md`.

A PWA on a real HTTPS origin remains supported for people who want one, and gets a client
replica built on the event log's own `(dev, lam)` watermarks. It is the second path, not
the only one.

One honest limit: this gives Tier 2 full parity immediately, and Tier 1 parity for views
already visited. Rendering an *unvisited* Tier 1 view offline needs handler logic in the
browser, which is an open `wasmoon` question, not a transport question.

### 2.6 Certificates are a browser problem, not a security problem

Browsers demand CA-signed certificates because they do not know your node. Your own
software does. So:

- **Native clients** pin the node's public key at pairing and use X25519 + ChaCha20-Poly1305
  thereafter. No CA is involved. This is stronger than webPKI, not weaker.
- **Browser clients on LAN** run the same handshake in JavaScript over plain HTTP, and
  every page, fragment and API call then travels inside an encrypted WebSocket channel
  (`spec/protocol.md §8.3`); only code and a bootstrap page cross the wire in the clear,
  and the scripts a page names are pinned by integrity to what the channel delivered.
  With genuine client code, this protects application data from passive listeners and
  refuses a substituted node. Every plain-HTTP load can still replace the bootstrap and
  client, exposing stored device keys even after pairing. Integrity does not authenticate
  a bootstrap whose hashes an attacker can also replace; imported framework and app
  modules have no per-import integrity. See `docs/security.md §4`.
- **Browser clients that need a real certificate** get one from a configured tunnel
  (Tailscale Serve) or a real domain with a DNS-01 issued certificate (DuckDNS). Both are
  optional.

A full-page transition opens a fresh bootstrap document with the destination app's
permissions. An already-produced form response can wait briefly as a bounded stream
in node memory while that document reconnects. Only a response reference crosses in
per-tab storage; consuming it does not repeat the write (`spec/protocol.md §8.3.1`).
HTMX fragments, in-document page swaps beneath the app's mount, and the data API stay on
the current channel.

## 3. Component map

```
┌───────────────────────────────────────────────────────────────────────────┐
│                         privatium-core  (Rust crate)                       │
│                                                                            │
│  log        append-only JSONL writer/reader, Lamport clock, replay         │
│  store      SQLite materialization, snapshot write/read, three-tier restore│
│  app        app-folder loader, manifest validation, SQL sandbox            │
│  lua        mlua host, sandbox, VM pool, LSP compiler + hot reload         │
│  identity   Ed25519 node key, device registry, keyring access              │
│  pair       SPAKE2 handshake, code generation and rendering                │
│  session    X25519 + HKDF + ChaCha20-Poly1305 framing                      │
│  discover   DNS-SD, UDP broadcast, pkarr publish/resolve, DNS               │
│  peer       hole punching, relay fallback, direct QUIC transport           │
│  sync       pull/push protocol over any transport                          │
└───────────────────────────────────────────────────────────────────────────┘
        │                │                │                │
   ┌────▼────┐     ┌─────▼─────┐    ┌─────▼─────┐    ┌─────▼─────┐
   │ server  │     │  desktop  │    │  mobile   │    │  uniffi   │
   │ (daemon)│     │  (Tauri)  │    │  (Tauri)  │    │ (Swift/Kt)│
   └─────────┘     └───────────┘    └───────────┘    └───────────┘
```

The daemon and the Tauri shells are thin. Everything of consequence is in `privatium-core`,
which is why the mobile clients can be separate repositories that only depend on the crate
and the protocol.

## 4. Client tiers (how devices reach a node)

| Tier | Client | Reachability | Crypto | Offline | Third party |
|---|---|---|---|---|---|
| **0** | Any browser, LAN | `http://<ip>:8420`, mDNS | PAKE-derived session, TOFU-pinned | ✗ | none |
| **1** | Native desktop / mobile | LAN direct, then iroh | Pinned static keys | ✓ | none |
| **2** | PWA / browser, remote | Tailscale `ts.net` or DuckDNS + Let's Encrypt | TLS | ✓ (PWA) | one, opt-in |
| **2b** | Tor Browser | `.onion` (in-process Arti) | Tor | ✗ | none |
| **3** | Any file syncer | Syncthing / rsync / USB on `data/` | filesystem | ✓ | optional |

Tier 0 and Tier 1 satisfy the hard requirement: **a working path to the app and its data
with no third party and no certificate authority.** Tiers 2 and 3 are configuration.

## 5. Clusters and the shape of "peer"

Each installation uses the node and cluster identity verified from its local keys and
certificate. The public registry keeps the records that restore and sync bring in, so it
can describe more than one node or cluster. Readers select this installation's records
by its identity. Losing local keys does not retire a cluster that another machine may
still use (`spec/data-dictionary.md §3.1, §3.1b`). LAN sync runs over the encrypted
channel; remote discovery and transports remain planned.

Nodes belonging to one owner form a **cluster** sharing a keypair (`spec/protocol.md §2.3`).
A device pins the *cluster* key at pairing, not a node key, so pairing a phone once makes it
trust the desktop, the laptop, and any node admitted later.

That one change does most of the work people expect from peer-to-peer, without any of the
machinery. mDNS already returns several instances; clients filter on the cluster ID in the
TXT record and key on Node ID. Two nodes on a LAN discover and sync with no coordination, no
election, and no primary — because single-writer append-only logs make sync a set union.

Full per-client matrices for bootstrap and reachability are in `docs/connectivity.md`.

Beyond the LAN, a node publishes its addresses as **pkarr** records — DNS records signed by
its key, stored on the BitTorrent mainline DHT. The cluster's public key becomes its name,
with no registrar, no dynamic-DNS account, and no payment. Native clients then hole-punch
directly, falling back to a relay that forwards ciphertext it cannot read.

This is the account-free path, and it is why peer-to-peer is in `pv/1` rather than deferred.
It is a **native-client capability**: browsers cannot open raw sockets and cannot treat a
public key as a name, so remote browser access still requires a real domain and a
certificate.

An always-on machine remains useful for four separable jobs — relay, DNS, certificate host,
and full node — of which only the last holds data (`docs/deployment.md §2`).

**Clients are not all equally capable, and that is a runtime property, not a setting.** A
browser cannot browse mDNS, cannot open UDP, cannot pin a certificate, and cannot hold more
than one origin. Native shells can do all four. That gap — not rendering — is what justifies
building them. Full capability matrix in `spec/protocol.md §10.7`.

Mobile is a caching client with an outbox by default. A native mobile client MAY be a full
replica by embedding the core library; a browser never can.

## 6. Data flow

### Write
```
form submit / action invoke
  → params bound into the app's action SQL (sandboxed, read-only DB)
  → SELECT returns rows shaped (op, tbl, id, d)
  → framework stamps seq / lam / ts / dev / app
  → appended atomically to data/<app>/log/<this-device>.jsonl, fsync
  → the SQLite cache updated
  → HTMX fragment re-rendered
```

Forms are the degenerate case of an action returning one row. There is exactly one write
path in the system.

### Read
```
1. cache/<app>.sqlite fresh?              → query it
2. else: the snapshot's <table>.sqlite files + the log tail WHERE lam > hi_lam
3. SQLite file unreadable? → CSV + schema.sql + log tail
4. Snapshots gone?         → full log replay from zero
```

Each fallback tier is logged loudly. A node that restored from tier 3 says so.

### Sync
```
peer A ──── "what's your highest seq per device?" ────▶ peer B
peer A ◀─── {dev1: 4192, dev2: 87, dev7: 12} ──────────  peer B
peer A ──── lines for any (dev, seq) B is missing ────▶ peer B
```

For each app, peers exchange sequence heads and transfer missing raw lines inside the
encrypted channel. The receiver writes the origin device's file byte for byte, then
rebuilds the cache from the log. System logs arrive before app logs.

Any peer, any direction, any number of them. A node that was powered off for a week is not a
special case — it is the ordinary catch-up path.

Because logs are append-only and single-writer, sync is a set union. There is no merge
algorithm, no vector clock reconciliation, and no conflict resolution step. Ordering for
last-write-wins is `(lam, ts, dev)` — see `spec/protocol.md §4`.

## 7. Multi-app hosting and solo mode

One node, many unrelated apps. The framework maintains an **app index** (`sys_app`, see
`spec/data-dictionary.md §3.4`) and mounts each app at `/a/<slug>/`.

Consequences of one node rather than one-node-per-app:

- One origin → one service worker, one credential, one pairing.
- One `data/` folder → one backup.
- One discovery record → the phone finds everything at once.
- Apps are isolated at the SQL level (separate SQLite files) and at the log level
  (separate directories), but not at the process level. An app folder is trusted code in
  the sense that its SQL runs on your node — see `docs/security.md §6`.

Apps advertise themselves as DNS-SD subtypes, so a client that only cares about one app
can browse for `_meds._sub._privatium._tcp` and never see the rest.

**Solo mode** collapses all of this: one app mounted at `/`, no launcher, no slug prefix,
the app's name and icon become the node's. Same binary, same config file, one line
different. Use it when you are shipping *your app* rather than *a node that runs it*.

## 8. What is deliberately absent

| Not doing | Why |
|---|---|
| Multi-user accounts | Still absent, and the word *account* stays wrong. Household profiles are coming as a partition, and real multi-user is deferred to `pv/2`. See below. |
| A server, or a primary node | Every node is a peer. An always-on node is a peer that happens to be reachable. If a "server" role appears in an implementation, that is a defect. |
| A discovery registry | pkarr rides the mainline DHT. Nothing to bootstrap, nobody to operate it, nothing to register. |
| A dedupe table for the outbox | ULIDs make replay idempotent. Adding one signals a misreading of the merge rule. |
| CRDTs | Single writer per file makes them unnecessary. Reserved for concurrent free-text fields only, if ever. |
| A JavaScript build step | HTMX and three vendored crypto files. If it needs webpack, it is out of scope. |
| An admin UI | Configuration is a TOML file and a settings page. There is no second, hidden application. |
| Plugins / extensions | An app folder *is* the extension mechanism; Tier 3 is the compile-in path. |
| A declarative app format | Removed. See §2.3. The scaffold generator emits source, not config. |
| A charting library in the framework | Anything real is Tier 2, where you pick your own. See `docs/frameworks.md`. |
| An opinion about your front end | The framework serves your `web/` directory and gets out of the way. |
| Cloudflare Tunnel automation | Documented, not implemented. Requires a domain on their DNS; the value/maintenance ratio is poor. |

### On multi-user, and what is reserved for it

The first row above changed, so here is the whole picture in one place.

**Household profiles** let the people in one home keep out of each other's data inside an
app — a partition, chosen at a screen that can ask for a PIN. They are not accounts, and
the documentation must never call them that. Anyone who can read the node's files can read
everyone's data, because `data/` is plain text by design and every node holds the cluster
key. That is the same deal a family NAS offers, and it is stated rather than implied.

**Real multi-user — separate people, provable to each other — is `pv/2`.** It needs
identities that can be authenticated, not just a partition key, and that is a protocol
major version rather than a feature.

Four things are reserved in `pv/1` so `pv/2` stays reachable. They look unused, and they
are load-bearing. **Do not remove them as scope creep:**

1. `usr` in the event envelope — without it, events already written can never be
   attributed to a person.
2. Segment directories rather than a `profile` column — with a column, one person's rows
   are interleaved with everyone else's, so removing them would mean rewriting a log and
   is never possible at all.
3. A segment scope on the sync endpoints.
4. `sys_audit.actor` as a subject rather than a device.

One thing profiles deliberately do **not** get: a way to tell other nodes to delete
something. A segment can be deleted on a node, and a peer that still holds it will send it
back at the next sync. Data that reached a cluster stays in that cluster.

`docs/decisions/0007-household-profiles.md` decides all of this and explains each one.
Read it before changing anything in the list.

---

Copyright © 2026 Gabriel Mongefranco
