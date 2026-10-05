# pantry — the Tier 2 reference app with tables

What is in the freezer and on the shelf, and what you took out of it. Shelves hold batches,
a batch holds an amount, and every change to that amount is recorded: stocked, taken out,
put back. Open it on your phone as well and both screens settle on the same numbers.

`sketch` is the other Tier 2 app, and the pair is deliberate. `sketch` has no
`schema.sql` and uses the log as a document store. This one has a schema: typed columns,
named views, exact decimals and forms. Same tier, opposite choice.

**The balance is not stored anywhere.** It is the sum of the recorded changes, worked out
when you look. That one decision is what the rest of this page explains.

## 1. Open it

A shelf, a batch on it, and the amount you have. Load the sample data from **Settings →
Apps → Load sample data** to start with a stocked freezer, or add a shelf and go — with
nothing stored yet the page asks for a shelf first, because a batch has to go somewhere.

## 2. Add a batch

Open **Add a batch**. Say what it is, how much, what it is measured in, which shelf, the
day it went in, and — only if the package says so — a use-by date.

Two things happen that are worth knowing:

- **The amount is text, all the way down.** You type `1.5`, and `1.500` is what is stored,
  at the three decimal places `schema.sql` declares. It is never turned into a JavaScript
  number, because `0.1 + 0.2` is not `0.3` in a JavaScript number and a kitchen does not
  need that kind of surprise. Reading it back gives you the string `"1.500"`.
- **A blank date stays blank.** Plenty of a pantry has no date on it. The screen says
  "no date", which is honestly different from "fresh".

Adding a batch writes **two events in one batch of the log**: the batch itself, and the
stock it arrived with. They land together or not at all, because a batch with no recorded
change would have no balance at all — the sum of no rows is nothing, not zero.

## 3. Take some out, and put some back

Open a row's **Take out or move** control and say how much. That writes one more change,
a negative one. The balance on screen goes down because the sum went down; no row was
edited.

What is out sits in the tray on the right. Put some of it back and the card says
"1 of 2 still out" until the rest comes home.

Here is the whole of it, in `schema.sql`:

```sql
decimal_sum(c.amount) AS balance
```

`decimal_sum` is the framework's exact addition over `DECIMAL` text. SQLite's own `SUM()`
would give you a floating-point number, which is why `privatium lint` refuses it (`PV308`).

## 4. Open it on your phone too

Pair the phone by scanning the QR code the node prints. Take something out on the phone
and watch the laptop follow within a second: `pv.subscribe` delivers every event from every
device, and the page re-reads the views it affects.

## 5. Undo

Every activity row has an **Undo**. It writes a tombstone on that one change. The change
leaves the tables, the balance goes back, and the log keeps both lines forever.

Undo the same change on two devices and nothing breaks: both write the same tombstone, and
the merge rule takes the last event for the row, which is a tombstone either way. Undoing
twice and undoing once leave the same state — no counter, no guard, no bookkeeping.

## 6. Move a batch

Moving writes the batch row again, under **the same id**, with a new shelf. It is not a
delete and a re-add: a tombstoned id is never reused, so a new id would leave every
recorded change pointing at a batch that is gone. Two devices moving one batch to two
shelves converge on one row, ordered by `(lam, ts, dev)` — the batch is in one place, not
two, and its history is intact.

## 7. Go offline, and both take the last portion

This is the honest part.

Take the network away from both devices. Each takes the last portion. Each writes its own
change, with its own id. When they meet again, **both rows are kept** — they are different
rows, and nothing in the merge rule throws one away. The balance is now `-1`.

The app says so. **Check stock** names the batch, shows the recorded `-1`, and says what
it means: two of you took the same last portion. It does not clamp the number to zero,
because zero would be a number nobody recorded. The same holds for a withdrawal that has
had more put back than went out.

A cache that hid this would be lying about what the log says.

## 8. Read your pantry without Privatium

The log is JSONL, one event per line:

```bash
jq -r 'select(.tbl=="batch") | .d.name' data/pantry/log/*.jsonl | sort -u
jq -r 'select(.tbl=="quantity_change") | [.d.reason, .d.amount] | @tsv' data/pantry/log/*.jsonl
```

