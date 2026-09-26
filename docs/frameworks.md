<!--
This file is part of Privatium
docs/frameworks.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-09-26
Summary: Which frameworks, libraries and engines work inside Privatium, which do not, and why.
         Selection criteria are explicit and testable.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Framework Compatibility

## 1. The four criteria

Every recommendation below is scored against these, in order:

1. **No build step.** Drop a file in `static/` or `web/vendor/` and it works. A library
   requiring npm, a bundler, or a transpiler before you can see a change fails the DX
   goal that motivated the whole Lua tier.
2. **LLM-writable.** An owner should be able to describe what they want to any assistant
   and get working code. This favours large training footprints and stable APIs over
   technical elegance. Mitigated for every choice by shipping a pinned API reference in
   `app-skills/` (`docs/skills.md`) so the model does not rely on training data alone.
3. **Small.** This runs on a phone over cell data. Kilobytes, not megabytes.
4. **Open source, and survivable.** OSI license, and a maintainership that will still
   exist in five years.

## 2. Defaults

Vendored, pinned, and shipped in the box. An app author writes nothing to get these.

| Slot | Default | Size | Why |
|---|---|---|---|
| Server templates (Tier 1) | **LSP** (`<? ?>`, built in) | 0 | Same language as handlers; PHP/EJS/ERB-shaped, so LLMs write it first try |
| Interactivity | **HTMX** | ~14 KB | Server-rendered fragments; the interaction model the Lua tier assumes |
| Local reactivity | **Alpine.js** | ~15 KB | Attribute-based; huge training footprint; no build |
| Icons | **Bootstrap Icons** | inlined | `docs/icons.md` |
| Charts | none — pick your own | 0 | The framework ships no charting. Tier 1 can emit inline SVG from Lua; anything interactive is Tier 2 (§4.1). |
| 2D games | **Phaser** | ~1 MB | Largest game training footprint by far |
| 3D | **Three.js** | ~600 KB | The standard; more library than engine |

Nothing here is mandatory. A Tier 2 app can delete all of it.

## 3. Tier 1 — Lua on the node

Tier 1 renders server-side. The only client question is what, if anything, to sprinkle on
top.

| Library | Verdict | Notes |
|---|---|---|
| **HTMX** | ✅ **Default** | Fragment swaps, `hx-*` attributes. Pairs naturally with LSP partials. |
| **Alpine.js** | ✅ **Default** | For state that has no business on the server — dropdowns, tabs, toggles. |
| **Datastar** | ✅ Supported alternative | Hypermedia plus signals, and **SSE-native**, which fits `/api/stream` well. Smaller community and less training data than HTMX, so it is documented rather than default. |
| **VanJS** | ✅ Supported | ~1 KB, function-based components, no JSX. Good when you want composition without a framework. |
| **Petite-Vue** | ⚠️ Works | ~6 KB, Vue template syntax, no build. Fine; Alpine has more momentum. |
| **jQuery** | ⚠️ Works | Not recommended for new work, but it will not break anything. |

**Lua libraries.** There is no package manager and no LuaRocks, and nothing is planned. A
pure-Lua library is vendored by copying its files under the app's `lib/`, keeping the
library's own layout — `lib/inspect.lua`, or `lib/penlight/…` — because `require` resolves
a dotted name against `lib/` and only there (`spec/lua-api.md §5`), and the library's own
internal `require`s have to resolve the same way. A rock that ships C sources cannot be
used: the sandbox removes `package.loadlib` and `package.cpath`, and iOS forbids loading
native code an app was not shipped with. Check the library's licence is GPL-3.0 compatible
and record where it came from, as `apps/animals/static/VENDOR.md` does for a browser
library; the linter reads `lib/` like any other app code.

Server-side templating alternatives, all rejected in favour of LSP: **MiniJinja** and
**Tera** (Rust-side, so a second language in one app), **etlua** (fine, but reimplementing
buys hot reload and escaping-by-default), **Mako Server / Barracuda** (non-commercial
license, unsandboxed Lua — see `spec/lua-api.md §1.1`).

## 4. Tier 2 — your own `web/`

The framework serves your directory and injects nothing. Anything that produces static
files works. The table is about *fit*, not permission.

### 4.1 Good fits

