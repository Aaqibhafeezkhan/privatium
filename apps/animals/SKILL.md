---
name: privatium-app-animals
description: Context for extending the animals reference app — its decision-tree schema, the three-event learning step, its recursive SQL, and the HTMX/Alpine boundary it demonstrates. Load alongside privatium-tier1-lua when modifying this specific app.
---

# animals

Tier 1. A binary decision tree that learns one animal per round. A fresh game starts
from the starter tree in `lib/tree.lua`: "Does it have legs?", yes is a dog, no is a fish.

## Schema

- `node(id, kind 'q'|'a', text, yes_id, no_id)` — leaves are animals, branches are questions
- `cursor(id='cursor', node_id, started)` — single row, where the current round is

## The learning step — do not "simplify" it

`POST /teach` emits exactly three events in one `pv.batch`:

1. The leaf you landed on is **rewritten in place into a question**, keeping its own ULID
2. A new leaf for the new animal
3. A new leaf carrying the old animal's text

The parent is never touched and never has to be found. Because the branch reuses the leaf's
id, every existing pointer into the tree stays correct. A version that creates a new node
and re-points the parent is wrong: the parent may not exist (the leaf may be the root), and
finding it requires a traversal.

## HTMX or Alpine — the rule this app exists to demonstrate

> **If losing it on refresh loses data, it is HTMX.
> If losing it on refresh is fine, it is Alpine.**

| Interaction | Tool | File |
|---|---|---|
| Submit a guess | HTMX | `views/_board.lsp` |
| Restart a round | HTMX | `views/_board.lsp` |
| Teach a new animal | plain form post | `views/teach.lsp` |
| Forget everything | plain form post | `views/knowledge.lsp` |
| Expand a question path | Alpine | `views/knowledge.lsp` |
| Confirm before forgetting | Alpine | `views/knowledge.lsp` |
| Show example questions | Alpine | `views/teach.lsp` |

Two things worth noticing, because both look like exceptions and are not:

- **Not every write is HTMX.** Teaching and forgetting are *navigations* — a
  separate page you arrive at and leave. Swapping a fragment there means owning
  the back button. `app.lua` comments say so at each route.
- **Every HTMX form still carries `method` and `action`.** `hx-post` is an
  enhancement on top, and `board()` in `app.lua` returns a fragment or a redirect
  depending on `req.is_htmx`. Do not delete either branch: without the redirect,
  recording a guess would require JavaScript.
- After HTMX replaces the board, `static/animals.js` focuses its new heading. Preserve
  that keyboard starting point so Tab reaches the next answers.
- **Every write is reachable with JavaScript off**, including the ones Alpine hides.
  `views/_assets.lsp` links `static/nojs.css` from a `<noscript>`; it reverts
  `x-cloak` and hides `.pv-js-only`. Mark a button whose only effect is Alpine state
  (`toggle`, `ask`, `cancel`) with `pv-js-only`, and put `x-cloak` on what it reveals —
  never hide a form behind Alpine without both. An inline `<style>` inside `<noscript>`
  would be blocked by the CSP; the external sheet is not.

## Alpine must be the CSP build

An app runs under `script-src 'self'` with no `'unsafe-eval'`
(`spec/app-contract.md §5.4`). Standard Alpine compiles attribute expressions
with the `Function` constructor and cannot run at all under that policy.

Consequences for anything you add:

- `x-data` names a component registered in `static/animals.js` via `Alpine.data()`.
  Inline objects (`x-data="{ open: false }"`) will not work.
- Bindings reference properties and methods **by key**. No expressions: write a
  getter (see `label`) instead of `x-text="open ? 'Hide' : 'Show'"`.
- Inline event handlers (`onclick`, `onsubmit`) are script and are blocked. An
  earlier version of `knowledge.lsp` used `onsubmit="return confirm(...)"`; that
  was a bug, not a style choice.
- **Do not set `eval = true` in `app.toml`** to get the shorter syntax back.
- Every `x-show` that starts closed needs `x-cloak`, and `static/animals.css`
  carries the matching rule.
- **`static/animals.js` loads before `alpine-csp.min.js`**, both `defer`. Alpine's CDN
  builds call `Alpine.start()` in a microtask the moment their script runs, and `start()`
  dispatches `alpine:init` right then — a component registered from a listener in a
  script loaded after Alpine is too late, and every `x-data` on the page becomes an
  "Undefined variable". `views/_assets.lsp` has the order; a test holds it.

See `static/VENDOR.md` for the vendored file and how to reproduce it.

## `lib/tree.lua`

Queries live here, not in `app.lua`. `root_id()` finds the node nobody points at.
`knowledge()` is a recursive CTE over event-sourced rows. `clean()` trims and rejects empty
input.

## Conventions to preserve

- `tx.append` returns the minted ULID; the branch references both new leaves before they
  exist. Do not pre-generate ids outside the batch.
- `reset` writes tombstones, then `tree.plant(tx)` in the same batch, so the game is
  never left without a question. It never rewrites a log.
- `GET /` plants the starter tree when `tree.root_id()` is nil, and that is the only GET
  that writes. Do not add others; every other write is a POST behind `csrf()`.
- `/answer` accepts only `yes` or `no` and refuses anything else with the board's error
  branch and no write. Keep the allowlist.
- The radio pair in `views/teach.lsp` is wrapped in `fieldset`/`legend`, and each radio
  has an `id` its wrapping label names with `for` (`PV402`). Keep both.
- `sample/seed.jsonl` is seven `CHECK`-clean events (three questions, four animals) with
  no `cursor` row. Keep every leaf without `yes_id`/`no_id`, and the root the only node
  nothing points at. It loads only into an empty log, so only before the game's first
  visit.
- `knowledge()` selects `n.id AS animal_id` only so the collapsible path can pair
  `id` with `aria-controls`. Two animals can share a name; ULIDs cannot.
- `views/_board.lsp` is a partial because HTMX swaps it. `views/play.lsp` is the
  page around it and holds the `#board` wrapper. Keep the split.
- `views/_assets.lsp` loads Alpine and this app's CSS. HTMX is the framework's and
  is already loaded — do not vendor a second copy.

Run `privatium lint apps/animals` before finishing.

A correct guess uses the read-only `/won` route, rendering the same win state with
or without HTMX. Start over posts to `/start` with CSRF protection and preserves
learned animals; the win message itself writes no event.
