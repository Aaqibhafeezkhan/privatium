---
name: project-preferences
description: Privatium's file header rules, the exemptions header-check honors, how to run the check, and the one name the project uses for each concept. Apply when creating or editing any file that carries a header, and when writing interface text or documentation.
---

<!--
This file is part of Privatium
skills/project-preferences/SKILL.md
Author(s): Gabriel Mongefranco
Created: 2026-09-09
Last Modified: 2026-10-22
Summary: Repository-specific header rules, exemptions, and terms for Privatium.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium

## Project preferences

Use this skill when planning, implementing, or reviewing changes in this repository. It
supplements [AGENTS.md](../../AGENTS.md), especially section 3, and cannot weaken its
security, privacy, accessibility, licensing, testing, or authorization rules.

### Header format

Section 3 of `AGENTS.md` gives the format, and every checked file in this repository now
carries it, in that language's comment syntax. Two details are specific to Privatium.

Source files take the GNU General Public License notice. Markdown under `spec/` and
`docs/` takes the GNU Free Documentation License notice instead, because documentation is
licensed separately. The notes line reads exactly `See README file for documentation and
full license information.` and stays whole on one line, so both a reader and the checker
can find it.

`.lsp` templates carry a reduced header: the project line, the file's own path, the
summary, and the notes line. Authorship and a full licence notice on every partial of an
app that nobody reads separately is noise.

### Checks

`cargo xtask header-check` covers `.rs`, `.lua`, `.sql`, `.js`, `.css`, `.lsp`, and `.md`
under `spec/` and `docs/`.

Markdown elsewhere is exempt by design: this file, `README.md`, every `apps/**` README and
`SKILL.md`, and everything under `skills/` are prose rather than source. `.html` and
`.toml` are exempt too. Manifests carry a header by convention here and should keep doing
so, but the convention is not enforced.

Anything vendored is exempt and must stay that way, since third-party code carries its own
provenance. The marker is a `VENDOR.md` beside the file or above it. Minified bundles are
exempt as well, because a minifier discards comments.

Dates are checked for shape, not accuracy. A mechanical `Last Modified:` check would either
be wrong or fight every commit that touches the file. Update it yourself on a material
change.

### Verification

`cargo xtask header-check` is the check of record. Run it after adding or editing any file
it covers, and fix what it reports rather than exempting the file.

### Terms

Use one name for each concept in the interface, messages, help, and documentation.
Section 1 of [spec/protocol.md](../../spec/protocol.md) defines these terms, and it wins
if this list and the specification disagree.

- **Node**: one installation of the Privatium program. Not "server", "instance", or
  "host" in text a person reads.
- **Device**: anything paired with a node, such as a browser, a phone, or another node.
  Not "client" in text a person reads.
- **Owner**: the one person who controls a node. Not "user" or "admin".
- **App**: a folder that follows `spec/app-contract.md`. Not "application", "plugin", or
  "module".
- **Pair** and **pairing**: connecting a device to a node with a code. Not "link",
  "register", or "log in".
- **Cluster**: the nodes that belong to one owner.

Event, log, replay, snapshot, and slug are names for the inside of the system. Keep them
out of interface text and end-user documentation unless the reader has to type them.

### Additional resources

- [Project instructions](../../AGENTS.md)
- [Response style skill](../response-style/SKILL.md)
- [Skills index](../../SKILLS.md)
