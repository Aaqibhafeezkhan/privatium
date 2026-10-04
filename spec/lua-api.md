<!--
This file is part of Privatium
spec/lua-api.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-10-04
Summary: NORMATIVE. Tier 1 — the Lua application API and LSP template engine.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Lua API — Tier 1

Tier 1 apps are written in Lua 5.4 and HTML templates. No build step, no compilation, no
restart. Save a file, refresh the browser, see the change.

---

## 1. Why Lua, and why 5.4

Lua exists to be embedded in a host application with a sandbox. nginx, Redis, Neovim,
World of Warcraft, and Roblox all arrived at it independently. It is a small language a
non-professional can learn in an afternoon, and it is far easier to hold in your head than
JavaScript or Rust.

**Lua 5.4 via `mlua`**, not LuaJIT and not Luau:

- **Not LuaJIT** — iOS forbids JIT compilation (W^X). A LuaJIT core cannot ship in the
  mobile shells, and LuaJIT is frozen at 5.1 semantics.
- **Not Luau** — genuinely attractive (gradual typing, sandboxing by design), but it is a
  dialect. Dialects break the "paste a snippet from the internet" experience that makes Lua
  approachable, and they degrade LLM assistance.
- **5.4** gives integer/float distinction, `goto`, to-be-closed variables, and a generational
  GC, with the largest body of ordinary Lua documentation behind it.

### 1.1 What Privatium does NOT use

The Barracuda App Server (BAS) and Mako Server were evaluated as a foundation and rejected
for two independent reasons:

1. **License.** The BAS library is distributed under a non-commercial license while the
   surrounding Lua is MIT. GPL-3.0 requires granting downstream users rights that a
   non-commercial license withholds. The combination is undistributable.
2. **Sandbox posture.** BAS executes Lua as trusted manufacturer logic with unrestricted
   Lua-to-C binding access. Privatium treats an app folder as semi-trusted code the owner
   downloaded. Opposite threat models.

Lua Server Pages, as a *concept*, is excellent and is reimplemented here (§4). The idea was
worth taking; the library was not.

---

## 2. App layout

```
apps/<slug>/
├── app.toml            manifest (tier = "lua")
├── app.lua             REQUIRED  entry point
├── lib/                your own modules; require'd by name
├── views/              LSP templates (.lsp)
├── static/             css, images, vendored js
├── schema.sql          OPTIONAL  tables, if you want SQL
└── SKILL.md            RECOMMENDED  see docs/skills.md
```

`static/` is served by the framework beneath the mount, at `<mount>static/*`, as a Tier 2
app's `web/` is: `url('/static/app.css')` reaches `static/app.css`. A route never sees
those paths. In solo mode the framework's own `/static/*` names come first and the app's
answer for the rest (`spec/protocol.md §9.1`).

---

## 3. The `pv` module

```lua
local pv = require 'privatium'
```

### 3.1 Routing

```lua
pv.get('/',            function(req) ... end)
pv.get('/fill/:id',    function(req) return req.params.id end)
pv.post('/fill',       function(req) ... end)
pv.route('PUT', '/x',  function(req) ... end)
```

Handlers return one of:

| Return | Result |
|---|---|
| `pv.render('index', ctx)` | Render `views/index.lsp` |
| `pv.redirect('/')` | 303 See Other |
| `pv.json(tbl)` | `application/json` |
| `pv.text(str)` | `text/plain` |
| a string | `text/html` |
| `nil` | 204 No Content |

