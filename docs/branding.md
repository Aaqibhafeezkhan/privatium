<!--
This file is part of Privatium
docs/branding.md
Author(s): Gabriel Mongefranco
Created: 2026-09-05
Last Modified: 2026-09-27
Summary: Approved visual direction, brand assets, and usage guidance. Copyright © 2026 Gabriel
         Mongefranco Privatium™ is a trademark of Gabriel Mongefranco. Documentation
         license: GFDL-1.3-or-later, with no Invariant Sections, no Front-Cover Texts, and
         no Back-Cover Texts. Software license: GPL-3.0-or-later. See ../README.md for full
         license notices and project credits.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium branding

Privatium combines a clear geometric **Pv** symbol with an elegant serif wordmark.
The square suggests a periodic-table element and connects to the tagline:

**The private element of personal software.**

## Logo

Use an uppercase **P** and lowercase **v**, side by side on the same baseline, inside
a square frame with a small opening near the upper-right corner. The letters in the
symbol are geometric sans-serif. The word **Privatium™** uses an elegant serif style.

Keep the letters separate. The earlier overlapping serif design looked like **Rx** at
a distance, which suggested a pharmacy. Do not overlap, stack, or join the letters, add
a diagonal leg to the P, or lower the v beneath its baseline.

Leave clear space around the logo of at least one quarter of the symbol's width.
Preserve its proportions. Use the symbol alone when the full wordmark would be too small.
Keep the frame opening visible whenever the output size allows it.

## Colors

These are the target colors for future exports and implementation. Generated artwork
includes tonal variation; its individual pixels are not a source for exact color values.

| Color | Hex | Use |
|---|---|---|
| Deep aubergine | `#251329` | Dark backgrounds and the logo on light backgrounds |
| Pale lilac | `#EBCBF3` | Symbol, frame, and restrained accents on aubergine |
| Warm ivory | `#FFF7F0` | Light backgrounds and wordmark on aubergine |

Use aubergine with ivory or lilac for legibility. Avoid lilac text on ivory.
Soft purple light belongs in banners and social artwork; keep small icons simple.

## Typography

Use the supplied wordmark artwork to preserve the approved letterforms. It has upright,
high-contrast serif lettering with a refined editorial character. The geometric symbol
deliberately uses a different style so **Pv** stays readable.

The production wordmark and tagline use **Latin Modern Roman 12 Regular**. Supporting
labels use **Latin Modern Sans 10 Regular**. These reproduce the elegant serif direction
of the concept artwork; they are not an exact trace of its generated lettering.
The Pv mark is custom path artwork. All SVG lettering is outlined, so the files render
without installed fonts, scripts, external resources, or embedded raster images.

Latin Modern is by Bogusław Jackowski and Janusz M. Nowacki, based on Donald E. Knuth's
Computer Modern. Its source fonts use the GUST Font License. The supplied
[font notice](../assets/branding/FONT-NOTICE.txt) preserves the installed package's
attribution and license text. No font program is included in this pack.

## Asset pack

Assets live in `assets/branding/`. Each named master below has matching SVG and PNG files.
SVG files contain editable vector paths. The PNG exports come from those same masters.

| Master | Use |
|---|---|
| `privatium-banner` | Wide README header, 1800 × 600 |
| `privatium-social-preview` | GitHub sharing image, 1280 × 640 |
| `privatium-logo-dark` | Aubergine logo on transparency, for light backgrounds |
| `privatium-logo-light` | Ivory logo on transparency, for dark backgrounds |
| `privatium-mark` | Lilac symbol on transparency |
| `privatium-mark-aubergine` | Aubergine symbol on transparency |
| `privatium-mark-white` | White symbol on transparency |
| `privatium-app-icon` | Square aubergine app icon, 1024 × 1024 |
| `privatium-personal-apps` | Supporting illustration, not a product screenshot |
| `privatium-workflow` | Horizontal local data-flow illustration |
| `privatium-workflow-mobile` | Vertical version of the same data flow |

Additional exports: `favicon.svg`, multi-size `favicon.ico`, and
`privatium-icon-{size}.png` at 16, 32, 48, 64, 128, 180, 192, 256, and 512 pixels.
The 180-pixel image is suitable for an Apple touch icon; the 192- and 512-pixel images
provide common web-app icon sizes. Manifest integration is a separate application change.

The workflow graphics show an app appending changes to the event log, the framework
updating its rebuildable SQLite cache, and the screen displaying data. The log is the
source of truth. Network sync is labelled as planned. This paragraph is the
text equivalent of both graphics. The personal-apps illustration depicts tracking,
organizing, and creating as examples of what an owner might build.

Preview: [banner](../assets/branding/privatium-banner.png),
[social image](../assets/branding/privatium-social-preview.png),
[dark logo](../assets/branding/privatium-logo-dark.png),
[app icon](../assets/branding/privatium-app-icon.png),
[workflow](../assets/branding/privatium-workflow.png),
[mobile workflow](../assets/branding/privatium-workflow-mobile.png).

## Layout and accessibility

The [project homepage](homepage.md) also uses three owner-supplied screenshots in
`assets/branding/`: `privatium-dashboard-preview.png`, `sketch-app-preview.png`, and
`device-pairing-preview.png`. These are unaltered application captures, not generated
illustrations. The dashboard includes a custom Prescription Tracker app in development;
the homepage identifies it separately from the included apps. The pairing capture
retains the address and QR-code redactions supplied by the owner.

- Keep the main logo and tagline away from image edges and bright background effects.
- Display the banner at its natural proportions. Do not crop the wordmark or tagline.
- Keep the tagline available as page text when it is needed to understand the page.
- Use `Privatium` as alt text for a standalone logo. If it labels a home link, use
  `Privatium home`. Use empty alt text for a decorative banner beside the same visible text.
- Keep a real Markdown or HTML heading; an image does not replace it.
- Check small-screen rendering, 200% zoom, and keyboard focus on linked images.
- Use high-contrast text and controls around the artwork. Do not communicate status by color alone.

## Name and voice

Write **Privatium™** at the first prominent mention and retain the trademark in the wordmark.
Use the tagline exactly as written above. Supporting copy should explain personal apps,
local data, and ownership in plain language. Describe sync and remote access as planned
until they are implemented.

Do not add invented domains, atomic numbers, pharmacy symbols, stock padlocks, clouds,
or extra slogans. The element tile is a brand metaphor, not a claim about a chemical element.

## Provenance

The owner selected the Private Element direction, the geometric Pv refinement, and an
elegant serif wordmark. Initial concepts were produced with the built-in image generation
tool. The production pack was rebuilt with custom SVG geometry and outlined Latin Modern
lettering, then rendered to PNG. The approved arrangement and palette are shared across
every export. This work is not a trademark-clearance result.

---

Copyright © 2026 Gabriel Mongefranco
