<!--
This file is part of Privatium
docs/plans/chrome-and-navigation.md
Author(s): Gabriel Mongefranco
Created: 2026-10-04
Last Modified: 2026-10-04
Summary: Implementation plan for the standard app chrome (top bar, menu, footer with a status
         slot), chrome for documents an app owns, and in-document navigation for framed
         Tier 1 apps. Non-normative. Where this plan and spec/ disagree, spec/ wins and this
         file is wrong.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Chrome and Navigation Plan

Target: every app shows the same top bar and footer, an app can add its own menu items and
status messages to them, an app that owns its document can turn them off, and a framed
Tier 1 app can move between its pages without a fresh document. Deliverable: the meds
app's pages change under a bar that never moves, with no flash, and the pantry app wears
the same bar as a Lua app.

## 0. How to use this

Read `AGENTS.md` in full first, then `spec/app-contract.md §3, §5, §5.4`,
`spec/lua-api.md §4.1`, `spec/protocol.md §8.3, §8.3.1, §9.1, §9.3`, `spec/cli.md §5`,
`docs/skills.md §7`, and `skills/accessibility/SKILL.md`. This plan is a work breakdown,
not a substitute for the contract. `docs/plans/phase-2.md` is the shape it follows.

Three milestones, C1 to C3, one branch and one pull request each, in order. Branch from
the current default branch with `git fetch` and
`git switch --no-track -c <branch> origin/main`. A milestone is done when its checklist
passes and its named tests are green, not when it compiles. Write the named tests first;
the milestone's shape is in them. Do not start C(n+1) before C(n) merges.

Section 2 lists the decisions this plan makes. All were taken by the owner on 2026-10-04,
and each milestone writes them into the spec in the pull request that implements them.
Section 3 is the list of spec edits, each tied to a milestone. Edit the spec in the same
change as the code, record the row here as fixed, and regenerate `app-skills/*/reference/`
with `cargo xtask gen-skill-reference` in the same change (`docs/skills.md §7`).

Do not invent config keys, routes, lint rule numbers, or asset names beyond the ones this
plan names. Where a milestone needs one more, add a row to §3 first.

The gates every pull request runs, from the workspace root with the Rust 1.90 toolchain on
`PATH`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask gen-skill-reference --check
cargo xtask header-check
cargo xtask lint-spec-refs
node --test crates/privatium-core/tests/js/
cargo build --release --locked -p privatium
```

The live client test in `crates/privatium/tests/channel.rs` needs Node 22 or newer for a
global `WebSocket`; on an older Node, skip it by name and say so in the pull request.

Commit messages and pull request text are plain English, in complete sentences, with no
co-author trailer and no tool or model name anywhere (`AGENTS.md §1, §13`).

---

## 1. Scope

### In

The three-zone top bar with the app title in the middle; one menu with app items before
the system items and without an Apps entry; the footer with a live status slot; the `[ui]`
manifest table; the chrome inserted into documents an app owns, on by default and off by
declaration; the scoped stylesheet and the small chrome script; `pv.status` and the
connection and outbox messages; the lint rules that hold the conventions; the pantry app
adopting the chrome; the sketch app declining it and linking its mark to the launcher;
in-document navigation for framed Tier 1 apps that ask for it; the animals app adopting
that navigation; the spec, docs, and app-building skills that describe all of it.

### Out — do not implement, do not stub, do not leave TODOs referencing

- Any change to the sketch app beyond the manifest line and the mark link. Its interface
  was hard to get right and is not to be touched.
- The meds app. It lives in its own repository and has its own plan there
  (`docs/plans/standard-chrome.md` in `privatium-app-meds`). This plan only has to make
  that plan possible.
- A runtime toggle that hides the chrome from a script. The manifest decides.
- Re-enabling HTMX's history cache, or storing decrypted page content anywhere. The back
  button stays a full reload.
- Upgrading HTMX. It stays at the vendored 2.0.9.
- Swap navigation for documents an app owns, or across apps. Crossing a mount always makes
  a fresh document (`spec/protocol.md §8.3.1`).
- A new client framework, a build step, or a new dependency of any kind.
- The plain-HTTPS and PWA paths beyond what the chrome script must do on them.

### The one-sentence test

A Tier 1 app with no `[ui]` table looks exactly as it did, plus a centered title; a Tier 2
app with no `[ui]` table gains the bar and footer; `chrome = "none"` gives the document
back untouched; `navigation = "swap"` changes pages without a flash; and nothing crosses
a mount without a fresh document.

---

## 2. Decisions this plan makes — all decided

### 2.1 The bar has three zones, and the title is not a heading — DECIDED

The frame's header, for a framed Tier 1 view and for the inserted chrome alike:

```html
<header class="pv-header">
  <p class="pv-brand"><a href="/">[logo] <span class="pv-visually-hidden">Privatium</span></a></p>
  <p class="pv-app-title"><a href="/a/meds/">[icon] Prescription Tracker</a></p>
  <nav aria-label="Framework">
    <a href="/">[icon] Apps</a>                      <!-- host mode only -->
    <details class="pv-menu"> ... </details>
  </nav>
