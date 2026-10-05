<!--
This file is part of Privatium
spec/app-contract.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-10-04
Summary: NORMATIVE. What an app is, the three tiers of app, and the three deployment modes. The
         declarative tier is one option, not the model.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# App Contract — `api = 1`

**An app is a folder containing an `app.toml`. Everything else is optional.**

The framework's job is storage, sync, discovery, pairing, encryption, and reachability.
It is not an application model. How you build your application is your business; the three
tiers below exist so you can take as much or as little help as you want.

---

## 1. The three tiers

Tiers are distinguished by **language**, not by how much freedom you give up. None of them
has a ceiling. Pick per app; a single node hosts all three at once.

| Tier | Language | Framework renders | Fits |
|---|---|---|---|
| **1 — Lua** | Lua 5.4 + LSP templates | Your templates, server-side | Records, lists, forms, reports, trackers |
| **2 — Web** | Your own HTML / CSS / JS / WASM | Nothing — it serves your files | Games, canvas, charts, animation |
| **3 — Rust** | Rust against `privatium-core` | Nothing — you own `main()` | Hardware, scheduled jobs, non-HTTP protocols |

**There is no declarative tier.** An earlier draft defined an app as `schema.sql` plus
`views.sql` plus `forms.toml`. It was removed: it had a hard expressiveness ceiling and it
imposed an application model the framework has no business imposing. What survives is a
**scaffold generator** (§4.6) that emits Lua and templates you then edit — a starting point
you escape from, not a runtime you are trapped in.

**Tier 1 is the default.** No build step, hot reload, no JavaScript required. Move to
Tier 2 only when the interface genuinely cannot be server-rendered HTML.

What every tier gets regardless:

- Append-only event storage that survives file sync (`spec/protocol.md §4`)
- Multi-device sync with no server (`§10`)
- Discovery, pairing, session encryption, device revocation (`§6–8`)
- LAN, remote, onion, and native-shell reachability
- Snapshots, three-tier restore, plain-text backup

That list is the product. The rest is optional scaffolding.

---

## 2. The three deployment modes

Orthogonal to tier. Set in `config.toml`.

### 2.1 Host mode (default)

One binary, many unrelated apps mounted at `/a/<slug>/`, with a launcher at `/`. One
pairing, one backup folder, one discovery record.

### 2.2 Solo mode

```toml
[node]
mode = "solo"
app  = "medtracker"
```

The same binary, one app, mounted at `/`. No launcher, no `/a/<slug>/` prefix. The app's
title and icon become the node's. Discovery advertises it directly.

To an end user this is indistinguishable from a purpose-built application. Use it when you
are shipping *your app*, not *a node that happens to run it*.

### 2.3 Embedded mode

```toml
[dependencies]
privatium-core = "1"
```

Your `main()`, your HTTP stack (or none), your routes. `privatium-core` gives you the log,
the materializer, sync, discovery, and pairing as a library. The framework has no opinion
about anything else.

```rust
use privatium_core::{Event, Node, new_ulid};

let mut node = Node::open(&data_dir)?;
node.open_app("myapp", "CREATE TABLE score (id VARCHAR PRIMARY KEY, points BIGINT);")?;
node.serve_discovery()?;          // mDNS, UDP, pairing
node.start_sync()?;               // LAN peers over the encrypted channel

// your own writes — seq, lam, ts and dev are the node's to stamp
node.append("myapp", Event::put("score", new_ulid(), json!({"points": 42})))?;

// your own reads — the materialized SQLite connection, sandboxed, parameters bound
let rows = node.query("myapp", "SELECT * FROM score WHERE points > ?", &[json!(10)])?;

// your own server — the peer comes from axum's ConnectInfo; without it the layer refuses everyone
let service = my_router.layer(node.auth_layer()).into_make_service_with_connect_info::<SocketAddr>();
axum::serve(listener, service).await?;
node.close()?;
```

