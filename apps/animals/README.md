# animals — Tier 1 at its interesting end

The guess-the-animal game that shipped with every BASIC and Unix system worth using. It
starts with one question, "Does it have legs?", a dog on the yes side and a fish on the no
side, and learns one animal per round, forever.

It is here because it forces the framework to prove things `hello` does not.

## What it demonstrates

| Feature | Where |
|---|---|
| **Multi-event atomic writes** | `app.lua` → `pv.batch` emits three events as one |
| **`tx.append` returning minted ULIDs** | The new leaves are referenced before they exist |
| **Recursive SQL over event-sourced rows** | `lib/tree.lua` → `knowledge()` |
| **Stored session state** | The `cursor` table — a round survives a reload and continues on another device |
| **Splitting logic out of routes** | `lib/tree.lua` is `require`d by name |
| **Tombstones** | `reset` empties the tree and plants the starter again; the log keeps every round you played |
| **Accessible forms** | `fieldset`/`legend` for the radio pair, labels on every input |
| **The HTMX / Alpine boundary** | Both on one screen — see below |
| **Progressive enhancement** | `app.lua` → `board()` returns a fragment or a redirect |
| **Swap navigation** | `app.toml` → `[ui] navigation = "swap"`: pages change under a bar that does not move |
| **A strict CSP, satisfied** | `static/animals.js` — Alpine's CSP build, no `eval` |

## Why this app exists, in one sentence

**The animal tree *is* the event log.** The system learns by appending, and nothing is ever
updated. Teach it "ostrich" and that is a new line in a JSONL file you can `cat`. That is
the founding invariant made visible to a non-technical person in about fifteen seconds —
`hello` cannot show it without ceasing to be minimal, and `sketch` never touches SQL.

## HTMX and Alpine, on one screen

These are often described as opposite philosophies. They are better understood as
complementary halves of one rule:

> **If losing it on refresh loses data, it is HTMX.
> If losing it on refresh is fine, it is Alpine.**

`animals` has both, unforced:

| Interaction | Tool | Why |
|---|---|---|
| Submit a guess | HTMX | appends an event |
| Teach a new animal | HTMX | appends events, atomically |
| Tree fragment re-renders | HTMX | the node owns the truth |
| "Show the question path" disclosure | Alpine | pure UI, nothing persisted |
| Show example questions when teaching | Alpine | a hint, not an answer |
| Confirm before forgetting everything | Alpine | ephemeral by definition |

Two cases that look like exceptions and are not. **Teaching and forgetting are
plain form posts**, with no `hx-post` of their own, because they are navigations — a
separate page you arrive at and leave, and swapping a fragment there means owning the
back button. The frame's swap navigation (below) carries them like any other page change,
and the redirect each route answers with is where you land. And **every HTMX form still
carries `method` and `action`**: `hx-post` is an
enhancement, and `board()` in `app.lua` returns a fragment or a redirect depending
on `req.is_htmx`, so recording a guess never requires JavaScript.

The rule has a corollary the Alpine half has to honour too: **every write is reachable
with JavaScript off.** Alpine hides the reset form behind a confirmation and the
question paths behind a toggle, and without Alpine nothing would ever reveal them. So
`static/animals.css` has an `@media (scripting: none)` block that reverts `x-cloak` and
hides the buttons marked `pv-js-only`, the ones whose only job is to toggle Alpine state.
It is a media query rather than a `<noscript>` link because a swapped page is parsed with
scripting off, and a link inside `<noscript>` would arrive in the page as a live
stylesheet with JavaScript on. With JavaScript off the paths are printed and the reset form is
simply on the page: one step instead of two, and nothing you can do with scripts that you
cannot do without them.

### Alpine here is the CSP build, and that is the interesting part

An app runs under `script-src 'self'` with no `'unsafe-eval'`
(`spec/app-contract.md §5.4`). Standard Alpine compiles `x-data="{ open: false }"`
with the `Function` constructor, so it cannot run at all under that policy. The
CSP build trades inline expressions for registered components: `x-data` names
something in `static/animals.js`, and bindings reference its properties and
methods by key.

