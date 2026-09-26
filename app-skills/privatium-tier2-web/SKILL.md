---
name: privatium-tier2-web
description: Write Tier 2 Privatium apps with your own HTML, CSS, JavaScript, or WASM in a web/ directory, using the pv.js data API for storage and sync. Covers the API surface, CSP permissions, offline behaviour, and library choice. Load for games, canvas, charts, animation, or any app with its own interaction design.
---

# Privatium Tier 2 — Custom Web UI

You write the front end; the framework serves it and handles storage, sync, auth, and
encryption. `web/index.html` is served at the app's mount point. You supply the whole UI.
On the LAN, the bootstrap loads it through the encrypted channel and pins external
scripts and stylesheets with integrity hashes (`spec/protocol.md §8.3`).

```
apps/<slug>/
├── app.toml          tier = "web"
├── web/
│   ├── index.html
│   ├── app.js
│   ├── style.css
│   └── vendor/       whatever you vendored
└── schema.sql        OPTIONAL
```

## The API

```js
import { pv } from '/static/pv.js';

const rows = await pv.query('v_upcoming', { days: 30 });   // $days in the CREATE VIEW
const rows2 = await pv.sql('SELECT * FROM fill WHERE drug = ?', ['Example']); // needs permission
const row  = await pv.get('state', 'game');               // the winning event, or null

await pv.put('state', 'game', myState);
await pv.append([{ op:'put', tbl:'stroke', id: pv.ulid(), d:{ points } }]);
await pv.del('stroke', id);

for await (const ev of pv.events({ tbl: 'stroke' })) apply(ev);   // the log, in order; dels too

const stop = pv.subscribe(ev => { if (ev.tbl === 'stroke') redraw(ev); });
pv.on('resync', reload);   // the node rebuilt its cache: re-read
pv.on('offline', () => …);  pv.on('online', () => …);  pv.on('rejected', e => …);

pv.ulid();  pv.url('/path');  pv.node();  pv.lam;  pv.online;
```

`pv.js` is optional on loopback, in a native shell and on an HTTPS origin, where every
endpoint is plain HTTP under `/a/<slug>/api/` (`/api/` in solo mode). On a plain-HTTP LAN
origin the API is reachable only through the encrypted channel the page holds
(`spec/protocol.md §8.3`), a plain `fetch` of it is refused, and `pv.js` is the way — it
uses the channel when there is one and `fetch` otherwise, so your code is the same on
every origin. It is under 12 KB, unminified and meant to be read — open it. A view may read
`$name` placeholders, bound from the query string of `/api/q/<view>`; a key the view does
not read is refused, and elsewhere the placeholder is NULL. `sys.v_app_nav` and the other
`sys.v_*` views are readable through `pv.sql`.

Full-page transitions create fresh documents with the destination app's declared CSP
and fresh module initialization. A full-page form's response can wait in node memory
while that document reconnects; the framework carries only a response reference in
per-tab storage and never resubmits the form automatically (`protocol.md §8.3.1`).
Keep that mechanism in the framework. It is not an app outbox or a deduplication table.

Same-origin external scripts and stylesheets are hashed through the channel. If an app
has an approved remote resource permission, its HTML must supply the resource's
integrity hash; the channel cannot proxy another origin. Imported modules still lack
per-import integrity, and every plain-HTTP load exposes client code and stored device
keys to active replacement (`protocol.md §7.7`). Prefer vendored resources.

`pv.subscribe` includes events received from other nodes, even below the current
Lamport mark. A reconnect using `after=` cannot replay such an event if it arrived
while disconnected. Re-read state after reconnect and on `resync`.

## MUST

- Use `pv.url()` for internal links — hardcoded `/a/<slug>/` breaks solo mode
- Keep `DECIMAL` and `BIGINT` values as strings; `pv.query` deliberately does not convert them
- Put scripts in external files — the default CSP is `script-src 'self'`, no inline
- Declare every non-default permission in `app.toml` and be able to justify it
- Handle `pv.subscribe` events from *other* devices, not just local input, and
  `pv.on('resync')` by re-reading — a `del` arrives as an event like a `put`
- Vendor libraries into `web/vendor/`; never load from a CDN
- Send the API JSON if you `fetch` it yourself — a POST is read only as `application/json`
- Write the whole document: `<html lang>`, a `<title>`, one `<h1>`, a `<main>`, a labelled
  `<nav>`, a zoomable viewport (never `user-scalable=no`), your own
  `prefers-reduced-motion` guard. The channel supplies transport, not your UI.