`open_app` is what a folder is to the other modes: the app's slug and the text its
`schema.sql` would hold — empty for the log as a document store (§4.5) — and the node
opens its log, its cache and its stream under `data/<slug>/` exactly as §8 does for a
folder. No folder, no mount, no index row (`spec/data-dictionary.md §3.4`). Call it at
every start. `examples/embedded.rs` in the repository is this shape, whole, in thirty
lines, and CI runs it.

This is the shape to use when Privatium is a dependency of your app rather than the other
way round. It is a first-class mode, not an escape hatch.

---

## 3. `app.toml` — the only required file

```toml
[app]
slug        = "myapp"          # REQUIRED  ^[a-z][a-z0-9-]{1,30}$
title       = "My App"         # REQUIRED  ≤ 40 chars
version     = "1.0.0"          # REQUIRED  semver
api         = 1                # REQUIRED  framework API targeted
tier        = "lua"            # REQUIRED  "lua" | "web" | "rust"
description = "..."
icon        = "diagram-3"      # Bootstrap Icons filename; see docs/icons.md
authors     = ["..."]
license     = "GPL-3.0-or-later"

[nav]
order     = 10
advertise = true               # advertise a DNS-SD subtype

[ui]
scripts = ["static/app.js"]    # loaded in the page frame's head on every page, deferred
styles  = ["static/app.css"]   # loaded in the page frame's head on every page

[[ui.menu]]
label = "Setup"                # REQUIRED  1 to 40 characters
path  = "/setup"               # REQUIRED  mount-relative; resolved through url()
icon  = "gear"                 # Bootstrap Icons filename; see docs/icons.md
```

Everything below `[app]` and `[nav]` is optional. A Tier 2 app's manifest can be the two
tables and nothing more.

`[ui]` describes what the framework's page frame carries for the app (`spec/lua-api.md
§4.1`). `[[ui.menu]]` entries are app-wide links the frame's menu lists first, in manifest
order, before the framework's own pages; a view adds page-specific items with the
`menu()` helper. `scripts` and `styles` name files beneath the app's `static/` folder,
spelled `static/<name>.js` and `static/<name>.css`; the frame loads them in its head on
every page it renders for the app, in the order written, the scripts deferred and each
with the hash of the file as served, so a view carries no `<script>` or `<link>` of its
own for what the whole app needs. They apply to a Tier 1 view rendered inside the frame.
Every default is the frame as it renders for an app that declares nothing.

The manifest parser refuses a key it does not know, so a node built before this table
existed refuses a manifest that carries `[ui]`. An app that adopts the table depends on a
release that has it, and says so where it pins the framework version it is linted against.

### 3.1 Validation

A node MUST refuse to load an app and MUST record `app.load_failed` when the slug is
reserved or malformed, when it collides with an installed app, when `api` exceeds what
the framework implements, or when a `[ui]` reference does not resolve: a script or
stylesheet that is not in the folder, a menu item without a label, or a menu path that is
not mount-relative. A menu icon the vendored set lacks is drawn as the fallback glyph and
reported as a load warning, like `app.icon`.

Folders are discovered from the owner's `<data-root>/apps/` before any bundled `apps/`
(`spec/data-dictionary.md §3.4` defines the two), and by name within each. When two folders
declare one slug, the one discovered second is the one refused, so the owner's copy always
wins and the outcome does not depend on the order a directory listing happens to return.

Tier-specific validation applies only to that tier. Refusal is per-app and loud; one broken
app MUST NOT prevent the node from starting.

---

## 4. Tier 1 — Lua

```
apps/<slug>/
├── app.toml
├── app.lua          REQUIRED  routes and handlers
├── lib/             your own modules
├── views/           LSP templates (.lsp)
├── static/          css, images, vendored js
├── schema.sql       OPTIONAL  tables, if you want SQL
└── migrations/      OPTIONAL
```

Full API in `spec/lua-api.md`. Summary follows.

### 4.1 Handlers

```lua
local pv = require 'privatium'

pv.get('/', function(req)
  return pv.render('index', { fills = pv.query('SELECT * FROM fill ORDER BY filled_on DESC') })
end)

