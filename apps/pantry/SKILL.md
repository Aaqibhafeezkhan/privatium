---
name: privatium-app-pantry
description: Context for extending the pantry reference app — a Tier 2 app with schema.sql, named views, exact decimals and validated forms. Load alongside privatium-tier2-web when modifying this specific app.
---

# pantry

Tier 2 with tables. No `app.lua` and no `views/`: `web/` is the whole front end, and
`schema.sql` is what the data API reads through.

## Storage model

Three tables (`schema.sql`), every one keyed by a ULID in `id`:

| Table | One row per | Notes |
|---|---|---|
| `shelf` | labelled space | `name`, `sort` |
| `batch` | stored batch | `name`, `icon`, `unit`, `shelf_id`, `stored_on`, `expires_on` (nullable) |
| `quantity_change` | recorded change to one batch | `amount DECIMAL(18,3)`, `reason`, `of_id`, `at` |

`reason` is `stocked`, `taken` or `returned`, held by a `CHECK`; a second `CHECK` ties
`of_id` to `returned` and to nothing else. A withdrawal is negative, a return positive.

**There is no balance column and there must never be one.** A balance is
`decimal_sum(amount)` over a batch's changes, in a view. A stored total is a value two
devices can disagree about, and row-granularity last-write-wins has no way to merge two
claims about one row: one of the two takings would vanish. Summing the rows keeps both.

## Conventions to preserve

- **One `pv.append` per action, and every write in `writes.js`.** A second
  `pv.put`/`pv.del`/`pv.append` in the same block is a `PV306` warning and, here, a bug:
  adding a batch writes the batch row and its first change together, because
  `decimal_sum` over no rows is NULL, not zero.
- **A move is a `put` on the live id.** Never a tombstone plus a new row: a tombstoned id
  is never reused (`spec/protocol.md §4.6`), so a new id would orphan every
  `quantity_change.batch_id` that names the batch.
- **Undo is a `del` on the change.** Two devices undoing the same change write the same
  tombstone and converge; no counter and no guard belongs anywhere near it.
- **A quantity is a string from the field to the column.** The variable is called `amount`
  on purpose: `PV302` keys on identifiers whose last segment is a declared `DECIMAL` or
  `BIGINT` column, so `Number(row.amount)` or `+row.amount` is a lint error anywhere in
  this app. Renaming it `qty` would make the rule unenforceable. Compare and format on the
  text — `trim()` and `isZero()` in `views.js` do exactly that.
- **A date is compared with a modifier.** `date('now', '+' || $days || ' days')`, never
  `expires_on + $days`, which SQLite reads as integer arithmetic (`PV308`).
- **Nothing is clamped.** A balance below zero and an over-return are reported by
  `v_check_batch` and `v_check_out` as the numbers the log actually holds.
- **The activity list reads the log, the rest reads views.** Materialization drops a
  tombstoned row entirely, so a view can show an undo's effect but never the undo itself.
  `pv.events` on boot, `pv.subscribe` live, keyed by event id so a tab's own echo replaces
  rather than duplicates.
- **No `innerHTML`.** Everything is `createElement` and `textContent` (`PV206`). Icons are
  cloned from the `<template id="icon-shape">` in the page, so no namespace string lives in
  the JavaScript.
- **No inline `<script>`, no `style=` attribute.** The default CSP has neither
  `script-src 'unsafe-inline'` nor a `style-src`; every colour is in `style.css`.
- No CDN, no vendored library, no build step.

## The views, and the grain of each

| View | Grain | Params |
|---|---|---|
| `v_shelf` | one row per shelf, with a count of the batches on it that hold something | — |
| `v_batch` | one row per non-empty batch on `$shelf` | `$shelf` |
| `v_out` | one row per withdrawal at or after `$since`, with what came back | `$since` |
| `v_stock_by_unit` | one row per unit of measure | — |
| `v_check_batch` | one row per batch whose balance is below zero | — |
| `v_check_out` | one row per withdrawal more than fully returned | — |
| `v_expiring` | one row per non-empty batch dated inside `$days`, or past it | `$days` |

Every placeholder arrives as **text** (`spec/data-api.md §1`); comparing it to a `VARCHAR`
id or to an ISO timestamp compares correctly as text, so no cast is needed. A zero-balance
batch is hidden by `HAVING decimal_cmp(decimal_sum(c.amount), '0') <> 0` and never deleted.

Adding a view means adding its `-- grain:` comment. Adding a column that could be derived
means asking whether it should be a view instead; the answer is almost always yes.

## Layout

One sheet, ruled rather than boxed. `.work` (shelves, then the batches on the open shelf)
and `.rail` (the tray, the checks, the activity) are **two independent stacks** either side
of one hairline. Do not lay them out as rows of a single grid: that ties every heading on
the right to the bottom of a box on the left, and the two halves drift out of line as soon
as one list grows. The empty states are the other half of that rule — with no shelf the
batch half is not rendered at all, because its Shelf field would have nothing to offer.

**The bar and footer are the framework's.** The page is served with the standard chrome
inserted at its three anchors (`spec/app-contract.md §5`), so `index.html` keeps
`</head>`, `<body>`, `</body>` and `<main id="main">` where they are (`PV109`), draws no
link of its own to the launcher or settings (`PV408`), and has no status element: `say()`
in `app.js` is `pv.status()`, which writes the footer's slot, and the framework writes
the connection wording there itself. The band keeps the app's `<h1>` and nothing else.
To own the whole window instead, set `[ui] chrome = "none"` in `app.toml` and draw the
way back yourself, as `apps/sketch` does.

## Accessibility conventions

- One `<h1>`, then `<h2>` per region (`PV404`). Real tables with `<th scope>` (`PV407`).
- Every field has a `<label for>` (`PV402`), an error paragraph of its own, and
  `aria-describedby` pointing at it; the icon choice sits in `fieldset`/`legend` (`PV403`).
- A refusal from the node names a `column`; `showRefusal()` puts the message under that
  field. The client's own checks come first, and neither replaces the other.
- Every state is a word — Expired, Use soon, no date, undone, check stock — beside its
  icon, never a colour alone (`PV405`). Controls keep 44 pixels; the row control that
  becomes icon-only on a phone keeps its `aria-label`.
- A table that scrolls sideways sits in a `.scroll` box, and that box is
  `position: relative`: a visually hidden label inside a scroller is absolutely positioned
  and, without it, escapes the box and widens the whole page — which is what breaks reflow
  at 320 pixels.

## Verify

```bash
privatium lint apps/pantry
cargo test --locked -p privatium-core --test reference pantry
```