- Size a `<canvas>` in CSS and match its backing store to
  `clientWidth × devicePixelRatio` in a resize handler, with `ctx.setTransform(r, 0, 0,
  r, 0, 0)` — sizing from `innerWidth` draws past the viewport on every HiDPI display,
  which is every Windows laptop at 125 % and every phone. `apps/sketch/web/sheet.js` is
  the worked example, and it shows the harder case too: a fixed coordinate space, so the
  same drawing lands on the same pixels on a phone and a desktop, carried in the context
  transform rather than in the canvas's pixel size. Give the canvas an `aria-label`; a
  keyboard alternative is still yours to build.

## MUST NOT

- Set `seq`, `lam`, `ts`, `dev`, or `app` on an event — the server rejects it
- Reuse an id after deleting the row — a minted ULID is never the key of another row; the
  server answers 409. Mint a fresh one
- Use `localStorage` for anything that should survive a device — that is what the log is for
- Assume you are online; `pv.query` throws `PvOffline`
- Implement your own outbox deduplication, transaction IDs, or acknowledgement protocol.
  ULIDs already make replay idempotent — adding these can create the divergence they were
  meant to prevent. A replay carries the mark it was queued at and, for every row the page
  read or wrote, the rank it last saw; the node judges it against the log as it appends:
  already there, dropped; the row moved since, refused as a conflict rather than written
  over the newer change; nothing, appended (`spec/data-api.md §2, §6`). A row the page saw
  only through a view carries the mark alone — read it with `pv.get` first if the edit
  must never overwrite an unseen change. An entry the node or the helper refuses reaches
  you as `pv.on('rejected')` — show it, since it is the user's work.
- Add a CSRF token, a CORS header or any other credential handling to the API. It is
  same-origin by construction (`spec/data-api.md §2.1`)
- Construct absolute URLs to other endpoints. A browser client has exactly one origin.
- Request `permissions.remote` unless the app genuinely must call out. It is the one thing
  this project exists to avoid, and the installer says so to the owner.

## Storage without SQL

Omit `schema.sql` and the log is a document store:

```js
await pv.put('state', 'game', entireGameState);
```

No validation, no SQL — but full replication, snapshots, and plain-text backup. For a game
or a drawing app this is usually right. With a `schema.sql`, every write is typed — a
`DATE` typed as `3/9/2026` lands as `2026-03-09`, a `DECIMAL(18,2)` sent as `12.5` as
`"12.50"` — and `NOT NULL` and `CHECK` refuse the whole batch naming the event's index.

**Two worked examples, one per choice.** `apps/sketch` is the no-SQL one: no `schema.sql`
at all, every mark a document. `apps/pantry` is the SQL one: three tables, seven views read
through `pv.query`, exact decimals kept as strings end to end, and forms whose errors are
rendered in place — the client checks first and the node checks again from the DDL. Read
`apps/pantry` when your app has records, numbers or forms; read `apps/sketch` when it has
none of those.

**A total is a view, not a column.** Anything you could keep updated — a balance, a count,
a running sum — is a value two devices can disagree about, and the log has no way to merge
two claims about one row. Sum the rows in a view instead (`decimal_sum()` for a `DECIMAL`),
and let a disagreement show as the number it really adds up to. `apps/pantry` reports a
negative balance rather than clamping it, which is why it can say what happened.

## Permissions

```toml
[permissions]
inline_script          = false   # prefer an external file
wasm                   = false   # 'wasm-unsafe-eval'
eval                   = false   # 'unsafe-eval'
remote                 = []      # extra script/img/connect origins
sql                    = false   # ad-hoc read-only SQL via pv.sql()
cross_origin_isolated  = false   # SOLO MODE ONLY — see privatium-games
```

## Libraries

Zero build, vendored: Alpine.js, Datastar, VanJS, Preact+htm, Lit, PixiJS, Chart.js, uPlot,
D3, wasmoon.

Build step you run and commit: Svelte, SolidJS, Vue SFC, React.

Never: Appsmith, Budibase, Saltcorn, NocoBase, Grist, Retool — those are platforms with
their own backends, not libraries. Webix is GPL/commercial dual-licensed; avoid.

Full matrix with reasoning: `docs/frameworks.md`.

## Verify

```bash
privatium new <slug> --tier web   # app.toml, web/index.html, web/app.js importing pv.js
privatium dev --app <slug>        # static files are served fresh; no restart, no build
privatium lint apps/<slug>        # index.html is held to PV401–PV407 as a whole document
```

Spec: `spec/data-api.md`; `reference/pv-js.md` here lists what `pv.js` exports at this
version, `reference/endpoints.md` the API, `reference/anti-patterns.md` every Tier 2 rule
failing and passing.