pv.post('/fill', function(req)
  pv.append('fill', { drug = req.form.drug, copay_amount = req.form.copay_amount })
  return pv.redirect(url('/'))
end)
```

Handlers return `pv.render`, `pv.redirect`, `pv.json`, `pv.text`, a string, or `nil`.

### 4.2 LSP templates

`views/*.lsp` is HTML with embedded Lua, compiled to a cached Lua chunk and invalidated on
file mtime. Save, refresh, done — no build, no restart.

| Tag | Meaning |
|---|---|
| `<? ... ?>` | Execute, emit nothing |
| `<?= expr ?>` | Emit, **HTML-escaped** |
| `<?raw expr ?>` | Emit unescaped. Flagged by the linter every time. |
| `<?-- ... --?>` | Comment |

Escaping is the default and there is no flag to disable it. The framework's own markup —
`icon()`, `csrf()`, a partial from `render()` — is an HTML value `<?= ?>` emits as it is;
everything else is text (`spec/lua-api.md §4`). A view that calls no `layout()` renders
inside the framework's page frame; `layout('base')` hands the document to the app
(`§4.1` there).

### 4.3 Reads and writes

Reads are SQL: `pv.query(sql, params)`. Writes are appends: `pv.append(tbl[, id], data)`,
`pv.delete(tbl, id)`, and `pv.batch(fn)` for atomic multi-event writes.

Implementations MUST reject string-concatenated SQL at lint time. `DECIMAL` and `BIGINT`
are strings; `pv.dec()` provides exact arithmetic.

### 4.4 Where logic lives is the author's choice

Handlers, `lib/` modules, and SQL views are all legitimate homes for application logic. The
framework has no preference and imposes no split. Views suit set-shaped questions; Lua suits
branching, formatting, and anything awkward to express in SQL. Most apps use both.

The one thing worth knowing: a native mobile client that embeds the core library can hold a
full replica and evaluate SQL locally, but cannot evaluate Lua handlers. So an app whose
logic is in views has an easier path to full mobile offline than one whose logic is in Lua.
That is a consequence to be aware of, **not a reason to contort an app into SQL.** Most apps
should ignore it.

### 4.5 `schema.sql` is optional

Include it for typed tables and SQL queries; omit it to use the event log as a document
store. Every table needs `id VARCHAR PRIMARY KEY` — a ULID, unless the app keys a
singleton itself with a constant the way `apps/animals` keys its `cursor` row
(`spec/protocol.md §4.1`). Use `DECIMAL(18,2)` for money and `DATE` for dates.

Changing `schema.sql` rematerializes from the logs. Safe at any time, loses nothing; new
columns are NULL for old events.

`schema.sql` is declarations: `CREATE TABLE`, `CREATE VIEW`, `CREATE INDEX` and comments.
Nothing in it writes a row, drops or alters an object, opens a transaction or sets a
pragma — rows arrive by append, and the linter refuses the rest (`spec/cli.md §5.1`,
`PV107`), judging each statement by the actions the engine reports for it rather than by
its first word.

A declared `CREATE INDEX` is honoured: the cache recreates every index the file declares
whenever it rebuilds a table, so a query planned against `schema.sql` runs against the
same indexes on the node. `UNIQUE` — as a column constraint, a table constraint or a
unique index — is refused at load and by the linter (`PV108`): two devices' logs may both
claim a value, `spec/protocol.md §4.5` keeps both rows, and a cache cannot enforce what
the log does not guarantee. The primary key on `id` is the one uniqueness the log keeps; a
value that must be unique is part of the `id`, the way `apps/animals` keys its `cursor`.

`migrations/` is **reserved and not implemented in `pv/1`** (`spec/data-dictionary.md §3.11`).
It would be needed only when the *meaning* of stored data changes — a unit conversion, a
re-encoding — and no such case exists yet. A migration will transform events at replay time;
it will never mutate a log.

### 4.6 Sandbox

`io`, `os.execute`, `os.getenv`, `debug`, `load`, `dofile`, and `package.loadlib` are
removed. `require` is confined to the app's `lib/`. Instruction, memory, and wall-clock
limits are enforced per request. A global assigned while `app.lua` loads is the VM's
baseline; one assigned in a handler lasts that request and is seen by no other; a
load-time table mutated from a handler persists per VM and is the footgun the linter
warns about — shared state MUST go through the event log. See `spec/lua-api.md §5`.

### 4.7 Scaffolding

```bash
privatium new medtracker                    # empty Lua app
privatium new medtracker --scaffold fill    # CRUD for a table in schema.sql
```

Emits `app.lua` and `views/*.lsp` as ordinary source files. The generator has no runtime
presence; delete or rewrite anything it produced. It exists so nobody hand-writes a fifth
list-and-form screen, not to define what an app is.

### 4.8 Tier 1 on mobile

Tier 1 renders on the node. A mobile client fetches **HTML**, never Lua — so a Tier 1 app
works on every platform including iOS, is delivered dynamically, and updates the instant the
node's files change. No rebuild, no store submission, no per-user compilation.

The limit is offline. A client that cannot reach the node has cached pages and an outbox,
but **cannot render a view it has not already seen**, because the code that produces it lives
on the node.

| | Tier 1 (Lua) | Tier 2 (Web) |
|---|---|---|
| Runs on iOS and Android | ✔ | ✔ |
| Delivered dynamically, updates instantly | ✔ HTML | ✔ HTML/JS/WASM |
| Offline — cached reads | ✔ | ✔ |
| Offline — queued writes | ✔ | ✔ |
| **Offline — render a view not yet visited** | ✘ | ✔ from the local replica |
| **Offline — compute over the full dataset** | ✘ | ✔ |

Implementations MUST NOT attempt to close this by shipping the Lua interpreter a copy of a
downloaded `app.lua`. Executing downloaded native code is prohibited on at least one target
platform, and the exemption that makes dynamic delivery legal covers scripts run by the
platform web view — which is what Tier 2 already uses.

**If full offline capability on mobile is a requirement, that is a reason to choose Tier 2.**
It is not a defect in Tier 1; it is the cost of rendering on the node, which is also what buys
Tier 1 its single implementation, its absence of a build step, and its hot reload.

Two mitigations an author may use without leaving Tier 1:

- **Warm the cache.** Have the client prefetch the app's main views while connected, so the
  common screens are available offline.
- **Put set-shaped logic in SQL views.** A replica client materializes locally and can
  evaluate SQL, which every platform treats as data rather than executable code. This is an
  option, not a guideline — do not contort an app into SQL to obtain it (§4.4).

## 5. Tier 2 — custom web UI

You write the front end. The framework serves your files and exposes the data.

```
apps/<slug>/
├── app.toml            tier = "web"
├── web/                served at /a/<slug>/
│   ├── index.html      entry point
│   ├── app.js
│   ├── style.css
│   └── vendor/         whatever you vendored
└── schema.sql          OPTIONAL — typed tables, and `CREATE VIEW` for `/api/q/<view>`
```

`web/index.html` is served at the app's mount point. Everything under `web/` is served
as-is. There is no build step imposed and no framework injected — if you want a build step,
run it yourself and commit the output.

### 5.1 What you can use

**Anything.** Canvas, WebGL, WebGPU, Web Audio, WASM, SVG, Chart.js, uPlot, D3, Three.js,
Svelte, Preact, or 300 lines of vanilla JS.

The framework's own UI is HTMX and ships no client framework — that is a decision about the
*framework*, not a rule imposed on *your app*. If you want React inside your app folder,
vendor it. You will pay for it in bytes on a phone, which is your call to make.

Your app is served from the same origin as the framework, so it inherits the session and
the encryption. You do not implement auth.

### 5.2 The data API

Documented normatively in `spec/data-api.md`. Summary:

```js
// read a named view
const rows = await pv.query('v_upcoming', { days: 30 });

// read ad-hoc SQL (requires the sql permission)
const rows = await pv.sql('SELECT * FROM fill WHERE filled_on > ?', ['2026-01-01']);

// write events
await pv.append([
  { op: 'put', tbl: 'stroke', id: pv.ulid(), d: { points, color, width } }
]);

// live updates from any device
pv.subscribe(ev => redraw(ev));
```

`pv` is served by the framework at `/static/pv.js`: one unminified file with no
dependencies and no build step, meant to be read. It is optional — the endpoints are plain
HTTP and you can `fetch` them yourself.

Besides the data calls, the helper reports its own state. `pv.on('online' | 'offline',
fn)` fires when the node becomes reachable or stops being; `pv.on('outbox', fn)` fires
with `{ waiting }` each time the number of queued changes changes; `pv.waiting` is that
number now. The same changes are dispatched on the document as `pv:online`,
`pv:offline` and `pv:outbox` (with `detail.waiting`) events, which is how the page
frame's status slot follows them without importing the helper, and how two copies of
the helper on one page do not disagree.

`pv.status(text)` writes task wording — "Saved.", "3 items added." — into the frame's
status slot, the `<p id="pv-status" role="status">` in the footer (`spec/lua-api.md
§4.1`), so assistive technology announces it without a focus change. The framework
writes connection wording into the same slot, on a change of state only; the last writer
wins. A document without the slot gets nothing from the call, and may draw its own
element of that id to receive both.

### 5.3 Storage without SQL

An app that does not want tables can omit `schema.sql` entirely and use the event log as a
document store:

```js
await pv.append([{ op:'put', tbl:'state', id:'game', d: myEntireGameState }]);
const saved = await pv.get('state', 'game');   // the winning event, or null
if (saved) restore(saved.d);
```

You still get sync, replication, snapshots, and plain-text backup. You just do not get SQL
queries over it. For a drawing app or a game this is frequently the right choice.

### 5.4 Content Security Policy

Each app is served with its own CSP. By default `script-src 'self'` scoped to the app's
own path — external `.js` files work, inline `<script>` does not.

**Inline event handler attributes are script too.** `onclick`, `onsubmit`, and their
relatives are blocked by the same default. This is the most common way an otherwise correct
app fails silently: nothing errors, the handler simply never runs.

**Markup built from data is the injection the policy cannot see.** `innerHTML`,
`outerHTML` and `insertAdjacentHTML` take a literal or nothing; a value goes through
`textContent`, or `createElement` and `append` (`spec/cli.md §5.1`, `PV206`). The CSP
stops a script from running, not a string from becoming an element.

**Reactive micro-frameworks usually need a CSP-specific build.** Alpine is the common case:
its standard build compiles attribute expressions with the `Function` constructor and
therefore requires `'unsafe-eval'`, while `@alpinejs/csp` drops inline expressions in favour
of components registered with `Alpine.data()` and needs no permission at all. Reach for the
CSP build rather than the `eval` permission — granting `eval` to shorten some attributes
hands any injected string a JavaScript engine. `apps/animals` is a worked example.

Some things need more. Declare it and take the warning:

```toml
[permissions]
inline_script         = false   # allows 'unsafe-inline' — avoid; use an external file
wasm                  = false   # allows 'wasm-unsafe-eval'
eval                  = false   # allows 'unsafe-eval'; some older WASM loaders need it
remote                = []      # additional origins for script-src/img-src/connect-src
sql                   = false   # allow ad-hoc read-only SQL via pv.sql()
cross_origin_isolated = false   # COOP/COEP for SharedArrayBuffer; solo mode only
```

Every non-default permission is shown to the owner at install time in plain language,
under the heading **Privacy warning**, on the apps settings page and when the node starts.
`remote` in particular is worded as the online services the app uses, listing each origin:
"this app uses the following online services: https://example.com". Each `remote` entry
MUST be an origin — `http(s)://host[:port]` and nothing more — because it is written into
a header verbatim.

`cross_origin_isolated` is refused at load in host mode: the headers it needs are
document-level on one origin and would break every other app on the node
(`docs/frameworks.md §5.4`). It is honoured only for the solo app, and honoured in full:
the node sends `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` on every response of the origin
(`spec/protocol.md §9.3`), so a subresource the app loads from elsewhere needs CORP or
CORS of its own.

---

## 6. Tier 3 — Rust

Your binary, `privatium-core` as a dependency. See §2.3.

You may also register a Tier 3 app *inside* a host-mode node by implementing the plugin
trait and compiling it in — but this means a custom build of the node binary, which is why
Tier 2 exists. Reach for Tier 3 when you need server-side work the HTTP surface cannot
express: a serial port, a scheduled job, a filesystem watcher, a non-HTTP protocol.

`privatium-core` API surface, stable across `api = 1`:

| Area | What you get |
|---|---|
| `Node::open` / `close` | Data root, identity, materialization; `close` flushes node-local state and releases the root |
| `open_app` | Your own app, with no folder: its slug and its `schema.sql` text (§2.3) |
| `append` / `append_batch` | One event, or a batch that lands whole or not at all, with `seq`/`lam`/`ts`/`dev` stamped by the node |
| `query` / `subscribe` | Sandboxed SQLite reads with bound parameters, rows typed as `spec/data-api.md §1` types them; the app's event stream |
| `serve_discovery` / `pair` | mDNS and UDP, started together (`spec/protocol.md §6.5`); `pair(ttl)` opens a pairing window for devices and `pair_node(ttl)` one for a node, each handing back the code in both renderings, the URL and the expiry (`§7`) — the device registry is what a completed pairing writes; `peers()` lists the cluster's other nodes discovery has seen and `strangers()` the rest (`§6.1`) |
| `join(url, code)` | Join the cluster of the node at `url`, whose owner opened a window for a node and read out `code` (`spec/protocol.md §2.3.1`, `§7.4.2`): the node dials `<url>/ws/pair`, both sides prove their keys, and whichever side the exchange admits adopts the other's cluster — this node discards the cluster it founded only while it is disposable (`§2.3`), or is re-admitted to its own after its certificate expired. Answers which side joined, the cluster and the peer, or the refusal by name; refused while sync would be unsafe, and never over a session |
| `start_sync` / `sync_now` | LAN peers over the encrypted channel; `start_sync` starts an idempotent background engine, and blocking `sync_now` returns `SyncReport` with peer ID, URL, completion, durable pulled/pushed line counts, and refusals |
| `auth_layer` | Tower middleware enforcing session and grants. `core::handle` applies it itself, so every adapter gets it without doing anything (`docs/decisions/0003`); an embedder wraps their own router with it, as §2.3 shows, and the layer reads the peer from axum's `ConnectInfo`. A request whose peer it cannot see is refused, naming the missing call, so a router served without `into_make_service_with_connect_info` admits nobody rather than everybody; a call an embedder makes in-process inserts the `Peer` extension the framework's own adapter inserts |
| `snapshot` / `restore` | Manual snapshot and three-tier restore |

A build that does not implement an area — one that says so in `--version`
(`spec/cli.md §1`), as `pv/1 (partial: phase 2)` does — MUST keep the method and answer
it with a typed error naming the phase the area arrives in. It MUST NOT return success from a no-op: a program built on an `Ok` from `start_sync`
would believe it was syncing.

`append` and `append_batch` refuse, writing nothing, when one append's log lines exceed
`api.max_body` — a batch crosses to another node whole or not at all
(`spec/protocol.md §4.1`). Split the batch, or store less in one row.

`start_sync` and `sync_now` are implemented; both refuse expired or revoked membership.
`sync_now` uses the running engine, or a temporary thread and runtime if it is stopped.
It drains all incoming ranges before returning. `refresh`, `refresh_app` and the append
methods also drain pending ranges before using the cache; `query` reads the cache as it
stands and drains nothing. An idle embedder sees incoming data at its next draining call;
subscribe to `sync_events()` and refresh on each wake to receive it promptly. `subscribe(app)` includes accepted synced events, even below a
previous Lamport mark. Lua callbacks run outside the node lock through
`Handler::fire_pending` or `Handler::drain_sync`. Closing or dropping the node stops
sockets and joins the engine before releasing the data-root lock.

---

## 7. The SQL sandbox (Tiers 1 and 2)

App SQL runs on a connection of its own to `cache/<slug>.sqlite`, and that connection MUST
be, at minimum:

1. **Opened read-only** at the file (`SQLITE_OPEN_READONLY`).
2. **`PRAGMA query_only = 1`**, so no statement that writes can run.
3. **Behind an authorizer** that refuses every write, every `PRAGMA`, `ATTACH` and
   `DETACH`, `VACUUM`, and `load_extension()`, and allows `SELECT`, reads, and the
   framework's own functions. Extension loading is also disabled at the API, so the
   refusal by name is a second fence, not the only one.

This is not optional hardening — a connection that can `ATTACH` can read
`identity/node.key` into a table, and one that can `VACUUM INTO` can write it anywhere.

The one attachment is the framework's own, made before the authorizer is installed:
`cache/_sys.sqlite` as `sys`, read-only like `main`, so an app reads `sys.v_app_nav` and
the other views of `spec/data-dictionary.md §4` on the same connection. It cannot be
detached; nothing else can be attached. The connection also carries `pv_param`, the
function a view's `$name` placeholder becomes (`spec/data-api.md §1`).

**The boundary is the connection.** The framework's own connection — the one that reads the
log, materializes, restores and writes snapshots — is a separate handle on the same file and
is never handed to app code. SQLite's locking lets the two coexist: the framework writes
while an app reads, and a rebuild is an ordinary transaction that a reader sees whole or not
at all. There is no privileged window to open and close, and an implementation MUST NOT
give app code the framework's handle under any setting.

Applying a newly appended event is `DELETE` plus `INSERT` over values the framework already
holds, on the framework's connection.

`DECIMAL` columns are text under the `decimal` collation, and the exact arithmetic over
them is the framework's `decimal_add`, `decimal_sub`, `decimal_mul`, `decimal_cmp` and
`decimal_sum` (`spec/data-dictionary.md §2.1`); an app's `SUM()` over such a column is a
float and is what `privatium lint` exists to catch.

Tier 3 runs in your own process and is not sandboxed. That is the trade: full power, full
responsibility.

---

## 8. Lifecycle

```
folder appears in apps/
  → parse app.toml                       (fail → app.load_failed, stop)
  → validate slug, api, tier             (fail → app.load_failed, stop)
  → tier 1: load app.lua in a sandboxed VM, register routes, compile views/, hash
  → tier 2: verify web/index.html exists; compute CSP from [permissions]
  → tier 3: (linked at compile time; index entry only)
  → upsert sys_app                       (event)
  → materialize from data/<slug>/        (if the app has tables)
  → mount, advertise subtype
```

**A stop still writes the row.** `sys_app` is keyed by the slug, and the slug equals the
folder name (§3.1), so the folder name is the row's key before anything is parsed. Every
folder whose name is a valid, unreserved slug has a row whether or not it loaded: a stop at
any step above writes it with `last_error` set and whatever the manifest yielded
(`spec/data-dictionary.md §3.4`), which is what lets settings say why an app is not
running. A folder whose name could never be a slug has no row to carry the error and gets
the `app.load_failed` audit alone. The upsert step above is where a clean load writes the
row; a refused one writes it where it stopped. The row is amended only when it would
change — a start that finds nothing different appends nothing.

---

## 9. Publishing

An app is a folder, so distribution is a zip or a git repository. There is no registry in
`pv/1`, deliberately: a registry implies curation, curation implies a trust signal, and a
false trust signal is worse than none.

**Warn the owner at install.** A Tier 1 app's Lua runs on their node against their data. A
Tier 2 app's JavaScript runs in their browser on their session's origin. Both are
sandboxed from the filesystem, but treat installing a third-party app the way you would
treat running a script someone emailed you — because that is what it is.

Include `sample/seed.jsonl` (synthetic events only) so an owner can see the app populated.

A seed is one event envelope per line (`spec/protocol.md §4.1`). A node loading it takes
`op`, `tbl`, `id` and `d` from each line and discards `seq`, `lam`, `ts`, `dev` and `app`:
the events are appended through the node's own log, as the node's, with every envelope
field minted afresh — never copied as another device's file (`spec/protocol.md §3.1`).
Loading is an explicit owner action and MUST NOT happen on its own, and it MUST be refused
when the app's log already holds an event from any device: a seed populates an empty app
or nothing.

---

Copyright © 2026 Gabriel Mongefranco
