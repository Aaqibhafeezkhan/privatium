<!--
This file is part of Privatium
docs/sketch-app-design.md
Author(s): Gabriel Mongefranco
Created: 2026-09-07
Last Modified: 2026-09-07
Summary: The design of the Sketch reference app: its coordinate model, its layout, what each tool
         writes to the log, the keyboard map, and the accessibility and colour rules the
         implementation is held to.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Sketch app design

Sketch is the Tier 2 reference app: a shared drawing surface with no `app.lua`, no
`views/`, no `schema.sql` and no SQL at all. It uses the event log directly as a document
store. This page explains how it is put together and why each decision is the way it is.

Read [Sample app design](sample-app-design.md) first for how the reference apps share a
look. Read [App contract](../spec/app-contract.md) for what a Tier 2 app is, and
[Data API](../spec/data-api.md) for the endpoints it calls.

Sketch stays inside Tier 2's own limits: no HTMX, no CDN, no vendored library, no build
step, no inline `<script>` and no `style` attribute. Everything below is expressed in
`web/index.html`, `web/style.css` and seven ES modules.

## The sheet is a fixed coordinate space

The sheet is **1600 × 1200 logical pixels**, always, on every device.

That is a separate thing from the canvas's pixel size. A canvas sized to each window
stores coordinates in that window's pixels, so a stroke drawn on a 1440-pixel-wide
desktop lands in the corner of a 390-pixel-wide phone. Sketch avoids that by keeping the
coordinate space fixed and putting the display scale in the context transform instead.

`web/sheet.js` owns the whole mapping. Nothing else in the app needs to know the device
pixel ratio or the zoom level.

| Quantity | How it is derived |
|---|---|
| CSS size | `round(1600 × scale)` by `round(1200 × scale)` pixels |
| Backing store | that CSS box times `devicePixelRatio`, never below 1600 × 1200 |
| Context transform | `ctx.setTransform(k, 0, 0, k, 0, 0)` where `k = backingWidth / 1600` |
| Pointer to sheet | `(clientX − rect.left) × 1600 / rect.width` |
| Scale | an explicit zoom, or `min((stageW − 28) / 1600, (stageH − 28) / 1200)` |

This satisfies the Tier 2 rule that a canvas's backing store follows
`clientWidth × devicePixelRatio` and never `innerWidth`, while keeping the coordinates
device-independent. At fit on a laptop `k` is 1. At 200 % on a 1.125-ratio display the
store is 3600 × 2700 and `k` is 2.25, so a zoomed sheet is sharp rather than upscaled.

One operation is unavoidably in device pixels and follows `k` explicitly: the eyedropper
samples at `p × k`. A translucent mark's offscreen buffer matches the target canvas and
its transform.

PNG export renders through the same painter at `k = 1` onto its own 1600 × 1200 canvas,
so the file is the sheet at its own resolution whatever the zoom, and never carries the
keyboard crosshair.

### Zoom

Zoom is view state. It belongs to one device and never reaches the log. It is stored in
`localStorage` under `sketch.zoom`; absent, or `0`, means fit, which is the default.

Buttons step in 5-point increments, snapped to the nearest 5 and clamped to 5–400 %.
Tapping the percentage turns it into a labelled text field: type a number and press Enter
to apply, Escape or blur to cancel. A value outside 5–400 is refused in `#status` rather
than silently clamped.

A portrait phone gets a dismissible "Rotate for more room" note over the sheet. Nothing
about the sheet changes when it is dismissed.

## Layout

Three regions, all in page flow, nothing clipped.

**Left rail**, 172 pixels wide. A 44-pixel header holds the Pv mark — the branding asset
copied unaltered into `web/`, swapped for the white version under
`prefers-color-scheme: dark` through a `<picture>` — and the word "Sketch". Below it, all
ten tools, one per 44-pixel line, each an icon, a name and its keyboard letter. Below
that, the active tool's options: four brush sizes as a 2 × 2 grid, the three line styles,
the two selection modes, the text field, or one line of guidance for a tool that takes no
options.

