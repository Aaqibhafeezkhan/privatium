<!--
This file is part of Privatium
spec/data-api.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-10-04
Summary: NORMATIVE. The HTTP data API that custom-UI (Tier 2) apps build against.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Data API — `api = 1`

The interface between an app's own front end and the framework's storage. This is what
makes Tier 2 possible: a replicated, syncing, backup-safe database reachable from any
JavaScript, WASM, or native client, with authentication already handled.

All endpoints are under `/a/<slug>/api/` and are scoped to that app. An app cannot read or
write another app's data through this API.

This namespace is reserved by the framework beneath an app's mount point
(`spec/protocol.md §9.1`), for either tier: a Tier 2 app MUST NOT serve its own files at
`web/api/`, and a Tier 1 app's routes never see the path. In solo mode the mount is `/`,
so the API is at `/api/…` and `/api/v1/*` stays the framework's. It is versioned by
`app.api` in the manifest rather than by a path segment, so that an app declares the
contract it was written against.

All endpoints require a live session (`spec/protocol.md §8`): on a plain-HTTP origin the
channel of `§8.3` carries it, on loopback the owner's own standing does, and no token
handling is required in app code — see §2.1 for what keeps another site from riding it.

Every response is JSON unless a section says otherwise, and a refusal is a JSON object
with `error` — the status and a sentence — plus `index` when one event of a batch is at
fault and `column` when one value is (§2). Every response carries the headers of
`spec/protocol.md §9.3`.

---

## 1. Read

### `GET /a/<slug>/api/q/<view>`

Run a view defined in `schema.sql` (`CREATE VIEW`, permitted by `PV107`). Query-string
parameters bind to `$name` placeholders.

```
GET /a/medtracker/api/q/v_upcoming?days=30
```

```json
{
  "view": "v_upcoming",
  "columns": [
    {"name":"drug","type":"VARCHAR"},
    {"name":"due_on","type":"DATE"},
    {"name":"copay_amount","type":"DECIMAL(18,2)"}
  ],
  "rows": [
    {"drug":"Example","due_on":"2026-09-04","copay_amount":"12.50"}
  ],
  "lam": 8830
}
```

- **How `$name` works.** SQLite refuses a parameter inside a view — "parameters are not
  allowed in views" — so the framework rewrites every `$name` in `schema.sql`, outside
  strings and comments, to `pv_param('name')`, a scalar function it registers on every
  connection. On this endpoint the function answers with the query-string value of the
  same name, as text; a name the query string does not bind is NULL; anywhere else —
  `pv.query`, `/api/sql`, a rebuild — every placeholder is NULL. A view's placeholders are
  listed by `/api/schema`, and a query-string key that is neither a placeholder of the
  view nor `limit`/`offset` is refused (400) naming the placeholders, so a typo is not a
  silent NULL.
- `columns` carries the declared types from `schema.sql` so a client can format
  correctly: the type of the column the result column originates in, through the view and
  through an alias; a computed column — `count(*)`, `decimal_sum(x)` — has `"type": null`.
- **Typing.** A value is typed by that origin (`spec/data-dictionary.md §2.1`): `DECIMAL`
  and `BIGINT` are JSON **strings**, `BOOLEAN` a boolean, `JSON` (`VARCHAR[]`) its value,
  NULL is `null`. A computed column arrives by storage class — an integer as a number, a
  float as a number, text as a string — so `decimal_sum()` is a string and `count(*)` a
  number. JSON numbers are doubles in most parsers and would silently corrupt money.
- `lam` is the Lamport high-water mark the result reflects, read before the query runs.
  Pass it to `/stream` to resume without a gap; it can only understate, never overstate,
  so a resume from it may repeat an event it already showed — idempotent — and never
  skips one.
- Pagination: `?limit=` and `?offset=`. Default limit 1000, maximum `api.max_rows`
  (10000): a larger `limit` is clamped, a `limit` or `offset` that is not a whole number
  is refused.
- A statement runs under the node's statement deadline (§7).

### `POST /a/<slug>/api/sql`

Ad-hoc read-only SQL. **Requires `permissions.sql = true` in `app.toml`**; without it
the answer is 403 naming the permission.