</header>
```

- The brand links to `/`, which is the launcher in host mode and the app itself in solo
  mode, as today.
- The app title comes from `app.title`, with `app.icon` when the manifest declares one,
  and links to the mount root. It is a paragraph, not a heading, because `PV404` gives
  the view the page's one `<h1>`. The window title keeps the form `<page> — Privatium`.
- The three zones are a grid: brand, title, controls. At or below the 30rem breakpoint the
  wordmark gives way to a compact mark and the title wraps to a second line rather than
  clipping. No zone may hide the title or the controls.
- The shell's own pages, the launcher and the settings pages, keep the brand as their
  `<h1>` and show no center title; they take the menu and footer changes of §2.2 and §2.3.
- Every control keeps a 44 by 44 CSS pixel target and a visible focus ring.

Where: `spec/lua-api.md §4.1` ("The page around a view"), `spec/app-contract.md §5`.

### 2.2 One menu, the app's items first — DECIDED

The Menu button stays a single `<details>`. Inside, in order: the app's items, a separator,
then the system pages. The separator is in the markup on every page and the stylesheet
hides it while the app's list is empty (`#pv-app-menu:empty + .pv-menu-rule`), so an item
a script appends later brings the separator with it and no script of the chrome's is
needed. "Apps" leaves the menu because the header carries an Apps link beside it. In solo
mode there is no Apps link, and the menu holds the system pages alone. (Amended
2026-10-04: the separator is hidden by CSS rather than omitted by the server.)

System pages, in this order: Space settings, App settings, Data settings, Devices.

An app contributes items three ways:

| Source | Who renders it | For |
|---|---|---|
| `[[ui.menu]]` in `app.toml`: `label`, `path`, optional `icon` | the node, into both the frame and the inserted chrome | app-wide links, such as Setup or Print list |
| `menu(label, path[, icon])` in a Tier 1 view, next to `layout()` | the node, for that page only | page-specific links |
| `<ul id="pv-app-menu">`, an empty list the menu always carries above the separator | the app's own script appends to it | script-driven actions: download, save as, print |

Rules the lint holds: an item is a link or a button, every item has a label, `path` is
mount-relative and resolved through `url()`, and `icon` names a vendored icon (`PV503`).
Under swap navigation (§2.6) the list refreshes out of band on every page, so page-specific
items follow the page.

Where: `spec/app-contract.md §3`, `spec/lua-api.md §4.1`.

### 2.3 The footer gets a status slot with two writers — DECIDED

```html
<footer class="pv-footer">
  <a href="https://github.com/gabrielmongefranco/privatium">Privatium</a>
  <p id="pv-status" role="status"></p>
  <div class="pv-footer-node"><span>[node label]</span><a class="pv-join" href="/settings/devices">[qr]</a></div>
</footer>
```

- The framework owns connection wording and writes it only on a state change: "Offline.
  Changes are saved on this device and sent later.", "Connected again.", and "N changes
  waiting to send." when the outbox is not empty. It never writes technical detail.
- An app owns task wording through `pv.status(text)`, a new method on the `pv` helper, or
  by writing to the element by id. Last writer wins.
- The behaviour lives in a new embedded asset, `chrome.js`. It does not import `pv.js`:
  an app's own `import` of `/static/pv.js` and an import by the chrome would be two module
  instances with two handler lists. Instead `pv.js` dispatches `pv:online`, `pv:offline`
  and `pv:outbox` events on the document beside its `pv.on()` handlers, and `chrome.js`
  listens for those and for the browser's own `online` and `offline` events, so one copy
  serves every page whether or not it loaded `pv.js`. A fourth phrase, "All changes
  sent.", is written when the queue drains, so the waiting count never goes stale. On a
  channel page `client.js` also writes "Connection lost." when the socket closes without
  a navigation. (Amended 2026-10-04 while implementing C1.)
