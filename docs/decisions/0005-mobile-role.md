<!--
This file is part of Privatium
docs/decisions/0005-mobile-role.md
Author(s): Gabriel Mongefranco
Created: 2026-08-31
Last Modified: 2026-08-31
Summary: Decision record. What a phone is in the cluster — full replica for durability, opportunistic
         peer for availability, never a server. Status: DECIDED.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# ADR 0005 — Mobile is a full replica and an opportunistic peer

**Status: DECIDED.**

This answers a premise that had been doing load-bearing work while unstated: whether a
phone is a full offline replica that syncs peer-to-peer, or a very good client to a home
node. It is both, and stating it as one thing is clearer than stating it as a compromise.

## Decision

> **Mobile is a full replica for durability and an opportunistic peer for availability.**

A phone holds a complete copy of the data — so it works offline and it is a genuine backup
— but it is **never a dependable target for anyone else**, because iOS and Android suspend
background processes. This is the same caveat ADR 0002 already applies to a sleeping laptop,
with a shorter fuse.

A phone is never a server. There is no configuration in which other devices depend on one
being reachable.

## Reasons this is not a compromise

The property that actually matters is that the app **just works** — reads, writes, and
rendering, with no network and no waiting. That property does not come from replica status
at all. It comes from the core running in-process (ADR 0003). Sync is a background nicety
layered on top, not a precondition for the app to function.

Peer-to-peer sync from a phone is a real capability and worth having. It is simply not on
the critical path for anything.

## Consequences

### A phone MUST NOT publish itself as a durable discovery target

Foreground-only reachability combined with DHT record lifetimes measured in hours means a
publishing phone is mostly advertising a stale address, to no one's benefit and at some
privacy cost (`docs/security.md §3b`).

**Default: mobile resolves, does not publish.** Exposed as a setting, default off.

### Phone-to-phone sync requires both devices foregrounded

True, and rare. The ordinary path is phone ↔ desktop or phone ↔ always-on node, and that
works whenever the phone is open. Documentation should say this plainly rather than imply a
mesh that is always live.

### Sync timing

Sync on foreground, and opportunistically in whatever short background windows the platform
grants — minutes, not indefinitely. The implementation must assume the process can be killed
at any point between two events and resume correctly, which the append-only log and ULID
idempotency already guarantee (`spec/protocol.md §4.5`).

### Reachability is a separate property from replica status

`sys_device.replica` reports `true` for mobile: it holds the whole log. A **separate**
property reports reachability, and for mobile it is foreground-only. Conflating them
produces a cluster that believes a sleeping phone is a sync target.

This is the mobile row of the statement already in `docs/architecture.md §5`: clients are
not all equally capable, and that is a runtime property rather than a setting.

## Would reopen if

A platform offers a durable background networking capability that survives suspension
without draining the battery — at which point a phone could become a dependable peer and
the publishing default could change. Nothing on the horizon suggests this.

---

Copyright © 2026 Gabriel Mongefranco
