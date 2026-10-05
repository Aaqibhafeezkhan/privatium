<!--
This file is part of Privatium
docs/usage.md
Author(s): Gabriel Mongefranco
Created: 2026-10-04
Last Modified: 2026-10-04
Summary: What the bar at the top and the footer at the bottom of every app do: the way back to
         the launcher, the Apps link, the Menu with the app's own items and the settings
         pages, and the status line that says when the node is offline or still sending.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium

## Using your apps

[← Back to README](../README.md)

Every app on your node wears the same bar at the top and the same footer at the bottom.
This page says what each part does, so you can move between apps, find an app's extra
actions, and read the status line when your phone loses the network. It is written for
anyone using a node, with no programming needed.

An app that needs the whole window, such as the Sketch drawing app, can turn the bar and
footer off. Such an app draws its own way back; in Sketch it is the Privatium mark at the
top of the tool rail, and the Apps entry in its menu.

### The bar at the top

The bar has three parts, left to right.

1. **The Privatium mark.** Tap it to go back to the launcher, the page that lists your
   apps. If your node runs one app only, the mark takes you to that app's first page.
2. **The app's name**, in the middle, with its icon. Tap it to go to the app's first
   page from anywhere inside the app.
3. **Apps and Menu**, on the right. **Apps** is the launcher again, one tap away.
   **Menu** opens a list.

On a phone the mark shrinks to a small square and the app's name wraps onto a second
line if it is long. Nothing is hidden and every button stays large enough to tap.

### The Menu

The Menu lists two things, with a line between them.

- **The app's own items** come first. An app can add actions here, such as a setup page
  or a print view. Some items appear on every page of the app, and some only on the
  page you are looking at. If the app has none, the list and the line are not shown.
- **The settings pages** come last and are the same in every app: Space settings, App
  settings, Data settings, and Devices.

You can open the Menu with the keyboard. Tab to the Menu button, press Enter or Space,
and Tab through the items.

### The footer

The footer has three parts.

- **Privatium**, on the left, links to the project's page.
- **The status line**, in the middle, is empty most of the time. It speaks only when
  something changes:
  - "Offline. Changes are saved on this device and sent later." means your device
    cannot reach the node right now. Keep working. What you change is kept on the
    device.
  - "N changes waiting to send." means the node is reachable again and your changes
    are on their way.
  - "All changes sent." means they arrived.
  - "Connected again." means the connection is back and nothing was waiting.
  - "Connection lost." means the encrypted connection to the node closed on its own.
    Reload the page to open a new one.
  An app can also write a short message of its own here, such as "Saved."
- **Your space's name**, on the right, with a QR code button that opens the Devices
  page, where you connect another phone or computer.

A screen reader reads the status line out loud when it changes, without moving you off
what you were doing.

### Moving between pages

Some apps change pages without reloading the whole screen. The Animals game is one. When
you tap a link or send a form, only the middle of the page changes: the bar and footer
stay where they are, and the screen does not flash white. The page's title, the address
bar and the app's Menu items follow along, and the keyboard and screen reader start at
the new page's heading.

Two things work as they always do. The back button takes you to the page before, reloaded
fresh. Leaving the app — for the launcher, Settings, or another app — loads a whole new
page.

### Conclusion

You now know how to get back to the launcher from anywhere, where an app keeps its extra
actions, what each status message means, and why some apps change pages without a flash. To add a device, open Devices from the
footer or from the Menu. To understand what is protected while you are offline, read the
security page.

### Additional resources

- [Backup and restore](backup-and-restore.md), for saving your data and getting it back
- [Connectivity](connectivity.md), for how each kind of device reaches your node
- [Security](security.md), for what is protected and what is not
- [The page frame in the contract](../spec/lua-api.md), section 4.1, for people building
  apps

[← Back to README](../README.md)