The rail is a flex column with `overflow: hidden`. The tool list is `flex: 1; min-height:
0; overflow-y: auto` with `flex-shrink: 0` on its rows, and the options block is a plain
sibling below it. On a short window the list scrolls inside its own space and the options
stay visible, so a panel can never overlap a tool row or swallow a click meant for one.
In-flow height is 44 + 10 × 44 = 484 pixels, so on an ordinary window the rail does not
scroll at all.

**Top bar**, a single row. Its left half follows the tool selected in the rail:

- Four quick tools — Select, Brush, Eraser, and a fourth slot showing whichever other
  tool was last chosen, Pan included. Each is a **split button**: see below.
- Then, for the tools that lay ink down, the colour row and the custom-colour circle.
- For the four that do not — Eraser, Eyedropper, Select, Pan — one line of explanation
  instead.

Right-aligned and always present: undo, redo, the high-contrast toggle, and a **Sketch
actions** menu holding **Apps**, Download PNG, Download SVG and New sketch. Every item
closes the menu behind it, and the menu carries no explanatory paragraph — the outcome is
reported in `#status`.

Which controls are on show follows `data-tool` on `<body>`, decided in CSS. The
contextual toolbar therefore costs no JavaScript and cannot fall out of step with the
current tool.

**Stage**, the white sheet centred on the page background with a hairline border and a
soft shadow, scrolling when zoomed past fit. It is two layers: an absolutely positioned
scroller holding the sheet, and a non-scrolling overlay above it. The zoom pill and the
rotate hint belong to the overlay — inside the scroller they would scroll away with the
drawing. Pan drags the scroller's offsets, so it only has an effect once the sheet is
larger than the window; when it is not, `#status` says so rather than the drag doing
nothing silently.

**Footer**, in order: the `aria-live="polite"` summary, the `role="status"` message, and
the key hint.

Below 820 pixels the rail becomes a horizontal strip *below* the stage
(`flex-direction: column-reverse`): the tool list flows into one scrolling row, the sizes
into four columns, and the rail heading is taken off the screen but left in the
accessibility tree, since it is the page's only `<h1>`. Every control keeps its 44-pixel
minimum.

### The quick tools are split buttons

A quick tool that has settings carries a small caret in its bottom-right corner. The
button chooses the tool; the caret opens that tool's own options as a menu, so a size or a
selection mode is one press away without going to the rail.

Four ways in, because a long press alone would be a gesture with no alternative:

| | |
|---|---|
| The caret | A control of its own, 24 × 24, reachable by tab and by a single tap. |
| Long press | 450 ms on the tool itself. The click that ends it does not also choose the tool. |
| Right click | On the tool itself. |
| `ArrowDown` or `Shift+F10` | With the tool focused. |

The menu is `role="menu"` with `role="menuitemradio"` items carrying `aria-checked`, and
`aria-haspopup="menu"` and `aria-expanded` on the caret. Up and down move through it, Home
and End jump, Enter applies, Escape closes it and returns focus to the tool. While it is
open it takes the keyboard, so a tool letter does not fire underneath it.

What each tool offers is the same set the rail shows: the four sizes for anything with a
width, the three line styles as well for Line, Rectangle and Ellipse, and the two modes
for Select. A tool with no settings — Fill, Eyedropper, Pan — has no caret at all.
Choosing an option also selects the tool the menu belongs to, which is what a split button
means. **Every option in a menu is also a full-size button in the rail**, so nothing is
reachable only this way.

### The custom colour dialog

The circle at the end of the colour row opens a 300-pixel dialog with the picker already
in it — there is no second click to reach a wheel. It holds a hue-and-saturation wheel, a
labelled **Brightness** slider, a two-row grid of twelve named inks, a labelled **Hex**
field, and **Apply** and **Cancel**. The dialog is capped at `calc(100vh - 80px)` and
scrolls internally, so it always fits.

The wheel is **HSV, not HSL**, because that is what the rendered disc actually shows:
angle from the top is hue, distance from the centre is saturation — white centre,
saturated rim — and the Brightness slider is value. Reading it as HSL is what makes the
slider jump to the middle on every click and makes distance from the centre appear to do
nothing. Value changes only from the slider. The one exception is that a click starting
from a zero-brightness colour, such as the default black, starts at full brightness,
since otherwise the wheel would return black everywhere. A black overlay at `1 − value`
dims the disc so it previews the colours actually reachable.