- `pv.js` grows as needed. The contract's sentence giving its size is replaced by a
  description of what it does; there is no size promise.

Where: `spec/app-contract.md §5.2`, `spec/lua-api.md §4.1`.

### 2.4 The `[ui]` manifest table — DECIDED

```toml
[ui]
chrome     = "standard"       # "standard" (default) | "none"; §2.5
navigation = "page"           # "page" (default) | "swap"; Tier 1 framed views only; §2.6
scripts    = ["static/forms.js"]    # loaded in the frame's head with defer, on every page
styles     = ["static/app.css"]     # loaded in the frame's head, on every page

[[ui.menu]]
label = "Setup"
path  = "/setup"
icon  = "gear"
```

- Every key is optional and every default is the current behaviour, except that a Tier 2
  app now gets the chrome unless it says `none`.
- `scripts` and `styles` are paths beneath the app's `static/` directory, mount-relative,
  resolved through `url()`. They apply to framed Tier 1 views. An app that owns its
  document loads its own assets; the lint warns when such an app sets them.
- `navigation = "swap"` is refused at load for a Tier 2 app, with the same wording the
  lint uses.
- Every framed body carries `data-pv-mount` from C1 on, not only under swap navigation:
  the chrome script and the scope rule of §2.6 both read the mount from it, and it costs
  nothing on a page that navigates by full loads. (Amended 2026-10-04.)
- The manifest parser denies unknown keys, so a node older than this change refuses a
  manifest that carries `[ui]`. An app that adopts the table therefore depends on a
  release that includes it. This is stated in `spec/app-contract.md §3`.

Where: `spec/app-contract.md §3`; `crates/privatium-core/src/app/manifest.rs`.

### 2.5 Chrome is inserted into the documents an app owns, unless the app declines — DECIDED

Applies to every `text/html` document served from a Tier 2 app's `web/` directory and to a
Tier 1 document that a view owns with `layout()`. The node inserts, at three anchors:

| Anchor | Inserted |
|---|---|
| before `</head>` | `<link rel="stylesheet">` for `chrome.css` and `<script defer>` for `chrome.js`, both at the content-addressed path with `integrity`, plus the `[[ui.menu]]` items as data the script does not need to fetch |
| after the first `<body …>` | the skip link and the header of §2.1 |
| before the last `</body>` | the footer of §2.3 |

- The document is buffered in full for the insertion; it is the entry document, not an
  asset. Everything else under `web/` streams as before.
- Nothing else in the document changes. The policy stays the app's own: the chrome's
  assets are same-origin, so `default-src 'self'` admits them, and no permission widens.
- When the document lacks an anchor, the node serves it untouched and records a load
  warning naming the file, so an owner sees why the bar is missing.
- `chrome = "none"` serves the document byte for byte as today.
- An app that takes the chrome gives its main region `id="main"`, the skip link's target,
  and does not draw its own exit link.
- The chrome's stylesheet is scoped. `shell.css` keeps the body, main, heading, form and
  table rules for the shell's own pages; a new `chrome.css` carries only the header,
  footer, menu, skip-link and status rules and the colour tokens they use, and the shell's
  pages load both.

The wording in the contract and the Tier 2 skill, which today says nothing is injected,
becomes: the framework never forces anything into an app's page; the top bar and footer
are on by default and can be turned off in the app's config.

Where: `spec/app-contract.md §5`, `spec/lua-api.md §4.1`; `crates/privatium-core/src/http/apps.rs`.

### 2.6 Swap navigation for framed Tier 1 apps, scoped to the mount — DECIDED

Opt-in with `navigation = "swap"`. For such an app the frame renders:

```html
<body data-pv-mount="/a/meds/" hx-headers='{"X-CSRF-Token":"…"}'>
…
<main id="main" hx-boost="true" hx-target="#main" hx-select="#main"
      hx-swap="outerHTML show:window:top">
```

- A boosted request fetches the ordinary full page; HTMX keeps only `#main` and takes the
  `<title>` from the response. Server rendering does not change.