```json
{ "sql": "SELECT drug, sum(copay_amount) AS total FROM fill WHERE filled_on > ? GROUP BY 1",
  "params": ["2026-01-01"] }
```

```json
{ "columns": [{"name":"drug","type":"VARCHAR"}, {"name":"total","type":null}],
  "rows": [{"drug":"Example","total":12.5}], "lam": 8830 }
```

- The body is `application/json` (§2.1) — `sql`, and `params` as an array of scalars:
  a string binds as text, an integer as an integer, another number as a real, a boolean
  as `1`/`0`, `null` as NULL; an object or an array in `params` is refused naming its
  index.
- Statement MUST be a single `SELECT` or `WITH ... SELECT`. Anything else is rejected —
  by its first keyword, and again by the connection, which refuses every write, `PRAGMA`,
  `ATTACH`, `DETACH` and `load_extension()` (`spec/app-contract.md §7`).
- Runs on the sandboxed connection, with `sys` attached (`spec/data-dictionary.md §4`).
  File functions are unavailable regardless of this permission.
- Parameters are bound, never interpolated. Implementations MUST reject a request
  containing `?` placeholders with a mismatched `params` length rather than substituting.
- Rows and `columns` are typed as `/api/q` types them; `lam` likewise. Note the `sum()`
  above is a float, which is what `privatium lint` exists to catch (`PV302`):
  `decimal_sum()` is the string.
- Rate limited. Default 20 requests per second per session (`api.sql_rate`); past it the
  answer is 429 with `Retry-After: 1`, and nothing is read.

### `GET /a/<slug>/api/row/<tbl>/<id>`

A single row, as its **winning event** (`spec/protocol.md §4.5`): the log line, byte for
byte, with `d` holding the row — the same answer for a declared table and for a table that
exists only as a `tbl` in the log (`spec/app-contract.md §5.3`). Returns `404` if absent
or tombstoned.

```json
{"seq":12,"lam":8831,"ts":"2026-09-03T14:03:11.412Z","dev":"k7m2q9xf","app":"sketch","op":"put","tbl":"state","id":"game","d":{"level":7}}
```

### `GET /a/<slug>/api/events`

Raw event access, for apps that use the log as a document store rather than as tables.

```
GET /a/medtracker/api/events?tbl=state&id=game
GET /a/medtracker/api/events?after=8800&limit=500
```

Returns raw event lines as NDJSON (`application/x-ndjson`), byte-identical to what is on
disk, in `(lam, ts, dev)` order — the order of `§4.5`, and the one a client resuming from
`after=` needs. `tbl` and `id` filter; `after` keeps the events with `lam` greater than
it; `limit` and `offset` page as `/api/q` does. Lines `§4.4` would reject are not
returned, so what this hands out is what the cache was built from.

---

## 2. Write

### `POST /a/<slug>/api/events`

Append a batch. Atomic: all lines or none — one write of one batch of the log, its first
line carrying the count, so a crash that lands it short leaves lines every reader skips
(`spec/protocol.md §4.1`).

```json
{
  "events": [
    {"op":"put","tbl":"stroke","id":"01J9YQ...","d":{"points":[[0,0],[4,9]],"color":"#00274C"}},
    {"op":"del","tbl":"stroke","id":"01J9YP..."}
  ]
}
```

The client supplies only `op`, `tbl`, `id`, and `d` — and, on a replay, `base` (below).
The framework stamps `seq`, `lam`, `ts`, `dev`, and `app` — a client MUST NOT set these
and the server MUST reject a request that does (`PV304`), as it rejects any other field.

Response:

```json
{ "appended": 2, "lam": 8832, "ts": "2026-09-03T14:03:11.412Z", "dev": "k7m2q9xf", "ids": ["01J9YQ...", "01J9YP..."] }
```

`lam` is the last event's — the app's high-water mark now. The batch is one batch of the
log (`spec/lua-api.md §3.3`): one `ts`, contiguous `seq` and `lam`; `ts` and `dev` are the
batch's, so a client knows the rank (`spec/protocol.md §4.5`) of what it wrote.

Constraints, each refusing the whole batch and naming the event's `index` from 0:

