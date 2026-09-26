---
name: privatium-games
description: Build games on Privatium with synced save files and no account or cloud. Covers engine selection for web and native, using the event log as a save-game store, and the cross-origin isolation limit that blocks Godot and Unity in host mode. Load alongside privatium-tier2-web for browser games or privatium-tier3-rust for native ones.
---

# Privatium Games

## What Privatium gives a game

Not rendering. **Save files that sync across the player's devices with no account, no
cloud, and no vendor.** Start on the laptop, finish on the phone. Copy a folder to back up
every save you ever made.

That is nearly orthogonal to engine choice, which is why the guidance below is permissive.

## Choosing

| Want | Use | Tier |
|---|---|---|
| 2D in the browser | **Phaser** (default) or **KAPLAY** | 2 |
| Fastest path to a first pixel | **KAPLAY** — terse declarative API, MIT | 2 |
| Renderer only, your own loop | **PixiJS** | 2 |
| 3D in the browser | **Three.js** or **Babylon.js** | 2 |
| Lua game logic in the browser | Phaser or PixiJS + **wasmoon** | 2 |
| Native game in Rust | **Bevy** or **Fyrox** | 3 |
| An existing native engine (LÖVE, Solar2D, MonoGame, cocos2d-x) | Keep it native; talk to the HTTP API | — |

Vendor the library into `web/vendor/`. **Never load from a CDN** — it breaks offline, leaks
the player's IP, and needs a `remote` permission.

## Saves without SQL

Omit `schema.sql`. The event log is a document store:

```js
await pv.put('save', 'slot1', { level: 7, hp: 42, inventory: [...] });
const save = await pv.get('save', 'slot1');   // the event, or null when there is no save
if (save) load(save.d);

// High scores are a list, not a blob
await pv.append([{ op:'put', tbl:'score', id: pv.ulid(),
                   d:{ points: 9001, at: new Date().toISOString() } }]);
```

Snapshots, plain-text backup and replication across your machines, with no schema to
maintain.

**Save on meaningful boundaries** — level complete, checkpoint, quit — not every frame. Each
append is a durable line in a log file, and one that syncs to every device.

Use `pv.subscribe` to notice a save written on another device mid-session, and offer to
reload rather than silently overwriting; handle `pv.on('resync', …)` the same way — the
node rebuilt its cache underneath you, so re-read.

## Cross-origin isolation — read this before choosing Godot or Unity

Godot 4, Unity WebGL, and love.js in threaded mode need `SharedArrayBuffer`, which requires
`COOP: same-origin` plus `COEP: require-corp`. Without those headers only Chromium browsers
load the build.

**These are document-level headers on a single origin, so they break host mode.** If
`/a/mygame/` sets `COEP: require-corp`, every subresource without CORP headers is blocked,
including the shell's own assets.

| Situation | Rule |
|---|---|
| Host mode (many apps) | **Not supported.** Export single-threaded — Godot 4.3+ has a Thread Support toggle. |
| Solo mode (one app at `/`) | `permissions.cross_origin_isolated = true` is allowed |
| Native engine over HTTP | Not affected |

An app declaring `cross_origin_isolated` in host mode fails to load with an explanatory
error. Do not attempt to work around it with a service worker.

Also weigh payload: multi-megabyte WebGL exports defeat the offline and mobile story that
makes Privatium worth using.

## Native engines are a first-class path

An engine with no web build runs as an ordinary native application and POSTs to
`/a/<slug>/api/events` after pairing. LÖVE on the desktop, Solar2D or Gideros on a phone,
MonoGame, cocos2d-x — all get cross-device saves and none need to live in `web/`.

Pair it with a small Tier 1 app that renders save history and statistics in the browser.
That is often the best arrangement.

## MUST

- Vendor engines locally; pin the version and note it in `app.toml`
- Provide keyboard controls and respect `prefers-reduced-motion`
- Give canvas an accessible name and a text alternative for essential information
- Never gate progress on colour alone

## Out

Unreal (HTML5 removed), Marmalade (discontinued), MonoGame web, cocos2d-x, and raw SDL,
bgfx, Ogre3D, or Tilengine — all can still use the native path above.

## Verify

```bash
privatium lint apps/<slug>
```

Detail: `docs/frameworks.md §5`, `spec/data-api.md`.
