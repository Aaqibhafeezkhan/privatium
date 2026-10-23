<!--
This file is part of Privatium
docs/plans/pantry-app.md
Author(s): Gabriel Mongefranco
Created: 2026-09-07
Last Modified: 2026-09-07
Summary: Plan of record for `apps/pantry`, the second Tier 2 reference app: a freezer and pantry
         inventory whose balance is the sum of recorded changes.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Plan — `apps/pantry`

A second Tier 2 reference app. It teaches the parts of the contract `sketch` leaves open:
`schema.sql` with views of stated grain, `pv.query`, exact decimals, Tier 2 forms with
validation, and concurrent writes made visible. Read in one sitting: target under 900 lines
of JavaScript across four modules, one stylesheet, one HTML page, one `schema.sql`.

## 1. Name

Slug `pantry`, title **Pantry**. It covers a freezer and a pantry, and "pantry" generalizes.
Six characters, so `nav.advertise = true` is safe under `PV501`. The seeded shelves are two
freezer drawers and two pantry shelves, so both halves of the name are worked through.

## 2. Data model

Three tables. Every table gets `id VARCHAR PRIMARY KEY` (`PV106`), no `UNIQUE` anywhere
(`PV108`), declarations only (`PV107`).

```sql
CREATE TABLE shelf (                -- one row per labelled space: a freezer drawer or a pantry shelf
    id   VARCHAR PRIMARY KEY,       -- ULID
    name VARCHAR NOT NULL,
    sort BIGINT  NOT NULL
);

CREATE TABLE batch (                -- one row per stored batch, live for the app's life
    id             VARCHAR PRIMARY KEY,
    name           VARCHAR NOT NULL,
    icon           VARCHAR NOT NULL,   -- a vendored Bootstrap Icons name
    unit           VARCHAR NOT NULL,   -- chosen at add time, never changed
    shelf_id       VARCHAR NOT NULL,
    stored_on      DATE    NOT NULL,   -- frozen on, or shelved on
    expires_on     DATE                -- NULL when nothing on it says
);

CREATE TABLE quantity_change (      -- one row per recorded change to one batch
    id       VARCHAR       PRIMARY KEY,
    batch_id VARCHAR       NOT NULL,
    amount   DECIMAL(18,3) NOT NULL,   -- negative takes out, positive stocks or returns
    reason   VARCHAR       NOT NULL,   -- 'stocked' | 'taken' | 'returned'
    of_id    VARCHAR,                  -- the withdrawal a return refers to; else NULL
    at       TIMESTAMPTZ   NOT NULL,
    CHECK (reason IN ('stocked', 'taken', 'returned')),
    CHECK ((reason = 'returned') = (of_id IS NOT NULL))
);

CREATE INDEX ix_change_batch ON quantity_change (batch_id);
CREATE INDEX ix_change_of    ON quantity_change (of_id);
```

Notes that carry weight:

- **Balance is never a column.** It exists only as `decimal_sum(amount)` over a batch's
  changes. Adding a batch writes the batch row *and* a `stocked` change in one
  `pv.append([...])`, so every batch has at least one change and `decimal_sum` is never
  NULL over an empty set.
- `DECIMAL(18,3)` because fractions are allowed — half a container, a third of a bowl.
  Three places is enough for a kitchen and keeps the strings short.
- `at` is the writing device's clock, in UTC, supplied by the app as an ordinary column.
  The envelope's `ts` is the node's and stays the node's (`PV304`). The app renders `at` in
  local time. This is a stated assumption: a phone with a wrong clock puts a card in the
  wrong order in the tray, and nothing worse.
- `expires_on` is nullable and stays nullable. Plenty of a pantry has no date on it, and a
  required field would be answered with a guess, which is worse than a blank.
- `unit` lives on `batch`, not on the change. You cannot take out 4 waffles from a batch
  measured in containers.
- No `shelf` history and no `moved` change type: a move is a fresh `put` on the
  batch's live id (§4).

### 2.1 Views, each with its grain

Every view carries a `-- grain:` comment (AGENTS.md, Engineering style). Placeholders are
`$name`, rewritten to `pv_param('name')` (`spec/data-api.md §1`).