That is more typing and it is the right trade — the alternative is granting the
app `eval`, which hands any injected string a JavaScript engine to save a few
characters. An earlier version of `knowledge.lsp` used
`onsubmit="return confirm(...)"`, which was not a style problem but a silent
failure: an inline handler is script, and the policy blocks it.

One more thing the browser taught this app: `static/animals.js` must load **before**
Alpine. Alpine's CDN builds start themselves in a microtask as soon as their script runs,
and `alpine:init` fires right then — a component registered afterwards does not exist as
far as Alpine is concerned, and every `x-data` is an "Undefined variable" in the console.
`[ui] scripts` in `app.toml` keeps the order, and the frame loads both with `defer`.

Each use is commented in the source with *why that tool*, not *how it works*. The teaching
happens in the contrast, on one page.

Across the reference apps a reader sees all three postures without a comparison
document: server-owned state (`animals`, HTMX), client-owned ephemeral state (`animals`,
Alpine), and client-owned everything (`sketch`).

## Pages change in place

`app.toml` sets `[ui] navigation = "swap"`. A link or a form inside the page fetches the
next page as usual, and the frame swaps only its main region in, so the bar and footer stay
put and the screen does not flash between pages. The new page's heading takes focus, the
window title changes, and the address bar shows where you are. The back button reloads the
page you go back to. Anything outside the app — Settings, the launcher, another app — is a
full page load, as before. See `spec/lua-api.md §4.1` for the rules.

Two things follow for the code. The stylesheet and both scripts are listed under `[ui]` in
`app.toml` and loaded once, in the head, so no view carries a `<link>` or a `<script>` of
its own (`PV111`): a swapped page never runs one. And `static/animals.js` binds its
listeners to `document` once, when it loads, so a swapped page needs nothing re-run. Alpine
starts the components of a swapped page by itself, since it watches the document for new
elements.

## The starter tree

The first visit to the game finds no tree and writes one: two animals and the question
that tells them apart, as three ordinary `put` events in one batch. `lib/tree.lua` holds
the words in `STARTER` and the write in `plant(tx)`. Nothing is special about those rows
afterwards; the first wrong guess turns a leaf into a question the same way it always has.

**Forget everything**, on the What I know page, tombstones every node and plants the same
starter tree in the same batch, so the game goes back to its first question rather than
to an empty screen.

## The learning step

You land on a leaf. The app guesses "is it a *penguin*?" You say no, and tell it you meant a
*wombat*, distinguished by "does it have cubic droppings?"

The naive move creates a new question node and re-points the parent at it — but the parent
may not exist (this could be the root), and finding it means a traversal.

Instead, **the leaf becomes the question, in place, keeping its own ULID:**

```
before:                  after:
                                [does it have cubic droppings?]
   [penguin]      →             /                            \
                           yes /                              \ no
                       [wombat]                            [penguin]
```

Three `put` events: one rewriting the leaf into a branch, two creating fresh leaves. The
parent is never touched, and every existing pointer into the tree stays correct because the
branch reuses the leaf's id.

That is only clean when identity is a ULID you control and writes are appends.

## Sample data

`sample/seed.jsonl` holds seven events — three questions and four animals — a larger tree
than the starter, for seeing the knowledge page and the question paths populated. It is
offered on the settings page while the app's log is empty and loaded only when you ask
(`spec/app-contract.md §9`); the events are appended as this node's own, with fresh
envelopes. There is no `cursor` row in it, so the first round starts at the root.

Load it before you open the game. Opening the game plants the starter tree, and a seed
populates an empty log or nothing, so once the log holds those three events the offer is
gone. To try the seed after that, delete the app's `data/animals` folder and start again.

## Reading your own game history

```bash
jq -r '[.ts, .op, .d.text] | @tsv' data/animals/log/*.jsonl
```

Every animal you ever taught it, in order, including the ones you reset away.

## Later: the sync demo

Once sync (`spec/protocol.md §10`) is built, this app becomes the best
demonstration of sync in the repository. Wire the history fragment to `/api/stream` with the
HTMX SSE extension — one attribute — and teaching an animal on the desktop updates the
phone's history live, on screen, with no polling code and no reload. Two devices, one
visible cause and effect.

---

Copyright © 2026 Gabriel Mongefranco