Everything in the dialog edits a draft. The pen colour changes only on Apply. The wheel is
focusable and takes arrow keys — left and right for hue, up and down for saturation,
Shift for larger steps — announcing hue, saturation and the resulting hex in `#status`.

The twelve inks are named in their accessible labels. Warm row: cadmium red `#E23D28`,
deep yellow `#F5A300`, grass green `#7CB518`, ultramarine `#2E4C9E`, burnt sienna
`#8A4B2A`, white `#FFFFFF`. Cool row: magenta `#D6006E`, lemon `#F2E63D`, phthalo green
`#00795B`, cyan `#00A6D6`, black `#000000`, mid gray `#808080`.

## Tools and what they write

Each mark is one append-only event. Freehand keeps `points`, `color` and `width` exactly
as the first version of the app wrote them, so every log written before these tools
existed replays unchanged. Everything else is an additive field.

| Tool | Key | Event payload |
|---|---|---|
| Brush | B | `{ points: [[x, y]] or [[x, y, w]], color, width }`, plus `blend: "multiply"` for translucent ink |
| Eraser | E | the same, with `color: "#FFFFFF"` |
| Fill | F | `{ kind: "fill", shape, a, b or points, color, anchor }` — the inside of one mark, or `{ kind: "page", color }` for the sheet |
| Line | L | `{ kind: "line", a: {x, y}, b: {x, y}, color, width }`, plus `dash` |
| Rectangle | R | `{ kind: "rect", a, b, color, width }`, plus `dash` |
| Ellipse | O | `{ kind: "ellipse", a, b, color, width }`, plus `dash` |
| Text | T | `{ kind: "text", x, y, text, color, width }` — re-editable |
| Eyedropper | I | nothing; reads one pixel and returns to the tool it interrupted |
| Select | V | nothing; a move is a tombstone and a fresh put per mark |
| Pan | H | nothing; view state only, never written |

`layer` is the painting-order key. It carries a mark's original position through a
restoration, which has to mint a fresh id because a tombstoned id is never the key of
another row (`spec/protocol.md §4.6`).

### A fill is the inside of a shape

Fill colours the inside of the shape under the pointer. That colour is its own mark,
carrying its own geometry: a rectangle or an ellipse inset by half its outline, which is
where the colour stops, or a freehand outline's own samples, closed.

`anchor` names the mark the colour belongs to. It is used for **moving and deleting**, so
a shape and its colour travel together and go together — nothing is looked up to draw the
fill. A stale anchor therefore means the colour does not follow its shape, which is a
visible, boring failure rather than a colour running across the sheet.

This is not a flood fill, and the difference is the whole point. A flood is a search
across the pixels from a seed point, and a seed is not an object: it cannot be selected,
it cannot be moved, and what it covers changes every time anything is drawn near it. In an
app where every mark is an event and Select moves whole marks, that was the one thing that
did not fit the model, and it produced every fill bug in turn — colour escaping onto the
page when a shape moved, colour jumping between regions as a shape crossed a line, a
filled shape exporting as a hollow one.

Two things a flood could do and this cannot:

- **A region no single mark encloses** — the lens between two overlapping circles, or one
  half of a circle cut by a line. Draw the shape you want coloured instead.
- **Colouring around the marks on the page.** The sheet has its own colour instead; see
  below.

Copying a shape brings its colour with it, and paste re-points the copy's anchor at the
new shape's fresh id. Restoring a deleted shape mints a new id, so every anchor to it is
re-pointed in the same batch — `history.js` returns the map.

### The sheet has a colour

Tapping Fill where there is no shape colours the sheet itself, as `{ kind: 'page', color }`.
The last one laid down wins; it is the surface every mark is drawn on, not a mark in the
painting order, and it is never selectable. It is an ordinary event, so it undoes, syncs
and exports like anything else, and **New sketch** returns the sheet to white.

### Selecting, moving and resizing

Select has two modes, shown in the top bar where the colour row sits.

**Stroke** taps a mark and drags it. Clicking the same spot again steps to the next mark
under the pointer, announced as "Mark 2 of 3 here" — overlapping marks are otherwise
unreachable. The cycle resets when the click moves more than 14 sheet pixels or the set of
marks under it changes. `Space` does the same from the keyboard at the pen position, and
the arrow keys then move the selection.

