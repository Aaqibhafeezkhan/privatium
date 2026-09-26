---
name: privatium-overview
description: Start here when building any Privatium personal app. Routes you to the correct tier (Lua, custom web, or Rust) and deployment mode, and states the invariants that hold across all of them. Load this before any other privatium skill when the tier is not already decided.
---

# Privatium: Choosing How to Build

Privatium is a personal-app framework. One binary provides append-only storage, multi-device
sync with no server, discovery, pairing, encryption, snapshots, and plain-text backup. What
you build on top is your choice.

## Pick a tier

| If the app is… | Tier | Load next |
|---|---|---|
| Records, lists, forms, reports, trackers | **1 — Lua** | `privatium-tier1-lua` |
| A game, canvas, drawing, animation, or has its own interaction design | **2 — Web** | `privatium-tier2-web`, `privatium-games` |
| Hardware, scheduled jobs, or a non-HTTP protocol | **3 — Rust** | `privatium-tier3-rust` |

**Default to Tier 1.** No build step, hot reload, no JavaScript required. Move to Tier 2
only when the interface genuinely cannot be server-rendered HTML.

Tiers mix freely on one node. A Tier 2 game can sit beside a Tier 1 tracker.

## Pick a mode

| Mode | Meaning |
|---|---|
| **host** (default) | Many apps at `/a/<slug>/`, with a launcher |
| **solo** | One app at `/`. Indistinguishable from a purpose-built application. |
| **embedded** | Your own Rust binary; the core is a library |

Use `url('/path')` (Lua) or `pv.url('/path')` (JS) for every internal link. **Never
hardcode `/a/<slug>/`** — it breaks in solo mode, and the linter flags it.

## Invariants — true in every tier

1. **JSONL is the only truth.** The SQLite cache, snapshots, and CSV are caches; deleting all of them
   must lose zero data. Never write an `UPDATE` — the answer is always an append.
2. **One writer per log file.** A device appends only to its own log.
3. **Append-only.** Corrections are new events. Deletions are tombstones.
4. **`DECIMAL` and `BIGINT` are strings in JSON.** JSON numbers are doubles. Converting
   money to a float is a bug every time, in every language.
5. **The client never stamps `seq`, `lam`, `ts`, `dev`, or `app`.** The framework does.
6. **No secret enters a log file.** Not keys, not codes, not tokens.
7. **XDG paths only.** Never write beside the binary.
8. **IDs are ULIDs.** No sequences, no auto-increment. They are also what makes an offline
   outbox idempotent — never add a dedupe table or transaction IDs; whether a queued write
   landed is read from the log, never remembered. The one exception is a deliberate
   singleton keyed by a constant, the way `apps/animals` keys its `cursor` row `'cursor'`;
   anything arriving over the HTTP data API must still be a ULID.
9. **No node is primary.** Every node is a peer. An always-on node on a VPS is a peer that
   happens to be reachable, not a server.
10. **Devices pin the cluster key, not a node key.** Pair a phone once and it trusts every
   node in the cluster. The cluster *private* key never leaves a node.
11. **Discovery runs concurrently, never chained** — mDNS, UDP broadcast, pkarr on the
   mainline DHT, and DNS all at once. They fail in different environments.
12. **A relay holds nothing; a node holds everything.** Never suggest putting a full node on
   rented hardware when a relay would do.

## Client capability

Not every client can do everything, and it is a property of the runtime, not a setting:

| | Node | Native desktop | Native mobile | Browser / PWA |
|---|---|---|---|---|
| Full replica | ✔ | ✔ | optional | ✘ |
| mDNS discovery | ✔ | ✔ | ✔ | ✘ |
| Multi-endpoint failover | ✔ | ✔ | ✔ | ✘ — single origin |

**A browser cannot find a node** — the user supplies the address. Never write code that has
a browser client try a LAN address and then a remote one; mixed content forbids it.

**Both tiers run on mobile and both update dynamically** — Tier 1 sends HTML, Tier 2 sends
web assets. Neither needs a per-user build. The one difference is offline: a Tier 1 app can
show cached pages and queue writes but cannot render a view it has not visited, because it
renders on the node. **If full offline on mobile is a requirement, choose Tier 2.**

Never propose shipping a native Lua interpreter that downloads and executes an app's source.

## Verify

```bash
privatium new <slug> [--tier lua|web|rust] [--from hello] [--scaffold <table>]
privatium new --examples        # hello, animals, sketch and pantry, written to <data-dir>/apps/
privatium dev --app <slug>
privatium lint apps/<slug>
privatium lint apps/<slug> --format json
privatium lint apps/<slug> --fix      # the two mechanical corrections of spec/cli.md §5.3, nothing else
privatium skill export          # these skills, matching the running version (spec/cli.md §6)
```

Generate, lint, fix, repeat. Do not present an app as finished until the linter is clean.
Every rule, with its severity and the section to read, is `reference/lint-rules.md`.

## Also load

`privatium-accessibility` and `privatium-security` apply to every tier. Load them alongside,
not instead of.

## Detail

`spec/app-contract.md`, `spec/lua-api.md`, `spec/data-api.md`, `docs/frameworks.md`,
`docs/connectivity.md` (what works over which network, per client kind)
