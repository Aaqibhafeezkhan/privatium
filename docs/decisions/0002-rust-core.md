<!--
This file is part of Privatium
docs/decisions/0002-rust-core.md
Author(s): Gabriel Mongefranco
Created: 2026-08-29
Last Modified: 2026-09-03
Summary: Decision record. Rust as the core language, and the discovery and transport stack that
         follows from it. Status: DECIDED.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# ADR 0002 — Rust core, pkarr discovery, direct peer transport

**Status: DECIDED.**

Companion to ADR 0001, which records what was declined. This one records what was chosen
and why the pieces fit together.

## The core is Rust

One workspace, one crate: `privatium-core`. It holds the event log, replay, SQLite
materialization, discovery, pairing, session cryptography, and sync.

It is consumed four ways, and that is the point:

| Consumer | Via |
|---|---|
| Node daemon | direct |
| Desktop shell | Tauri v2 |
| iOS and Android shells | Tauri v2, `uniffi` bindings |
| Someone else's binary | a crate dependency — Tier 3 embedded mode |

**One implementation of the hard parts, on every platform.** That is the property that
decided against BAS (ADR 0001), and it is what makes a phone capable of being a full replica
rather than permanently a thin client.

Supporting choices:

- **Lua 5.4 via `mlua`** for Tier 1 — not LuaJIT, which cannot ship on iOS because the
  platform forbids JIT; not Luau, because a dialect fragments documentation and degrades
  assistance from language models.
- **SQLite**, via `rusqlite`, with the framework supplying exact decimals and typing every
  value it writes (ADR 0006 — this bullet said DuckDB until then, and ADR 0006 records why).
- **Pure-Rust dependencies where they exist** — `rustls` over native-tls, `arti` rather than
  a Tor daemon — so cross-compilation stays a one-liner and packaging stays sandbox-safe.

## Discovery: everything at once

Four mechanisms, run concurrently and never chained (`spec/protocol.md §6.5`), because they
fail in different environments and none dominates:

| Mechanism | Reaches | Fails when |
|---|---|---|
| mDNS | same broadcast domain | different subnet, multicast filtered, AP isolation |
| UDP broadcast | same subnet | broadcast also filtered |
| **pkarr on the mainline DHT** | anywhere | DHT blocked by DPI; node asleep |
| DNS (pkarr over ordinary DNS) | anywhere | resolver unreachable |

**pkarr is the significant addition.** DNS records signed by an Ed25519 key, stored on the
BitTorrent mainline DHT as BEP44 mutable items. The cluster's public key becomes its name.
No registrar, no dynamic-DNS account, no domain, no payment.

The fit is exact rather than convenient: the cluster keypair already exists
(`spec/protocol.md §2.3`), so there is nothing new to mint.

Note what this is **not**: it is not a vendor's network. The traversal library supplies code;
discovery rides a 10-million-node DHT with a fifteen-year history and no operator. Configure
a self-hosted relay and no third-party infrastructure remains in the path.

## Transport: direct, with a relay that reads nothing

Hole punching first, relay when it fails — roughly 10% of connections, and more on
carrier-grade NAT, which is standard on mobile.

A relay forwards ciphertext. It cannot decrypt, and it stores nothing. That produces a
conclusion worth stating because the intuition runs the other way:

> **A relay is a safer thing to place on rented hardware than a node.** A relay holds
> nothing; a node holds a complete plaintext replica.

An always-on machine therefore has four separable jobs — relay, pkarr DNS, certificate host,
and full node — of which only the last touches data (`docs/deployment.md §2`).

## Reasons peer transport is in `pv/1`

An earlier draft deferred it, arguing an always-on node solved the same problem. That
reasoning was flawed: a VPS requires a provider account, a card, and a monthly fee, so
recommending it while dismissing a mesh VPN "because it needs an account" was not a fair
comparison.

**pkarr plus hole punching is the only route to remote access with no account, no domain,
and no payment.** That property is the project's reason to exist, not a nicety.

It is a native-client capability. Browsers cannot open raw sockets and cannot treat a public
key as a name, so remote browser access still requires a real domain and a certificate
(`docs/connectivity.md §4.2`).

## Routes retained

Nothing is removed. mDNS and the raw LAN address remain the zero-dependency floor. Mesh VPN,
tunnel, DDNS with a DNS-01 certificate, Tor, and file sync on `data/` all remain first-class,
and the PWA path remains the insurance policy for any platform with a gatekeeper.

## Transport library: `iroh`, confirmed over libp2p

Re-examined against `rust-libp2p` and confirmed. libp2p is genuinely capable — Kademlia,
AutoNAT, DCUtR, circuit relay v2, Noise, QUIC, mDNS — and the decision is fit rather than
quality. Its advantages (polyglot implementations, browser WebRTC, million-node routing)
are not things Privatium needs, and its default DHT bootstrap is more operator-dependent
than mainline, not less.

The decisive point: **iroh already ships `discovery-pkarr-dht` and `ConcurrentDiscovery`.**
The "everything at once" design above is a feature flag rather than a build. Full reasoning
and the "would reopen if" are in `docs/decisions/0004-declined-alternatives.md §3`.

One behaviour to know about, recorded in `docs/security.md §3b`: iroh's DHT discovery
publishes only the home relay address by default. Direct addresses require
`include_direct_addresses`, which is what the account-free path needs and which publicly
associates the node ID with an IP.

## Consequences

- Two shells to build, but one core behind them.
- `spec/protocol.md §6.5` concurrent discovery is satisfied by `ConcurrentDiscovery` rather
  than by hand-written orchestration.
- Application traffic reaches the core through one interface regardless of shell — see
  `docs/decisions/0003-in-process-adapter.md`, which is what makes offline work without a
  certificate.
- A phone is a full replica but never a dependable peer; see
  `docs/decisions/0005-mobile-role.md`.
- A Rust toolchain is required to extend the framework. Tier 3 authors write Rust; Tier 1 and
  Tier 2 authors are unaffected.
- The DHT is blocked on many managed networks, so concurrent DNS discovery is required rather
  than optional.
- Discovery records expire within hours, so a sleeping laptop is not a dependable discovery
  target. An always-on machine is.

## Would reopen if

Hole punching proves unreliable enough in practice that the relay carries most traffic — at
which point the account-free argument weakens and a simpler broker-style transport deserves
another look.

---

Copyright © 2026 Gabriel Mongefranco