- **Scope.** A boosted request is allowed to swap when its path is beneath the mount and
  does not begin with a framework prefix (`spec/protocol.md §9.1`): `/settings`, `/api`,
  `/skills`, `/static`, `/ws`, and in host mode anything outside `/a/<slug>/`. Anything
  else is a full navigation, as every boosted request is today. The rule lives in one
  exported function in `chrome.js`, which enforces it on plain-HTTPS pages, and which
  `client.js` imports and applies before bridging a request over the channel.
- A boosted non-GET request in scope goes through the HTMX bridge like a fragment request.
  When the bridge follows a redirect it sends `HX-Boosted` along with `HX-Request`, so the
  destination renders as a full page for `hx-select`. A non-redirect response, such as a
  form re-rendered with errors, swaps the same way. The response handoff of
  `spec/protocol.md §8.3.1` stays for requests that leave the scope.
- After each swap the frame moves focus to the first `<h1>` inside `#main`, or to `#main`
  itself, and the swap scrolls to the top. HTMX's history cache stays disabled, so the back
  button is a full reload.
- `#pv-app-menu` and the per-page items of §2.2 carry `hx-swap-oob="true"` in every full
  page, so the menu follows the page.
- **Scripts.** HTMX strips `<script>` elements from swapped content, deliberately. Under
  swap navigation an app's scripts come from `ui.scripts`, loaded once in the head, and a
  view carries no `<script>` or `<link rel="stylesheet">` of its own (`PV111`). Scripts
  bind by event delegation on `document` or re-run on `htmx:load`, guard against binding
  twice, and never assume a fresh page. The tier 1 skill shows the pattern.

Where: `spec/protocol.md §8.3.1`, `spec/lua-api.md §4.1`, `spec/app-contract.md §3`.

### 2.7 Lint rules — DECIDED

| Rule | Severity | Holds |
|---|---|---|
| `PV109` | error | A document that takes the standard chrome has `</head>`, `<body>`, `</body>` and a main region with `id="main"` |
| `PV110` | error | Every `[ui]` reference resolves: `scripts` and `styles` exist under `static/`, `menu` items have a label and a mount-relative path, `navigation = "swap"` is not declared by a Tier 2 app; `scripts` and `styles` on an app that owns its document are a warning |
| `PV111` | error | A view of an app with `navigation = "swap"` carries no `<script>` or `<link rel="stylesheet">`; they belong in `ui.scripts` and `ui.styles` |
| `PV408` | warn | An app under the standard chrome does not also draw a link to the launcher or settings as its own way back |

Each rule gets a `pass/` and a `fail/` fixture under `apps/_lint/`, a row in
`spec/cli.md §5`, and a named test in `crates/privatium-core/tests/lint.rs`.

### 2.8 Sketch declines the chrome and links its mark — DECIDED

`apps/sketch/app.toml` gains `[ui] chrome = "none"`. In `web/index.html` the Privatium mark
inside the `<h1>` becomes a link, and `app.js` gives it the destination and accessible
name its exit link already computes: Apps in host mode, Settings in solo mode. The exit
entry in its own menu stays. Nothing else in the app changes.

### 2.9 Pantry takes the chrome — DECIDED

Pantry removes its exit link and the script lines that fill it, writes its status through
`pv.status`, keeps its own `<h1>` and band, and gives its main region `id="main"`. Its
README and SKILL explain that the bar and footer are the framework's and how to opt out.

---

## 3. Spec edits — made in the milestone that meets them