**Rectangle** drags a box; every mark whose bounding box intersects it is selected, then
the group drags as one.

Both move **whole marks**, never a region of pixels. A pixel marquee cannot be expressed
as edits to the log, and the canvas is shared, which is the reason to keep the log clean.
The selection is outlined with dashed blue boxes and the marquee with a dashed ink
rectangle. Escape clears it, and changing tools drops it.

A selection also carries a square **resize handle** at the bottom-right of its own box.
Dragging it scales everything selected about the opposite corner; Shift keeps the
proportions, taking the smaller factor so the shape stays within the pointer. Because a
drag may never be the only way to do something, **Bigger** and **Smaller** sit in the rail's
Select options and `Ctrl`/`⌘` with an arrow key does the same from the keyboard. The handle
is sized in CSS pixels rather than sheet pixels, so it stays a 24-pixel target at every
zoom.

Two rules the scale follows:

- **An outline keeps its thickness.** `width` is the pen a mark was drawn with, so a
  scaled shape is still drawn with the same pen, and a pressure stroke keeps the width it
  recorded at every sample. Text is the exception, because its `width` *is* the type size:
  it scales uniformly by the smaller of the two factors, since one size cannot follow two
  axes.
- **A group takes one transform.** Every selected mark, and every colour inside one, is
  scaled about the same corner by the same factors, so the arrangement of a group survives
  the resize. An axis with no extent — a horizontal line has no height — is left alone
  rather than divided by; a scale that would collapse or invert a mark is clamped.

Moving and resizing are the same operation with a different transform, and both are built
by `strokes.rewriteEvents`. That is deliberate: it is the single place a fill can be
carried along with its shape, and keeping one path is what stops a fill being given a
transform of its own.

### Smoothing

Raw samples drawn as straight segments are what make slow circular motion look faceted.
Sketch smooths in two places.

On capture: `getCoalescedEvents()` is used where available so fast movement keeps its
intermediate samples; each sample is filtered toward the previous point; and samples
closer than 1.2 pixels are folded into the last one instead of stored. The filter is
distance-adaptive — 0.55 under 8 pixels, 0.85 up to 24, and raw beyond that — so it
removes hand jitter without lagging behind real movement, which a fixed factor does badly
on sparse samples.

On render: a stroke is one path of quadratic curves through the sample midpoints rather
than a run of lines, with a per-segment variant carrying pressure widths. The eraser gets
the same treatment, since it was the worst case. SVG export writes the same curve as a
`path` with `Q` segments, so the file matches the sheet.

Stylus pressure sets a width per point: `width × (0.35 + 1.3 × pressure)` when
`pointerType` is `pen`. A mouse or a finger uses the fixed size and leaves the log in the
original two-number shape. The four sizes are Fine 2, Medium 6, Bold 14 and Broad 32.

### Translucent ink and line style

Translucent ink is a **property of the mark, not a mode**. A mark drawn with it on carries
`blend: "multiply"`, so replay never depends on the toggle's current position and two
devices render the same log identically.

Three rules make it correct. The eraser never blends, because white is the identity colour
for multiply and a blended eraser would do nothing. Each blended mark is painted whole to
an offscreen canvas and composited once, so it does not darken against its own overlapping
segments. Fills are never blended.

Note what it is not: multiply is translucent-marker overlap, not pigment mixing. Saturated
primaries multiply toward black — pure red over pure blue is exactly black — so it reads
best with lighter or desaturated colours.

Line style is the same shape of decision. Line, Rectangle and Ellipse take
`dash: "dashed"` or `"dotted"`, written onto the mark only when it is not solid, so old
events and solid marks are byte-identical to before. The pattern scales with the stroke
width — dashed is `[3w, 2w]`, dotted is `[1, 2.2w]` with a round cap — so it reads the
same at Fine and at Broad. Freehand is not offered it: a freehand mark is drawn as many
short segments, and a dash pattern restarts on each one.

### Copy, cut and paste