- Maximum `api.max_batch` (1000) events per batch (400), maximum `api.max_body` (4 MB)
  per request (413, before the body is read when the length is declared). The log lines
  a batch produces are bounded by `api.max_body` too (413, nothing written), since a
  batch reaches another node whole or not at all (`spec/protocol.md §4.1`).
- `op` is `put` or `del`; `tbl` is a table name; a put carries `d`, a JSON object, and a
  del carries none.
- `id` MUST be a valid ULID. Mint one client-side with `pv.ulid()` or server-side by
  omitting it from a put (the server mints and returns it); a del names its id.
- A minted id that keyed a row whose winning event is a tombstone is never the key of
  another row (`spec/protocol.md §4.6`): a put under it is 409, whether the tombstone is
  in the cache or earlier in the same batch. A repeated del is a replay and lands. This
  endpoint is where `§4.6` is enforced; a server-side caller may re-assert a key it chose.
- If the app has a `schema.sql`, every value that names a declared column is normalized
  as `spec/lua-api.md §3.3` describes — `12.5` lands as `"12.50"` in a `DECIMAL(18,2)`,
  `3/9/2026` as a `DATE` — and a value that is not its type is refused (400) naming
  `index` and `column`; then `NOT NULL` and `CHECK` constraints are validated before the
  append, by the author's own DDL, and a violation is refused (400) naming `index`. The
  same holds for `pv.append` and the seed: there is one write path.
- If the app has no `schema.sql`, `d` is stored as-is with no validation.

The body MAY also carry `node` and `app`: the `id` and the `app` of `/api/node` (§4) as
the client last learned them. A `node` that is not this node's ID, or an `app` that is not
this mount's, is 409 and nothing is appended. `pv.js` sends both with every write, so an
entry queued against one node or one app is never written into another (§6), and the
check is the node's own, made as it appends, rather than a read the client made a moment
earlier.

A replay MAY also carry `since` — the mark `pv.lam` held when the entry was queued — and,
per event, a `base`: `{"lam", "ts", "dev"}`, the rank of the row's winning event as the
page last saw it, from `/api/row`, `/api/events`, the stream, or this response. With
either present the node decides, under the lock in which it appends, what
`spec/protocol.md §10.6` says. For each event it reads the row's events ranked past
`base`, or past `since` where the event has none. One equal to the event — compared as the
node would store it, so a typed app's normalization does not tell them apart — means it
landed; any other means the row moved, and the whole batch is 409 with the event's
`index` and `conflict: {tbl, id}`, nothing appended; none means it is fresh. A batch every
event of which landed answers 200 with `appended: 0`; otherwise the batch is appended
whole, and `§4.5` folds an event that had already landed into the same row. `base` is
stripped before the append and never reaches the log. With neither `since` nor `base` the
write is unconditional: last write wins by arrival.

A Tier 1 app's `pv.on('append')` fires for an API append as for any other of this node's
(`spec/lua-api.md §3.4`), after the response is decided.

### 2.1 Same origin, no token

Tier 1 forms carry `csrf()` because a form is a request any site can make. The data API
takes a different fence, because `pv.js` has no page to read a token from and a native
client has no page at all:

- A POST is read only as `application/json`; anything else is 415. A page on another
  origin cannot send that content type without a CORS preflight, which the node never
  answers.
- A request a browser marks `Sec-Fetch-Site: cross-site` is refused (403) on every
  route, before anything is read. A same-origin page, a navigation, and a client that is
  not a browser carry no such marking.
- A GET answers JSON or a stream that no cross-origin page can read: there is no CORS
  header, `nosniff` is on every response (`spec/protocol.md §9.3`), and `EventSource` is
  bound by the same rule as `fetch`.

An app never sends a token to this API, and an implementation MUST NOT require one.

### Nothing else

There is no framework-defined action endpoint. An earlier draft specified
`POST /a/<slug>/api/x/<action>` for invoking named server-side actions; that belonged to the
declarative tier and was removed with it.

An app has exactly one tier (`app.toml` `tier`), so there is no Tier 1 handler for a Tier 2
front end to call. A Tier 2 app that wants server-side logic writes it against
`spec/app-contract.md §6` (Tier 3) or performs it client-side.

