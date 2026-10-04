<!--
This file is part of Privatium
docs/security.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-10-04
Summary: Threat model, protections, and honest statements of what is not protected.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Security Model

Non-normative narrative. Normative requirements live in `spec/protocol.md §7–9`.

**Current build:** the node provides live pairing at `/ws/pair` and the encrypted
application channel at `/ws`. The node binds IPv4 on every interface and IPv6 where
available. Unpaired LAN browsers receive only the bootstrap set; a request from the
node's own machine — loopback, or the node's own LAN address opened from its keyboard —
keeps the owner's existing access, held to a `Host` header that names the machine the
same way (`spec/protocol.md §8.4`). The node advertises itself over mDNS and answers UDP probes
(`spec/protocol.md §6`); what it advertises is its public identity and app slugs, never
data. What it hears back is checked before it is listed — an ID that is not shaped as
one is dropped, a name is cut to length — and the UDP responder answers a bounded number
of probes a second from a bounded number of addresses, so a flood of invented addresses
cannot use it to amplify traffic.

Pairing opens only from the node itself — the devices page, `privatium pair`, or
`--open` on a node no device has paired with yet — and the code is shown there as four
emoji and two words. A phone that loads the node's address gets the pairing screen; a
paired device is listed on the devices page, where the owner can label or revoke it, and
a revoked device's open channel closes at once. A paired device cannot open pairing,
revoke another device or rename the node: those need the owner at the node.

The session helpers reject invalid keys, expired or mismatched certificates, altered
handshake transcripts, and unauthentic frames. A failed frame permanently closes its
direction; the transport must close the whole connection. Fresh ephemeral keys are
required on reconnect, and counters stop at 2³² frames per direction. Browser helpers
use vendored Noble cryptography and `crypto.getRandomValues`, without `crypto.subtle`.

## 1. Who this protects against, and who it does not

| Adversary | Outcome |
|---|---|
| A passive sniffer on your LAN | **Defeated for channel content.** Application requests and responses are encrypted; bootstrap documents, public assets and network metadata remain visible. |
| Someone who finds your node on the network — a guest on your Wi-Fi | **Defeated.** No pairing, no data. Unauthenticated endpoints return an ID and nothing else. See §2.1. |
| An active on-path attacker on any plain-HTTP load, including after pairing | **Wins by replacing the client.** Stored device keys are exposed. See §4. |
| A substituted node reached by a genuine client | **Refused.** Pinned key mismatch, no override. |
| Someone with your unlocked laptop | **Wins.** Privatium is not disk encryption. Use LUKS/FileVault/BitLocker. |
| Someone who steals your backup folder | **Wins.** Backups are plain text by design. Encrypt the destination. |
| A malicious app folder you installed | **Contained, not eliminated.** See §6. |
| A nation-state | Out of scope. This is a refill tracker. |

## 2. Three properties, often confused

Almost every muddled conversation about pairing comes from collapsing three separate
guarantees into one. Normative version: `spec/protocol.md §7.0`.

| # | Property | Question | Mechanism |
|---|---|---|---|
| **1** | Program authenticity | Is this client genuine? | Package signature, notarization, or a CA chain |
| **2** | Device authentication | May this device talk to the owner's cluster? | PAKE first contact, pinned keys after |
| **3** | Transport security | Is the channel confidential? | The derived session key |

| Client | 1 | 2 | 3 |
|---|---|---|---|
| Native desktop / mobile | ✔ | ✔ | ✔ |
| Browser or PWA over TLS | ✔ | ✔ | ✔ |
| **Browser over plain HTTP on LAN** | **✘ §4** | Conditional on genuine client code | Conditional on genuine client code |

**Property 2 is not a login step layered on property 3.** A password-authenticated key
exchange does both at once — deriving a usable key *is* the proof that both sides held the
code. There is no "authenticate, then encrypt." An attacker without the code does not get a
rejected login; the handshake produces nothing at all.

The practical consequence: never run a PAKE to open a channel and then send the code as a
bearer token to log in. That puts the code on the wire, makes it replayable, and throws away
the reason for choosing a PAKE.