These work on the selection and need no new event kind. `Ctrl/⌘ C` copies, `Ctrl/⌘ X`
copies then deletes as one act, and paste re-appends the marks offset by 32 sheet pixels
as ordinary new events with fresh ids. The pasted marks land selected in Stroke mode,
ready to drag. Copy, Cut and Paste also appear as buttons in the rail options, since touch
has no shortcut.

Copying also writes the selection to the **system clipboard as SVG**, stamped
`data-sketch-clip`. On paste, a clipboard payload carrying the current stamp uses the
internal marks so nothing is lost — per-point widths, blend, dash. Anything else goes
through a deliberately small **SVG reader**: `line`, `rect`, `ellipse`, `circle`,
`polyline`, `polygon`, and `path` with only `M`, `L` and `Z`, taking `stroke` and
`stroke-width` where they are plain values. Curves, text, images and transforms are
skipped and the count is reported in `#status`. Foreign geometry outside the sheet is
translated onto it.

Paste is handled on the DOM `paste` event rather than a `Ctrl+V` key branch, so it needs
no clipboard permission and also catches menu-driven paste. Pastes into the hex or text
field are left alone.

### SVG export

SVG export falls out of the log. Freehand and eraser marks become a `path` with the same
curve the canvas draws, or per-point `line` elements where the width varies; Line,
Rectangle and Ellipse become their SVG equivalents with `stroke-dasharray` carrying the
line style; Text becomes `text`. A blended mark is wrapped in a group with
`mix-blend-mode: multiply`, which composites the group as a unit and so matches the
canvas.

A fill is transcribed rather than reconstructed: it already carries its area, so a
rectangle becomes a `rect`, an ellipse an `ellipse`, and a freehand outline the same closed
curve, each filled and unstroked at the fill's own place in painting order. The sheet's
colour becomes the background rectangle.

The one thing that cannot be written is a fill from a log old enough to have named no mark
at all. Those are counted and reported in `#status`.

### Undo, and the batch ceiling

Undo works over the shared canvas, not over this tab's own writes. Every change the tab
sees — its own, another window's, another device's, and the log replayed at load — joins
one history in arrival order, and undo reverses the latest, bounded to 50 changes. A batch
written as one act is undone as one. Redo puts back what undo reversed.

**A tab's own write reaches it twice.** The node publishes an append to the stream and
answers the request that made it, in no fixed order, so the echo can arrive before the
call that wrote it returns. The history holds the events of the write in flight and
applies their echo without remembering it. Counting the echo as a change of its own
remembers every mark twice, and the second undo then refuses forever — the stack's next
entry describes a mark the first undo already removed.

Restoring a deleted mark writes compensating events under a fresh id carrying the original
`layer`, and re-points every reference held elsewhere: the other entries in both stacks,
and any fill anchored to the mark.

The node refuses a batch over `api.max_batch` and writes none of it
(`spec/data-api.md §3`). An action that can name more marks than that — clearing a full
sheet, a large paste, a move over a big selection — is written in ceiling-sized chunks
instead of failing at the door. A mark's tombstone and the fresh put that replaces it
always stay in the same chunk. Each chunk is then its own undo step, and `#status` says
how many presses the action now takes.

## Opening state

The tool the app opens in follows the sheet.

An **empty sheet opens in Brush**, so the colour row and the translucent toggle are on
screen and the first thing you can do is draw. A **sheet that already holds marks opens in
Select**, so the first thing you can do is pick one of them, and `#status` says "This
sheet already has marks. Select is ready — pick Brush to draw."

This runs after the log has been read, not before, and it never overrides a tool the
person has already chosen. It re-runs on a resync and on reconnect, because a Lamport
cursor is not a gap-free resume: an event that arrived over sync while the tab was away is
not replayed by `after=`, so the log is re-read rather than resumed.

The rule exists because the top bar's left half is contextual. Opening in Select on an
empty sheet would show no colour interface at all.

## Keyboard

The keyboard draws everything the pointer draws. Shortcuts are ignored while focus is in a
text or colour field.

