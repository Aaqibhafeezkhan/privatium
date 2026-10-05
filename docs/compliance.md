<!--
This file is part of Privatium
docs/compliance.md
Author(s): Gabriel Mongefranco
Created: 2026-10-04
Last Modified: 2026-10-04
Summary: The security and accessibility posture of the framework's own pages as evidence: the
         controls in place, the automated tests that hold them, the measured contrast of the
         shared bar and footer, and the manual checks with their dates and their gaps.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium

## Compliance: what is checked, and how

[← Back to README](../README.md)

This page records what the framework's own pages are held to, which test holds each
rule, and what has been checked by hand. It is evidence, not a claim. Where a check still
needs a person, it says so. It is for maintainers and for anyone auditing a node.

### Targets

- **Accessibility:** WCAG 2.2 AA for every page the node renders: the launcher, the
  settings pages, the error pages, the frame a Tier 1 view renders inside, and the bar
  and footer it inserts into a document an app owns.
- **Security:** the headers and policies of `spec/protocol.md` section 9.3 on every
  response, and the threat model of the [security page](security.md).

### Automated evidence

| Control | Where it is held |
|---|---|
| Every rendered shell page has `lang`, one `<main>`, labelled `<nav>` elements, a working skip link, one `<h1>`, headings in order, no inline script, style or event handler | `tests/reference.rs::test_spec_cli_5_pv4xx_shell_pages` and `test_spec_cli_5_pv4xx_app_frame_and_reference_views` |
| The colour tokens meet 4.5:1 for text and 3:1 for focus rings and control borders, in light and dark schemes; the bar's fixed colours meet the same floors; no rule removes a focus outline without a replacement; no rule dims text with `opacity` | `tests/reference.rs::test_spec_cli_5_pv406_declared_tokens_meet_contrast` |
| The frame's three zones render in order, the title is not a heading, the body names the mount, and the chrome assets load with their hashes | `tests/wire.rs::test_spec_4_1_frame_renders_three_zones_and_the_title_links_to_the_mount` |
| The one menu lists the app's items, then a separator, then the settings pages, with no launcher entry | `tests/wire.rs::test_spec_4_1_menu_lists_app_items_then_a_separator_then_system_pages_without_apps` |
| The footer's status line is written once per connection change and never on load | `tests/js/chrome.test.mjs::test_spec_5_2_pv_status_writes_the_slot_and_connection_changes_announce_once` |
| The node's label, the app's title and a menu label are escaped | `tests/reference.rs::test_footer_node_label_is_escaped` |
| A Tier 2 document receives the chrome at its three anchors, with the app's title and menu items, its body attributes kept, and the result clean under the `PV4xx` rules | `tests/wire.rs::test_spec_5_standard_chrome_is_inserted_at_the_three_anchors_of_a_web_document` |
| `chrome = "none"` serves the document byte for byte; `apps/sketch` is the reference | `tests/wire.rs::test_spec_5_chrome_none_serves_the_document_byte_for_byte` |
| A document without an anchor is served untouched and the load report names the file and the tag | `tests/wire.rs::test_spec_5_a_document_without_an_anchor_is_served_untouched_with_a_load_warning` |
| A `layout()` document receives the chrome, a fragment does not, and `chrome = "none"` leaves the frame alone | `tests/wire.rs::test_spec_4_1_a_layout_owned_lua_document_receives_the_chrome` |
| An inserted document keeps the app's own policy and headers, with nothing inline and nothing loaded cross-origin | `tests/wire.rs::test_spec_5_inserted_chrome_keeps_the_apps_own_policy_headers` |
| Pantry's served page wears the chrome with one `<h1>` and no way back of its own; sketch's is the file with its mark as a link | `tests/reference.rs::test_pantry_end_to_end`, `test_sketch_end_to_end` |
| Sketch's way out leads to the launcher under one and to Settings in solo mode, from both the mark and the menu | `tests/js/sketch.test.mjs::test_spec_5_the_mark_links_to_apps_in_host_mode_and_settings_in_solo_mode` |
| `PV109`, `PV110` and `PV408` each have a clean pass fixture and a fail fixture that trips them | `tests/lint.rs::test_lint_rule_pv109_*`, `test_lint_rule_pv110_*`, `test_lint_rule_pv408_*` |
| Every response carries the policy headers of section 9.3 | `tests/wire.rs::test_spec_9_3_headers_present` |
| Embedded assets carry an `ETag` and a content-addressed path | `tests/wire.rs::test_spec_9_3_chrome_assets_are_addressable_by_build_and_carry_an_etag` |

