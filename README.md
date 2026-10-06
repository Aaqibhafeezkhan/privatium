<!--
This file is part of Privatium
README.md
Author(s): Gabriel Mongefranco
Created: 2026-08-31
Last Modified: 2026-10-06
Summary: Overview, quick start and examples for Privatium. The documentation index lives in
         docs/README.md.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco
Privatium™ is a trademark of Gabriel Mongefranco.
Documentation license: GFDL-1.3-or-later, with no Invariant Sections,
                       no Front-Cover Texts, and no Back-Cover Texts.
Software license: GPL-3.0-or-later.
See the License and Credits sections below for the full notices and attribution.
-->
<img src="assets/branding/privatium-banner.svg" alt="" width="1800">

# Privatium™

***The private element of personal software.***

## Description

Privatium™ is an open-source, local-first framework for small personal apps — a tracker,
a collection, a form, a list. Your apps and your data live on hardware you control. There
is no account, no cloud subscription, and no database server to look after.

It is one program, written in Rust, with Lua and SQLite built in. Every record you save
is a line of plain text in a file on your disk; SQLite is only a cache the program can
rebuild at any time. Backing up is copying a folder. Restoring is copying it back. If
you can open a text file, you can see everything Privatium knows about you.

You choose how much to write. A few Lua pages will do for most trackers and lists. If you
want your own HTML and JavaScript, or a whole Rust program, the framework gets out of your
way and gives you the same storage, the same backup and the same launcher. Run one app or
keep several side by side. Start from an example, or hand the included guides to an AI
assistant and let it write the first draft; a linter checks the result against the rules
that matter, including accessibility.

Other devices on your home network reach a node over an encrypted channel once they are
paired: scan a QR code, tap four emoji, done. None of it requires a domain name, a DNS
provider, or a port forward.

Privatium is for people who want an app for exactly one purpose, want it private by
construction, and do not want to run a server, buy a domain, trust a cloud, or learn a
framework to get it.

## Quick Start Guide

You can run Privatium on Windows, macOS, or Linux. Lua and SQLite are included.