| Keys | What they do |
|---|---|
| `B E F L R O T I V H` | Select a tool. The letter is shown beside its name in the rail. |
| Arrow keys | Move the pen 8 pixels, or 60 with Shift. A dashed crosshair marks it while the sheet has focus, and turns red while the pen is down. |
| `Space` or `Enter` | Put the pen down and lift it. With Fill, Text or Eyedropper it acts once at the pen position. |
| `Escape` | Discard the mark in progress, clear the selection, or close an open menu. |
| `Delete` or `Backspace` | Remove the current selection, from anywhere on the page. |
| `Ctrl/⌘ Z`, `Ctrl/⌘ Shift Z` | Undo, redo. |
| `Ctrl/⌘` + arrow | Resize the selection; Shift for a larger step. |
| `Ctrl/⌘ C`, `X`, `V` | Copy, cut, paste. |
| `+`, `−`, `0` | Zoom in, zoom out, fit to view. |

The keyboard crosshair is excluded from PNG export.

## Accessibility

The target is WCAG 2.2 AA, as it is for everything in this repository.

- Every tool shows an icon **and** a visible text label in the rail. The top bar's
  duplicates are icon-only with accessible names; their labelled instance is the rail.
- Icons are `<use>` references into one sprite at the top of the page, and every symbol in
  it is a vendored Bootstrap Icon copied verbatim ([Icons](icons.md)) — never hand-drawn
  path data. Each is `aria-hidden="true"` and `focusable="false"`. `.ic` sets
  `fill: currentColor`, without which every glyph paints black and disappears on the dark
  panel and on the pressed fill.
- Mode is exposed with `aria-pressed` on tools, sizes and swatches. The active state is a
  filled pill, never colour alone, and the pressed control carries the label a screen
  reader reads.
- `#status` (`role="status"`) announces tool changes, pen down, pen up and saved,
  discarded, picked colours, zoom, selection counts, refused writes and the offline state.
  `#summary` (`aria-live="polite"`) says how many marks in how many colours on a
  1600 by 1200 sheet, refreshed on load, on every change from any source, and on clear.
- The sheet is `tabindex="0"`, labelled, and `aria-describedby` the key hint and the
  summary.
- With the eyedropper active, the top bar shows a **live preview chip**: a rounded square
  filled with the pixel under the pointer, with the dropper glyph and that pixel's hex
  beside it. The chip's ink flips between white and black by whichever has the higher
  contrast against the sampled colour, and its boundary follows the 3:1 rule below, so it
  stays readable over black, over white and over anything between. Its empty state is a
  dashed square reading "Hover the sheet". Moving the keyboard pen updates it too, so the
  readout is not pointer-only.
- The eyedropper is momentary: picking a colour returns to the tool that was active
  before it and says so. It is **only offered where it means something** — with Eraser,
  Select or Pan active it is disabled, its `title` explains why, and the `I` shortcut
  answers in `#status` rather than doing nothing.
- Disabled controls take muted ink, a dashed border and the default cursor, so an
  unavailable control never advertises itself as available.
- All controls are at least 44 × 44 pixels, at every breakpoint.
- Focus ring: 3 pixels solid, 2 pixels offset, inset by 3 on the canvas so it is not
  clipped.
- The viewport stays zoomable and the canvas takes `touch-action: pinch-zoom`. No gesture
  exists without a button or single-key equivalent.
- `prefers-reduced-motion` collapses all durations. Nothing flashes.

## Colour

Neutral surfaces, with the Privatium lilac as the only accent.

| Token | Light | Dark | High contrast |
|---|---|---|---|
| `--bg` | `#F6F1EE` | `#151016` | `#FFFFFF` |
| `--panel` | `#FFFCFA` | `#1E1720` | `#FFFFFF` |
| `--ink` | `#241A22` | `#F7F0F4` | `#000000` |
| `--muted` | `#6B5F68` | `#B2A5B4` | `#000000` |
| `--line` | `#E2D7D6` | `#372C39` | `#000000` |
| `--accent` | `#EBCBF3` | `#6C4A79` | `#FFE600` |
| `--accent-ink` | `#241A22` | `#FFF7F0` | `#000000` |

Dark follows `prefers-color-scheme`. High contrast is a toggle that overrides the tokens
on `:root`; it is not a theme picker. The sheet does not follow either of them: it is
white until someone colours it, because the drawing is shared and exported and must look
the same to everyone.