### 2.1 The guest on your Wi-Fi

The common worry, and the one property 2 exists for.

Discovery is public — anyone on the network can see a node exists, which is inherent to
mDNS. What a guest gets from that:

1. They load the page.
2. Pairing mode is closed unless you pressed the button. The handshake is refused.
3. Even with it open: 16 bits, five attempts, a 120-second window, rate limiting. Roughly
   1 in 13,000 — and every attempt writes a replicated audit event that appears on your
   phone.

A guest is a **passive** adversary, so property 1 never enters into it. Property 2 excludes
them, and it does so identically on plain HTTP and on TLS.

### 2.2 Pair once, trusted after

The browser generates its own keypair during pairing and stores it with the pinned cluster
key under the page origin. Subsequent visits use those directly — no code, no prompt. Native
clients use Keychain, Keystore, or the OS keyring.

Lose that storage and you re-pair. There is deliberately no recovery path that skips
pairing. Each device is separately revocable, so your laptop's browser and your phone's
browser are two entries. Revoking one on the devices page writes the revocation to the
registry — the record of the pairing stays — closes that device's open channel at
once, and refuses its next handshake. A device's key is never registered twice: a browser
that lost its storage pairs again with a new key and appears as a new entry.

Browser storage is per-origin, so a device paired at a LAN address has no credential at a
public name and must pair again — the everyday argument for one resolvable name across all
paths.

## 3. What is encrypted where

| Path | Confidentiality | Peer authentication |
|---|---|---|
| Native client ↔ node | X25519 + HKDF + ChaCha20-Poly1305, pinned statics | Pinned key, TOFU at pairing |
| Browser ↔ node, LAN HTTP | Same, in pure JS, inside the `/ws` channel (`spec/protocol.md §8.3`); only code and the bootstrap page cross in the clear (`§8.4`) | Pinned cluster key, TOFU at pairing |
| Browser ↔ node, Tailscale | WireGuard + TLS on `ts.net` | Tailnet identity |
| Browser ↔ node, DuckDNS + LE | TLS | webPKI |
| Tor Browser ↔ node | Tor | Onion address is the key |
| Node ↔ node, LAN | X25519 + HKDF + ChaCha20-Poly1305 inside `/ws` | Pinned cluster certificate and static key |
| Node ↔ node, via relay | Same — the relay forwards ciphertext | Pinned key |
| `data/` at rest | **None.** Plain text, deliberately. | — |

The at-rest decision is the whole product. Encrypting the logs would defeat the restore
story, which is the reason the project exists. The correct place for at-rest encryption is
the filesystem, where the OS already does it well.

Node sessions can use only the sync routes, health and manifest; they cannot browse
apps, settings, skills or pairing routes. Sync carries whole log streams, including
apps that are not mounted locally. Every cluster member therefore holds all data that
has reached the cluster. Revocation overrides remembered peer hints and is checked
again before queued incoming app ranges are written.

### 3.1 Full-page response handoff

Full-page forms cross the encrypted channel once. The node can retain the response
stream in memory while the browser opens a fresh document with the destination app's
permissions. Per-tab storage holds only an opaque reference and destination metadata,
never the form body or rendered page. The same active device can consume the response
once. Slots are bounded and expire after 120 seconds; nothing is written to a disk
cache. This is response delivery, not event deduplication (`protocol.md §8.3.1`).

If delivery is lost, the operation may already have completed. The browser asks you to
check before submitting again. It does not automatically repeat the form.

## 3b. Relays, discovery, and what leaks

Three pieces of infrastructure sit outside your machines. None can read your data; each
leaks something.

| Component | Sees | Does not see |
|---|---|---|
| **Relay** (hole-punching fallback) | Source and destination addresses, timing, volume | Anything inside — transport encryption is end to end |
| **Mainline DHT** (pkarr discovery) | That a key published an address, and which address | Who you are, what the node is, any content |
| **pkarr DNS server** | Which keys are being resolved, and by whom | The same — nothing readable |

### A relay is a safer place than a node