| # | Where | Edit | Milestone | Status |
|---|---|---|---|---|
| 1 | `spec/app-contract.md §3` | Document `[ui]`: `menu`, `scripts`, `styles`, defaults, and the note that an older node refuses a manifest carrying the table | C1 | fixed 2026-10-04 |
| 2 | `spec/lua-api.md §4.1` | Rewrite "The page around a view" for the three-zone bar, the menu order, and the footer status slot; add `menu(label, path[, icon])` to the helper table | C1 | fixed 2026-10-04 |
| 3 | `spec/app-contract.md §5.2` | Add `pv.status(text)` and the `outbox` event; replace the size sentence with a description | C1 | fixed 2026-10-04 |
| 4 | `spec/app-contract.md §3` | Add `chrome = "standard" \| "none"` | C2 | fixed 2026-10-04 |
| 5 | `spec/app-contract.md §5` | Replace "no framework injected" with the owner's wording; state the three anchors, `id="main"`, and the no-second-way-back rule | C2 | fixed 2026-10-04 |
| 6 | `spec/lua-api.md §4.1` | A `layout()` document receives the chrome unless the manifest declines it | C2 | fixed 2026-10-04 |
| 7 | `spec/cli.md §5` | Rows for `PV109`, `PV110`, `PV408` | C2 | fixed 2026-10-04 |
| 8 | `spec/app-contract.md §3` | Add `navigation = "page" \| "swap"`, Tier 1 framed views only | C3 | open |
| 9 | `spec/protocol.md §8.3.1` | One paragraph: an in-document swap that stays beneath the mount and off the framework prefixes is a fragment request; crossing either needs the fresh document; the scope rule and the boosted redirect follow | C3 | open |
| 10 | `spec/lua-api.md §4.1` | Under `navigation = "swap"` views carry no script or stylesheet elements; `ui.scripts` and `ui.styles` load in the head; focus moves to the new heading | C3 | open |
| 11 | `spec/cli.md §5` | Row for `PV111` | C3 | open |
| 12 | `spec/protocol.md §9.3` | Verify the new assets need no text change; they are embedded assets at fixed and addressed paths like the rest | C1 | verified 2026-10-04, no change needed |
| 13 | `spec/data-api.md §5` | The normative copy of the size sentence; replace it, and add `pv.status`, `pv.waiting`, the `outbox` event and the document events | C1 | fixed 2026-10-04 |

---

## 4. Workspace layout — what this plan touches

```
crates/privatium-core/
  assets/shell/
    chrome.css        NEW   header, footer, menu, skip link, status; tokens only
    chrome.js         NEW   scope rule, menu slot, status writers, focus after swap
    shell.css         trimmed of what moves to chrome.css; shell pages load both
    client.js         imports the scope rule; bridges in-scope boosted requests
    pv.js             pv.status, the outbox event
  src/app/manifest.rs       Ui struct, Menu item struct, validation
  src/app/mod.rs            load warnings for a missing anchor and a refused navigation key
  src/http/shell.rs         page(), app_frame(): three zones, menu, footer slot, swap attributes
  src/http/apps.rs          insertion into owned documents; layout() documents
  src/lua/lsp.rs            menu() helper, registry data beside layout
  src/lua/mod.rs            LuaResponse carries the page's menu items
  src/lint/mod.rs           PV109, PV110, PV111, PV408
  tests/apps.rs tests/lint.rs tests/lua.rs tests/wire.rs tests/reference.rs
  tests/js/chrome.test.mjs  NEW
  tests/js/navigation.test.mjs tests/js/client.test.mjs tests/js/pv.test.mjs tests/js/sketch.test.mjs
apps/_lint/pass/PV109 PV110 PV111 PV408 ; apps/_lint/fail/PV109 PV110 PV111 PV408
apps/pantry/   web/index.html web/app.js web/style.css README.md SKILL.md
apps/sketch/   app.toml web/index.html web/app.js
apps/animals/  app.toml views/_assets.lsp (removed) static/animals.js README.md SKILL.md
apps/README.md
app-skills/privatium-tier1-lua/SKILL.md  privatium-tier2-web/SKILL.md  privatium-overview/SKILL.md
app-skills/privatium-accessibility/SKILL.md  privatium-security/SKILL.md  */reference/ (generated)
docs/architecture.md docs/usage.md docs/compliance.md docs/security.md docs/roadmap.md
spec/app-contract.md spec/lua-api.md spec/protocol.md spec/cli.md
```

The asset table in `src/http/assets.rs` walks the directory, so the two new files get an
ETag, an integrity hash and a content-addressed path without a code change there; the
existing asset test is extended to name them.

---

## 5. Dependencies

None added. HTMX stays at 2.0.9, which already handles `hx-select` with `hx-boost`,
out-of-band swaps before selection, and a `<title>` in a full response.

---

## 6. Milestones

### C1 — The bar, the menu, the footer slot, and the `[ui]` table

**Goal.** Every framed Tier 1 view and every shell page shows the three-zone bar, the
single menu with app items first and no Apps entry, and the footer with a working status
slot. The `[ui]` table exists with `menu`, `scripts` and `styles`.

**Work.**

1. `manifest.rs`: `Ui { menu: Vec<MenuItem>, scripts: Vec<String>, styles: Vec<String> }`
   with `deny_unknown_fields`, defaults, and validation of labels, paths and icons.