| Library | Build step | Notes |
|---|---|---|
| **Vanilla JS + `pv.js`** | none | The floor, and often enough |
| **Alpine.js** | none | Reactive sprinkles — **use the `@alpinejs/csp` build.** The standard build compiles attribute expressions with `Function`, which needs `'unsafe-eval'`; the CSP build swaps inline expressions for components registered with `Alpine.data()`. Working example in `apps/animals`. |
| **Datastar** | none | Signals + SSE |
| **VanJS** | none | 1 KB components |
| **Preact + htm** | none | Component model via tagged templates, no JSX, ~4 KB |
| **Lit** | none | Web components, standards-based, ~5 KB |
| **PixiJS** | none | 2D WebGL rendering |
| **Chart.js / uPlot / D3** | none | Charting. uPlot is ~45 KB and very fast. |
| **wasmoon / Fengari** | none | Lua in the browser; share logic with Tier 1 |
| **RxDB** | none (free storages) | A client-side replica with its own sync engine. Works, and usually unnecessary — the framework's own outbox covers the same ground with no dependency. Its useful storages are paid, and it has no discovery layer of its own. See `docs/decisions/0004 §2`. |

### 4.2 Work, with a build step you run yourself

Commit the output to `web/`. The framework never runs your build.

| Library | Notes |
|---|---|
| **Svelte** | Compiles away; end users pay no framework cost. The best of this group. |
| **SolidJS** | Fine-grained reactivity, small output |
| **Vue (SFC)** | Works. Also usable build-free via the global build, at a size cost. |
| **React / Next.js** | Works in `web/`, and you may vendor it. Next.js's server features are dead weight here — you already have a server. |
| **Angular** | Works, but the payload is hard to justify for a personal app |

### 4.3 Schema-driven form libraries

| Library | Verdict |
|---|---|
| **JSON Forms** | ✅ Vanilla renderers available; pairs well with `pv.append()`. Reasonable for form-heavy Tier 2 apps. |
| **AMIS** | ⚠️ Powerful JSON-to-app renderer, but **requires React at runtime**. Only worth it for a genuinely complex app. |
| **react-jsonschema-form** | ⚠️ React-only and form-only |

Note that Tier 1 plus a scaffold generator covers most of what these exist for, without the
runtime.

### 4.4 Will not work — full platforms

| Platform | Why not |
|---|---|
| **Appsmith, Budibase, Saltcorn, NocoBase, Grist, Retool** | Each brings its own database, auth, user model, and server. They are alternatives to Privatium, not libraries inside it. There is no way to drop one into `web/`. |
| **Webix** | GPL/commercial dual license. Incompatible with shipping alongside GPL-3.0 unless you accept GPL for the whole app, and heavy besides. |

**Worth stealing ideas from, though:** Grist's user-editable formula columns are a superb
feature and become trivial once Lua is in the binary. NocoBase's plugin boundaries are
instructive. Study them; do not embed them.

## 5. Games

### 5.1 What Privatium gives a game

Not rendering. **Save files that sync across your devices with no account, no cloud, and no
vendor.** That is nearly orthogonal to engine choice, which is why the list below is
permissive.

### 5.2 In `web/` (Tier 2)

| Engine | Fit | Notes |
|---|---|---|
| **Phaser** | ✅ **Default** | 2D, mature, MIT, script-tag usable. Largest training footprint. Caveat: LLMs sometimes mix Phaser 2 and 3 idioms — pin the version and ship its reference in `skills/`. |
| **KAPLAY** | ✅ Excellent | MIT, formerly Kaboom.js, community-maintained after Replit dropped it. Terse declarative API (`add([sprite("bean"), pos(80,40)])`) that LLMs handle very well, and the fastest path to a first pixel. Governance is a community fork, which is the one risk. |
| **PixiJS** | ✅ | Renderer only; you write the loop |
| **Three.js / Babylon.js** | ✅ | 3D. Three is a library, Babylon an engine. |
| **PlayCanvas (engine)** | ✅ | Engine is open source; editor is cloud-hosted |
| **Heaps.io / Kha** | ✅ | Haxe → JS. Good output if your author knows Haxe. |
| **love.js** | ⚠️ | LÖVE via Emscripten genuinely exists and is current — Davidobot tracks LÖVE 11.5, and love-web-builder targets 12.0 on an experimental SDL3 port. But it is a community port with real friction, and its threaded mode hits §5.4. For Lua game logic, **Phaser or PixiJS plus wasmoon is the better path.** |
| **Godot** | ⚠️ Solo mode only | See §5.4 |
| **Unity** | ⚠️ Solo mode only | Multi-megabyte WebGL exports wreck the offline story |
| **Bevy (→ WASM)** | ⚠️ | Works, but you are running a Rust→WASM pipeline. Prefer Tier 3. |