Copy `data/` and you have copied everything.

## What each part of the app is for

| What you see | What it teaches |
|---|---|
| **The bar and the footer** | Not the app's: the framework inserts them around the page (`spec/app-contract.md §5`). The page writes the three anchors and `<main id="main">` for them (`PV109`), draws no way back of its own (`PV408`), and says what happened through `pv.status`, in the footer's status line. Set `[ui] chrome = "none"` in `app.toml` to own the whole window instead, as `sketch` does. |
| **Shelf map** | One `<h1>` and headings in order (`PV404`), and a labelled control for each shelf (`PV401`). Which shelf is open is `localStorage`, never an event. |
| **Add batch form** | A `<label for>` on every field (`PV402`) and the icon choice inside `fieldset`/`legend` (`PV403`). Errors are rendered in place, the client checks first and the node checks again from the DDL, and the batch row and its first change go in **one** `pv.append` — which is what `PV306` is about. |
| **Batch list** | A real `<table>` with `<th scope="col">` (`PV407`). The columns are **stored on**, days in and use by; the balance beside them is a view's `decimal_sum`, not a column, and it is set large because it is what you came to read. Narrow screens drop the columns that say least rather than scrolling sideways. |
| **Expiry** | A date column plus an **Expired** or **Use soon** label and an "Expiring soon" list. It teaches `PV308`'s date half: `date('now', '+30 days')`, never `expires_on + 30`, which SQLite would read as integer arithmetic. A missing date is shown as "no date", not as fresh. |
| **Tray** | `$since` bound from the query string into a view (`spec/data-api.md §1`), and a return that points at the withdrawal it undoes part of. |
| **Activity list** | Driven by **the log**, not by a view: materialization drops a tombstoned row entirely, so SQL can show an undo's effect but never the undo. The tables are what is true now; the log is what happened. |
| **Check stock** | Two views over the same sums, reporting what two devices actually wrote. |
| **Stock summary** | `decimal_sum` across grains, and why an add writes its stock in the same batch: the sum over no rows is NULL. |
| **44-pixel targets, 200 % zoom, 320 px reflow** | The accessibility target in `AGENTS.md`. No linter rule measures a pointer target; this one is checked by a person. |
| **No third parties** | No CDN (`PV504`), no external origin (`PV207`), and every declared permission carries the comment `PV205` wants — all of them left at their defaults. |

## What is not here

- **A stored balance, or a cached total of any kind.** See §7.
- **A pending-write count.** `pv.js` exposes none, and inventing one would be guessing.
- **Notifications.** "Expiring soon" is a list you open. There is no background job in
  Tier 2 and no scheduler in `pv/1`; a reminder is a Tier 3 capability.
- **A dedupe table, a transaction id, an acknowledgement.** ULIDs make a replay idempotent.
- **A way back, or a status line of its own.** The bar links to the launcher and the
  footer carries the status slot; the app writes into it and draws neither.
- **A graphics library, a build step, a vendored dependency.** Four ES modules and a
  stylesheet.

## The files

| File | What is in it |
|---|---|
| `app.toml` | The manifest, with every permission at its default and said out loud |
| `schema.sql` | Three tables, two indexes, seven views, each view with the grain it returns |
| `web/index.html` | The page: the work, the rail, every form field labelled, the icon sprite — served with the framework's bar and footer around it |
| `web/app.js` | Boot, the queries, and what each control does |
| `web/views.js` | Everything drawn, with `createElement` and `textContent` |
| `web/forms.js` | Reading the forms, and saying what is wrong with them in place |
| `web/writes.js` | The six changes this app can make, one `pv.append` each |
| `web/style.css` | The sheet, the rule that divides it, the shelf slats, and every colour |
| `sample/seed.jsonl` | Four shelves, six batches and twelve changes, all invented |

## Solo mode

```toml
# config.toml
[node]
mode = "solo"
app  = "pantry"
```

Now the binary *is* Pantry: mounted at `/`, no launcher, its icon and title become the
node's. The grid icon at the top right, which returns to the launcher in the ordinary
setup, goes to the settings page instead — there is no launcher left to go back to.

---

Copyright © 2026 Gabriel Mongefranco