2. `shell.rs`: rewrite `page()` and `app_frame()` for §2.1 to §2.3. `app_frame` takes the
   app's title, icon, mount, menu items and asset lists. Shell pages pass no title and no
   app items. Remove the Apps entry from the menu.
3. `lsp.rs` and `lua/mod.rs`: the `menu()` helper, stored beside `layout` in the template
   registry data, returned with the view and rendered by the frame.
4. `chrome.css` and the `shell.css` split; the shell pages load both. Check contrast of
   every new token pair at 4.5:1 for text and 3:1 for controls and record the values in
   `docs/compliance.md`.
5. `chrome.js`: the status writers of §2.3 and the menu slot. `pv.js`: `pv.status(text)`
   and the `outbox` event. `client.js`: "Connection lost." on an unexpected socket close.
6. Spec rows 1, 2, 3, 12. Docs: `docs/architecture.md` describes the chrome and who
   renders it; `docs/usage.md` describes the bar, menu and status line for an owner.
   Skills: tier 1 (frame description, `[ui]`, `menu()`, where scripts go), overview (the
   common bar), accessibility (status region etiquette, title is not a heading, one `<h1>`).
   Regenerate the reference.

**Named tests.**

- `tests/wire.rs::test_spec_4_1_frame_renders_three_zones_and_the_title_links_to_the_mount`
- `tests/wire.rs::test_spec_4_1_menu_lists_app_items_then_a_separator_then_system_pages_without_apps`
- `tests/wire.rs::test_spec_4_1_solo_mode_frame_has_no_apps_link_and_no_separator_without_app_items`
- `tests/lua.rs::test_spec_4_1_menu_helper_adds_a_page_item_and_a_partial_may_not_call_it`
- `tests/wire.rs::test_spec_3_ui_scripts_and_styles_load_in_the_frame_head_with_defer_and_integrity`
- `tests/apps.rs::test_spec_3_a_manifest_with_an_unresolvable_ui_reference_is_refused_at_load`
- `tests/wire.rs::test_spec_9_3_chrome_assets_are_addressable_by_build_and_carry_an_etag`
- `tests/js/chrome.test.mjs::test_spec_5_2_pv_status_writes_the_slot_and_connection_changes_announce_once`
- `tests/js/pv.test.mjs::test_spec_5_2_outbox_event_reports_the_queue_length_on_change`

The tests that render a page live in `tests/wire.rs`, beside the request helpers, rather
than in `tests/apps.rs` as first written; the load refusal stays in `tests/apps.rs`.
(Amended 2026-10-04.)

**Checklist.**

- [x] Named tests green; all gates green.
- [x] Keyboard only: skip link, brand, title, Apps, Menu, every menu item, footer links, in
      that order (verified from document order; the visible ring in a browser is a pending
      row in `docs/compliance.md`).
- [ ] 320 CSS pixels wide: no horizontal scroll; the title wraps; both controls visible.
      Needs a person in a browser; recorded as pending in `docs/compliance.md`.
- [ ] 200% zoom: the same. Pending likewise.
- [ ] A screen reader reads the status slot once per connection change and not on load.
      The write count is held by the named test; the announcement is pending a person.
- [x] `apps/hello` and `apps/animals` render unchanged apart from the centered title.
- [x] `docs/compliance.md` records the contrast values and the manual checks above with
      the date.

### C2 — Chrome for the documents an app owns; pantry in, sketch out

**Goal.** A Tier 2 document and a `layout()` document receive the bar and footer at the
three anchors unless the manifest says `none`. Pantry wears the chrome; sketch declines it
and its mark links out.

**Work.**

1. `manifest.rs`: `chrome: Chrome` with `Standard` as default and `None`.
2. `apps.rs`: for a `text/html` response from `web/` and for a complete `LuaResponse::View`,
   buffer the document, find the anchors, insert the three pieces, serve the result under
   the app's headers. A missing anchor serves the document untouched and records a load
   warning that names the file.
3. The inserted header is the §2.1 header with the app's title and menu items, in host or
   solo form. `chrome.js` fills the menu slot and the status slot exactly as on a frame.