### 5.3 Native engines: talk to the HTTP API

**This is a first-class path, not a failure.** An engine that produces no web build can run
as an ordinary native application and POST to `/a/<slug>/api/events`. LÖVE on the desktop,
Solar2D or Gideros on a phone, MonoGame, cocos2d-x — all get cross-device saves with no
account, and none of them need to live inside `web/`.

Pair the game with a small Tier 1 app that renders save history, statistics, and a
leaderboard in the browser. That is often the best of both.

Better still, link `privatium-core` directly. It exposes a C ABI, so LÖVE reaches it through
LuaJIT's FFI, Godot through GDExtension, Unity through P/Invoke, and Bevy as an ordinary
crate — **with no server, no localhost port, and no daemon**. The HTTP path above remains
correct and is the right answer when a process boundary is wanted anyway. The planned `lantern`
reference app demonstrates the linked path with a paired Tier 1 app rendering its history.

### 5.4 The cross-origin isolation problem

Godot 4, Unity, and love.js in threaded mode need `SharedArrayBuffer`, which requires
cross-origin isolation — `COOP: same-origin` plus `COEP: require-corp`. Without the
headers, only Chromium browsers load these builds, and Godot ships an export toggle plus a
"disable Thread Support" option (4.3+) to avoid the requirement at the cost of audio.

**This applies to a browser SQLite build too, and it matters more.** A shared-memory build
needs `SharedArrayBuffer` and therefore cross-origin isolation. A Tier 1 offline query
runtime that required cross-origin isolation would impose those headers on the framework's
own origin and **break host mode for every other app on the node**. If browser SQLite is
ever adopted for offline Tier 1 rendering, it is the **single-threaded,
asynchronous build only** — no exceptions, regardless of benchmark results. Otherwise Tier 1
offline becomes solo-mode-only, which is not a trade worth making.

**This conflicts directly with host mode.** COOP/COEP are document-level headers on a
single origin. If `/a/mygame/` sets `COEP: require-corp`, every subresource lacking CORP
headers is blocked — including the shell's own assets. Setting it globally breaks every
other app on the node.

Resolution:

| Situation | Rule |
|---|---|
| Host mode (many apps) | Isolation-requiring builds are **not supported**. Export single-threaded. |
| Solo mode (one app at `/`) | `permissions.cross_origin_isolated = true` is allowed, and the node then sends both headers on every response of the origin (`spec/protocol.md §9.3`) |
| Native engine over HTTP | Not affected |

An app declaring `cross_origin_isolated` in host mode MUST fail to load with an explanatory
error, not a broken page.

### 5.5 Out

**Unreal Engine** (HTML5 pipeline removed), **Marmalade SDK** (discontinued), **MonoGame**
(no viable web target), **cocos2d-x** (native C++; Cocos Creator is a separate product),
**SDL, bgfx, Ogre3D, The Forge, Tilengine** (native rendering libraries — you would be
building the Emscripten pipeline yourself). Any of these can still use §5.3.

## 6. Tier 3 — Rust

`privatium-core` is a Rust crate. Only Rust links against it.

| Crate | Fit |
|---|---|
| **axum / actix-web / poem** | ✅ Bring your own HTTP stack |
| **Bevy** | ✅ Native game with synced saves |
| **Fyrox** | ✅ Same, with an editor |
| **rust-sdl2 / macroquad** | ✅ Lightweight 2D |
| **egui / iced / slint** | ✅ Native GUI over the same data layer |
| **Tauri** | ✅ How the official desktop and mobile shells are built |

**Not only Rust, in practice.** `privatium-core` also exposes a C ABI (`privatium-ffi`),
which is how a non-Rust engine links it without a server:

| Consumer | Via | Server needed |
|---|---|---|
| **LÖVE** | LuaJIT FFI | no |
| **Godot (native)** | GDExtension | no |
| **Unity** | P/Invoke | no |
| **Swift / Kotlin** | `uniffi` | no |
| **Bevy, macroquad, egui, …** | crate dependency | no |

This is the capability a JavaScript sync core would have removed, and the reason the core is
Rust — see `docs/decisions/0004 §6`.

Nothing from §3–§5 applies here. Those are web technologies; Tier 3 is a binary.

## 7. Adding to this document

A library earns a row by being tested against a real app in `apps/`, not by being
plausible. Include the version tested, the size, and whether a build step was needed.

---

Copyright © 2026 Gabriel Mongefranco