**`--line` separates regions; `--muted` bounds controls.** `--line` is 1.38:1 against the
panel, which is right for a rule between two areas and never enough for the edge of
something a person operates. Every button, field and chip therefore takes its border from
`--muted`, which clears 3:1 against the panel, the page and the pressed accent fill in
both schemes (WCAG 1.4.11).

### The colour row

The colour row is **seven editable slots**, not a fixed palette plus extras. They start as
Black `#000000`, Red `#E63946`, Blue `#457B9D`, Yellow `#FFB703` and three empty, and
every one of them can be changed, the four defaults included.

- Clicking a slot selects it — `aria-pressed`, a 2-pixel ring — and makes it the pen
  colour. Clicking an empty one selects it and opens the picker for it.
- The circle at the end of the row edits **the selected slot**, and always shows that
  slot's colour, so what you are about to change is never in doubt. Apply writes in place:
  slot 6 stays slot 6. Nothing shifts and nothing is prepended.
- The eyedropper writes into the selected slot too, so picking a colour off the sheet
  updates the circle you were on and then hands back: "Picked #2459CF into colour 5. Back
  to Line."
- Slots keep their colours for the session and reset to the defaults on **New sketch**, or
  when the app opens on an empty sheet.
- An empty slot is a dashed circle with a plus inside. A slot's accessible label is its
  position and its colour's name where one is known — "Colour 6, warm green, grass" —
  falling back to the hex.

### Every swatch gets its boundary from one rule

Rather than a hand-maintained list of which swatches need a ring: linearise the fill's
sRGB channels, compute its true WCAG contrast against the *current* `--panel`, and use a
2-pixel `--muted` ring whenever that is below 3:1, otherwise the `--line` hairline.

This is what makes Yellow `#FFB703`, at 1.71:1 on the light panel, and white legible as
controls, and it holds in the dark scheme too, where black is the colour that needs the
ring. A `prefers-color-scheme` listener re-renders so the rule re-evaluates when the
scheme flips. The same linearisation picks black or white for a glyph on an arbitrary
colour.

## Typography

`system-ui` throughout, with `ui-monospace` for keyboard letters, hex values and the zoom
percentage.

The design was drawn in IBM Plex Sans. It is not shipped: `apps/` is embedded in the
binary, so vendoring four woff2 faces would grow every copy of the program and every phone
download for a typeface the app does not depend on. If it is ever wanted, vendor the faces
under `web/` with their licence, and change the `font` shorthand in `style.css` together
with the two `font` strings in `paint.js` and `clip.js` — text marks name their family so
the SVG export matches the canvas.

## Checking a change

```bash
privatium lint apps/sketch
cargo test --locked -p privatium-core --test reference
```

The linter holds the app folder to the `PV4xx` rules of `spec/cli.md §5`. The reference
test serves the page and every file it loads, replays the calls `app.js` makes against a
real log, checks the page against the accessibility baseline, verifies that each sprite
symbol is the vendored Bootstrap Icon it claims to be, and checks the declared colour
tokens for contrast in both schemes.

`node --test crates/privatium-core/tests/js/` runs the modules that carry no DOM —
`sheet.js`, `strokes.js`, `paint.js`, `clip.js`, `tools.js` and `history.js` — against
their own code.

### Checks that still need a person

Neither the linter nor the test suite replaces a browser. **The manual pass on this design
is outstanding.** It is:

1. Open the app in two windows. A mark drawn in one appears in the other, and undo in
   either reverses whichever mark was drawn last.
2. Traverse the whole app with the keyboard alone: every tool by its letter, the pen by
   the arrow keys, Space to draw, the colour wheel by its arrow keys, and a visible focus
   ring throughout.
3. At 200 % text zoom and at 320 CSS pixels wide, with no sideways scrolling.
4. A screen reader on the drawing path, the colour dialog and the status region.
5. A stylus on a real tablet, for the pressure widths, and a finger on a real phone.
6. The eyedropper on a HiDPI display, where the device scale is not 1.

`fromSvg` in `clip.js` needs the browser's `DOMParser` and so has no test under
`node --test`; a foreign-SVG paste is part of the manual pass.

---

Copyright © 2026 Gabriel Mongefranco
