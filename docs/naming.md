<!--
This file is part of Privatium
docs/naming.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-08-28
Summary: Name, tagline, and the rename procedure. Status: SETTLED.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Naming

**Name: `Privatium`. Settled.** Trademark and collision checks are complete. The rename
procedure below is retained because the tokens it lists are load-bearing, not because
another rename is expected.

## Origin of the name

*Privatium* reads as an element — the private element of personal software. It carries
"private" without saying it flatly, and the `-ium` suffix suggests something elemental and
substantive rather than another app named after a verb.

Practical properties:

- 9 characters — fits the DNS-SD service-name limit of 15 (`_privatium._tcp`)
- Unambiguous ASCII, no homoglyphs, no plural or possessive trap
- Pronounceable on first sight in English and Spanish
- `com.mongefranco.Privatium` is a clean reverse-DNS app ID

## Tagline

> **The private element of personal software.**

One tagline, used everywhere: README, repository description, site header, package
metadata. Do not introduce variants. A project this size gets one line to be remembered by,
and consistency is worth more than novelty.

Two things to avoid in supporting copy:

- **Do not claim "no third parties" as an unqualified property.** Mesh VPNs, tunnels, and
  DDNS providers are supported options, and the accurate claim is that none of them is
  *required* (`docs/connectivity.md §4`). The unqualified version is the kind of overclaim
  that a single sceptical reader can disprove.
- **Do not promise privacy the design does not deliver.** Data is plain text at rest by
  deliberate choice (`docs/security.md §3`). The honest pitch is sovereignty — your
  hardware, your files, no account — not encryption at rest.

## Rename procedure

Kept for reference. The name is settled, but these three tokens are load-bearing across the
codebase and the wire format, so knowing where they live is useful regardless. Changing them
after any node ships is a **breaking protocol change**.

| Token | Appears in |
|---|---|
| `Privatium` | Docs, UI strings, README, trademark notices |
| `privatium` | Crate names, XDG directory, mDNS service type, binary name |
| `pv` | Protocol identifier `pv/1`, Lua module, HKDF labels, UDP magic `PVDISCO1` |

```bash
grep -rl 'Privatium\|privatium\|pv/1\|PVDISCO' . | xargs sed -i \
  -e 's/Privatium/NewName/g' -e 's/privatium/newname/g' \
  -e 's|pv/1|nn/1|g' -e 's/PVDISCO1/NNDISCO1/g'
```

Note that `pv` is not fully mechanical: it is also the Lua module name bound in every Tier 1
app (`local pv = require 'privatium'`) and the CSS class prefix (`pv-btn`), so a rename
breaks published apps as well as the wire format.

Then update by hand: the mDNS service type (verify ≤ 15 characters), the Flatpak app ID, the
HKDF context strings in `spec/protocol.md §8`, and the UDP magic bytes in `§6.2`.

---

Copyright © 2026 Gabriel Mongefranco