This is worth stating plainly because the intuition runs the other way. A relay forwards
ciphertext and stores nothing. A **full node holds a complete plaintext replica**. If you
have one rented machine and health data, host the relay and decline the node
(`docs/deployment.md §2.1`).

### pkarr exposure equals dynamic DNS

Anyone holding the key resolves the current address — exactly as anyone holding a DDNS
hostname resolves theirs. It is not worse. Specifically:

- BEP44 targets derive from the public key, so the keyspace is **not enumerable**
- Records expire within hours, so an offline node leaves no trail
- pkarr uses **BEP44 mutable items, not BEP5 infohash announcements** — a node never appears
  as a peer for any content and must never be made to
- Publishing is optional and separately disableable; a LAN-only node should not publish

For better than DDNS-equivalent, publish under a key derived per period from the cluster key.
An observer who captures the key once then cannot track the node indefinitely.

### Direct addresses are opt-in, and publishing them is the trade

The transport publishes only the home relay address by default. Direct addresses require
`include_direct_addresses`, and **the account-free remote path needs them** — a relay-only
record means relayed traffic, which is exactly what hole punching exists to avoid.

The trade is explicit: publishing direct addresses associates the node's public key with
its current IP address, for anyone holding the key. This is the same exposure as the
dynamic-DNS comparison above rather than a new one, but it is a separate switch and should
be presented as one. A LAN-only node should publish nothing.

### One operational note

DHT traffic resembles BitTorrent to deep packet inspection and is blocked on many corporate
and campus networks. Nothing is being shared, but on a managed network the traffic pattern
alone may draw attention. Owners in those environments should disable `discovery.pkarr` and
rely on DNS discovery.

## 4. Property 1: the honest gap

Every load over plain HTTP is exposed to client replacement, including the stored
device keys. An active on-path attacker can replace the bootstrap page and JavaScript
on any visit. The replacement runs under the same origin and can read the keys the
browser kept in localStorage, impersonate the paired device and read its data. Pairing
does not have to be open, and the attacker need not have been present at first pairing.

The bootstrap's integrity hashes arrive over the same HTTP connection. An attacker can
replace those too. Pinning a cluster key protects a genuine client from a substituted
node; it cannot make a replaced client check that key. This differs from SSH, where the
installed client executable is already trusted.

With genuine client code, the encrypted channel protects application data from passive
listeners. Scripts and stylesheets recreated by that client receive integrity hashes
from authenticated channel bytes for same-origin resources. A permitted remote
resource needs a canonical integrity hash of the declared digest length in the
authenticated HTML; malformed hashes are refused before the resource is loaded. Imported JavaScript
modules, including framework modules, have no per-import integrity. Neither protection authenticates the next bootstrap.

The plain-HTTP path still needs no domain, account or certificate setup. The pairing
screen a new device sees and the node's code page both disclose the risk, in the same
words, and the risk applies to every visit. A paired device sees no notice between pages:
the bootstrap it loads on each navigation shows nothing of its own until the requested
page arrives. A signed native client or an independently authenticated transport on every
visit closes this gap; protecting only initial pairing does not protect later HTTP loads.

## 5. Why there is no verification screen

An earlier draft included a short-authentication-string step: both screens show three
emoji, the human confirms they match.

It was removed because **a PAKE already does that job**. SAS exists in ZRTP and Signal
because their Diffie-Hellman is unauthenticated — there is no shared secret, so a human has
to supply the authentication out of band. Privatium has a shared secret: the pairing code.
An attacker without it cannot complete the handshake at all.

And against the one attack the PAKE cannot stop — the substituted client in §4 — a SAS is
useless, because the attacker's client displays whatever numbers it wants.

So the SAS would have been a screen that adds friction, teaches users to click through
security dialogs, and protects against nothing. It is normatively forbidden
(`spec/protocol.md §7.6`).

## 6. Pairing code strength

16 bits, node-generated, single-use, 120-second TTL, 5 attempts maximum, rate-limited to
one attempt per two seconds.

