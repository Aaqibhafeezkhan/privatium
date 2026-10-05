# sketch — the Tier 2 reference app

A shared canvas. Draw in one browser window and it shows up in another. A page already
open can queue strokes while disconnected and replay them on reconnect. A plain-HTTP
LAN page cannot load anew while the node is offline. A phone on the same network pairs
by scanning the QR code the node prints and tapping four emoji, then draws on the same
canvas over the encrypted channel. Draw with a pointer, or from the keyboard: focus the
canvas, move the pen with the arrow keys, put it down with Space.

It exists to prove one thing: **the framework does not impose an application model.**

One canvas means one canvas. If you drew on your desktop and your laptop before joining
them, both drawings are there afterwards, merged, every stroke kept — and undo reverses
whichever was drawn last.

## What is not here

| File | Present? |
|---|---|
| `views/` | **No.** No server-rendered HTML. |
| `app.lua` | **No.** No server-side code. |
| `schema.sql` | **No.** This app has no tables. |
| A build step | **No.** Plain ES modules; `app.js` is what ships. |
| The framework's bar and footer | **No.** `app.toml` sets `[ui] chrome = "none"`; see below. |

`web/` contains an HTML page, a stylesheet, eight small JavaScript modules and the Pv
mark in its two schemes. Together with `app.toml`, that is the entire app.

The sheet is a fixed 1600 × 1200 surface, so a mark lands on the same pixels on every
device that shares it — sizing the canvas to each window instead is what once put a
desktop drawing in the corner of a phone. Ten tools sit in a labelled rail with the
active tool's own options beneath them; the top bar carries the tools reached most often
and, for the tools that lay ink down, the colour row. Everything is reachable from the
keyboard, every control keeps its touch size, and on a narrow screen the rail becomes a
strip below the sheet. Open **Help** for the keyboard map. Help and status text stay
readable when zoomed; see [Sample app design](../../docs/sample-app-design.md) for the
layout and its limits.

## Why it declines the bar

Every other app wears the framework's bar and footer, inserted around its page
(`spec/app-contract.md §5`). Sketch turns them off with `[ui] chrome = "none"` in
`app.toml`, because a drawing surface wants the whole window: a bar above the sheet and a
footer below it would take height from the one thing the app is for, and the sheet's own
status line already says what the drawing holds. The document is therefore served byte
for byte, and the way back is the app's to draw. It draws it twice from one source
(`web/exit.js`): the Privatium mark at the top of the tool rail is a link, and the menu
carries an Apps entry; both lead to the launcher, or to Settings in solo mode, where
there is no launcher to return to. On a narrow screen the rail's heading is hidden and
the menu's entry is the way out. An app that keeps the bar gets all of this for nothing,
and `apps/pantry` shows that side.

## What the framework still gives it

The framework provides:

- **Storage** — every stroke is an append-only event in `data/sketch/log/<device>.jsonl`
- **Backup** — copy `data/`, and every stroke you ever drew comes back
- **Offline** — writes queue in an outbox, replay on reconnect
- **Live updates** — a stroke drawn in one window reaches every other open window, and
  undo reverses the latest change to the shared canvas whichever window or device made it
- **Authentication and encryption** — `pv.js` uses the paired browser's encrypted
  channel on the LAN; no credential code belongs in this app
- **Sync** — a stroke drawn on one machine reaches the others on your network

Reaching them from outside your home, and over a synced folder, is still to come.

## The event log as a document store

No `schema.sql` means no validation and no SQL — `d` is stored as-is:

```js
await pv.put('stroke', pv.ulid(), { points, color, width });
```

A freehand stroke is still exactly that. A shape, a text block, the colour inside a shape
and the sheet's own colour each add a `kind`, and options add a field each — `dash`,
`blend`, and a fill's `anchor` — so every log written before those tools existed replays
unchanged.

For a drawing app or a game this is frequently the right call. You still get replication,
snapshots, and a plain-text backup. Read your own drawing back with no Privatium installed:

```bash
jq -r '.d.color' data/sketch/log/*.jsonl | sort | uniq -c
grep -c '"op":"put"' data/sketch/log/*.jsonl
```

## Use a real framework if you want

The framework's own UI is HTMX and ships no client framework. That is a decision about
*the framework*. Your `web/` directory is yours — vendor React, Three.js, Chart.js, a WASM
blob, whatever the app needs. You pay for it in bytes on a phone, and that is your call.

This app uses vanilla JS because a canvas needs no framework, not because one was forbidden.
Ten tools, a colour wheel and a clipboard did not change that: the sheet is imperative
drawing, and the toolbars are buttons with `aria-pressed`.

## Why this is not a Tier 1 app

A drawing canvas has no server-rendered form of itself. Tier 1 would mean shipping a
`<canvas>` and then writing all the JavaScript anyway, with an LSP template that does
nothing but wrap it. When the interface *is* the interaction, Tier 2 is the honest choice.

## Solo mode

```toml
# config.toml
[node]
mode = "solo"
app  = "sketch"
```

Now the binary *is* Sketch. Mounted at `/`, no launcher, its icon and title become the
node's. Indistinguishable from a purpose-built app.

---

Copyright © 2026 Gabriel Mongefranco
