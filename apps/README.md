# apps/

Reference applications. These are normative examples of `spec/app-contract.md`, not
demos — the framework's own tests run against them.

| App | Tier | Read it for |
|---|---|---|
| [`hello`](hello) | 1 — Lua | Three routes, one table, two LSP templates. **Start here.** |
| [`animals`](animals) | 1 — Lua | Atomic multi-event writes, recursive SQL, stored session state, `lib/` modules — and the clearest place to *see* that nothing is ever updated. Also where HTMX and Alpine.js sit side by side. |
| [`sketch`](sketch) | 2 — Web | Your own HTML and JavaScript, no SQL at all. The framework as a syncing datastore. |
| [`pantry`](pantry) | 2 — Web | The same tier with tables: `schema.sql`, named views through `pv.query`, exact decimals, forms that validate twice, and two devices disagreeing in the open. |

Four apps, four jobs: **`hello` is the floor, `animals` is the ceiling, `sketch` is the
escape hatch, and `pantry` is the same escape hatch with a schema.** The pair of Tier 2
apps is the point: `sketch` shows the log used as a document store, `pantry` shows the log
materialized into tables you query. Nothing else is planned for Tier 2.

`hello` and `animals` contain no build step, because Tier 1 renders server-side; `animals`
carries a few lines of Alpine.js for state that is not worth persisting, which is the point
of §"HTMX and Alpine" in its README. `sketch` and `pantry` are nothing but JavaScript,
because Tier 2 renders itself, and neither has a build step or a vendored library either.
All of this is normal.

A fourth reference app, **`lantern`** (Tier 3, LÖVE linking the C ABI with no node process),
arrives with `privatium-ffi`, the C ABI, which is not built yet.

**Tiers differ by language, not by capability.** None has a ceiling. If your app is records
and forms, Tier 1 saves you a front end. If it is a game or a canvas, use Tier 2 — you lose
no storage, sync, auth, or backup by doing so. See `spec/app-contract.md`.

Each app also carries a `SKILL.md` describing its own schema and conventions, so an
assistant extending it has the local context. See `docs/skills.md`.

For layout and accessibility choices, see [Sample app design](../docs/sample-app-design.md).

## Bundled vs installed

Apps in this directory are *bundled*: in a checkout they sit beside the binary and are on
the launcher without copying, and a packaged install will carry them read-only beside the
program (a Flatpak install directory is not writable).

A release binary carries the same four folders inside it. When you run it and your
data directory's `apps/` holds no app yet, it writes them there, so the launcher has
something to show and you have something to edit; `privatium new --examples` writes them
again if you want a fresh copy (`spec/cli.md §2`, `§4`).

Apps you write go in `$XDG_DATA_HOME/privatium/apps/<slug>/`, which is writable and
survives upgrades. The framework loads both and records the origin in `sys_app.source`.

```bash
cp -r apps/hello ~/.local/share/privatium/apps/myapp
```

then set `slug = "myapp"` (and the title) in the copy's `app.toml`: the loader refuses a
folder whose name and slug differ (`spec/app-contract.md §3.1`). `privatium new myapp
--from hello` does the copy and the rewrite in one step.

## The first real application

The medication fill and prior-authorization tracker that motivated this framework will
live in its own repository as an app folder, once `pv/1` is implemented and proven. It is
deliberately not here: if the framework needs to be modified to support it, that is a
finding about the framework, and it should surface as a spec change rather than a special
case in the reference apps. The steps for setting up such a repository are in
[your app in its own repository](../docs/app-repository.md).

---

Copyright © 2026 Gabriel Mongefranco