---

## 3. Live updates

### `GET /a/<slug>/api/stream`

Server-Sent Events. Emits every event appended to this app **from any device**, including
events arriving via sync from another node.

```
GET /a/medtracker/api/stream?after=8830
```

```
event: append
data: {"seq":12,"lam":8831,"ts":"2026-09-03T14:03:11.412Z","dev":"b3nn8t2q","app":"medtracker","op":"put","tbl":"fill","id":"01J9...","d":{...}}

event: resync
data: {"reason":"rematerialized","lam":8900}

event: ping
data: {"lam":8900}
```

| Event | Meaning |
|---|---|
| `append` | One new event, as its log line — the same bytes `/api/events` returns. Live delivery includes synced events below an earlier Lamport mark; reconnect limits are described below. |
| `resync` | State changed underneath you. Re-query. `reason` is `rematerialized` — the cache was rebuilt: a changed `schema.sql`, a restore, a line that reached the log some other way — or `lagged`, this stream fell too far behind and the backlog was dropped. `lam` is the high-water mark now. |
| `ping` | Keep-alive, every 30 seconds, carrying the high-water mark. Its stat of the app is what notices a log that grew on an otherwise idle node, so a resync follows it without another request. |

`after=` is a `lam`: the events with a greater `lam` are sent first, from the log, then
live ones as they land — subscribed and read under one hold of the node's lock, so nothing
appended in between can be missed, and nothing is sent twice. Live delivery MUST include
accepted synced events regardless of their `lam`; the high-water mark never decreases.
Without `after` the stream starts from now. A client keeps the greatest `lam` it saw and
reconnects with it. A Lamport cursor cannot replay a synced event at or below that mark
which arrived while the client was disconnected. Clients MUST re-read state after a
reconnect and on `resync`; `after=` alone is not a gap-free cross-device resume cursor.
`HEAD` answers the headers alone. At most `api.max_streams` streams per device;
past it the answer is 429.

The body is the streaming response body of the core interface (`docs/decisions/0003`):
each event is one frame, sent when it happens, never buffered, and never routed through
the Lua host.

SSE rather than WebSocket because it reconnects automatically, survives proxies, and needs
no framing. Apps needing bidirectional streaming may open `/ws` and speak the session
protocol directly.

**A host MUST serve this endpoint as SSE.** A host MAY additionally offer a long-poll
fallback, and a client MAY negotiate it with `Accept: application/json` plus `after=`,
receiving the same event objects in a JSON array and reissuing the request on each response.
The fallback exists because custom-scheme streaming inside a platform webview — WKWebView in
particular — is unproven; it is not an invitation to skip SSE. `pv.js` selects between them
and apps see no difference (`§5`). This build implements SSE only.

**Note:** a quick Cloudflare tunnel does not pass SSE. This does not affect LAN, Tailscale,
Let's Encrypt, onion, or native transports.

---

## 4. Metadata

### `GET /a/<slug>/api/schema`

Tables, columns, types, and available views. Lets a generic client render an
app it has never seen.

```json
{ "tables": [ { "name": "fill", "columns": [
      {"name":"id","type":"VARCHAR","not_null":true},
      {"name":"drug","type":"VARCHAR","not_null":true},
      {"name":"copay_amount","type":"DECIMAL(18,2)","not_null":false} ] } ],
  "views": [ { "name": "v_upcoming", "params": ["days"] } ],
  "schema_hash": "…" }
```

`id` is listed first on every table. `params` are a view's `$name` placeholders (§1).
An app with no `schema.sql` has empty `tables` and `views`.

### `GET /a/<slug>/api/node`

Node ID, device ID, display name, the app's slug, `solo` flag, the number of paired nodes
(`peers`, counted as `spec/lua-api.md §3.4` counts them), restore tier in use. No
application data.

```json
{ "id": "k7m2q9xf", "dev": "k7m2q9xf", "name": "Study", "app": "medtracker", "solo": false, "peers": 0, "restore_tier": 3 }
```

