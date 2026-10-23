<!--
This file is part of Privatium
docs/icons.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-09-03
Summary: Icon system. Bootstrap Icons, bundled and inlined server-side.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Icons

**Bootstrap Icons only.** No other icon set, no emoji-as-icon, no custom SVGs in app
folders.

## Reasons for Bootstrap Icons

- <cite>MIT licensed, standalone SVGs that work without any Bootstrap CSS</cite>
- <cite>Over 2,000 icons on a 16px grid</cite>
- <cite>Available as raw SVGs, an SVG sprite, or a web font</cite>
- No JavaScript, no dependency, no CDN required

Pin one version in the repository. The pinned release is **v1.13.1**; re-verify against
npm before moving it.

## Delivery

**Bundled raw SVGs, inlined server-side at render time.** Not the web font, not a runtime
`<use>` reference to an external sprite.

```
crates/privatium-core/assets/icons/   vendored from twbs/icons, `icons/` directory only
├── LICENSE
├── VERSION             the exact tag vendored
├── VENDOR.md           provenance: tag, commit, date, what was taken
├── check-circle.svg
├── diagram-3.svg
└── ... (~2,000 files, ~1KB each)
```

The framework embeds this directory in the binary (`include_dir!`) and inlines the
requested icon into the HTML at render.

### Rejected option: the web font

- `@font-face` requires adding `font-src` to the CSP for no benefit.
- Icon fonts are read aloud as garbage by screen readers unless carefully suppressed.
- A blocked or failed font swaps every icon for a tofu box.
- Fonts do not inherit `currentColor` as cleanly across theme switches.

### Rejected option: a runtime external sprite

Cross-document `<use href="/static/bi.svg#name">` has a long history of browser quirks,
adds a second request on cold cache, and buys nothing when the SVGs are already in the
binary. Inlining is one embedded file read and a string write.

### Rejected option: a build-time subset

Apps are installed at runtime and declare their own icon in `app.toml`. A subset computed
at build time cannot know about them. Two thousand 1KB files compress to a few hundred
kilobytes in the binary — cheaper than the complexity of a runtime sprite builder.

## Template helper

```lsp
<?= icon('diagram-3') ?>
<?= icon('trash', 'Delete this fill') ?>
```

The label is the second positional argument, a string. There is no options table and no
size option: an icon is `1em` and takes its size from the surrounding `font-size`, which
is the one lever that keeps it aligned with its text. The same helper is a global in
handler code (`spec/lua-api.md §4.0`). It returns an HTML value, so `<?= icon('gear') ?>`
emits the SVG as it is while `<?= ?>` escapes everything else (`spec/lua-api.md §4`); in
handler code it concatenates like a string.

Emits:

```html
<svg class="pv-icon" width="1em" height="1em" fill="currentColor"
     viewBox="0 0 16 16" aria-hidden="true" focusable="false">…</svg>
```

Rules the helper enforces:

- `width`/`height` default to `1em` so icons scale with `font-size`, as Bootstrap
  recommends.
- `fill="currentColor"` always. Never a hard-coded color; themes and dark mode depend on
  inheritance.
- `aria-hidden="true"` and `focusable="false"` **by default**. An icon next to a text label
  is decorative and must not be announced twice.
- Passing a label flips it to `role="img"` with an `<title>` child. **Required** for any
  icon that is the only content of a control. An icon-only button with no label is an
  accessibility bug and should fail review.
- An unknown icon name renders `question-circle` and logs a warning. It never renders
  nothing and never breaks the page.

## Naming in `app.toml`

```toml
[app]
icon = "diagram-3"    # the Bootstrap Icons filename, without .svg
```

Validation: the name must match `^[a-z0-9-]+$` and must exist in the vendored set. A
missing icon is a warning in the load report, shown on the settings page beside the app,
not a load failure — an app should not refuse to start over a picture. It is not written
to `sys_app.last_error`, which `spec/data-dictionary.md §3.4` reserves for the text of a
load or validation failure; a loaded app with an error set would read as one that had not.

## Framework icon vocabulary

Use these consistently so every app looks like it belongs on the same node.

| Meaning | Icon |
|---|---|
| Add / new | `plus-lg` |
| Edit | `pencil` |
| Delete | `trash` |
| Save / confirm | `check-lg` |
| Cancel | `x-lg` |
| Settings | `gear` |
| Devices | `phone` |
| Pairing | `qr-code` |
| Sync | `arrow-repeat` |
| Backup / snapshot | `archive` |
| Warning | `exclamation-triangle` |
| Alert (key mismatch, tier-3 restore) | `shield-exclamation` |
| Info | `info-circle` |
| Search | `search` |
| Apps launcher | `grid-3x3-gap` |
| Unknown app icon fallback | `question-circle` |

Verify each of these against the vendored version at build time; the framework should fail
its own test suite if one is missing rather than silently falling back. `cargo xtask
icons-verify` is that check: every name this table, the shell, the reference apps and the
skills refer to must exist in `assets/icons/`, and `VERSION` there must be the release
pinned above.

## Attribution

Bootstrap Icons is MIT licensed. `assets/icons/LICENSE` must be vendored alongside the
SVGs and `NOTICE` must list it.

---

Copyright © 2026 Gabriel Mongefranco
