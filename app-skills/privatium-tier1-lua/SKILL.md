---
name: privatium-tier1-lua
description: Write Tier 1 Privatium apps in Lua 5.4 with LSP templates. Covers routing, the pv module, reading and writing events, LSP syntax, the sandbox, and the specific mistakes LLMs make in this framework. Load for any app that is records, lists, forms, reports, or trackers.
---

# Privatium Tier 1 — Lua

A Tier 1 app is `app.toml`, `app.lua`, and `views/*.lsp`. No build step. Save a file,
refresh the browser, see the change.

## Skeleton

```lua
-- app.lua
local pv = require 'privatium'

pv.get('/', function(req)
  return pv.render('index', {
    fills = pv.query('SELECT * FROM fill ORDER BY filled_on DESC LIMIT 50')
  })
end)

pv.post('/fill', function(req)
  pv.append('fill', {
    drug         = req.form.drug,
    filled_on    = req.form.filled_on,
    copay_amount = req.form.copay_amount,   -- string, stays a string
  })
  return pv.redirect(url('/'))
end)
```

```html
<!-- views/index.lsp -->
<h1>Fills</h1>
<? for _, f in ipairs(fills) do ?>
  <li><?= f.drug ?> — <?= fmt.money(f.copay_amount) ?></li>
<? end ?>
```

## LSP tags

| Tag | Meaning |
|---|---|
| `<? ... ?>` | Execute, emit nothing |
| `<?= expr ?>` | Emit, **HTML-escaped** |
| `<?raw expr ?>` | Emit unescaped — every use is a review trigger |
| `<?-- ... --?>` | Comment |

Helpers in every template: `render`, `layout`, `icon`, `url`, `fmt.date`, `fmt.money`,
`fmt.rel`, `csrf`, `t`.

- `<?= ?>` escapes every string. `icon()`, `csrf()` and `render()` return an HTML value
  that passes as it is — write `<?= icon('gear') ?>`, never `<?raw icon('gear') ?>`.
  `'x ' .. icon('gear')` is a plain string again and is escaped. `nil` emits nothing; a
  table is an error naming the line.
- The ctx keys are bare names: `pv.render('index', { fills = ... })` makes `fills` visible.
  A name not in the ctx is the Lua global of that name, so never key a ctx by a builtin —
  `error`, `type`, `select`, `table`. Use `err` for a message.
- A view with no `layout()` renders inside the framework's page frame (title, stylesheet,
  htmx, header, `<main>`): write the page's one `<h1>` and the content, nothing more. To
  own the document, call `layout('base')` and put `<?= content ?>` in `views/base.lsp`.
- The one `<h1>` is per *rendered page*, not per file (`PV404`): a page and its partials
  together carry exactly one, so it may live in the partial htmx swaps — `_board.lsp` has
  it, `play.lsp` does not. Every state of the view supplies one, the empty state included.
- A request htmx makes gets the view's output alone, so `pv.render('_board', ctx)` from a
  `req.is_htmx` branch is a fragment swap; `render('_board', ctx)` includes it in a page.
- `static/` is served at `url('/static/...')` beneath the mount; put CSS and vendored JS
  there.
- Anything a script hides must be reachable with scripts off. Link a sheet from
  `<noscript>` — `<noscript><link rel="stylesheet" href="<?= url('/static/nojs.css') ?>"></noscript>`
  — that reverts `x-cloak` and hides `.pv-js-only`, the buttons whose only job is
  toggling client state; `apps/animals` is the worked example. An external sheet, not an
  inline `<style>`: the default CSP has no `style-src`.
- Save a file, refresh: `views/*.lsp`, `app.lua`, `lib/`, `schema.sql` and `app.toml` are
  reloaded on the next request, no restart. A save that does not load is the error page,
  with the line, until the next save loads.
- A third-party pure-Lua library is copied under `lib/` with its own file layout, so its
  internal `require`s resolve; there is no package manager, and a rock with C sources
  cannot load (`docs/frameworks.md §3`). Record where it came from and its licence.

`pv.on('append')` also receives accepted events from other devices. Its VM is checked
out by the drain, outside the node lock, and `pv.device()` names the origin device.
Guard callbacks against append loops as you do for local events.

## MUST

- Bind SQL parameters: `pv.query('... WHERE drug = ?', {name})`
- Read a row as SQLite holds it: a `BIGINT` and `count(*)` are Lua integers, a `DECIMAL`
  is a string, a `BOOLEAN` a boolean, a `JSON` column a table. Print or `fmt.money` a
  `DECIMAL` as it is; for arithmetic wrap it in `pv.dec()` — `+ - * /` are exact, `/`
  rounds half away from zero at the larger scale, `a:div(b, 4)` names the scale