`name` falls back to the Node ID while the owner has set none (`spec/protocol.md §9.2`);
`dev` is the device the request is authenticated as — this node's own on loopback, the
paired device's through the channel (`spec/protocol.md §8.3`); `app`
is the slug this API is scoped to, which a page at a solo mount cannot read from its path
and which `pv.js` keys its outbox entries by (§6); `restore_tier` is `null` for an app
this node has not materialized.

---

## 5. The `pv.js` helper

Served at `/static/pv.js`. One unminified file, meant to be read — there is no minifier
in the runtime path — with no dependencies, no framework, no build step.
**Optional** on loopback, in a native shell and on an origin `spec/protocol.md §8.2`
exempts, where every endpoint is plain HTTP and `fetch` works fine. On a plain-HTTP LAN
origin the API is reachable only through the channel (`§8.3`, `§8.4`) and a plain
`fetch` of it is refused naming this helper; `pv.js` uses the channel when the page has
one and `fetch` otherwise, and an app sees no difference.

```js
import { pv } from '/static/pv.js';

const rows  = await pv.query('v_upcoming', { days: 30 });
const rows2 = await pv.sql('SELECT * FROM fill WHERE drug = ?', ['Example']);
const row   = await pv.get('state', 'game');          // the envelope, or null

await pv.append([{ op:'put', tbl:'state', id:'game', d: state }]);
await pv.put('state', 'game', state);          // sugar for the above
await pv.del('stroke', id);

for await (const ev of pv.events({ tbl: 'stroke' })) apply(ev);   // the log, in order

const stop = pv.subscribe(ev => { if (ev.tbl === 'stroke') redraw(ev); });
pv.on('resync', reload);                       // re-read; the node rebuilt its cache

pv.on('outbox', ({ waiting }) => say(waiting));   // the queue grew or drained (§6)
pv.status('Saved.');                           // task wording in the page frame's status line

pv.ulid();        // client-side ULID
pv.node();        // cached /api/node
pv.url('/path');  // beneath the mount — the only way to build an internal URL
pv.lam;           // the last high-water mark seen; what a reconnect resumes from
pv.waiting;       // how many queued changes wait for the node
```

`pv.query` and `pv.sql` return plain arrays of objects. `DECIMAL` columns arrive as
strings; the helper does **not** convert them to numbers, because that is exactly the bug
this design exists to prevent. Use a decimal library or integer cents in your own code.

- `pv.get` returns the winning event (§1) or `null` for a 404, so "is there a saved
  game?" is one call.
- `pv.events(filter)` is an async iterator over `/api/events` — `tbl`, `id`, `after`,
  `limit` — one envelope per line; a `del` is an event too, so apply them in order.
- `pv.subscribe(fn)` opens the stream on the first subscriber and closes it on the last;
  `fn` receives each `append` envelope. The helper reconnects itself, by hand rather than
  by `EventSource`'s retry, so that `after=` carries the last `lam` seen; it also tracks
  `lam` from every query, so a subscription opened after a query resumes from what the
  query reflected.