| View | Grain | Params | Notes |
|---|---|---|---|
| `v_shelf` | one row per shelf | — | name, sort, and a count of batches whose balance is not zero |
| `v_batch` | one row per non-empty batch on one shelf | `$shelf` | name, icon, unit, `stored_on`, `days_in`, `expires_on`, `expiry` (`fresh`/`soon`/`past`/`unknown`), `balance` |
| `v_out` | one row per withdrawal | `$since` | `taken`, `returned`, `still_out`, batch name/icon/unit, shelf name, `at` |
| `v_stock_by_unit` | one row per unit across every shelf | — | `decimal_sum` across grains |
| `v_check_batch` | one row per batch whose balance is below zero | — | the recorded amount, not clamped |
| `v_check_out` | one row per withdrawal returned more than it took | — | the recorded amounts |
| `v_expiring` | one row per non-empty batch expiring within `$days` or already past | `$days` | shelf name, `expires_on`, `expiry` |

- Zero-balance batches are hidden by `HAVING decimal_cmp(decimal_sum(c.amount), '0') <> 0`,
  never deleted. `v_check_batch` uses `decimal_cmp(...) < 0` on the same expression.
- `days_in` is `CAST(julianday('now') - julianday(stored_on) AS INTEGER)`. Verified against
  the linter's tokenizer: `PV308`'s DATE check fires on `+`/`-` immediately beside a bare
  DATE column, and a word followed by `(` is skipped as a function name, so
  `julianday(stored_on)` does not trip it. Every `-` here sits between two `julianday()`
  results.
- **`expires_on` is where `PV308`'s date half earns its keep.** "Expiring soon" is
  `expires_on <= date('now', '+' || $days || ' days')`, never `expires_on + $days`, which
  SQLite evaluates as an integer and the linter refuses. Until now the app corpus taught
  only the `SUM()` half of that rule; the expiry date gives the other half a real home, and
  it is the reason `v_expiring` exists as its own view rather than a flag on `v_batch`.
- `expiry` is a `CASE` returning `'past'`, `'soon'`, `'fresh'` or `'unknown'` — a word, not
  a colour and not a boolean, so the screen can label it in text (`PV405`) and `NULL`
  stays honestly distinct from fresh.
- **There is no activity view.** Materialization drops a tombstoned row entirely
  (`spec/protocol.md §4.5` step 4), so SQL cannot show an undo — it can only show its
  effect. The activity list therefore reads the log: `pv.events({ tbl: 'quantity_change' })`
  on boot, `pv.subscribe` live. That split *is* the lesson: **the tables are what is true
  now, the log is what happened.** Same rows, two readings.

## 3. What is not written down

- A pending-write count. `pv.js` exposes none.
- A dedupe table, a transaction id, an acknowledgement (`PV305`, invariant 11).
- Any clamp on a balance. The recorded number is shown.
- View state. Last shelf opened is `localStorage` under `pantry.shelf`, and it
  never reaches the log.

## 4. Event flows

Every write is one `pv.append([...])` call. A second `pv.put`/`pv.del`/`pv.append` in the
same block is a `PV306` warning, and in this app it would also be wrong.