- Total a `DECIMAL` column with `decimal_sum(col)`, never `SUM(col)`, and add to a `DATE`
  with `date(col, '+30 days')`, never `col + 30` — the linter refuses both (`PV308`)
- Write dates, times and timestamps in any common spelling — `3/9/2026`, `March 9, 2026`,
  `2:30 pm`, `2026-09-03 14:03` — the framework normalizes them to ISO on write and
  refuses what it cannot read, naming the column
- Wrap every internal link in `url()`
- Put `<?= csrf() ?>` in every non-GET form — the host refuses a non-GET request without
  the token with 403; an `hx-delete` button gets it from the page frame's `hx-headers`
- Give every icon-only control a label: `icon('trash', 'Delete this fill')`
- Use `pv.batch()` when more than one event must land together
- Expect `NOT NULL` and `CHECK` from `schema.sql` to refuse a `pv.append` that breaks
  them — the whole batch, naming the event — and handle the error where the form is
- Read the framework's views as `sys.v_app_nav`, `sys.v_device_active`, `sys.v_health`,
  `sys.v_audit_recent` — attached read-only on your connection; a `$name` placeholder in
  one of your own views is NULL here (it is bound by the data API's query string)

## MUST NOT

- Concatenate values into SQL
- Call `io`, `os.execute`, `os.getenv`, `os.setlocale`, `debug`, `load`, `dofile` — all removed
- Write `INSERT`/`UPDATE`/`DELETE` — reads are SQL, writes are `pv.append`/`pv.delete`
- Store state in a Lua global expecting it to persist — a global assigned in a handler
  lasts one request; `app.lua`'s definitions are the baseline every request starts from
- Set `seq`, `lam`, `ts`, `dev`, or `app` on an event
- Call `pv.append` inside `pv.batch` (use `tx.append`), or `pv.query`/`pv.append` while
  `app.lua` loads — reads and writes run inside a handler
- Catch a limit error with `pcall` and carry on — the request fails regardless

## Anti-patterns

```lua
-- WRONG: SQL injection, and it will not even be reached — the linter rejects it
pv.query("SELECT * FROM fill WHERE drug = '" .. req.form.drug .. "'")
-- RIGHT
pv.query('SELECT * FROM fill WHERE drug = ?', {req.form.drug})

-- WRONG: money as a float. 0.1 + 0.2 ~= 0.3
local total = tonumber(a.copay_amount) + tonumber(b.copay_amount)
-- RIGHT
local total = pv.dec(a.copay_amount) + pv.dec(b.copay_amount)

-- WRONG: breaks in solo mode
return pv.redirect('/a/medtracker/')
-- RIGHT
return pv.redirect(url('/'))

-- WRONG: two events, one can land without the other
pv.append('node', a, {...}); pv.append('node', b, {...})
-- RIGHT
pv.batch(function(tx) tx.append('node', a, {...}); tx.append('node', b, {...}) end)

-- WRONG: mutation
pv.query('UPDATE fill SET copay_amount = ? WHERE id = ?', {x, id})
-- RIGHT
pv.append('fill', id, { copay_amount = x })

-- WRONG: XSS
<?raw req.form.note ?>
-- RIGHT
<?= req.form.note ?>
```

## Schema

`schema.sql` is optional. Include it when you want typed tables and SQL queries; omit it to
use the log as a document store. Every table needs `id VARCHAR PRIMARY KEY`, holding a
ULID — unless the table is a singleton you key by a constant, the way `apps/animals` keys
its `cursor` row `'cursor'`. Use `DECIMAL(18,2)` for money and `DATE` for dates — never
text.

Changing `schema.sql` rematerializes from the logs. This is safe at any time and loses
nothing; new columns are simply NULL for old events.

## Escalate

If the app needs canvas, WebGL, animation, or a custom interaction model, stop and load
`privatium-tier2-web`. Do not fight LSP into being a game engine.

## Verify

```bash
privatium new <slug>            # an empty app; --from hello copies the reference app,
                                # --scaffold <table> emits CRUD screens for a table
privatium dev --app <slug>      # runs it; a save is served on the next request, no restart
                                # --open uses the local browser URL; LAN callers need pairing
privatium lint apps/<slug>      # exit 3 on findings; --format json to read them back
privatium lint apps/<slug> --fix   # only url() for a literal mount path and focusable="false"
```

Full API: `spec/lua-api.md`, and `reference/pv-api.md` here for the surface this version
registers. CLI and lint rules: `spec/cli.md`; `reference/anti-patterns.md` shows every
Tier 1 rule failing and passing.