1. **Download Privatium.** Download directly for your platform: [Windows](https://github.com/gabrielmongefranco/privatium/releases/latest/download/privatium-windows.zip), [Windows portable](https://github.com/gabrielmongefranco/privatium/releases/latest/download/privatium-windows-portable.zip), [macOS](https://github.com/gabrielmongefranco/privatium/releases/latest/download/privatium-mac.zip), [Linux](https://github.com/gabrielmongefranco/privatium/releases/latest/download/privatium-linux.tar.gz). After downloading, extract the `privatium` (or `privatium.exe` on Windows) binary to a folder of your choice. The portable zip also holds a `privatium-data` folder with the example apps; keep it beside the program and everything Privatium knows stays in that one folder.
2. **Run it.** Open a terminal in that folder and run:

   ```sh
   ./privatium
   ```

   On Windows, use `./privatium.exe` in place of `./privatium`. The first run writes the
   four [example apps](#example-applications) into its data folder and prints where
   that folder is. To keep everything in one place instead, create a folder named
   `privatium-data` beside the program: the program uses that folder whenever it
   exists, and the example apps are written into it on the first run.
3. **Open it in your browser.** Visit [Privatium on your computer](http://127.0.0.1:8420/)
   and pick **Hello**. Keep the terminal open while you use your apps. To use them from
   your phone on the same network, run `./privatium --open` instead: it prints a QR code
   and, the first time, a pairing code. Scan the QR code with the phone and tap the four
   emoji it shows, or type the two words. Later, open pairing again from
   **Settings › Devices** or with `./privatium pair`, and label or remove a paired phone
   on the same page; see [connectivity](docs/connectivity.md) for what each kind of
   device can reach.
4. **Make your own app.** In a second terminal, run:

   ```sh
   ./privatium new myapp
   ```

   Refresh the launcher and **myapp** is there. Edit the files in the folder it printed,
   then refresh again; there is nothing to restart. To keep an app in a git repository
   of its own, with the assistant guides and a pinned framework version, see
   [your app in its own repository](docs/app-repository.md).

The example apps are ordinary folders in the same place, so you can edit them, copy them
or delete them; `./privatium new --examples` brings them back. See
[backup and restore](docs/backup-and-restore.md) for saving your data and
[command-line options](spec/cli.md) for the rest of the commands.

**Building from source?** Run `cargo build --release` from a checkout with the Rust
version in `rust-toolchain.toml`. The program is written to `target/release/`.
To use Privatium inside your own Rust application, see the
[embedded example](crates/privatium-core/examples/embedded.rs).

## Documentation

Every guide, use case and specification is indexed in **[docs/README.md](docs/README.md)**.

## Example Applications

These small apps show what you can build. The first run writes them into your data folder,
and a source checkout serves them straight from `apps/`:

- **[Hello](apps/hello)** — a simple Lua app with a form and a list. Start here.
- **[Animals](apps/animals)** — a guessing game that learns new animals as you play.
- **[Sketch](apps/sketch)** — a drawing app built with HTML and JavaScript.
- **[Pantry](apps/pantry)** — what is in the freezer and on the shelf, and what you took
  out of it.
- **[Medication Tracker](https://github.com/gabrielmongefranco/privatium-app-meds)** — your
  family's medications, refills, insurance and prior authorizations, and what to refill
  next. It lives in its own repository and is installed separately.

[AI assistant guides](docs/skills.md) help an assistant build apps that follow Privatium's
requirements, including security and accessibility.

## About the Author

Privatium is built by [Gabriel Mongefranco](https://gabriel.mongefranco.com), a database,
analytics, and software architect with two decades of experience across healthcare,
research, and enterprise systems.

His work includes establishing the University of Michigan’s first data architecture for
research using mobile and wearable technologies. His research on automating Fitbit
sleep-data cleaning was published in *SLEEP*, sleep medicine’s premier peer-reviewed
journal. Earlier in his career, his analytics helped hospitals avoid more than $5 million
in readmission penalties.

He brings the same priorities to Privatium: understandable data, durable local ownership,
accessible software, and systems people can operate without becoming infrastructure
experts. Learn more at: https://gabriel.mongefranco.com


## Contact

Questions, bug reports, enhancement ideas and requests are welcome as GitHub issues. Feel free to send pull requests as well!


## Credits

### This work is based in part on the following projects and libraries:

- [SQLite](https://sqlite.org/) — the in-process SQL engine the event log is materialized
  into, via [rusqlite](https://github.com/rusqlite/rusqlite); public domain, on every
  platform the framework targets.
- [HTMX](https://github.com/bigskysoftware/htmx) and [Alpine.js](https://github.com/alpinejs/alpine)
  — server-rendered interactivity and local reactivity, both without a build step.
- [Lua](https://www.lua.org/) and [mlua](https://github.com/mlua-rs/mlua) — the Tier 1
  application language and its Rust bindings.
- [Mako Server / Barracuda App Server](https://github.com/RealTimeLogic/BAS) — Real Time
  Logic's Lua Server Pages inspired this project's Tier 1 template engine and much of its
  developer-experience goal. No code from those projects was used. See [docs/decisions/0001](docs/decisions/0001-barracuda-evaluation.md).
- [Bootstrap Icons](https://github.com/twbs/icons) — MIT-licensed SVG icon set, vendored
  and inlined server-side; the only icon source used anywhere in the project.
- [DataLaVista](https://github.com/DepressionCenter/datalavista) — its normalization
  pipeline informed the list of date, time and timestamp spellings the framework accepts
  on write (`spec/lua-api.md §3.3`): ISO, Excel-style `M/D/YYYY`, Oracle-style
  `DD-MMM-YY`, long month names, and epochs. No code from that project was used; the
  parser is the framework's own.
- [Animal](https://github.com/coding-horror/basic-computer-games/tree/main/03_Animal) —
  the `apps/animals` example follows the classic "Animal" guessing game from
  David H. Ahl's *BASIC Computer Games* (1973), preserved and ported to many
  languages by the basic-computer-games project under the Unlicense. No code is
  copied: that project's Lua port is a console program with an in-memory tree,
  while this one stores the tree as an append-only event log so it can sync
  across devices.

- [Noble cryptography](https://github.com/paulmillr/noble-curves) — audited, dependency-free
  JavaScript implementations of X25519, Ed25519 and ChaCha20-Poly1305, used in the
  browser because `crypto.subtle` is unavailable on plain-HTTP origins.
- [SPAKE2 (RFC 9382)](https://www.rfc-editor.org/rfc/rfc9382.html) — the
  password-authenticated key exchange for device pairing, implemented on both sides from
  the libraries above rather than taken as a package.
- [mdns-sd](https://github.com/keepsimple1/mdns-sd) — mDNS and DNS-SD in Rust, for
  finding nodes on the local network.
- [Magic Wormhole](https://github.com/magic-wormhole/magic-wormhole) — inspiration for the
  short human-readable pairing code experience. No code from that project is used.

### Chosen, and not yet in the build:

- [iroh](https://github.com/n0-computer/iroh) — QUIC-based peer-to-peer transport with
  hole punching, for direct node-to-node sync.
- [pkarr](https://github.com/pubky/pkarr) — signed DNS records on the BitTorrent mainline
  DHT, which is how a public key will become an address with no registrar.
- [Arti](https://gitlab.torproject.org/tpo/core/arti) — the Tor Project's Rust
  implementation of Tor, for in-process onion service hosting with no external daemon.
- [Tauri](https://github.com/tauri-apps/tauri) — desktop and mobile application shells.

## License

### Copyright Notice

Copyright © 2026 Gabriel Mongefranco

### Trademark Notice

Privatium™ is a trademark of Gabriel Mongefranco.

### Software and Library License Notice

This program is free software: you can redistribute it and/or modify it under the terms
of the GNU General Public License as published by the Free Software Foundation, either
version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with this program.
If not, see <https://www.gnu.org/licenses/gpl-3.0-standalone.html>.

### Documentation License Notice

Permission is granted to copy, distribute and/or modify the documentation in this
repository under the terms of the GNU Free Documentation License, Version 1.3 or any later
version published by the Free Software Foundation; with no Invariant Sections, no
Front-Cover Texts, and no Back-Cover Texts. See
<https://www.gnu.org/licenses/fdl-1.3-standalone.html>

## Citation

If you find this repository or its specifications useful, please cite it.

> *Mongefranco, Gabriel (2026). Privatium™. Software. <https://github.com/gabrielmongefranco/privatium>*

---

Copyright © 2026 Gabriel Mongefranco