An online attacker gets 5 guesses out of 65,536 — a 0.008% success rate — and must be
attacking during a window the owner deliberately opened. Because the code goes through a
PAKE rather than being sent as a bearer token, there is no offline dictionary attack: each
guess costs a full network round trip. A guess is counted the moment the node answers a
device's first message, because that answer already reveals whether the code matched; a
device that never sends its confirmation has still spent one, and five spent guesses
replace the code inside the same window. A guess is against the code it was made
against: once five failures have replaced the code, or another device has paired, a
confirmation still on its way is refused and nothing is sealed for it. A device that
opens the pairing socket and then says nothing is dropped after thirty seconds, with no
guess counted.

For comparison, a 6-digit banking OTP is roughly 20 bits with far worse ergonomics and
frequently a 5-minute window.

The code is never owner-chosen. Owner-chosen codes get reused, get written on the monitor,
and get set to `0000`.

## 7. App folders are trusted-ish code

Installing an app folder means running its SQL on your node. The sandbox
(`spec/app-contract.md §7`) blocks the dangerous part — a connection that can `ATTACH`
can read `identity/node.key` into a table — but within its own tables an app sees
everything.

Therefore:

- App SQL runs on a read-only, `query_only` connection behind an authorizer that refuses
  every write, every `PRAGMA`, `ATTACH` and extension loading. These are hard requirements,
  not defaults to be overridden, and the framework's own connection is never handed out.
- Apps may read `sys.v_*` views but MUST NOT write to `sys` tables.
- The install flow warns the owner in the same terms you would warn them about running a
  script from a stranger.
- There is no app registry in `pv/1`, deliberately. A registry implies curation, curation
  implies a trust signal, and a false trust signal is worse than none.
- Apps share the node's origin. Script from an installed app running in the owner's own
  browser on the node has the owner's standing there: it can open pairing through
  `/api/v1/pair` or submit the devices page's forms, as the owner could. That is one more
  reason to treat an app folder as code you chose to run, not a boundary the framework
  draws for you.

## 8. Revocation

Revoking a device sets `revoked_at` on its `sys_device` row. Because that row is
replicated, revocation propagates to every node. Sessions are checked per request, so
revocation is immediate on any node that has received the event.

Revocation is not a delete. The record of what was paired and when survives permanently —
that history is a security feature.

**Revoking a node** (`spec/protocol.md §2.3.4`) is the same button on the devices page,
on a row whose kind is a space. It marks the node's device row revoked and writes a
`sys_node_revocation` row beside it, both of which replicate; every node that holds them
refuses that node's channels and never admits its key again. The revoked node itself,
once the row reaches it, stops syncing, refuses every channel, and tells its owner; to
use that machine again, stop it, delete its `identity/` folder, start it, and admit it as
a new space. A phone or a browser that never syncs keeps trusting the revoked node's
certificate until that certificate expires, at most 180 days; an owner who needs a hard
cut rotates the cluster.

**Rotating the cluster** (`spec/protocol.md §2.3.5`) is a procedure, not a command. On
each node you keep: stop it, delete `identity/cluster.key`, `identity/cluster.pub` and
`identity/node.cert`, and start it again. Each now stands alone in a fresh cluster.
Admit them to one another as `docs/deployment.md §3` describes — a window on one, `pair
--join` from each other. Then, on the devices page, revoke every device row the old
cluster had — phones, browsers and nodes alike — because a row's keys are accepted by
every node that holds the row, whatever cluster it was paired into, and a node cannot
tell an old cluster's row from a new one on its own. Pair the devices again.

There is no key rotation in `pv/1` (`spec/protocol.md §2.3`). A compromised *node* key
means re-initializing the node and re-pairing every device. This is a known gap.

## 9. Data destruction

There is no "hard delete." `op: "del"` writes a tombstone; the original event remains in
the log forever, and that is intentional — an append-only log you can secretly rewrite is
not an append-only log.

The supported way to destroy data irrecoverably is to destroy `data/`, including every
copy your sync tool has made. Say that plainly in the UI. Anyone whose threat model
requires deniable deletion should not use an append-only system.

## 10. Reporting

Report suspected vulnerabilities to <privatium@mongefranco.com>. Do not open a public
issue for a security defect.

---

Copyright © 2026 Gabriel Mongefranco