`req` fields: `method`, `path`, `params`, `query`, `form`, `body`, `headers`, `device`
(the paired device's ID), `is_htmx` (true when `HX-Request` is present).

`path` is the path beneath the app's mount, `/` for the mount point. `query` is the query
string decoded, `form` an `application/x-www-form-urlencoded` body decoded, `body` the
body as received — at most 64 KiB, because Tier 1 does not stream — and `headers` is keyed
by lower-case name. `device` is the ID of the device the request was authenticated as:
the paired device's through the channel (`spec/protocol.md §8.3`), and this node's own
on loopback (`§8.4`). Routes register while `app.lua` loads and are matched in
registration order; a path that matches a pattern with no route for the method is a 405,
one that matches nothing a 404.

### 3.2 Reading

```lua
local rows = pv.query('SELECT * FROM fill WHERE drug = ?', {'Example'})
local one  = pv.query1('SELECT count(*) AS n FROM fill')
local row  = pv.get_row('fill', id)
```

Runs on the sandboxed SQLite connection (`spec/app-contract.md §7`), with
`cache/_sys.sqlite` attached read-only as `sys`, so `pv.query('SELECT * FROM
sys.v_app_nav')` answers (`spec/data-dictionary.md §4`). Parameters are bound, never
interpolated; string concatenation into SQL MUST be rejected by the linter. Sums over
`DECIMAL` columns use `decimal_sum()`; date arithmetic uses `date(x, '+30 days')`
(`spec/data-dictionary.md §2`). A view that reads a `$name` placeholder
(`spec/data-api.md §1`) reads NULL here — the query string that binds one belongs to the
data API — so a handler that needs the parameter writes the `SELECT` itself.

**How a result column is typed.** What SQLite holds, as Lua holds it — the rule every
SQLite binding follows: an `INTEGER` is a Lua integer, a `REAL` a Lua number, `TEXT` a
string, and a `NULL` an absent key in the row's table, never a value. So a `BIGINT` column
is a Lua integer — 64-bit and exact, since the reason JSON needs a string
(`spec/data-dictionary.md §2.1`) does not apply to Lua — `count(*) AS n` is a number, and
a `DECIMAL` column, which is text in the cache, is a **string**: Lua has no exact decimal,
and a float would lose what the text keeps. Two conveniences apply to a column that
originates in a declared column, read directly or through a view: `BOOLEAN` arrives as a
Lua boolean (an `INTEGER` `0` would be truthy in Lua), and `JSON` (`VARCHAR[]`) arrives
decoded into a table. A computed column has no declaration and follows the storage rule
alone, so `decimal_sum()` is a string. `pv.query1` returns `nil` when there is no row;
`pv.get_row` returns `nil` for an id that is absent **or whose winning event is a
tombstone** — `spec/protocol.md §4.5` materializes no row for one, exactly as the data API
answers 404 for both.

`pv.dec(s)` returns a decimal userdata from a string, an integer or another decimal —
never from a Lua float, which is refused because it is already inexact. It supports `+`,
`-`, `*`, `/`, unary minus, `==`, `<`, `<=`, `tostring`, `d:with_scale(n)`, `d:scale()`
and `d:compare(other)`, exact at the larger scale of the operands, and an operation whose
result does not fit 36 digits is an error rather than a saturated value. A quotient is not
exact, so `/` rounds half away from zero at the larger scale of the two operands —
`pv.dec('10.00') / 3` is `3.33` — and `a:div(b, scale)` does the same at a scale the
author names; a zero divisor is an error either way. Display and storage never need any
of this: a template prints the string, `fmt.money` formats it, and totals belong in SQL
(`decimal_sum`). Never convert money to a Lua number.

### 3.3 Writing

```lua
local id = pv.append('fill', { drug = 'Example', copay_amount = '12.50' })  -- mints a ULID
pv.append('fill', id, { copay_amount = '10.00' })                          -- amend by id
pv.delete('fill', id)                                                      -- tombstone

pv.batch(function(tx)                                -- atomic multi-event
  local a = tx.append('node', { kind = 'a', text = 'wombat' })   -- returns the new ULID
  local b = tx.append('node', { kind = 'a', text = 'penguin' })
  tx.append('node', existing_id, { kind = 'q', yes_id = a, no_id = b })
  tx.delete('cursor', 'cursor')
end)
```

**Signatures.** Both `pv.append` and `tx.append` accept two forms:

| Call | Behaviour |
|---|---|
| `append(tbl, data)` | Mints a ULID and returns it |
| `append(tbl, id, data)` | Appends under the given `id` and returns it |

`id` may be `nil` in the three-argument form, which is equivalent to the two-argument form.
That makes `pv.append('profile', existing and existing.id or nil, data)` a legal
create-or-amend without a branch.

`tx.append` returns the minted ULID **before** the batch is written, so later events in the
same batch may reference it. A `pv.batch` either appends every event or none; the framework
assigns contiguous `seq` values to the batch, under one `ts`, writes it in one write with
its first line carrying the count, and a batch a crash landed short is skipped whole by
every reader (`spec/protocol.md §4.1`), so "every event or none" holds on the way out of
the log as well as on the way in. A batch whose lines together exceed `api.max_body` is
an error and writes nothing: a batch reaches another node whole or not at all, so one
that large could never leave this node (`spec/protocol.md §4.1`, `§10.2`).

**Encoding `data`.** `data` is a Lua table with string keys, written as the JSON object
`d`: a string as a string, an integer as an integer, a boolean as a boolean, a `pv.dec` as
its digits in a string, and a nested table as an array when its keys are exactly `1..n`
(an empty table is an empty array) or an object when they are all strings. A key whose
value is `nil` is absent (`spec/data-dictionary.md §2.1`).

**Typed writes.** When `schema.sql` declares the table, every value that names a declared
column is checked and normalized before the append, and a value that is not its type
refuses the whole append — the batch too — with an error naming the table and column.
Nothing reaches the log, so the log stays clean and nothing has to materialize as NULL
later (`spec/data-dictionary.md §2.1`). Keys naming no declared column pass through
untouched.

| Declared | Accepted | Written |
|---|---|---|
| `BIGINT` | a Lua integer, or a string of digits | the digits, as a string |
| `DECIMAL(p,s)` | a string, an integer, a Lua number, a `pv.dec` | the digits at scale `s`, as a string; more fractional digits than `s` holds is refused, not rounded |
| `BOOLEAN` | `true`/`false`, `'true'`/`'false'`, `'yes'`/`'no'`, `1`/`0` | `true`/`false` |
| `DATE` | `2026-09-03`, `2026/09/03`, `20260903`, `3/9/2026`, `3-9-26`, `03.09.2026`, `March 9, 2026`, `9 March 2026`, `9-Mar-2026`, `09-SEP-26`, an epoch in seconds or milliseconds, a timestamp | `YYYY-MM-DD` |
| `TIME` | `14:03`, `14:03:11`, `2:03 pm`, `9am` | `HH:MM:SS` |
| `TIMESTAMPTZ` | RFC 3339 with `Z` or an offset, `2026-09-03 14:03[:11]` or with `T` (read as UTC), a date (midnight UTC), `3/9/2026 2:30 pm`, an epoch | RFC 3339 UTC to the millisecond |

Where a numeric date's day and month are both twelve or less, `ui.date_format = "eu"`
reads the day first and anything else the month first; a two-digit year `00`–`69` is this
century and `70`–`99` the last. The same normalization applies to `sample/seed.jsonl` and
to the data API.

**Constraints.** After normalization, every put is held against the schema's `NOT NULL`
and `CHECK` constraints — the author's own DDL, run by the engine — and a violation refuses
the whole append naming the table, the event's index in the batch and what SQLite said.
`spec/data-api.md §2` promises this for the API; there is one write path, so `pv.append`,
`pv.batch` and the seed get the same answer.

Inside `pv.batch`, `pv.append` and `pv.delete` are errors — `tx.append` and `tx.delete`
are the batch — and `pv.batch` does not nest; a `tx` used after its function returned is
an error. An error raised anywhere in the function discards the batch. `pv.on('append')`
handlers run for each event after the batch is written, in the VM that wrote it; a
handler that errors fails the request, but the batch is already durable.

### 3.4 Other

```lua
pv.ulid()                     -- fresh ULID
pv.now()                      -- RFC 3339 UTC string
pv.device()                   -- the device this request is from: a paired device's ID, or this node's
pv.node()                     -- { id, name, solo, peers, restore_tier }
pv.setting('key', default)    -- read a sys_setting; `default` when the key is unset
pv.log('info', 'message')     -- the diagnostic log; never write to stdout directly
pv.on('append', function(ev) ... end)   -- server-side reaction to any event
```

`pv.node()`: `id` is the Node ID; `name` is `sys_node.display_name`, or the Node ID while
the owner has set none (as `spec/protocol.md §9.2`'s manifest does); `solo` is whether the
node runs in solo mode; `peers` is the number of paired **nodes** — active `sys_device`
rows with `kind = 'node'` other than this one, so a paired phone or browser does not
count — and is `0` until a second node is admitted;
`restore_tier` is `1`, `2` or `3` for the tier that built this app's cache
(`spec/protocol.md §5.3`), or `nil` for an app this node has not materialized.

`pv.setting(key, default)` returns `sys_setting.value` decoded from its JSON — `"365"` is
the string, `365` the number — or `default` when no row has that key; with no default,
`nil`.

`pv.log(level, message)`, with `level` one of `debug`, `info`, `warn`, `error`, writes one
line to the node's **diagnostic log** — its standard error, prefixed with the app's slug — and nowhere else: never the event log, never `sys_audit`. `print` is routed
there too, as `info`, so an app cannot write to the node's standard output at all.

`pv.on('append', fn)` fires `fn(ev)` — `ev` being the envelope of `spec/protocol.md §4.1`
with `d` decoded — for **every event this node appends**: a handler's `pv.append`,
`pv.delete` or `pv.batch`, and the owner loading `sample/seed.jsonl`
(`spec/app-contract.md §9`). It fires synchronously after the write, in the VM that wrote
(any VM, for the seed), once per event in order; a handler that appends fires it again,
bounded only by the request's limits, so guard against loops. It also fires for accepted
events arriving from other devices, in a VM checked out by the drain that landed them,
with `pv.device()` the origin device. It never runs in a request's own VM or under the
node lock. Short batches and rejected future events do not fire it.

Routes and `pv.on` register only while `app.lua` loads; `pv.query`, `pv.append`,
`pv.node`, `pv.setting` and their kin run only inside a request or append callback. Calling either at the
wrong time is an error.

---

## 4. LSP templates

`views/*.lsp` is HTML with embedded Lua. Compiled once to a Lua chunk, cached, invalidated
on file mtime. In development that means **save, refresh, done** — no build, no restart.

```html
<!-- views/index.lsp -->
<h1>Upcoming refills</h1>

<? if #fills == 0 then ?>
  <p class="empty">Nothing due. <?= icon('check-circle') ?></p>
<? else ?>
  <ul>
  <? for _, f in ipairs(fills) do ?>
    <li>
      <?= f.drug ?> — due <?= fmt.date(f.due_on) ?>
      <button hx-delete="/fill/<?= f.id ?>" hx-target="closest li">
        <?= icon('trash', 'Delete this fill') ?>
      </button>
    </li>
  <? end ?>
  </ul>
<? end ?>
```

| Tag | Meaning |
|---|---|
| `<? ... ?>` | Execute Lua, emit nothing |
| `<?= expr ?>` | Emit `expr`, **HTML-escaped** |
| `<?raw expr ?>` | Emit unescaped. Flagged by the linter every time it appears. |
| `<?-- ... --?>` | Comment, stripped |

**Escaping is the default and is not optional.** `<?= ?>` always escapes. There is no
configuration flag to change this. `<?raw ?>` exists because escaping cannot be universal,
and its presence in a diff is a review trigger.

**What `<?= ?>` emits.** A string, a number, a boolean, or a value with a `tostring` (a
`pv.dec`) is emitted as text, escaped; `nil` emits nothing; a table or a function is an
error naming the template line. The one thing not escaped is markup the framework itself
produced: `icon()`, `csrf()`, `render()` and a layout's `content` return an **HTML value**
that `<?= ?>` emits as it is. It is a value, not a flag — concatenating it into a string
(`'x ' .. icon('gear')`) yields a plain string, which is then escaped like any other. Data
never becomes markup except through `<?raw ?>`. A `<?-- --?>` comment is stripped whatever
it contains, tags included.

### 4.0 Where helpers are available

`url`, `icon`, `fmt.*`, and `t` are available **both in templates and in handler code**, as
globals in the app's sandbox. `render`, `layout`, `menu`, and `csrf` are template-only.

```lua
pv.post('/name', function(req)          -- url() in a handler
  return pv.redirect(url('/'))
end)
```

`pv.url(path)` is available as an alias of the global `url(path)`, identical in behaviour.
Both exist because `PV301` and `spec/protocol.md §9.1` refer to the qualified form, while
sandbox code more naturally reaches for the bare one. Implementations MUST provide both.

Hardcoding `/a/<slug>/...` anywhere — handler or template — breaks the app in solo mode and
is rejected by `privatium lint` rule `PV301`.

### 4.1 Helpers available in every template

| Helper | Purpose |
|---|---|
| `render('partial', ctx)` | Include another template |
| `layout('base')` | Wrap this template in `views/base.lsp` |
| `menu(label, path[, icon])` | Add a link to this page's menu in the page frame, before the framework's own pages |
| `icon(name[, label])` | Inline a Bootstrap Icon (`docs/icons.md`) |
| `url('/path')` | Mount-aware URL (host mode prefixes `/a/<slug>`, solo mode does not) |
| `fmt.date`, `fmt.money`, `fmt.rel` | Locale-aware formatting from `sys_setting` |
| `csrf()` | Hidden CSRF field. Required in every non-GET form. |
| `t('key')` | Translation lookup, if `locales/` exists |

`url()` is not cosmetic — hardcoding `/a/myapp/...` breaks the app in solo mode. The linter
flags literal mount paths.

`icon(name[, label])` takes the label as a **string**, the second positional argument;
there is no options table and no `size` option — an icon is `1em` and scales with
`font-size`. `fmt.date` follows `ui.date_format` (`iso` | `us` | `eu`); `fmt.money`
renders two places, grouped, with the point and the group separator `ui.locale` implies
(`spec/data-dictionary.md §3.6`); `fmt.rel` is a relative time (`just now`, `3 days ago`,
`in 2 hours`). Each returns a value it cannot parse unchanged. `t(key)` returns `key`
unchanged while no `locales/` format exists, which is all of `pv/1`.

`csrf()` emits a hidden `_csrf` field whose token is bound to the app's mount for the life
of the process. The host MUST verify it on every non-GET
request beneath the mount — as the `_csrf` form field, or as an `X-CSRF-Token` header for
a request that carries no form, such as `hx-delete` on a button — and refuse a request
without it with 403 before any handler runs. The page frame (below) puts the token in
`hx-headers` on `<body>`, so every request htmx makes beneath the mount carries the header
with nothing for the author to do; a view that owns its document with `layout()` supplies
it the same way, or with `hx-include="[name=_csrf]"`. `_csrf` stays visible in `req.form`.

**The page around a view.** A view that calls no `layout()` is rendered inside the
framework's page frame, the standard chrome every app shares. The head carries
`<title><app title> — Privatium</title>`, the chrome stylesheet and the shell stylesheet,
htmx, the chrome script, and then the stylesheets and scripts the manifest's `ui.styles`
and `ui.scripts` name (`spec/app-contract.md §3`), in that order, the scripts deferred
and each with the hash of the file as served. The body carries `data-pv-mount`, the
app's mount, and `hx-headers` with the CSRF token. The header has three zones. On the
left the Privatium mark links to `/`, which is the launcher in host mode and the app
itself in solo mode. In the centre the app's title, with `app.icon` when the manifest
declares one, links to the mount root; it is a paragraph and not a heading, so the view
keeps the page's one `<h1>`. On the right stand an Apps link to the launcher, in host
mode only, and the one Menu. The menu lists, in order: the manifest's `[[ui.menu]]`
items, the items the view added with `menu()`, a separator, and the framework's pages —
Space settings, App settings, Data settings, Devices. The separator shows only while the
app's list holds an item. That list is `<ul id="pv-app-menu">` and is always present,
empty when nothing declared an item, so an app's script may append `<li>` items of its
own: each a link or a button with a text label. The launcher is not in the menu, because
the header carries the Apps link beside it. The footer carries a link to the project, an
empty `<p id="pv-status" role="status">` that the framework and the app write status
into (`spec/app-contract.md §5.2`), the space's name, and the way to connect a device.
Between header and footer, `<main id="main">` holds the view, which supplies the page's
one `<h1>`.

`menu(label, path[, icon])` adds one item for the page being rendered. The label is 1 to
40 characters; the path is mount-relative, beginning with one `/`, and the frame
resolves it through `url()`; the icon, when given, names a vendored icon. Anything else
is an error naming the template line. Like `layout`, it is for the view `pv.render`
named, and a partial calling it is an error, since a partial is included from several
pages and cannot know whose menu it would be adding to. The items last for one request.

`layout('base')` replaces the frame with `views/base.lsp`, which runs after the view
with the same ctx plus `content`, the rendered view, and then owns the whole document —
`<?= content ?>` places it, and the framework adds nothing. `layout` is for the view
`pv.render` named; a partial calling it is an error. A request htmx makes (`HX-Request`
present, `HX-Boosted` absent) gets the view's output alone, since htmx swaps it into an
element: that is how `pv.render('_board', ctx)` answers `req.is_htmx`.

**Names in a template.** The ctx table's keys are bare names; a name absent from the ctx
is the sandbox global of that name — `ipairs`, `os.date`, `icon` — or `nil` when there is
none. So a key MUST NOT be named after a Lua builtin: `error`, `type`, `select` and `table`
are functions and tables already, and `<? if error then ?>` is always true. `err` is the
reference apps' spelling. A bare assignment in a template is request-scoped exactly as in
a handler (§5). `render('partial', ctx)` gives the partial that ctx and nothing of its
parent's.

### 4.2 Why LSP and not Jinja

The handler is Lua; the template should be Lua. Two syntaxes for one app is a tax on the
author and a source of LLM confusion. LSP's `<? ?>` is also isomorphic to PHP, ASP, EJS,
and ERB, which is an enormous body of training data — an LLM writes it correctly on the
first attempt far more often than it writes a bespoke DSL.

---

## 5. Sandbox

The Lua state MUST be created with these removed or replaced:

| Removed | Reason |
|---|---|
| `io` | Filesystem access. Use `pv.*`. |
| `os.execute`, `os.exit`, `os.getenv`, `os.remove`, `os.rename`, `os.tmpname` | Process and filesystem control |
| `package.loadlib`, `package.cpath` | Loading native code |
| `debug` | Escapes every other restriction |
| `load`, `loadstring`, `dofile`, `loadfile` | Arbitrary code from data |
| `os.setlocale` | Process-wide state: one app's call would change how every other app, and the node, formats a number |

`require` MUST be replaced with a loader confined to the app's own `lib/` directory plus
the framework's whitelisted modules.

Retained: `os.time`, `os.date`, `os.clock`, `string`, `table`, `math`, `coroutine`, `utf8`.

**Resource limits, all REQUIRED:**

| Limit | Default | Setting |
|---|---|---|
| Instruction count per request | 50,000,000 | `lua.max_instructions` |
| Memory per VM | 64 MB | `lua.max_memory_mb` |
| Wall clock per request | 5 s | `lua.max_seconds` |
| VM pool size | CPU count | `lua.pool_size` |

Instruction limits are enforced with a debug hook installed by the host before app code
runs; memory via a custom allocator. Exceeding a limit aborts the request, returns 500, and
writes a `lua.limit_exceeded` audit event. It MUST NOT take down the node.

**What "aborts" means.** The count is to the nearest thousand instructions. The error a
limit raises is an ordinary Lua error — a `pcall` can observe it — but the verdict is the
host's, not the handler's: the request fails with 500 and the audit row whether or not the
error was caught; from the first trip the hook fires at every instruction, so a handler
cannot loop inside a `pcall`; and the VM that tripped is discarded and rebuilt for the next
request, so a limit never poisons the pool. The wall clock is checked by the same hook
and, while a statement runs inside SQLite where the hook cannot fire, by the connection's
progress handler, which interrupts the statement on the same deadline. A single operation
that runs entirely inside the interpreter's C code — a pathological `string.find` pattern
— is stopped at its next instruction. The memory limit is the allocator refusing: an app
that catches that and lets go of what it held has not exceeded the limit; one that does
not is refused again. `app.lua` loads under the same limits.

Lua states are not thread-safe. The host maintains a pool; one request holds one VM.
**Global state.** A global assigned while `app.lua` loads — a helper function, a constant,
a table — is the VM's baseline and is visible to every request on that VM. A global
assigned inside a handler, or in a `lib/` module a handler calls, **lives until that
request ends and is never seen by another request**, on that VM or any other: the host runs
app code in an environment whose assignments are request-scoped, so one request's data
cannot leak into the next and nothing can be cached across requests by assignment. What a
handler can still do is mutate a table the baseline holds (`cache[k] = v` where `cache =
{}` was defined at load); that persists per VM and is not shared, which is the footgun the
linter checks. Apps needing shared state MUST use the event log.

---

## 6. Client-side Lua (optional)

For sharing pure-logic modules between node and browser:

| Option | Size | Notes |
|---|---|---|
| **wasmoon** | ~250 KB wasm | Lua 5.4, matches the server exactly. Requires `permissions.wasm = true`. |
| **Fengari** | ~200 KB js | Lua 5.3, no WASM permission needed, slower |

Share **logic only** — validation, date math, scoring — from `lib/shared/`. Do not attempt
to share handler or template code; `pv.*` does not exist in the browser.

This is optional and off by default. Most apps never need it.

---

## 7. Development loop

```
privatium dev --app myapp
```

- Watches `app.lua`, `lib/`, `views/`, `static/`, `schema.sql`
- Lua and templates: reloaded in place, next request picks them up. **No restart.**
- `schema.sql`: triggers rematerialization from the logs, which is safe at any time
- Errors render in the browser with the Lua traceback and the offending template line
- `--open` opens the app in a browser; a bare `privatium --open` also prints the QR code
  a phone on the LAN scans to reach the node (`spec/cli.md §2`)

The reloading is the host's, on every run, not a mode: a change is noticed by a stat on the
next request, so `privatium dev` adds nothing to it beyond its flags (`spec/cli.md §3`). A
save that does not load — a syntax error in `app.lua`, a template that does not compile —
is the error page, with the traceback and the offending line, on every request beneath
the mount until the next save loads; the code from before the error is not served in its
place.

The tight loop is the point. If a change requires a restart, that is a bug in the host.

---

Copyright © 2026 Gabriel Mongefranco
