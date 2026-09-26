---
name: privatium-accessibility
description: Accessibility requirements for every Privatium app, targeting WCAG 2.2 AA. Covers forms, icons, status messages, keyboard operation, and the two pairing paths that must both work. Load alongside whichever tier skill you are using — this applies to Lua, web, and Rust equally.
---

# Privatium Accessibility

Target: **WCAG 2.2 AA**. This applies to every tier. Load it alongside the tier skill, not
instead of it.

The framework serves people managing medications, chronic conditions, and mental health.
Assume some of them are tired, in pain, using a screen reader, or dyslexic — because some
of them are.

## MUST

**Forms**
- Every input has a `<label for>`. `placeholder` is not a label and disappears on focus.
- Group radios and checkboxes in `<fieldset>` with a `<legend>`
- Errors are text next to the field, announced with `role="alert"`, and never colour-only
- `autocomplete` on name, email, address, and telephone fields
- Do not disable zoom or set `maxlength` so tight it truncates real input

**Icons**
- Decorative icons beside text: `aria-hidden="true"` (the framework's default)
- Icon-only controls: `icon('trash', 'Delete this fill')` in Lua, or `aria-label` in HTML.
  **An icon-only button with no label is a bug and the linter fails it.**

**Colour and contrast**
- 4.5:1 for body text, 3:1 for large text and UI boundaries — the focus ring and a
  control's border included, in **both** colour schemes. The framework's tokens clear
  that (`PV406`); if you declare your own, check light and dark separately. Maize on
  white is 1.5:1: never a focus ring.
- Never colour alone — pair it with text, an icon, or a pattern
- Do not dim text with `opacity` to mean "unavailable"; say so in words. Dimming takes a
  token that passed below the floor.

**Keyboard**
- Everything reachable and operable by keyboard, in a sensible order
- Visible focus indicator; never `outline: none` without a replacement
- No keyboard trap. A modal returns focus where it came from.

**JavaScript off**
- On loopback, Tier 1 forms retain their no-JavaScript path. A plain-HTTP LAN browser
  needs JavaScript for the encrypted channel; its bootstrap explains that limit and
  directs the owner to the browser on the node (`spec/protocol.md §8.4`).
- Every Tier 1 write works without JavaScript on loopback. `hx-post` sits beside `method`/`action`; a
  handler answers a fragment to htmx and a redirect to a plain post.
- What Alpine hides must still be reachable: link a stylesheet from `<noscript>` that
  reverts `x-cloak` and hides the buttons whose only job is toggling Alpine state
  (`apps/animals/static/nojs.css`). An external sheet — an inline `<style>` in
  `<noscript>` is blocked by the default CSP.

**Structure**
- One `<h1>` per rendered page — the view with its partials inside the page frame, or
  the document your `layout()` owns. A partial htmx swaps in is judged by the element it
  replaces: `_board.lsp` carries the `<h1>` because `play.lsp` has none. Headings in
  order, no level skipped (`PV404`).
- Landmarks: `<main>`, `<nav aria-label="…">`, `<footer>` — a Tier 2 page writes its own
- Real `<table>` with `<th scope>` for tabular data — never a grid of divs

**Motion**
- Honour `prefers-reduced-motion` — the framework's stylesheet already guards every
  animation and transition; a Tier 2 page carries its own guard
- Nothing flashes more than three times per second

**The framework's own pages**
- The launcher, settings, error pages and the Tier 1 page frame are held to `PV401`–
  `PV407` by the framework's tests over their rendered HTML (`spec/cli.md §5.4`). Your
  view inherits a frame that already passes: `lang`, one `<main>`, a labelled `<nav>`, a
  skip link. Supply the `<h1>` and the content.

**Language and clarity**
- Set `lang` on the document
- Plain language. Short sentences. Say "due in 3 days," not "T-minus 72h."
- Never rely on the user remembering a screen they have left

## Pairing — both paths are required

The pairing flow MUST be completable **without reading text** (the 16-glyph emoji pad, with
labels) **and without seeing images** (the two-word code, read by a screen reader). Both.
Not either. See `spec/protocol.md §7`.

Additional requirements there:
- Every emoji shows its text label beneath it
- Word input is case-insensitive and ignores spaces, hyphens, and punctuation
- The code can be regenerated freely; no aggressive countdown pressure
- Generous letter spacing and large type on the code display

The framework's own pairing screen is the worked example: the node's code page shows
the four emoji with their labels, the two words, and the QR code as a labelled image with
the URL as text beside it; the phone's screen is a pad of sixteen labelled buttons beside
a word field, both visible at once, and every outcome — paired, wrong code, closed — is
said in a `role="status"` region. The emoji inside each key is hidden from the screen
reader so its label is read once. Match it when an app of yours shows or takes a code.

## Anti-patterns

```html
<!-- WRONG: no accessible name -->
<button hx-delete="/fill/1"><?= icon('trash') ?></button>
<!-- RIGHT -->
<button hx-delete="/fill/1"><?= icon('trash', 'Delete this fill') ?></button>

<!-- WRONG: placeholder as label, colour-only error -->
<input name="drug" placeholder="Drug name" style="border-color:red">
<!-- RIGHT -->
<label for="drug">Drug name</label>
<input id="drug" name="drug" aria-describedby="drug-err">
<p id="drug-err" role="alert">Enter the drug name.</p>

<!-- WRONG: status by colour -->
<span class="dot red"></span>
<!-- RIGHT -->
<span class="dot red"><?= icon('exclamation-triangle') ?> Overdue</span>

<!-- WRONG: fake table -->
<div class="row"><div class="cell">Drug</div></div>
<!-- RIGHT -->
<table><thead><tr><th scope="col">Drug</th></tr></thead>…</table>
```

## Verify

```bash
privatium lint apps/<slug>          # catches labels, icon names, heading order, contrast
```

Then, by hand — the linter cannot do these:
1. Unplug the mouse. Complete the main task.
2. Turn on the screen reader. Complete the main task.
3. Zoom to 200%. Nothing overlaps or is cut off.
4. Disable images. The pairing flow still works.
5. Disable JavaScript. Every write still lands.

Do not present an app as finished before doing all five.
