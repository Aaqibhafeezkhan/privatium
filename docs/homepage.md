<!--
This file is part of Privatium
docs/homepage.md
Author(s): Gabriel Mongefranco
Created: 2026-09-27
Last Modified: 2026-09-27
Summary: Editing, translating, and serving the static project homepage and its screenshots.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium

## Project homepage

[Back to project README](../README.md)

The root `index.html` introduces Privatium and explains how to download it.
It is a static single-page website with a small script for switching screenshots.
This guide covers its assets, translation points, and local preview.

### Files and hosting

Serve `index.html` and `assets/branding/` together, preserving their relative paths.
The intended address is <https://dev.mongefranco.com/privatium/>. The page has no
build step, backend, external font requests, analytics, or package dependencies.
It does not start the Privatium program or run commands for the visitor.

Open `index.html` directly in a browser, or serve the repository with Python:

```sh
python -m http.server 8000 --bind 127.0.0.1
```

Then visit <http://127.0.0.1:8000/>. This command previews the website; running
Privatium itself is a separate step described on the page.

### Screenshots and branding

The page reuses the approved wordmark, favicon, touch icon, and social preview.
The [branding guide](branding.md) documents their provenance and font notices.
Gabriel supplied these screenshots for the homepage, with permission to include
them in the project. They are stored without pixel changes:

| File in `assets/branding/` | Content |
|---|---|
| `privatium-dashboard-preview.png` | The app launcher, including the custom Prescription Tracker app in development. |
| `sketch-app-preview.png` | Sketch with drawing tools and PNG/SVG export options. |
| `device-pairing-preview.png` | Pairing with two words or four labeled emoji. The supplied image obscures its address and part of the QR code. |

The screenshots show no prescription records. The pairing code is a historical,
short-lived example, not an instruction to pair with that code. Retain the
redactions when replacing this image, and use a closed or expired pairing window.
Prescription Tracker must remain labeled as a custom app in development, not an
included application.

### Updating copy or adding Spanish

All explanatory copy, headings, captions, and instructions are HTML text.
`data-i18n` attributes provide stable names for copy blocks; they do not load
translations or imply that a Spanish version already exists. Commands use
`translate="no"` so translation tools can leave them intact.

When adding a locale:

1. Translate complete blocks, retaining their links and semantic emphasis.
2. Translate image alternatives, accessible names, metadata, and navigation too.
3. Set the document language, title, description, canonical URL, and Open Graph
   locale for the actual translated address. Add alternate-language links only
   once both pages exist.
4. Replace screenshots with equivalent localized captures when available. Their
   essential meaning is also explained in the HTML captions and surrounding copy.
5. Check long Spanish labels at 320 CSS pixels and with enlarged text. Do not
   shrink text to force it into the English layout.

The gallery's live announcement uses its translated button label. It has no
separate English message hidden in JavaScript. Keep existing `data-gallery`
values, IDs, asset paths, and commands unchanged unless updating their references.

### Accessibility and privacy

The page targets WCAG 2.2 AA. It includes a skip link, native links and disclosures,
visible focus, image descriptions, responsive layouts, and no automatic motion.
Screenshot links open the original PNG at full size. Without JavaScript, both
gallery images and all essential instructions remain available; nonfunctional
gallery buttons stay hidden.

The gallery only switches between names declared in the page. It does not read
URL parameters, fetch remote content, inject HTML, or store visitor data. Existing
browser security and privacy controls remain in place.

Screen-reader testing and testing by disabled users are still required before
making any accessibility-conformance claim. The source and header checks do not
replace those reviews.

### Verification record

The homepage was checked on 2026-09-27 in Chromium 153.0.8010.0:

- Desktop at 1366 CSS pixels and mobile at 320 CSS pixels had no horizontal
  overflow or broken images. Both had zero axe-core 4.11.1 WCAG A/AA violations.
- Keyboard checks covered the skip link, visible focus, screenshot selection,
  its live announcement, and expandable instructions.
- Enlarged text at 200%, wider text spacing, forced colors, and reduced motion
  retained the layout. The forced-colors view keeps the light logo legible.
- With JavaScript disabled, both gallery screenshots and the instructions
  remained usable at 320 CSS pixels.
- The page loaded from a file URL and a `/privatium/` web path, with no script
  errors, failed resources, or third-party requests.

These checks cover the homepage only. The Rust header-check command could not
run because Cargo was unavailable. The changed file headers were checked against
`AGENTS.md`, and the HTML header was also compared with the repository template.

### Keeping the page accurate

Check commands and download filenames against the [README](../README.md).
The local-sync and phone-access examples follow the current implementation and
[architecture](architecture.md). Remote sync and offline phone apps are labeled
as planned. Do not remove that qualification until the capabilities ship.

You can now preview and edit the homepage without a build system. Keep translated
copy, screenshots, and metadata aligned whenever the page changes.

### Additional resources

- [Homepage source](../index.html)
- [Intended homepage address](https://dev.mongefranco.com/privatium/)
- [Project README and quick start](../README.md)
- [Branding and image provenance](branding.md)
- [Architecture and current local sync](architecture.md)
- [Local preview address](http://127.0.0.1:8000/)
- [GNU Free Documentation License](https://www.gnu.org/licenses/fdl-1.3.html)

[Back to project README](../README.md)