| Action | Events, in one batch |
|---|---|
| **Add shelf** | `put shelf` |
| **Add batch** | `put batch` + `put quantity_change` (`reason: 'stocked'`, positive `amount`) — atomic, so a batch never exists with no stock |
| **Move batch** | `put batch` on the **live id**, whole row, new `shelf_id` |
| **Take out** | `put quantity_change` (`reason: 'taken'`, negative `amount`, `of_id: null`) |
| **Put back** | `put quantity_change` (`reason: 'returned'`, positive `amount`, `of_id` = the withdrawal's id) |
| **Undo** | `del quantity_change` on that one row |

Ids are minted client-side with `pv.ulid()` so a queued write replays under the same id.

### 4.1 Why a move is a `put`, not a tombstone plus a `put`

Tombstone-plus-put would need a new id, because `spec/protocol.md §4.6` forbids reusing a
tombstoned one. Every `quantity_change.batch_id` would then point at a dead row and the
batch would lose its whole history. Row-granularity last-write-wins (`§4.5`) does the job:
two devices moving one batch to different shelves converge on one row, ordered by
`(lam, ts, dev)`. The batch is in one place, not two, and no change is orphaned.

### 4.2 Conflict case one — both phones take the last portion

Two disconnected phones each write a `taken` of -1 against a batch holding 1. Both are
their own rows with their own ULIDs; `§4.5` keeps both, because they are different rows. The
balance becomes -1. `v_check_batch` shows the batch with "check stock" and the recorded
-1. Nothing is clamped and nothing is hidden. The app says what happened in the person's
own terms: *the log says minus one portion, so one of you took something that was not
there.*

### 4.3 Conflict case two — both phones put the same waffles back

Two `returned` rows against one withdrawal of 4, each of 4. `v_check_out` shows returned
greater than taken. Same treatment.

### 4.4 Convergence, which is not a conflict

Two phones each take 2 portions of soup from a batch of 6. Two rows, no clash, both
balances settle at 2 within a second of the second phone reconnecting. This is the tray's
demonstration and the reason the tray is a first-class screen.

### 4.5 Undo, twice

Two devices each undo the same change. Both write `del` on the same id. `§4.5` takes the last
event in the group, which is a tombstone either way. Undoing once and undoing twice are the
same state. No counter, no guard.

## 5. Screens

One page, no routing. Four regions in the DOM, laid out by CSS.

1. **Shelf map** — `<h1>Pantry</h1>`, then shelves as `<button>` elements with
   `aria-pressed` on the open one and a batch count. A small "New shelf" disclosure
   holds one labelled text field. Opening a shelf writes `localStorage`.
2. **Batch list** — a `<table>` with `<th scope="col">` (`PV407`): icon, name, balance,
   unit, stored on, days in, expires on, and a **Take out** button per row that reveals an
   inline amount field. Empty batches are absent. A past-date row carries the word
   **Expired** and a soon row **Use soon** — words plus an icon, never colour alone
   (`PV405`). Expiry is shown, never enforced: nothing is hidden, removed or blocked by a
   date, because a freezer is not a regulator and the person decides.
3. **Taken out today** — one card per withdrawal from `v_out` where `still_out` is not
   zero: icon, amount still out, batch, shelf, local time, and a **Put back** field.
   A partly-returned card reads "2 of 4 still out"; a fully-returned one leaves the tray.
   An **Earlier** toggle moves `$since` back and shows the rest.
4. **Activity** — a `<table>`, newest first, from the log: took, put back, stocked, and
   undone entries rendered struck through with the word "undone" beside them (never colour
   alone, `PV405`). Each live entry carries a small **Undo**.

Plus a status line (`role="status"`) for offline, rejections and saves, and a **Stock
summary** disclosure showing `v_stock_by_unit`.

**Desktop** is two columns: map and batches left, tray right, activity beneath the tray.
**Phone** is one column in the order map, batches, tray, activity. Breakpoint at 820 px,
matching `sketch`. No control below 44 × 44 at any width; text zooms to 200 % and reflows
at 320 px.

### 5.1 Forms

Three forms: add shelf, add batch, and the two amount fields. The add-batch form is
the Tier 2 form idiom in full:

- `<label for>` on every field (`PV402`); unit as a `<select>` with a label; icon as a
  radio group inside `<fieldset><legend>` (`PV403`).
- Validation runs client-side first, and errors are re-rendered **in place** — a
  `<p class="err" id="...">` beneath the field, the field carrying `aria-describedby` and
  `aria-invalid`, focus moved to the first bad field. Built with `createElement` and
  `textContent`, never `innerHTML` (`PV206`).
- The node validates again, from `schema.sql`: `NOT NULL` and the two `CHECK`s, and the
  type of every declared column, before anything is appended (`spec/data-api.md §2`). A
  refusal names `index` and `column`; the app puts that message beside the field it names.
  Two fences, and the second one is the framework's.
- The quantity input's value is read into a variable named `amount` and stays a **string**
  all the way to `d.amount`. `12.5` lands as `"12.500"` at the column's declared scale.
  Naming it `amount` is deliberate: `PV302` keys on identifiers whose last segment is a
  declared `DECIMAL` or `BIGINT` column name, so `Number(row.amount)` or `+row.amount`
  anywhere in this app is an error the linter catches. Naming the variable `qty` would
  make the lesson unenforceable.

### 5.2 Icons

Vendored Bootstrap Icons inlined into a page sprite, `<use>` references, `fill:
currentColor`, `focusable="false"`, `aria-hidden` beside text (`docs/icons.md`, `PV401`,
`PV503`). The icon picker offers a fixed set — snow, cup-hot, egg-fried, basket and a few
more — because an arbitrary name would fail `PV503`.

## 6. Offline and resync

- **Boot**: `pv.node()`, then `pv.query` each view, then `pv.events` for activity, then
  `pv.subscribe`.
- **Live**: every `append` envelope on the stream re-runs the affected queries and appends
  one activity row. A tab's own write arrives back on the stream; the activity list is keyed
  by event id, so an echo replaces rather than duplicates.
- **`resync`**: re-read everything. `after=` is a Lamport cursor and is not a gap-free
  cross-device resume, so a reconnect re-reads too (`spec/data-api.md §3`).
- **`offline`/`online`**: a banner, and queries that throw `PvOffline` leave the last
  rendered values in place with the banner saying they are not current. Writes queue.
- **`rejected`**: the entry is named in the status line in the person's own words — "the
  soup moved while you were offline; your take-out was not recorded" — carrying the
  `conflict: { tbl, id }` the helper reports. No retry loop, no dedupe.
- No pending count is shown, because the helper exposes none.

## 7. `app.toml`

`tier = "web"`, `api = 1`, `icon = "box-seam"`, `[nav] order = 40, advertise = true`.
`[permissions]` all defaults and written out with the comment `PV205` wants: `remote = []`,
`sql = false` (every read is a named view), `inline_script = false`, `wasm = false`,
`eval = false`. No vendored library, no CDN, no build step (`PV504`, `PV207`).

`sample/seed.jsonl`: four shelves (two freezer drawers, two pantry shelves), six batches, a
dozen changes including one partly-returned withdrawal and one batch already at zero, all
synthetic (`PV208`). The batches cover every expiry state: one past, one within a week, one
far off, one with no date at all.

## 8. What the README teaches, in order

Written instructionally, start to finish, in the house style — least technical reader who
still needs the page.

1. **What it is** and the picture: a freezer with labelled shelves.
2. **Add a batch.** The form, the unit, an optional expiry date, and the fact that quantity
   is a string. First lesson: `schema.sql`, why `DECIMAL` is text, and why a date is
   compared with a modifier rather than added to.
3. **Take some out, put some back.** The tray. Second lesson: **the balance is not stored.**
   Show the `decimal_sum` view beside the number on screen.
4. **Open it on your phone too.** Third lesson: `pv.subscribe`, and both devices settling on
   the same balance.
5. **Undo.** Fourth lesson: a tombstone on one row, and why undoing twice is undoing once.
6. **Move a batch.** Fifth lesson: last-write-wins at the row, and why not a tombstone.
7. **Go offline and both take the last portion.** Sixth lesson, and the honest one: the app
   says "check stock" and shows -1. Why it does not clamp.
8. **Read your pantry without Privatium**: a `jq` line over the log, mirroring `sketch`'s.
9. **The feature table**, corrected as §11 says.
10. **Features left out**: global undo, images, a pending count, a graphics library.

`SKILL.md` is the same material as conventions rather than a tour: the schema and its
grains, the seven views, the one-append rule, the `amount` naming rule and why it exists,
the log-versus-table split for activity, the accessibility conventions, and
`privatium lint apps/pantry` as the last line.

## 9. Tests and lint

New `#[test]` functions in `crates/privatium-core/tests/reference.rs`, alongside the
existing hello/animals/sketch end-to-end tests, each named for the section it holds:

| Test | Holds |
|---|---|
| `test_pantry_end_to_end` | load, mount, add a batch as one batch of the log, take out, put back, read every view |
| `test_spec_4_5_pantry_move_is_last_write_wins` | two puts on one batch id from two devices; one row, the later rank wins |
| `test_spec_4_6_pantry_move_does_not_orphan_changes` | after a move, every `quantity_change.batch_id` still resolves |
| `test_spec_4_5_pantry_double_undo_is_one_undo` | two `del`s on one change id; the row is absent, once |
| `test_pantry_balance_is_never_stored` | no `balance` column exists; the view's number equals `decimal_sum` over the log |
| `test_pantry_negative_balance_is_reported_not_clamped` | two concurrent takes of the last portion; `v_check_batch` shows -1 |
| `test_pantry_over_return_is_reported` | two returns of one withdrawal; `v_check_out` shows it |
| `test_pantry_expiry_uses_the_date_modifier` | `v_expiring` over a seeded past, soon, fresh and undated batch; `$days` binds as text and the modifier spelling is what runs |
| `test_pantry_decimal_scale_is_preserved` | `12.5` in, `"12.500"` out, through the API and through the seed |
| `test_pantry_schema_rejects_bad_reason` | the `CHECK` refuses `reason: 'eaten'` with `index` and nothing appended |
| `test_pantry_seed_loads_into_empty_app_only` | `spec/app-contract.md §9` |
| a11y | the rendered page through `tests/common/a11y.rs`, as every app is |

Lint: `privatium lint apps/pantry` clean at `--severity warn`. The rules this app is the
corpus for are `PV302`, `PV308`, `PV402`, `PV403`, `PV407` and `PV306`; fixtures already
exist for all of them under `apps/_lint/`, so nothing new is needed there — this app is
the *realistic* passing case, not the fixture.

Manual pass, reported: keyboard-only traversal of every form and the tray, visible focus,
200 % zoom and 320 px reflow, a screen reader over add → take out → put back → undo.

## 10. Changes needed outside the app folder

Each is a real edit, not a nicety.

| File | Change | Why |
|---|---|---|
| `apps/README.md` | Add the `pantry` row; rewrite "**`hello` is the floor, `animals` is the ceiling, `sketch` is the escape hatch.** There is deliberately no fourth." | That sentence forbids this app. `lantern` is already named as a fourth for Tier 3, so the count is wrong twice over. |
| `crates/privatium-core/src/app/examples.rs` | `SLUGS: [&str; 3]` → `[&str; 4]`, a fourth `include_dir!`, a fourth match arm | Bundled apps are compiled in |
| `crates/privatium-core/tests/apps.rs` | the "three reference apps load as bundled" test and its doc comment | it asserts the count |
| `crates/privatium-core/tests/lint.rs`, `crates/privatium/tests/cli.rs` | the slug lists and the CLI's "hello, animals, sketch" message | they enumerate the examples |
| `README.md` | add a bullet beside the Sketch one | its example list is the index |
| `docs/sample-app-design.md` | a `Pantry` bullet under "Each app's layout", and its breakpoint | the doc covers every reference app |
| `skills/privatium-tier2-web/SKILL.md` | name `pantry` as the SQL-and-forms Tier 2 example beside `sketch` as the no-SQL one | a change to `apps/` that skills do not reflect is incomplete (AGENTS.md, Skills) |
| `skills/privatium-overview/SKILL.md` | the `privatium new --examples` line names three apps | same |

Nothing under `spec/` needs to change. The plan uses the contract as written; every
behaviour it relies on is specified.

One documentation defect found while reading, unrelated to this app but worth a line in the
same change: `apps/sketch/SKILL.md` cites `spec/data-api.md §3` for `api.max_batch`. §3 is
Live updates; the ceiling is stated in §2 and tabled in §7.

## 11. Corrections to the feature table

The table becomes part of `README.md`, so these are applied before it lands.

- Rename **Freezer** to **Pantry** throughout; keep the freezer shelves as the worked
  example.
- **Shelf map** teaches `PV404` (one `<h1>`, no skipped levels) and `PV401` (a labelled
  icon control), not `PV403`. `PV403` is `fieldset`/`legend` around a radio or checkbox
  group, which belongs to the **Add batch** row.
- **44 px targets** is AGENTS.md's accessibility target and a manual check. No `PV4xx` rule
  measures pointer target size; do not imply the linter catches it.
- **Add batch form** — add `PV402` and `PV403` to what it teaches, and state that the
  atomic `put batch` + `put quantity_change` is what `PV306` is about.
- **Activity list** — it is driven by the log, not by a view, because a tombstoned row is
  gone from the table. Say so in the cell; it is the sharpest lesson in the app.
- **Expiry** is a new row: what the user sees is a date column plus an **Expired** or **Use
  soon** label and an "Expiring soon" list; what it teaches is `PV308`'s date half —
  `date('now', '+N days')` rather than `expires_on + N` — and a nullable date rendered
  honestly as "no date" rather than as fresh.
- **Batch list** — the column is **stored on**, not "date frozen", since the app covers a
  pantry shelf too.
- **Stock summary** — `decimal_sum` across grains and `PV308`. Note that `decimal_sum` is
  NULL over zero rows, which is why an add writes its stock in the same batch.
- **No third parties** — the rules are `PV504` (no CDN), `PV207` (no external origin) and
  `PV205` (a declared permission carries a justifying comment).

## 12. Open questions and unverified claims

- **`CHECK` with a framework function.** The two `CHECK`s above use only `IN` and `IS NULL`,
  deliberately. It is **not** verified that a `CHECK` may call `decimal_cmp` — the constraint
  is evaluated on the framework's writing connection, which registers the functions, but no
  test in the repository exercises it. If a sign constraint on `amount` is ever wanted,
  that needs a test first.
- **`HAVING` with `decimal_cmp` inside a view.** Ordinary SQLite, and the functions are on
  every connection (`spec/app-contract.md §7`). No view in `apps/` used an aggregate before
  this app, which is the first `CREATE VIEW` in the repository's app corpus. Confirmed early
  rather than late: `test_pantry_balance_is_never_stored` is what holds it.
- **`pv_param` typing.** `$shelf`, `$since` and `$days` arrive as text
  (`spec/data-api.md §1`). Comparing text to a `VARCHAR` id and to a `TIMESTAMPTZ` stored as
  ISO text both compare correctly as text. Stated so a reader does not reach for a cast.
- **No notifications, of any kind.** There is no background job in Tier 2 and
  no framework scheduler in `pv/1`; "Expiring soon" is a list you open, not something that
  reaches you. A reminder is a Tier 3 capability and does not belong in a reference app.
- **Shelf creation** is in scope by inference: without it a fresh install with no seed has
  nowhere to put a batch. One labelled field and one event.

## 13. Record

- The app, its tests and the changes of §10 landed together on one branch.
- **Size.** 1041 lines of JavaScript across the four modules, of which 637 are code and the
  rest are the file headers and the doc comments the house style asks for. The intent of
  the 900-line target — one sitting, four modules, nothing hidden — holds.
- **One spec edit was needed after all**, against §10's expectation: `spec/cli.md §2` and
  `§4` name the example apps the binary carries, so both now name `pantry`. Nothing else
  under `spec/` changed, and no generated skill reference moved.
- **A design pass followed the first landing**, after the owner read the screen: the two
  halves of the page were laid out as rows of one grid, so every heading in the rail lined
  up with the bottom of a box in the work column and the page read as scattered boxes; the
  batch half was rendered with no shelf to put a batch on, and its Shelf field had nothing
  to offer; and the whole thing was a kit of identical cards with no hierarchy. The page is
  now one ruled sheet with two independent stacks, shelves drawn as slats, and the balance
  set large in tabular figures. `docs/sample-app-design.md` carries the layout and
  `apps/pantry/SKILL.md` the rule that keeps it, and `test_pantry_end_to_end` holds both
  the two stacks and the hidden batch half.
- **Checked in a real browser** at 1440, 390 and 320 pixels, in both colour schemes, with
  the seeded app and with an empty one: no horizontal page scroll at any of them, the batch
  table fits a phone without scrolling sideways, and the reflow trap that broke 320 pixels —
  a visually hidden label inside a scrolling table escaping its box — is fixed in
  `style.css`.
- **Still owed: the manual accessibility pass.** Keyboard-only traversal of every form and
  the tray, visible focus, 200 % zoom, reflow at 320 px, and a screen reader over add →
  take out → put back → undo. Browser automation does not run on the owner's machine
  (`docs/plans/phase-2.md`, "Phase 2 hardening"), so this is a person's pass and it has not
  happened yet.
- The `CHECK`-with-a-framework-function question of §12 stays open; nothing in this app
  depends on the answer.

---

Copyright © 2026 Gabriel Mongefranco