4. Lint: `PV109`, `PV110`, `PV408` with fixtures.
5. Pantry per §2.9. Sketch per §2.8, and only that.
6. Spec rows 4 to 7. Docs: `docs/architecture.md` (insertion and anchors),
   `docs/security.md` (the chrome's assets are same-origin and the policy is unchanged),
   `apps/README.md`, `apps/pantry/README.md` and `SKILL.md`, `apps/sketch/README.md` (why
   it declines). Skills: tier 2 (the default, the anchors, `id="main"`, the menu slot,
   `pv.status`, how to opt out; the old "write the whole document" sentence reworded),
   security (nothing inline, nothing cross-origin). Regenerate the reference.

**Named tests.**

- `tests/wire.rs::test_spec_5_standard_chrome_is_inserted_at_the_three_anchors_of_a_web_document`
- `tests/wire.rs::test_spec_5_chrome_none_serves_the_document_byte_for_byte`
- `tests/wire.rs::test_spec_5_a_document_without_an_anchor_is_served_untouched_with_a_load_warning`
- `tests/wire.rs::test_spec_4_1_a_layout_owned_lua_document_receives_the_chrome`
- `tests/wire.rs::test_spec_5_inserted_chrome_keeps_the_apps_own_policy_headers`
- `tests/wire.rs::test_spec_5_chrome_anchors_match_tags_case_insensitively_and_name_the_missing_one`
- `tests/lint.rs::test_lint_rule_pv109_passes` and `_fails`, `test_lint_rule_pv110_passes`
  and `_fails`, `test_lint_rule_pv408_passes` and `_fails`, through the corpus table every
  rule is held by.
- `tests/reference.rs`: pantry and sketch lint clean; pantry's served document carries the
  chrome; sketch's does not.
- `tests/js/sketch.test.mjs::test_spec_5_the_mark_links_to_apps_in_host_mode_and_settings_in_solo_mode`

The serving tests live in `tests/wire.rs` beside the request helpers, as C1's did, and
the lint tests take the names the corpus macro gives every rule. (Amended 2026-10-04.)

Three decisions were made while implementing, and the spec says each:

- `PV110` has one severity, error, like every rule. `scripts` or `styles` on a Tier 2
  app is therefore an error, not the warning §2.4 and §2.7 first said: nothing would ever
  load the file, so the entry is a mistake to fix, not advice.
- The missing-anchor load warning is for Tier 2 documents, checked over every `.html`
  under `web/` at load. A `layout()` document is rendered at request time, so the lint
  (`PV109`) is what names its missing anchor; the serve path serves it untouched.
- Sketch's two exits share one module, `web/exit.js`, so the mark and the menu entry
  cannot disagree and a test can hold the rule without a document. That is the one
  change beyond §2.8's list. (Amended 2026-10-04.)

**Checklist.**

- [x] Named tests green; all gates green.
- [ ] Pantry in the browser: bar and footer present, skip link lands in its main region,
      its status messages appear in the footer slot, one `<h1>`, no second way back.
      The rendered document is held by the tests; the landing and the announcement need
      a person, recorded as pending in `docs/compliance.md`.
- [ ] Sketch in the browser: no bar, no footer, the mark is a link with a visible focus
      ring and a 44-pixel target, and every drawing interaction behaves as before.
      Pending likewise.
- [ ] The pantry page at 320 pixels and 200% zoom reflows without horizontal scroll.
      Pending likewise.
- [x] `docs/compliance.md` updated with the manual checks and the date.

### C3 — Swap navigation for framed Tier 1 apps

**Goal.** A Tier 1 app with `navigation = "swap"` changes pages inside one document, with
focus on the new heading, the menu following the page, and nothing crossing the mount.
Animals adopts it.

**Work.**

1. `manifest.rs`: `navigation: Navigation` with `Page` as default and `Swap`; refused at
   load for a Tier 2 app.
2. `shell.rs`: under `Swap`, the `data-pv-mount` body attribute, the boosted `<main>`, and
   `hx-swap-oob` on the menu slot and page items.
3. `chrome.js`: the exported scope rule of §2.6; a `htmx:beforeRequest` listener that turns
   an out-of-scope boosted request into a full navigation; `htmx:afterSettle` focus and
   scroll handling. `client.js`: import the rule; bridge in-scope boosted requests over the
   channel, GET and non-GET; send `HX-Boosted` when following a redirect for a boosted
   request; keep the handoff path for out-of-scope requests.
4. Lint: `PV111` with fixtures.
5. Animals: `navigation = "swap"`, its stylesheet and scripts declared in `[ui]`, the
   `_assets.lsp` partial removed, `animals.js` checked for binding twice. The Alpine CSP
   build initialises swapped content by observing the document, so it needs no change;
   verify it.
6. Spec rows 8 to 11. Docs: `docs/architecture.md` (the navigation model and the scope
   rule), `docs/security.md` (why a mount boundary is a document boundary),
   `apps/animals/README.md` and `SKILL.md`. Skills: tier 1 (when to use fragments and
   `req.is_htmx`, when to turn on swap navigation, the script pattern with a worked
   example, what the frame does so the app need not, what the lint refuses), accessibility
   (focus after a swap, the title from the response, the one `<h1>` in every state),
   security (scope rule, stripped scripts). Regenerate the reference.

**Named tests.**

- `tests/js/navigation.test.mjs::test_spec_8_3_1_a_boosted_request_beneath_the_mount_stays_in_the_document`
- `tests/js/navigation.test.mjs::test_spec_8_3_1_a_boosted_request_leaving_the_mount_or_a_framework_prefix_navigates`
- `tests/js/navigation.test.mjs::test_spec_8_3_1_solo_mode_scope_excludes_the_framework_prefixes_only`
- `tests/js/navigation.test.mjs::test_spec_8_3_1_a_boosted_post_follows_its_redirect_with_the_boosted_header`
- `tests/js/client.test.mjs::test_spec_8_3_1_after_a_swap_focus_lands_on_the_new_heading_and_the_menu_follows`
- `tests/apps.rs::test_spec_4_1_swap_navigation_marks_main_boosted_and_the_body_with_its_mount`
- `tests/apps.rs::test_spec_3_navigation_swap_is_refused_for_a_web_app_at_load`
- `tests/lint.rs::test_spec_5_pv111_reports_a_script_or_stylesheet_in_a_view_of_a_swap_app`
- `tests/reference.rs`: animals lint clean under swap; a boosted GET of `/knowledge` is a
  full page whose `#main` holds the view.

**Checklist.**

- [ ] Named tests green; all gates green.
- [ ] Animals on a phone over the LAN: play, knowledge, teach and back again, with no flash
      and the bar fixed; the back button reloads to the right page.
- [ ] A screen reader announces the new page heading after each change.
- [ ] A link to `/settings` from inside an app page is a full navigation.
- [ ] Teaching an animal, a non-GET form, lands on the redirect target inside the document
      with the right URL in the address bar.
- [ ] `docs/compliance.md` updated with the manual checks and the date.

---

## 7. Risks

- **A swap under the wrong policy.** The scope rule is the control, enforced in one function
  on both the channel and HTTPS paths and covered by three named tests. Nothing in
  the node can see what a client does with a response, so the client check must be exact.
- **Scripts that assume a fresh page.** Mitigated by `ui.scripts` in the head, `PV111`, the
  skill's worked pattern, and animals as the reference. The meds plan audits each of its
  scripts by name.
- **Duplicate submission under swap.** The handoff path protected a full-page form response;
  the swap path does not reserve capacity. The frame disables a form's submit control while
  its request is in flight, through HTMX's `hx-disabled-elt`, and the test for the boosted
  post covers it.
- **Insertion into a document that is not what it seems.** Only `text/html` responses from
  `web/` and complete Lua views are candidates; anchors are matched once; a miss serves the
  file untouched and warns. No parsing of arbitrary markup is attempted.
- **A restyled app.** The stylesheet split keeps the shell's global rules out of app
  documents. Pantry is the check: its own stylesheet must still win inside its main.
- **A chattering live region.** The framework writes on state change only, and the test for
  the status slot asserts one write per change.
- **An older node refusing a manifest with `[ui]`.** Stated in the contract. Apps outside
  this repository pin the release they lint against and bump it when they adopt the table.

---

## 8. Pull request sequence

| PR | Branch | Carries |
|---|---|---|
| C1 | `chrome-bar-menu-status` | §6 C1, spec rows 1, 2, 3, 12 |
| C2 | `chrome-owned-documents` | §6 C2, spec rows 4 to 7, pantry, sketch |
| C3 | `swap-navigation` | §6 C3, spec rows 8 to 11, animals |

After C3 merges and a release carries it, the meds app adopts the bar and swap navigation
from its own plan. That work is not part of this repository.

---

Copyright © 2026 Gabriel Mongefranco