The lint rules an app is held to, with the WCAG criterion behind each, are in
`spec/cli.md` section 5.1. The same `PV4xx` rules bind the framework's pages through the
tests above (`spec/cli.md` section 5.4).

### Contrast of the shared bar and footer

Measured on 2026-10-04 with the linter's contrast formula, from `chrome.css`.

| Pair | Where | Ratio | Floor |
|---|---|---|---|
| white text on the bar (`#ffffff` on `#17212b`) | bar text, menu text | 16.29:1 | 4.5:1 |
| current-page pill (`#17212b` on `#c6dcff`) | the Apps link on the launcher | 11.70:1 | 4.5:1 |
| focus ring and borders on the bar (`#c6dcff` on `#17212b`) | focus ring, Menu button border | 11.70:1 | 3:1 |
| rule between the menu's two lists (`#8a98ab` on `#17212b`) | menu separator | 5.56:1 | 3:1 |
| footer text, light (`#46515f` on `#f4f6f8`) | footer | 7.45:1 | 4.5:1 |
| footer text, dark (`#c5ced9` on `#17212b`) | footer | 10.25:1 | 4.5:1 |
| status line, light (`#17212b` on `#f4f6f8`) | status line | 15.04:1 | 4.5:1 |
| status line, dark (`#ffffff` on `#17212b`) | status line | 16.29:1 | 4.5:1 |

The full token table for both schemes is generated into
`app-skills/privatium-accessibility/reference/rules.md`.

### Manual checks

Checks a test cannot make. Each row names the date it was last done and by what means.

| Check | Status | Date | How |
|---|---|---|---|
| Keyboard order: skip link, brand, title, Apps, Menu, each menu item, footer links | Verified from the rendered document order and the absence of positive `tabindex` | 2026-10-04 | Rendered HTML of `/a/hello/` and `/`, read in the test output |
| Every control is at least 44 by 44 CSS pixels | Verified in `chrome.css`: `min-height: 44px` on every link and button of the bar and footer, `width: 44px` on the Menu button and the QR link | 2026-10-04 | Stylesheet review |
| 320 CSS pixels wide: no horizontal scroll, the title wraps, both controls visible | Needs a person in a browser | pending | Resize to 320 px; the bar's grid gives the title `minmax(0, 1fr)` and `overflow-wrap: anywhere` |
| 200% zoom: the same | Needs a person in a browser | pending | Zoom to 200% on the launcher and on `/a/hello/` |
| A screen reader reads the status line once per connection change and not on load | The write count is held by the test above; the announcement itself needs a person with a screen reader | pending | Toggle the network on a paired phone with VoiceOver or TalkBack running |
| Visible focus ring on every control of the bar in both schemes | Verified by the contrast test for the ring's colour; needs a person to see it | pending | Tab through the bar in light and dark mode |
| Pantry in the browser: bar and footer present, the skip link lands in its main region, its status messages appear in the footer's slot, one `<h1>`, no second way back | The rendered document is held by the tests above; the skip link's landing and the slot's announcement need a person | pending | Open `/a/pantry/`, press Tab then Enter on the skip link, add a shelf and listen for "Added …" |
| Sketch in the browser: no bar, no footer, the mark is a link with a visible focus ring and a 44-pixel target, every drawing interaction as before | The served bytes and the link are held by the tests above; the ring and the target need a person | pending | Open `/a/sketch/`, Tab to the mark, then draw with the pointer and from the keyboard |
| The pantry page at 320 CSS pixels and 200% zoom reflows without horizontal scroll under the bar | Needs a person in a browser | pending | Resize to 320 px and zoom to 200% on `/a/pantry/` |

When a pending row is done, replace its status with what was seen and the date.

### Known gaps

- The manual rows marked pending above have not been done in a browser for this version.
- The chrome is rendered for the framework's pages and for Tier 1 views inside the
  frame, and inserted into a Tier 2 document and a `layout()` document unless the app
  declines it. An app that declines it, as `apps/sketch` does, draws its own way back
  and its own status region, and is held to the lint rules rather than to these tests.

### Conclusion

The automated rows run on every change and fail the build when they break. The pending
manual rows are the work left for a person before this version is called checked. The
next pages to read are the [security page](security.md) for the threat model and the
[accessibility skill](../app-skills/privatium-accessibility/SKILL.md) for what an app
must do on its side.

### Additional resources

- [Security](security.md)
- [The lint rules](../spec/cli.md), section 5
- [The page frame in the contract](../spec/lua-api.md), section 4.1
- [The accessibility skill](../app-skills/privatium-accessibility/SKILL.md)
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)

[← Back to README](../README.md)