- `pv.on(event, fn)` for `online`, `offline`, `outbox` (with `{ waiting }`, each time the
  number of queued entries changes — §6), `resync` (with the event's data) and
  `rejected` (an outbox entry the node refused — §6); it returns an unsubscribe function.
  The connection and outbox changes are also dispatched on the document as `pv:online`,
  `pv:offline` and `pv:outbox` events (`spec/app-contract.md §5.2`), so a listener that
  did not import the helper — the page frame's status line — follows them too.
- `pv.status(text)` writes `text` into `<p id="pv-status" role="status">` when the page
  has one (`spec/lua-api.md §4.1`); a page without it gets nothing.
- `pv.append` returns the response of §2, or `{ queued: true, ids }` when it went to the
  outbox; a put with no `id` is minted one client-side before it is sent or queued, so a
  replay carries the same id. `pv.flush()` replays the outbox now and resolves when the
  pass is over.
- The helper keeps the rank of every row it handed the app — through `pv.get`, `pv.events`
  and `pv.subscribe` — and of every row it wrote, and a queued edit of such a row carries
  it as the event's `base` (§2, §6). A row the app saw only through a view has no rank the
  helper can know, and an edit of it carries the mark alone; an app that wants the
  guarantee for such a row reads it with `pv.get` first.
- The mount is read from the page's path — `/a/<slug>/` or, in solo mode, `/` — and
  exposed as `pv.mount`. `pv.url()` is the only URL construction point (`§6`).

---

## 6. Offline

The API is local. In a native shell or an installed PWA it is served by the node process on
the same device, so it works with no network at all.

When the node is remote and unreachable, `pv.append` queues to an outbox and replays on
reconnect; `pv.query` throws `PvOffline` and the app decides what to show. The helper
exposes `pv.online` and a `pv.on('online' | 'offline')` event. The outbox is one queue in
the page's memory, mirrored to `localStorage` while storage is available — a page without
it still queues for its own lifetime — as one key per entry beneath the mount, so two
pages over one storage never write over each other's entries and either page replays
both. Each entry is keyed by a ULID and holds the events exactly as they will be sent —
each with the `base` the helper knew for its row (§5) — the mark `pv.lam` held when it was
queued, and the `app` and the node `id` that `/api/node` named (§4). A fetch that fails to reach the node marks the helper offline; the browser's
own `online` event, or any request that succeeds, marks it back and replays the queue in
order, one entry at a time — after asking `/api/node` again, so the replay knows which
node and which app the origin serves now, not what it served when the page loaded.

**Replay is idempotent and needs no bookkeeping.** A queued write carries its ULID, so
resending one that may already have landed converges to the same row under
`spec/protocol.md §4.5`. Where an entry stands is read, not remembered
(`spec/protocol.md §10.6`), and the node reads it: the replay carries the entry's mark as
`since` and each event's `base` (§2). Every event already there, and the node appends
nothing. Another event on one of its rows, and the node refuses the whole entry with 409
and the row, which the helper reports: the row moved since the write was queued, and a
browser never writes over what it did not see — a row the page read is judged by what it
read, so an unrelated read that moved the mark hides nothing. No event on any row, and it
is appended. Apps MUST NOT implement their own deduplication, transaction identifiers, or
acknowledgement protocol — all three indicate a misreading of the merge rule, and all three
can introduce the divergence they were meant to prevent.

An entry the node refuses — a 4xx other than 429, such as `§4.6`'s reused id — is dropped
and reported through `pv.on('rejected')`, as is one the helper refuses: a conflict, whose
error carries `status` 409 and `conflict: { tbl, id }`; an entry queued while the mount
served another app, which a solo node's `/` can across an `app.toml` change; an entry
queued against another node — a different data root answering on the same port — which
the node refuses itself from the `node` and `app` every POST carries (§2); and an entry
queued before the helper knew which app the mount serves — a solo page loaded while the
node was unreachable with nothing cached, since a host-mode mount names its app in the
path. A 429, a 5xx or a node that cannot be reached keeps the entry for the next replay and
ends this one. Nothing else about a replay is remembered.

Endpoint selection and failover are handled by the client runtime
(`spec/protocol.md §10.4`), not by the app. In a browser there is exactly one endpoint — the
page's own origin — because mixed content forbids an HTTPS page from fetching a plain-HTTP
LAN address. Apps MUST NOT construct absolute URLs to other endpoints; use `pv.url()`.

An app that must work fully offline against a remote node should keep its own read cache.
The framework does not impose one, because what to cache is an application decision.

---

## 7. Rate limits and quotas

| Limit | Default | Setting |
|---|---|---|
| Ad-hoc SQL | 20/s per session | `api.sql_rate` |
| Append batch size | 1000 events | `api.max_batch` |
| Request body | 4 MB | `api.max_body` |
| Query result rows | 10000 | `api.max_rows` |
| SSE connections | 8 per device | `api.max_streams` |
| Statement wall clock | `[lua] max_seconds` (5 s) | `config.toml` — the same deadline a Tier 1 request's SQL runs under (`spec/lua-api.md §5`) |

The `api.*` settings are `sys_setting` keys (`spec/data-dictionary.md §3.6`), read on
every request, so a change is immediate and replicates.

These protect the node from a buggy app, not from a hostile one. A hostile app you
installed has already won at the SQL level.

---

Copyright © 2026 Gabriel Mongefranco
