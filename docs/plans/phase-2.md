<!--
This file is part of Privatium
docs/plans/phase-2.md
Author(s): Gabriel Mongefranco
Created: 2026-09-05
Last Modified: 2026-10-04
Summary: Implementation plan for Phase 2 — other devices on the LAN: cluster identity, session
         cryptography, pairing, the encrypted browser channel, discovery, and the device
         registry. Non-normative. Where this plan and spec/ disagree, spec/ wins and this
         file is wrong.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Phase 2 Implementation Plan

Target: `docs/roadmap.md` Phase 2 — *other devices on the LAN*. Deliverable: open the app
on your phone by scanning a QR code.

## 0. How to use this

Read `AGENTS.md` in full first, then `spec/protocol.md §2, §6, §7, §8, §9`,
`spec/data-dictionary.md §3.1–§3.3, §3.6, §3.10`, `spec/data-api.md §2.1, §5`,
`spec/cli.md §2, §8`, `docs/security.md`, and `docs/decisions/0003-in-process-adapter.md`.
This plan is a work breakdown, not a substitute for the contract. `docs/plans/phase-1.md`
is the shape it follows and the record of what it builds on; that plan's §2.1 (loopback
only, no flag) and §2.2 (the node is the device) are the two decisions this phase retires.

One milestone per branch, one PR per milestone, in order — M14 to M19, continuing Phase 1's
numbering. A milestone is done when its checklist passes and its named tests are green on
all three platforms, not when it compiles. Write the named tests first; the milestone's
shape is in them. Do not start M(n+1) before M(n) merges.

Section 2 lists the decisions this plan makes that the spec did not. **All eleven are
decided**, and the spec carries every one; each section ends with where.
Section 3 is the record of the spec gaps found while writing this plan — all fixed, as
Phase 1's §3 was, so an implementer is not handed a specification they have been told is
wrong. A milestone edits those sections only where the code proves them wrong, in the PR
that finds it, with `skills/` regenerated in the same change (`docs/skills.md §7`).

The Phase 1 rule stands: do not invent CLI flags, `sys_*` column values, routes, or config
keys. Every one of those surfaces is specified. Where Phase 2 needs a new one, §3 names the
spec edit and the milestone makes it — never quietly.

---

## 1. Scope

### In

Cluster identity founded on this node; the X25519 static key; the session layer of
`protocol.md §8` in Rust and in browser JavaScript; pairing — the PAKE, the emoji and word
renderings of the code, `/ws/pair`, the device registry, revocation; the encrypted
application channel at `/ws`; the LAN bind and the auth policy that goes with it; mDNS
advertisement and browsing, the UDP responder; the settings pages for devices and pairing;
`privatium pair`, a real `--no-discovery`, and the QR code behind `--open`; the
documentation and skills that describe all of it.

### Out — do not implement, do not stub, do not leave TODOs referencing

Admitting a second **node** (a pairing that declares `kind = "node"` is refused naming
Phase 3), sync, node certificate renewal on sync, revocation of a node, pkarr, DNS
discovery, relays, iroh, onion services, native shells, `uniffi`, packaging, `privatium
firewall`, HTTPS and certificates, the PWA, attachments.

`start_sync` and `sync_now` stay `Error::Unimplemented`, now naming Phase 3 alone.

### The one-sentence test

If a Phase 2 change lets a second *node* hold this node's data, it is Phase 3's.

---

## 2. Decisions this plan makes — confirm before M14

Eleven, all decided. §2.1, §2.2, §2.3, §2.4, §2.5 and §2.8 change the wire or the
posture; §2.6, §2.7, §2.9 and §2.11 are shape; §2.10 follows from §2.1. Each ends with
the sections that now carry it.

### 2.1 The encrypted channel is an adapter over `core::handle`, and it carries everything a browser does on the LAN except the bootstrap set — DECIDED

`protocol.md §8.2` forbids skipping the session layer on plain HTTP; `§9.2` names `/ws`
"the encrypted application channel"; the roadmap's acceptance bullet is "Wireshark on the
LAN shows no plaintext application data". Nothing in the spec says how a server-rendered
HTMX page gets from the node to a browser through that channel. This is the decision that
shapes the phase.

**The design.** `/ws` is a WebSocket. After the `§8` handshake, every frame from the browser
is an encrypted, serialized HTTP request — method, path, headers, body — and every frame
back is an encrypted piece of the response: a head, then body chunks, then an end marker.
The node decodes a frame into the `Request` of ADR 0003, attaches the session's `Device`,
calls `Handler::handle`, and encrypts the `Response` as it streams. **The channel is a
second adapter in the same process**, beside the socket adapter, and adds no route of its
own — which is exactly what ADR 0003 exists for. Requests on one channel run concurrently
and answers interleave by id, so an SSE stream and a page navigation share a connection.

**What a browser on a plain-HTTP LAN origin fetches in the clear** is the *bootstrap set*
and nothing else: `GET /` and every other page path, which answer the same bootstrap page
— no app data, the client script, a `<noscript>` explanation, and the path that was asked
for; `/static/*`, the framework's own embedded assets; a mounted app's `static/` and `web/`
files; `/api/v1/health` and `/api/v1/manifest`; and the two WebSocket routes. Everything
else — every page, every fragment, every form post, the whole data API and its stream —
is refused on plain HTTP from a non-loopback peer with 403, and is reachable only through
the channel. A Tier 2 app's own `fetch('/a/x/api/...')` is therefore refused on that
origin; `pv.js` routes through the channel and is the way, which §3 row 7 writes down.

**The browser side** is `client.js`, an ES module served from `/static/`, with
`@noble/curves`, `@noble/ciphers` and `@noble/hashes` vendored beside it as ES modules
(`AGENTS.md`, browser crypto). It holds the device keys, runs the handshake, fetches the
real page through the channel and parses it in the fresh bootstrap document. HTMX
requests and the data API stay on the channel. Full-page navigation creates a new
bootstrap document with the destination app's declared CSP and a fresh module map.
`pv.js` uses the channel when present and ordinary `fetch` on loopback or an exempt
transport, as `spec/data-api.md §5` promises.

For a full-page form response, the node reserves capacity before dispatch, calls
`handle` once and retains the unpolled response stream in RAM. Only an opaque reference
and destination metadata cross the document transition in per-tab storage. A newly
authenticated channel for the same active device consumes the response once; it never
executes the request again. Capacity is 32 responses per node and 4 per device; a ready
response expires after 120 seconds. There is no form-body storage, response disk cache,
event acknowledgement or deduplication table. Loss of the response leaves any committed
write intact and requires checking the result before resubmission.
`protocol.md §8.3.1` is normative.

**Scripts and stylesheets stay plaintext, and a genuine client pins them.** A page delivered through the
channel names its scripts with `<script src>`; the browser fetches those over plain HTTP,
which permits substitution on the network. A genuine client uses Subresource Integrity:
for every same-origin external script and stylesheet element it re-creates, it
hashes the file through the channel and sets `integrity` on the plain element. A remote
resource allowed by the app's existing permissions requires a hash in the authenticated
HTML; the channel does not become a remote proxy. The browser refuses differing bytes. No CSP change, no `blob:` scripts. This does not authenticate the downloaded
client or its bootstrap on any visit, and does not protect imported framework or app
modules; §2.10 states the exposure, including stored device keys.

The rejected alternatives, so they are not re-litigated: a service worker (unavailable on
a LAN IP, ADR 0003); encrypting bodies over plain HTTP requests without a WebSocket (the
same channel with worse framing, and `§9.2` already names `/ws`); loading app scripts from
`blob:` URLs decrypted in the page (needs `script-src blob:`, a one-way CSP widening).

*Decided. `protocol.md §8.3` is the channel's normative form — the handshake, the frame,
the request and response kinds, the integrity rule — and `§8.4` is the bootstrap set;
`§7.7`, `§9.1` and `§13` follow it, and `data-api.md` says where a plain `fetch` works.
`docs/security.md §3–§4` and `docs/architecture.md §2.6` describe it. M15 and M17 hold
the code to those sections and edit them only where implementation proves them wrong.*

### 2.2 The node binds every interface, and adds no flag to say so — DECIDED

Phase 1 bound loopback because it had no session layer; Phase 2 has one, so the bind is
`0.0.0.0` and `[::]` on `[node] port`, with the IPv6 listener skipped where the platform
refuses it. No `--bind` flag, for the reason Phase 1's §2.1 gave: `spec/cli.md §10` keeps
the surface narrow and a bind address is a property of the phase. Startup prints the LAN
URL — the address of the interface the default route uses, found by connecting a UDP
socket to a public address without sending anything — and every other interface under
`--verbose`.

**A loopback request keeps Phase 1's meaning.** The owner at the keyboard is this node's
own device: no pairing, no session, every route, exactly as `docs/plans/phase-1.md §2.2`
made it, with the `Host` check against DNS rebinding unchanged. A native shell and an
embedder's in-process call keep the same standing. Only a non-loopback peer meets the
policy of §2.1.

The first non-loopback bind is where Windows Defender prompts (`docs/deployment.md §4`);
the helper that opens the port is Phase 6, and the prompt is documented rather than
worked around.

*Decided: `cli.md §2` and `protocol.md §8.4`.*

### 2.3 The cluster is founded now, and devices pin the cluster key from the first pairing — DECIDED

`protocol.md §2.3.2` has devices pin the *cluster* public key, and `docs/roadmap.md` Phase
3's first bullet — a second node admitted with one pairing, the phone reaching it
without re-pairing — only holds if the phone pinned the cluster key in Phase 2. So M14
founds the cluster on the first Phase 2 start: `identity/cluster.key` (`0600`),
`identity/cluster.pub`, this node's `identity/node.cert` signed by the cluster key, the
`sys_cluster` row, and `cluster_id`, `cert` and `cert_expires_at` on `sys_node`. A root
Phase 1 created has none of these and is founded on its next start; that is the ordinary
first Phase 2 run, not a migration.

The certificate's signed bytes are the canonical form of §3 row 4. The founding node
renews its own unexpired certificate whenever fewer than ninety days remain, at start;
an expired certificate is refused and requires re-admission. Renewal on
sync (`§2.3.1`) is Phase 3's, since a sync is.

A node that founded a cluster alone can still be admitted to another one later: while it
has paired nothing and admitted nobody its cluster is empty and disposable, and joining
discards it and tombstones only its own empty cluster's `sys_cluster` row. That keeps
"found at first start" from making every node the first node.

The public registry can contain other node and cluster records after restore or sync.
Verified local identity selects this installation's current records; startup preserves
the others. Missing local keys do not authorize retiring another cluster. Public records
do not replace pairing, admission or certificate verification. Section 3 row 20 records
this correction to the original singleton wording.

*Decided: `protocol.md §2.3` and `§2.3.1`, `data-dictionary.md §3.1b`.*

### 2.4 The PAKE is SPAKE2 as RFC 9382 specifies it, over edwards25519, written on both sides from vetted primitives — DECIDED

`§7.4` allows CPace or SPAKE2 over X25519/Ristretto255. The browser side has to be
JavaScript either way, and no audited JavaScript PAKE exists; the Rust `spake2` crate
follows the CFRG draft's own constants rather than the RFC's and has no JavaScript
counterpart; the `cpace` crate is a 2020 snapshot of a draft that has moved since. So
both sides are written to **RFC 9382 §3**, with the RFC's `M` and `N` for edwards25519,
its transcript `TT` and its confirmation MACs — in Rust over `curve25519-dalek`, already
in the graph beneath `ed25519-dalek`, and in JavaScript over `@noble/curves`. Neither side
hand-rolls a primitive: the group arithmetic, the hashes, the KDF and the MAC are the
libraries'. What is written is the protocol, and it is held together by a vector file the
Rust side generates and both sides' tests read (`tests/fixtures/pake-vectors.json`),
because the RFC's own vectors cover P-256 only.

The identities bind both static keys, which `§7.4` step 4 requires: `A` is
`"pv/1 device " ‖ device Ed25519 public key (base64)`, `B` is `"pv/1 node " ‖ node Ed25519
public key (base64)`. The password `w` is `HKDF-SHA256(ikm = the two code bytes, big
endian; salt = ""; info = "pv/1 pake w")`, 64 bytes reduced modulo the group order.

*Decided. `protocol.md §7.4` names SPAKE2 alone and strikes CPace; `§7.4.1` fixes the
ciphersuite, the identities, `w`, the transcript encodings and the key schedule; `§7.4.2`
is the message sequence M16 implements. R9 stands: the vector file is what holds the two
implementations to one another.*

### 2.5 The word list is 256 words from the EFF short wordlist, checked in as normative — DECIDED

`§7.2` named "the 256-word list" and no list existed anywhere. The list is wire meaning —
`amber otter` must decode to the same sixteen bits on every implementation — so it is a
spec artefact, `spec/pairing-words.txt`, one word per line, index order normative, and
changing it is a breaking protocol change exactly as `§7.3` says of the glyphs. The words
come from the EFF Short Wordlist 2.0 (1,296 words, every one distinct in its first three
letters and at edit distance three from every other, which is what lets a screen-reader
user abbreviate and lets a typo be caught rather than mis-decoded): the words of four to
six letters, in alphabetical order, the first 256, with three words unsuited to saying
aloud skipped on review. A four-or-five-letter rule yields only 193 words, which is why
the rule says six.

*Decided, and the file is written — `spec/pairing-words.txt`, `abyss` first.
`protocol.md §7.2` names it and `NOTICE` attributes it (CC BY 3.0 US).*

### 2.6 The node's X25519 static key is derived from the node key, as the CSRF key is — DECIDED

`§7.4` and `§8` use an X25519 static key; `§3`'s layout has no file for one. Rather than
add a file the spec does not show, M14 derives it: `HKDF-SHA256(ikm = node private key,
info = "privatium/x25519/v1")`, the shape `docs/plans/phase-1.md §2.2` chose for the CSRF
key, with a new `info` string because one purpose is one string. Deterministic, never
stored, wiped on drop. A browser device generates a real X25519 keypair beside its
Ed25519 one, since it has storage to keep both in.

*Decided: `protocol.md §8`.*

### 2.7 Pairing state lives in memory, and there is no Argon2 — DECIDED

`data-dictionary.md §3.3` gives `sys_pairing` a `code_hash` "so that a crash dump or stray
log does not contain a live code". The node has to hold the PAKE secret `w` for the whole
window to answer the handshake at all, so hashing the code beside it protects nothing.
M16 keeps one pairing at a time in memory — the code, `w`, `created_at`, `expires_at`,
`attempts`, `consumed_by` — and writes nothing to disk: `local/` keeps its two files
(`§3`), and a code that never touched a file needs no hash. §3 row 6 amends `§3.3` to
say so. No `argon2` crate. The window keeps the code's bytes rather than dropping them
once `w` exists; row 23 records why — the window is shown again on request, and `w` is a
function of the code.

*Decided: `data-dictionary.md §3.3`.*

### 2.8 `privatium pair` asks the running node over loopback, and `--open` on a node with no paired device opens pairing once — DECIDED

A data root is one process's (`§3.1`), so `privatium pair` cannot open the node the
daemon holds; `spec/cli.md §1` already lists it among the commands that take no lock. It
therefore talks to the running node: `POST /api/v1/pair` opens a window and returns the
code, `GET /api/v1/pair` reports it, both answered for this node's own device only —
loopback, in Phase 2 — and both spec edits (§3 row 5). Without a running node, `pair` is a
runtime error saying to start one. The settings page's button posts the same act as a
form.

`§7.1` allows pairing to open on "a button press, a CLI flag, or first-run". This plan
reads *first-run* narrowly: `privatium --open` on a node whose `sys_device` holds no row
but its own opens one pairing window as it starts, prints the code beside the QR, and
never does so again once any device is paired. That is what makes the quick start "run
it, scan it, tap four emoji" true with one command, and it is a posture decision to
confirm. `--open` on a node that has a paired device prints the QR and opens the browser
and opens nothing else.

The QR code encodes the node's LAN URL and nothing more — never the code. The code is on
the screen for the person standing there, which is `§7.1`'s authorization.

*Decided: `protocol.md §7.1` and `§9.2`, `cli.md §2` and `§8`.*

### 2.9 Browser keys live in `localStorage` under the origin, and a browser without JavaScript cannot pair — DECIDED

`§2.2` allows `localStorage` or IndexedDB. `pv.js` already keeps the outbox in
`localStorage`; the device keys, the pinned cluster public key and the node's identity go
beside it under one key, `pv:device`, and loss means re-pairing with no other path
(`§7.6`). The bootstrap page carries a `<noscript>` block saying that pairing needs
JavaScript and that the node at the keyboard needs none — every Phase 1 no-JavaScript
path holds on loopback exactly as before, since loopback never sees the channel.

*Decided: `protocol.md §7.6` and `§8.4`.*

### 2.10 What is claimed about program authenticity — DECIDED, corrected

M17 keeps §2.1's channel design and corrects its security claim.
Every load over plain HTTP permits active replacement of the bootstrap and client,
including access to stored device keys after pairing. The bootstrap's integrity hashes
can be replaced with it. A genuine client refuses a substituted node and protects
application data from passive listeners; this is conditional on genuine client code.
No blanket detection claim applies to an active attacker on the plain-HTTP path.

`protocol.md §7.7`, `docs/security.md §4` and the security skill state this exposure.
The disclosure is worded for every visit and appears in the bootstrap and, with M19,
the pairing screen. Imported framework and app modules also lack per-import integrity.
No TLS requirement, new origin, recovery path or architecture change is introduced.

### 2.11 Two small shapes: `peers`, and what `--version` claims — DECIDED

`spec/lua-api.md §3.4` calls `pv.node().peers` "the number of paired peers" and
`spec/data-api.md §4` calls `/api/node`'s the "sync peer count". Both mean **paired
nodes** — active `sys_device` rows with `kind = 'node'` other than this one — so both stay
`0` through Phase 2 while browsers appear on the devices page (§3 row 10).

`privatium --version` prints `pv/1 (partial: phase 2)` from M19: the `§13` items Phase 2
cannot claim are all sync's and the remote transports' (§7).

*Decided: `lua-api.md §3.4` and `data-api.md §4`; the version string is `cli.md §1`'s
rule applied.*

---

## 3. Spec gaps found

Rows 1–39 are fixed. As in
Phase 1, this records what changed and why;
`cargo xtask gen-skill-reference` ran with the edits.

| # | Was | Proposed | Files | Milestone |
|---|---|---|---|---|
| 39 | `§9.3` gave the embedded assets `Cache-Control: no-cache` and nothing to revalidate with, so a browser downloaded the client, the channel code and the hashing library again on every navigation, and the bootstrap stayed on screen for the download | Every embedded asset, skill document and the skill bundle carries a strong `ETag` and answers `304` on its fixed path; the framework's own pages name assets under a content-addressed `/static/<build>/` prefix that is cached for a day, which a new build cannot serve stale because the prefix changes with the asset set | `protocol.md §9.3` | **Fixed**; after M19; `test_spec_9_3_assets_revalidate_by_etag_and_the_addressed_path_caches_for_a_day`, `test_spec_9_3_assets_are_addressable_by_build_and_carry_an_etag` |
| 38 | The `§7.7` notice was written for a security reader: "plain HTTP", "change network traffic", "replace this client", "stored device keys", "verify the downloaded client". The owner it is shown to found it too technical | The notice is reworded for the owner: use Privatium on a network you trust, nobody can listen in, but on a shared network someone could change the pages and read the data. The spec keeps one requirement, that a changed page can read the data and that the risk is not presented as ending at pairing, and drops the "every visit" wording | `protocol.md §7.7` | **Fixed**; after M19; `test_spec_9_2_bootstrap_page_carries_no_app_data` holds the two facts |
| 37 | `app-contract.md §5.4` worded the `remote` permission as "this app phones out, the one thing this project exists to avoid", and the loader and the apps settings page repeated it under a heading of "Load warnings". The owner found the sentence untrue of an app that only reads public reference data and beside the point: what matters is which services the app can reach | Every permission widening is a **Privacy warning**, on the apps settings page and at startup; `remote` reads "this app uses the following online services: …" and lists each origin; warnings about an app's shape, such as an unknown icon, stay load warnings | `app-contract.md §5.4`, Tier 2 and security skills, generated `permissions.md` | **Fixed**; after M19; `test_csp_default_blocks_inline_handlers`, `test_spec_cli_5_pv4xx_shell_pages` |
| 36 | `§7.7` had the disclosure SHOULD-shown on the bootstrap document as well as the pairing screen, so a paired phone read "Connect to Privatium" and the disclosure as a flash between every two pages of every app | The disclosure belongs to the pairing screen and the devices page; the bootstrap MUST NOT present it, or any text of its own, as an interstitial on a paired device's navigation, and SHOULD show nothing until the page arrives. The reference bootstrap keeps its heading and status invisible for 1.5 seconds and the pairing screen lifts that when it has to appear | `protocol.md §7.7`, `docs/security.md §4` | **Fixed**; after M19; `test_spec_7_7_plain_http_pairing_page_discloses_the_gap`, `test_spec_7_7_pairing_screen_lifts_the_bootstraps_quiet_delay` (JavaScript) |
| 35 | `§8.3` refused a `dev` that is not an active row with an X25519 key; the node's own row is one, and nothing said a node never pairs with itself. Neither `§7.4.2` nor `§8.3` bounded a peer that opens a socket and falls silent | The node's own ID is refused before its hello is answered; a node SHOULD bound the handshake and close a silent peer with nothing counted — ten seconds on `/ws`, thirty on `/ws/pair` in the reference node | `protocol.md §7.4.2, §8.3` | **Fixed**; hardening; `test_spec_8_3_the_nodes_own_row_cannot_open_a_channel`, `test_spec_7_4_a_silent_pairing_peer_is_closed_without_an_attempt` |
| 34 | `§6.1` keyed discovered nodes on `id` and said nothing about a record whose `id` is not an ID, a name past 63 bytes, a value that is not a slug, or a flood of invented records | A record whose `id` or `cl` is not shaped as an ID is not a `pv/1` record; `nm` is read to 63 bytes; `apps` keeps slugs and the marker alone, to a stated bound; the list of nodes seen is bounded | `protocol.md §6.1` | **Fixed**; hardening; `test_spec_6_1_a_record_off_the_wire_is_validated_and_bounded` |
| 33 | `§6.4` limited answers per source only; a probe is twelve bytes from an unverified address and an answer a kilobyte, so a flood of spoofed private sources was an amplifier | Nodes SHOULD bound the answers sent in any second across every source and the sources remembered; a known source keeps its answer while strangers are refused | `protocol.md §6.4` | **Fixed**; hardening; `test_spec_6_4_a_probe_storm_is_bounded_by_a_global_budget_and_a_source_cap` |
| 32 | `§7.4.2` said when an attempt is counted and what a peer that leaves is, but not what becomes of an attempt whose `pA` was accepted under a code that was then replaced, or a window that was then consumed or expired, before its `cA`; the node answered such a `cA` and sealed | An accepted `pA` is an attempt against that code and that window alone: refused at `cA` with 4429 after a replacement and 4404 after consumption or expiry, before the message is read and before anything is sealed; a sealed message after the window closed is refused before it is opened; each is one audited failure | `protocol.md §7.4.2` | **Fixed**; hardening; `test_spec_7_5_an_attempt_begun_before_a_rotation_cannot_finish_after_it`, `test_spec_7_4_a_second_attempt_cannot_finish_once_the_window_is_consumed_or_expired` |
| 31 | `§3.2` had `last_seen_at` "written at the channel handshake" unconditionally; since M17 every full-page navigation opens a channel of its own, so that sentence meant one `sys_device` event per page view | Written at the handshake only when the row holds no mark or one more than an hour old, and by a request an hour or more after the last write — at most hourly, never per request, never per page navigation. The node decides (`Node::note_device_seen`, the instant passed in) and the channel asks at the handshake and on every request | `data-dictionary.md §3.2` | **Fixed**; M19; `test_spec_3_2_last_seen_at_is_written_at_most_hourly` |
| 30 | The data root was the platform directory or `--data-dir`, nothing else; on Windows that directory is hidden, so owners could not find their apps, and a zip download had no way to keep everything in one folder | Three sources, most explicit first: `--data-dir`; a `privatium-data` folder the owner created beside the executable (portable mode — the program never creates it, and one that cannot be written is a runtime error, never a fall-through); the platform directory. Every start prints the root and the rule that chose it; the data page shows the same. The Windows release gains `privatium-windows-portable.zip` carrying `privatium-data/apps/` with the three examples | `cli.md §1`, `protocol.md §3`, `AGENTS.md` invariant 7, `README.md`, `docs/backup-and-restore.md §1`, Tier 3 skill, `release_tools.py`, `release.yml` | **Fixed**; between M18 and M19 |
| 29 | `cli.md` had a release binary start with an empty launcher — the reference apps existed only in a checkout — and `new --from hello` failed without one | The binary carries the three example apps; a start whose `apps/` holds no app folder writes them there, whether the data directory is new or was used before the binary carried them, `new --examples` writes them on request, `--from` finds the embedded copy, and a checkout keeps mounting its own `apps/` as `bundled` and writes nothing | `cli.md §2, §4`, `data-dictionary.md §3.4`, `apps/README.md`, `README.md`, overview skill | **Fixed**; M18 |
| 28 | The integrity rule assumed every resource could be fetched by an origin-local channel | Hash same-origin external resources through the channel; require an existing integrity hash in authenticated HTML for permitted remote resources; keep inline script CSP and imported-module limits explicit | `protocol.md §8.3`, this plan §2.1, security and Tier 2 skills | **Fixed**; M17 |
| 27 | Full-document replacement and `pushState` retain the previous CSP and module map | Fresh bootstrap documents use destination app permissions. A bounded, unpolled response stream stays in node RAM across a form transition; only a reference crosses in per-tab storage. Same-device attachment consumes it once without re-executing the request | `protocol.md §8.3.1`, this plan §2.1; `app-contract.md §5.4` remains binding | **Fixed**; M17 |
| 26 | The active-attacker gap was described as first pairing and Tier 2 imports only | State that every plain-HTTP load can replace the bootstrap and client, exposing stored device keys; keep the channel design and word the disclosure for every visit | `protocol.md §7.0, §7.7`, `docs/security.md §1, §4`, this plan §2.10, security skill | **Fixed**; M17 |
| 25 | `§7.3` row 10 gave the flamingo's codepoint as U+1FAB0, which is 🪰; the glyph and the label in the same row were right | U+1F9A9, which both implementations already emit; the conformance test now reads the table's glyph, codepoints and label and holds all three to the code | `protocol.md §7.3` | **Fixed**; M16; `test_spec_7_3_glyph_table_is_normative_and_keeps_variation_selectors` |
| 24 | `§7.4.2` never said when an attempt is counted. Counting at `cA` lets a client guess for free: the node's answer to `pA` already tells it whether the code matched, and it need never send `cA` | An attempt is counted when `pA` is accepted; an exhausted code is replaced before the 4429; a refusal before `pA` — closed, rate-limited, malformed — is no attempt and writes no audit row; a peer that leaves after `pA` is a failed attempt the transport reports; a registered device key is refused with 4403 | `protocol.md §7.4.2, §7.5` | **Fixed**; M16; `test_spec_7_5_code_expires_at_120s_and_five_attempts_issue_a_new_one` |
| 23 | `§3.3` had the node drop the code's bytes once `w` existed, but `§9.2`'s `GET /api/v1/pair` answers the code again on request, and `w` is a function of sixteen bits, so dropping the code hides nothing | The window holds the code beside `w`; `generation` counts replaced codes so a surface can notice a new one | `data-dictionary.md §3.3` | **Fixed**; M16 |
| 22 | `§7.4.1` reduced 64 HKDF bytes "modulo the order" with no byte order | Little-endian, as RFC 8032 reads a scalar and as both curve libraries do | `protocol.md §7.4.1` | **Fixed**; M16; `test_spec_7_4_spake2_matches_the_checked_in_vectors` |
| 21 | The plan requires unmodified Noble modules, but their bare `@noble/hashes/...` imports cannot resolve in a browser without a build step or import map | Permit import-path-only edits to relative URLs. Keep cryptographic code unchanged and record upstream and vendored SHA-256 hashes, archive integrity, and original licences | This plan §5; `assets/shell/vendor/noble/VENDOR.md` | **Fixed**; M15; protocol and CSP unchanged |
| 20 | The global singleton wording for `sys_node` and `sys_cluster` conflicts with restored and replicated identity records | Preserve all restored records. Verified local keys and certificate select this installation's current node and cluster; startup appends corrections to their public identity fields without retiring other clusters. Registry presence alone establishes no cluster trust | `data-dictionary.md §3.1, §3.1b`, `protocol.md §2.3` | **Fixed**; M14; `test_spec_3_1b_data_only_restore_preserves_records_and_selects_local_identity`, `test_spec_3_1b_restored_keys_select_the_original_cluster`, `test_spec_3_1b_replayed_rows_cannot_change_local_cluster_identity` |
| 19 | `§2.3.1` requires re-admission after expiry but startup renewal has no expiry exception | Startup renewal applies only to unexpired certificates; at or after expiry the node refuses self-renewal and requires re-admission | `protocol.md §2.3.1` | **Fixed**; M14 |
| 1 | `§7.4` gives the handshake's six steps and no message shapes, encodings or close codes | The messages of M16 spelled out: the node's hello, the client's `pA` with its identity, the node's `pB` and `cB`, the client's `cA`, the two key exchanges over `K_pair`, and the WebSocket close codes for *closed*, *wrong code* and *exhausted* | `protocol.md §7.4.1, §7.4.2` | **Fixed**; M16 |
| 2 | `§8` gives the key schedule and says nothing about how a session starts on `/ws`, what a frame is, or what a request or response looks like inside one | The handshake messages, the frame — `nonce = direction ‖ counter`, one AEAD ciphertext per WebSocket binary message, no associated data — the request and response frames of §2.1 with their `id` and `kind`, the confirm frame, and the rule that a side closes at 2³² frames rather than rekeying | `protocol.md §8.3` | **Fixed**; M15, M17 |
| 3 | `§7.2` says "the 256-word list" and no list exists | `spec/pairing-words.txt`, index order normative, produced by the rule in §2.5; `§7.2` names it and says a change is a breaking protocol change | `protocol.md §7.2`, `spec/pairing-words.txt`, `NOTICE` | **Fixed**; M16 |
| 4 | `§2.3.1` signs "the other fields" of the certificate and never says which bytes | The signed message is the JSON object `{"node_id","node_pub","cluster_id","issued_at","expires_at"}` in that key order, no whitespace, UTF-8; `sig` is base64 of the Ed25519 signature; the certificate is that object plus `sig`, and `sys_node.cert` holds it base64-encoded | `protocol.md §2.3, §2.3.1`, `data-dictionary.md §3.1b` | **Fixed**; M14 |
| 5 | `§9.2` has no route that opens pairing; `cli.md §8` does not say how `pair` reaches a running node, or what happens without one | `POST /api/v1/pair` (`{"ttl": seconds}`, answers the code, the URL and `expires_at`) and `GET /api/v1/pair` (the open window or `null`), this node's own device only; `pair` uses them and is a runtime error with no node running; `--open`'s first-run window per §2.8 | `protocol.md §9.2`, `cli.md §2, §8` | **Fixed**; M16, M19 |
| 6 | `§3.3` `sys_pairing` holds an Argon2id `code_hash` and a `salt` | The row is held in memory by the node for the window and never written; it holds the PAKE secret rather than the code; the hash and salt columns go | `data-dictionary.md §3.3` | **Fixed**; M16 |
| 7 | `data-api.md` says "Cookies carry [the session]" and `§5` says "`fetch` works fine" | On a plain-HTTP origin from a non-loopback address the session is the channel and nothing else: no cookie, and a plain `fetch` of the API is refused, naming `pv.js`; on loopback, in a native shell and on an origin `§8.2` exempts, `fetch` works as written | `data-api.md` preamble, `§5` | **Fixed**; M17 |
| 8 | `§7.7` says "any later substitution is refused (§8.1)"; `§8.1` is about keys, and a script on plain HTTP is not a key | `§7.7` says what is pinned after pairing — the session, and every file the page names, by integrity — and what is not, per §2.10; `docs/security.md §4` carries the same | `protocol.md §7.7`, `docs/security.md §4` | **Fixed**; M17 |
| 9 | `§9.1` reserves five prefixes; `/ws` and `/ws/pair` are routes of `§9.2` and `ws` a reserved slug of `§1.1`, but `/ws` is in no prefix table and the router does not know it | `/ws` joins the table as the framework's; the reserved slug already covers the mount | `protocol.md §9.1` | **Fixed**; M17 |
| 17 | `§13` had no line for what plain HTTP may serve, or for the integrity rule | Two items, `§8.4` and `§8.3` | `protocol.md §13` | **Fixed**; M17 |
| 18 | `§7.4` step 5 had the client pin "the node's key" while `§2.3.2` and `§7.6` pin the cluster's | Step 5 says the cluster public key and the node's certificate | `protocol.md §7.4` | **Fixed**; M16 |
| 10 | `lua-api.md §3.4` `peers` "paired peers"; `data-api.md §4` "sync peer count" | Both are the paired nodes, per §2.11 | `lua-api.md §3.4`, `data-api.md §4` | **Fixed**; M19 |
| 11 | `§6.4` refuses a probe "from outside RFC 1918 / RFC 4193 space"; loopback is neither, and the responder's test has nowhere else to probe from | Loopback and link-local are accepted too | `protocol.md §6.4` | **Fixed**; M18 |
| 12 | `§7.1` allows pairing to open on "first-run" and says nothing about what that is | §2.8's definition | `protocol.md §7.1`, `cli.md §2` | **Fixed**; M19 |
| 13 | `app-contract.md §6` lists `pair` and the Phase 1 signature is `pair(&mut self) -> Result<()>`, which cannot hand back a code | `pair(&mut self, ttl: Duration) -> Result<Pairing>`; `serve_discovery(&mut self) -> Result<()>` stays | `app-contract.md §6` | **Fixed**; M16 |
| 14 | `cli.md §2` "prints the LAN URL" — a machine has several | The default route's, and the rest under `--verbose` (§2.2) | `cli.md §2` | **Fixed**; M17 |
| 15 | `§3.2` `sys_device.replica` for a browser is defined; `last_seen_at` "at most hourly" names no writer | The channel handshake writes it, and a session older than an hour writes it again on its next request | `data-dictionary.md §3.2` | **Fixed**; M19 |
| 16 | `§6.1` instance name is "the owner-set display name" and no surface sets one | The node settings page sets `sys_node.display_name`; while unset the Node ID stands in, as `§9.2` already says | `protocol.md §6.1` | **Fixed**; M19 |

Two are additions rather than corrections and deserve to be called out: **`/api/v1/pair`**
widens `§9.2`, and **`spec/pairing-words.txt`** is a new normative file (§2.8, §2.5).

**The rule from Phase 1 stands:** when implementation reveals a further gap, fix the spec
in the PR that found it. Do not accumulate a list and do not code around it.

---

## 4. Workspace layout — what Phase 2 adds

```
crates/privatium-core/
├── src/
│   ├── identity.rs           + cluster keypair, node certificate, the X25519 static (M14)
│   ├── session/
│   │   ├── mod.rs            key schedule, frames, the nonce discipline (M15)
│   │   └── handshake.rs      the /ws handshake, both roles (M15, M17)
│   ├── pair/
│   │   ├── mod.rs            the pairing window: state, TTL, attempts, the refusals (M16)
│   │   ├── code.rs           the 16-bit code, glyphs, words, parsing (M16)
│   │   ├── spake2.rs         RFC 9382 over edwards25519 (M16)
│   │   ├── handshake.rs      the six messages of /ws/pair, both roles, as data (M16)
│   │   └── node.rs           Node::pair and the handshake driven against the window, with the audits (M16)
│   ├── discover/
│   │   ├── mod.rs            serve_discovery: the two mechanisms started together (M18)
│   │   ├── txt.rs            the TXT record and its budget (M18)
│   │   ├── mdns.rs           advertise and browse (M18)
│   │   └── udp.rs            the 52525 responder and probe (M18)
│   ├── wire/channel.rs       /ws and /ws/pair: frames in, handle, frames out (M17)
│   ├── http/auth.rs          the Phase 2 policy (M17)
│   ├── http/devices.rs       the devices page's actions (M19)
│   └── http/pairing.rs       the code page, the QR, the bootstrap page (M16, M19)
├── assets/shell/
│   ├── session.js            the key schedule, the frame and the client handshake (M15)
│   ├── pair.js               the code, SPAKE2, the device's side of /ws/pair, pv:device (M16)
│   ├── client.js             keys, handshake, channel, the htmx extension, pairing UI (M17, M19)
│   ├── pair.css              the pad and the code page (M19)
│   └── vendor/noble/         @noble/curves, ciphers, hashes as ES modules, with VENDOR.md (M15)
└── tests/
    ├── identity.rs           + the cluster tests (M14)
    ├── session.rs            (M15)
    ├── pair.rs               (M16)
    ├── channel.rs            through handle, with a faked peer (M17)
    ├── discover.rs           (M18)
    ├── fixtures/session-vectors.json, pake-vectors.json   generated by Rust, read by both (M15, M16)
    └── js/session.test.mjs, pake.test.mjs, pair.test.mjs, client.test.mjs   under node --test (M15–M17)
crates/privatium/
├── src/lib.rs                the bind of §2.2 (M17)
├── src/pair.rs               privatium pair (M19)
└── tests/channel.rs, pairing.rs   real sockets, a peer-faking service, a capturing proxy (M17, M19)
spec/pairing-words.txt        (M16)
```

`Node` is `Send` and not `Sync` (`wire/mod.rs`), and stays so. Discovery threads get a
`watch` channel of the facts the TXT record needs, which the node updates on every change
of its own — pairing opened or closed, apps loaded — so `serve_discovery(&mut self)` keeps
its signature. Nothing in Phase 2 needs to reach *into* the node from another thread; the
channel decoder calls `Handler::handle`, which already shares the node the way every
request does.

---

## 5. Dependencies

| Need | Crate / package | Version, licence | Note |
|---|---|---|---|
| X25519 | `x25519-dalek` | 3.0.0, BSD-3-Clause | `§8`; `static_secrets` feature for the derived static |
| AEAD | `chacha20poly1305` | 0.11.0, Apache-2.0 OR MIT | `§8` requires it over AES-GCM |
| Group arithmetic for SPAKE2 | `curve25519-dalek` | 5.0.0, BSD-3-Clause — already in `Cargo.lock` beneath `ed25519-dalek` | Taken directly for `EdwardsPoint` and `Scalar`; no new compile unit |
| KDF, MAC, hash | `hkdf`, `hmac`, `sha2` | already here | The salt of `§8`, the confirmation MACs of RFC 9382 |
| WebSocket in the core | `axum` feature `ws` | pulls `tokio-tungstenite` 0.29.0, MIT, plus `sha1` and `base64` | `/ws` and `/ws/pair` are routes of the core (`§9.2`); the upgrade is answered where `handle` is |
| WebSocket client, tests | `tokio-tungstenite` | 0.29.0 | A dev-dependency of `crates/privatium` for the socket tests only |
| Stream and sink helpers | `futures-util` | 0.3.34, MIT OR Apache-2.0 | Existing graph dependency, now direct for WebSocket and response streams |
| Separate IPv6 socket | `socket2` | 0.6.5, MIT OR Apache-2.0 | Existing graph dependency, now direct to set IPv6-only before binding; avoids platform-dependent dual-stack defaults |
| Verbose interface list | `if-addrs` | 0.15.0, MIT OR BSD-3-Clause | The standard-library UDP probe finds the default route but cannot enumerate every interface; the maintained cross-platform crate supplies that list |
| mDNS | `mdns-sd` | 0.21.2, Apache-2.0 OR MIT | Registers with TXT, browses, runs its own thread — no runtime dependency, so an embedder without tokio can call `serve_discovery`; one subtype per registration (R17); taken with `default-features = false` |
| QR | `qrcode` | 0.14.1, MIT OR Apache-2.0 | Renders to text for the terminal and SVG for the page; last released 2024-07 — check `cargo deny` and its issue tracker at M19 |
| Browser crypto | `@noble/curves`, `@noble/ciphers`, `@noble/hashes` | 2.4.0, MIT | ES modules vendored at `assets/shell/vendor/noble/`, including their import dependencies; only bare imports become relative URLs (§3 row 21). Original and vendored hashes, licences, `VENDOR.md` and a `NOTICE` entry accompany them. Loaded under `script-src 'self'`; no bundle is built here |

**Not taken, and why:** `spake2` (one group, the draft's constants, no JavaScript
counterpart — §2.4); `cpace` (0.1.0 from 2020); `argon2` (§2.7); `notify` (Phase 3
decides it does not need one either); a JavaScript QR library (the QR is rendered by the
node); `local-ip-address` for default-route selection (the UDP-connect trick needs no
crate). `if-addrs` is used only for the separate verbose enumeration requirement.

M17 enables the planned axum WebSocket feature and takes the stream and socket helpers
above directly. The [if-addrs package record](https://docs.rs/crate/if-addrs/0.15.0)
records the 2026-02-08 release and maintained cross-platform implementation. Its
`libc` and `windows-sys` dependencies already exist in the graph. `cargo deny check`
passes advisories, bans, licences and sources; the WebSocket dependency tree introduces
duplicate-version warnings alongside the existing `syn` warning. SHA-1 is used by the
standard WebSocket upgrade, never for Privatium's identity or session cryptography.

M14 adds only `x25519-dalek`, for the static session identity required by `protocol.md
§8`, with `static_secrets` and `zeroize`. The [3.0.0 package record](https://docs.rs/crate/x25519-dalek/3.0.0)
records its 2026-07-06 release and the maintained Dalek dependency family. The lockfile
adds one package; `cargo deny check` reports advisories, bans, licences and sources OK.

Raw event lines stay `String`/`&[u8]` end to end. A frame carries a request's body as
bytes it never parses; `§4.2` is unchanged by the channel.

---

## 6. Milestones

### M14 — Cluster identity, the certificate, the X25519 static

**Implementation status, 2026-09-05:** implemented on `m14-cluster-identity` and merged
as PR #27; the three-platform CI run on the merge passed, and the checklist below is
ticked on that run. Section 3 rows 19 and 20 record the confirmed expiry and restore
decisions. All 26
identity tests pass on Windows, including preservation of restored registry records,
selection with and without restored keys, and correction of forged public identity
fields. The 498-test workspace suite, Clippy with warnings denied, formatting, header
check, generated-reference check, spec-reference check and named conformance checks pass
on Windows. `cargo deny check` passes advisories, bans, licences and sources, with the
existing duplicate-`syn` warning. The three-platform CI run remains pending.

The certificate fixture also verifies with OpenSSL 3, independently of the Rust
implementation. Python's standard base32 encoder independently checks the z-base32
fixture by alphabet translation.

Documentation received a source review for headings,
table structure, and plain language; rendered keyboard, zoom, and screen-reader checks
remain a human check. No app folder, client script, or rendered template changed.
Phase 2's roadmap acceptance bullets remain unchecked.

- `Identity` gains the cluster: `load_or_create` founds one when `identity/cluster.key`
  is absent — keypair generated, `cluster.key` written `0600` with the same `create_new`
  discipline as `node.key`, `cluster.pub` beside it, the Cluster ID derived as a Node ID
  is (`§2.3`), and `node.cert` issued and written. A root with a cluster loads it.
- The certificate (`§2.3.1`, §3 row 4): `Certificate { node_id, node_pub, cluster_id,
  issued_at, expires_at, sig }`, `issued_at + 180 days`, canonical JSON for the signed
  bytes, base64 in `sys_node.cert`. `Certificate::verify(&cluster_pub, now)` refuses a
  bad signature and an expired certificate as two distinct errors.
- Renewal at start only while unexpired and fewer than ninety days remain;
  `cert.renewed` audit (info). Expired certificates require re-admission, which arrives
  in Phase 3; this build refuses them without changing the certificate.
- `sys_cluster` row (`§3.1b`) on founding, keyed by the Cluster ID: `pubkey`,
  `pkarr_name` (z-base32 of the public key — a thirty-line encoder with a test vector,
  not a crate for one function), `created_at`, `created_by`. `sys_node` amended with
  `cluster_id`, `cert`, `cert_expires_at`. One batch, then `cluster.created` (info).
- `Identity::x25519_static() -> x25519_dalek::StaticSecret`, derived per §2.6, and
  `x25519_public_base64()`; this node's own `sys_device` row amended with `ed25519_pub`
  and `x25519_pub` — a `put` under the same key, blessed by `§4.6`.
- `Identity::cluster_public()`, `cluster_id()`, `certificate()`; the cluster private key
  is reachable only through `Identity::sign_certificate(&self, node_pub, now)`, so nothing
  outside the module can copy it.
- The `Debug` impl stays hand-written: no key material prints.

**Produces:** `identity::{Certificate, ClusterId}`, `Identity::{cluster_id, cluster_public,
certificate, sign_certificate, x25519_static, x25519_public_base64}`; `sys::ClusterRow`;
`sys::{KIND_CLUSTER_CREATED, KIND_CERT_RENEWED}`.

**Tests** (`tests/identity.rs`): `test_spec_2_3_first_start_founds_a_cluster`,
`test_spec_2_3_a_phase_1_root_founds_a_cluster_on_its_next_start`,
`test_spec_2_3_1_certificate_verifies_against_the_cluster_key_and_expires_at_180_days`,
`test_spec_2_3_1_certificate_signed_bytes_are_canonical` (a checked-in key, a checked-in
certificate), `test_spec_2_3_1_certificate_renews_under_ninety_days`,
`test_spec_2_3_3_cluster_private_key_is_absent_from_every_event_snapshot_and_backup` (the
raw bytes and their base64 grepped out of `data/`, every `snap/`, and a `backup::Plan`
copy), `test_spec_2_1_x25519_static_is_derived_and_stable`,
`test_spec_3_1b_pkarr_name_is_zbase32_of_the_cluster_public_key`,
`test_identity_second_run_keeps_the_cluster_and_the_node_id`. Unix only:
`test_spec_2_3_cluster_key_mode_0600`.

**Documentation:** `protocol.md §2.3, §2.3.1` and `data-dictionary.md §3.1b` are written
(row 4); `docs/backup-and-restore.md §1` names `cluster.key` beside `node.key`.

**Acceptance checklist** — check only after the named tests pass on all three platforms:

- [x] First start and Phase 1 upgrade preserve node identity:
  `test_spec_2_3_first_start_founds_a_cluster`,
  `test_spec_2_3_a_phase_1_root_founds_a_cluster_on_its_next_start`,
  `test_identity_second_run_keeps_the_cluster_and_the_node_id`.
- [x] Canonical certificates, signature and lifetime validation, bounded input:
  `test_spec_2_3_1_certificate_signed_bytes_are_canonical`,
  `test_spec_2_3_1_certificate_verifies_against_the_cluster_key_and_expires_at_180_days`,
  `test_spec_2_3_1_signed_invalid_certificate_fields_are_refused`,
  `test_spec_2_3_1_startup_refuses_another_nodes_certificate`.
- [x] Unexpired renewal under ninety days and one renewal audit:
  `test_spec_2_3_1_certificate_renews_under_ninety_days`,
  `test_spec_2_3_1_startup_renewal_is_audited_once`.
- [x] Derived session identity and discovery name:
  `test_spec_2_1_x25519_static_is_derived_and_stable`,
  `test_spec_3_1b_pkarr_name_is_zbase32_of_the_cluster_public_key`.
- [x] Keys stay private and invalid identity material fails closed:
  `test_spec_2_3_3_cluster_private_key_is_absent_from_every_event_snapshot_and_backup`,
  `test_spec_2_3_invalid_cluster_material_is_refused_without_replacement`,
  `test_spec_2_3_missing_cluster_key_is_not_silently_replaced`,
  `test_spec_2_3_cluster_key_mode_0600` (Unix only).
- [x] Restore preserves records and verified local identity selects the current ones:
  `test_spec_3_1b_data_only_restore_preserves_records_and_selects_local_identity`,
  `test_spec_3_1b_restored_keys_select_the_original_cluster`,
  `test_spec_3_1b_replayed_rows_cannot_change_local_cluster_identity`,
  `test_spec_4_2_identity_amendment_preserves_unknown_json_bytes`.

Restored-device pairing and channel authorization remain M16/M17 work: public registry
records must not bypass pairing or pinned-key verification (`protocol.md §2.3, §7, §8`).
Node admission and sync remain Phase 3 work; this change provides no successful stub.

---

### M15 — The session layer, in Rust and in JavaScript

**Implementation status, 2026-09-05:** implemented on `m15-session` and merged as PR
#28; the three-platform CI run on the merge passed, and the checklist below is ticked
on that run. Reverting the import specifiers of every vendored Noble file reproduces its
upstream hash (row 21). Session helpers do no network or storage I/O.
The node handshake takes
active pairing facts from its caller; wiring that lookup to the registry and enforcing
revocation on channel requests remain M17 work. The bind remains loopback until M17.

The only plan exception is the owner-approved import-path edit in §3 row 21. No
normative spec or CSP changes were needed. Frame sealing returns `Result` because an
exhausted or closed sender must refuse. A completed handshake carries owned send and
receive frames, preserving the counter consumed by the confirm; it does not return raw
keys from which a caller could restart that counter. A client has peer proof when its
first inbound frame authenticates.

Hello text is bounded to 8192 UTF-8 bytes before parsing; certificates retain M14's
4096-byte bound. Invalid keys, IDs, versions, pins and confirmations fail closed, with
errors that contain no input. Frame failure is terminal. Callers must close both
directions and the connection on error, and check `Frame::is_closed()` or the JavaScript
`closed` getter after successful frames to close at the budget boundary.
JavaScript explicitly wipes owned byte arrays,
but garbage collection and runtime copies prevent a claim of guaranteed memory erasure.

The only new direct Rust dependency is `chacha20poly1305` 0.11.0, required for §8's
authenticated encryption, with `alloc` and `zeroize`; randomness stays with `rand`.
Its [package record](https://crates.io/crates/chacha20poly1305/0.11.0) records a
2026-06-28 release in the maintained RustCrypto family. Noble's
[curves](https://github.com/paulmillr/noble-curves),
[ciphers](https://github.com/paulmillr/noble-ciphers), and
[hashes](https://github.com/paulmillr/noble-hashes) packages are 2.4.0, as planned.
Their npm archives were checked against registry SHA-512 integrity before vendoring.
The browser tests verify every vendored file's hash and its relative import closure.
`cargo deny check` passes advisories, bans, licences and sources, with the existing
duplicate-`syn` warning. The OSV query for all three npm packages at 2.4.0 returned no
listed vulnerabilities on 2026-09-05.

Regenerate synthetic vectors only with:

```sh
cargo test -p privatium-core --locked --test session generate_session_vectors -- --ignored --exact
```

Normal tests read the committed fixture and never rewrite it. It includes fixed
synthetic statics and ephemerals, ten frames in each direction, and a certificate-bound
handshake using the existing synthetic identity fixture.

**Verification on Windows:**

| Command | Outcome |
|---|---|
| `cargo test -p privatium-core --locked --test session` | 11 passed; fixture generator ignored by default |
| `cargo test -p privatium-core --locked --lib test_spec_8` | 2 passed: counter limit and asset path controls |
| `cargo test --workspace --locked` | 511 passed, 0 failed; 1 intentional fixture-generator ignore |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo xtask header-check` | 305 files passed |
| `cargo xtask gen-skill-reference` | Regenerated 21 files; no reference content changed |
| `cargo xtask gen-skill-reference --check` | 21 files match |
| `cargo xtask lint-spec-refs` | 37 rules resolve |
| `target/m15-tools/node.exe --test crates/privatium-core/tests/js/*.test.mjs` | 25 passed; portable Node 24.20.0, upstream checksum verified |
| `cargo deny check` | Passed; existing duplicate-`syn` warning |
| `bash .github/scripts/conformance.sh` | Every named Phase 1, identity and session test passed using Git Bash |

Node.js was absent from PATH. The portable test executable stays in ignored `target/`;
no system package or PATH setting changed. CI runs the same JavaScript test glob through
Bash on all three platforms. No app folder changed, so no targeted app lint is needed;
the existing reference-app tests and CI lint still run.

The staged whitespace check reports 25 upstream trailing-whitespace lines in vendored
Noble modules. These bytes are preserved under the import-path-only exception; the
check excluding that vendor directory passes.

Security review covered input bounds, certificate and pin validation, rejection of
noncontributory keys, nonce ownership, terminal authentication failures, dependency
integrity, and errors free of peer input. No application data or real key material is
in the fixtures. The new asset path accepts only safe components under the embedded
Noble directory; it reads no filesystem path from a request.

Documentation received a source review for heading order, descriptive links, table
headers, and plain language. No rendered page, control, CSS, or application flow changed.
Rendered documentation checks with keyboard, visible focus, 200% zoom, and a screen
reader remain a human check. Phone pairing, refusal screens and live-channel checks
remain with M17–M19, where those flows are implemented.

**Acceptance checklist** — check only after the named tests pass on all three platforms:

- [x] Key schedule and cross-language frames:
  `test_spec_8_key_schedule_matches_the_checked_in_vectors`,
  `test_spec_8_key_schedule_and_bidirectional_frames_match_rust_vectors` (JavaScript).
- [x] Counters, empty frames, tampering, replay and exhaustion:
  `test_spec_8_frames_round_trip_and_the_counter_never_repeats`,
  `test_spec_8_a_tampered_frame_is_refused`,
  `test_spec_8_a_session_closes_at_the_counter_limit` (Rust unit),
  `test_spec_8_counter_limit_cannot_be_raised_and_key_input_is_copied` (JavaScript).
- [x] Handshake agreement, exact transcript and pin/certificate refusal:
  `test_spec_8_handshake_derives_the_same_keys_on_both_sides`,
  `test_spec_8_3_confirm_binds_the_exact_hello_bytes`,
  `test_spec_8_1_a_static_key_that_is_not_the_pinned_one_fails_the_confirm`,
  `test_spec_8_1_certificate_mismatch_and_expiry_are_refused_before_confirm`,
  `test_spec_8_client_handshake_matches_rust_without_crypto_subtle` (JavaScript).
- [x] Unknown/revoked devices, missing/invalid keys and malformed hellos:
  `test_spec_8_3_unknown_revoked_and_missing_device_keys_are_refused`,
  `test_spec_8_3_malformed_hello_and_wrong_version_are_refused`,
  `test_spec_8_noncontributory_keys_and_invalid_ids_are_refused`.
- [x] Browser modules load from the embedded asset set with preserved provenance:
  `test_spec_8_browser_crypto_modules_are_served_without_path_traversal` (Rust unit),
  `test_spec_8_noble_hashes_and_relative_import_closure_match_provenance` (JavaScript).

- `session::Keys::derive(role, my_static, their_static, my_eph, their_eph, node_id,
  device_id)` — `§8` verbatim: `ss`, `salt = SHA-256(sorted(node_id, device_id) ‖ "pv/1
  session")`, `prk = HKDF-Extract(salt, ss ‖ ee)`, `k_c2s`, `k_s2c`.
- `session::Frame`: `seal(&mut self, plaintext) -> Result<Vec<u8>>` and `open(&mut self,
  ciphertext) -> Result<Vec<u8>>` per direction, `nonce = direction tag (4 bytes, big
  endian: 1 for c2s, 2 for s2c) ‖ counter (8 bytes, big endian)`, counter from 0, no
  associated data, one ciphertext per WebSocket binary message. A counter that reaches
  2³² closes the session (§3 row 2). A counter never repeats because the type owns it and
  it is not `Clone`.
- `session::handshake` — the `/ws` messages of §3 row 2, as plain data with no I/O so
  the same code is driven by a test, by the channel and, in Phase 3, by the sync client:
  `ClientHello { v, dev, e }`, `NodeHello { v, id, e, cert }`, then `Confirm { transcript
  }` as the first sealed c2s frame, transcript = `SHA-256(client hello bytes ‖ node hello
  bytes)`. `Handshake::node(identity, lookup, hello)`
  answers a hello and, on the confirm, yields `Session { device, send, receive }`; the lookup is
  `sys_device` — active, not revoked, with an `x25519_pub`.
- Vendor `@noble/*` (§5) with `VENDOR.md` (versions, SHA-256 per file, licence) and the
  `NOTICE` entry. `assets/shell/session.js`: the same schedule and frame over
  `x25519`, `hkdf`, `sha256` and `chacha20poly1305` from the vendored modules; the
  client role of the handshake.
- `tests/fixtures/session-vectors.json`: fixed statics and ephemerals, the derived keys,
  ten sealed frames in each direction. Generated by a Rust test with a `--` flag that
  writes it (kept out of `cargo test`'s default run), read by the Rust tests and by
  `session.test.mjs`. The Rust side seals, the JavaScript side opens, and back.
- `.github/workflows/ci.yml`'s `node --test` step runs the whole
  `crates/privatium-core/tests/js/` directory from here on, so every later `.test.mjs` is
  in the gate without another edit.

**Produces:** `session::{Keys, Frame, Direction, Session, handshake::{ClientHello,
NodeHello, Confirm, Handshake}}`; `assets/shell/session.js` exporting `derive`,
`Frame`, `clientHandshake`.

**Tests** (`tests/session.rs`): `test_spec_8_key_schedule_matches_the_checked_in_vectors`,
`test_spec_8_frames_round_trip_and_the_counter_never_repeats`,
`test_spec_8_a_tampered_frame_is_refused`,
`test_spec_8_handshake_derives_the_same_keys_on_both_sides`,
`test_spec_8_1_a_static_key_that_is_not_the_pinned_one_fails_the_confirm`,
`test_spec_8_a_session_closes_at_the_counter_limit` (the limit lowered by a constructor
the test alone uses). Under `node --test`: `session.test.mjs` — the vectors, the
cross-language frames, the client handshake against the fixture's node hello.

**Documentation:** `protocol.md §8.3` is written (row 2); this milestone edits it only
where the code proves it wrong.

---

### M16 — Pairing

**Implementation status, 2026-09-05:** implemented on `m16-pairing`; the Windows gates
pass, and the three-platform run recorded under "Phase 2 hardening" ticked the checklist.
Section 3 rows 22–25 record the spec gaps
the code found. Pairing is data: `Node::pair` opens the window; `Node::pairing_hello`,
`pairing_begin`, `pairing_confirm`, `pairing_finish` and `pairing_abandon` drive the six
messages against it under the node's lock and write every audit row of `§7.5`; the Rust
`pair::handshake::Client` and `assets/shell/pair.js` are the device's side. Nothing
listens at `/ws/pair` until M17 wires `wire::channel`, and the node still binds
loopback, so the URL in the pairing object is the loopback URL until M17's bind replaces
`Node::listen_url`.

Three shapes differ from the sketch below, none of them wire meaning. `Node::pair(ttl)`
returns a `PairingSnapshot` — both renderings of the code, the URL, the expiry, the
attempts and the outcome, the object `POST /api/v1/pair` will answer with — while
`Pairing` is the window itself, reached through `Node::pairing()`. An attempt is counted
when `pA` is accepted rather than when `cA` arrives (row 24), so five silent peers
exhaust a code exactly as five wrong guesses do, and a peer that leaves after `pA` is
reported through `pairing_abandon`. `client.test.mjs` arrives with `client.js` in M17;
the device-ID derivation test lives in `pair.test.mjs`. The vector file is written by
`generate_pake_vectors` (ignored by default) from the identity fixture; `Pairing::open_with`,
`Exchange::begin_with` and `Client::start_with` take the secrets the fixture fixes and
are the only way to make a run reproducible. `spec/pairing-words.txt` and its `NOTICE`
entry were already in the tree; the list is compiled into the crate as a `const`, so a
malformed list is a build error rather than a runtime one.

`curve25519-dalek` 5.0.0 is taken directly, as §5 planned: it was already in the
lockfile beneath `ed25519-dalek`, so the graph gains no package. `cargo deny check`
passes advisories, bans, licences and sources, with the existing duplicate-`syn`
warning.

Regenerate the vector file only with:

```sh
cargo test -p privatium-core --locked --test pair generate_pake_vectors -- --ignored --exact
```

**Verification on Windows:**

| Command | Outcome |
|---|---|
| `cargo test -p privatium-core --locked --test pair` | 19 passed; the generator ignored by default |
| `cargo test -p privatium-core --lib --locked pair::` | 7 passed |
| `cargo test --workspace --locked` | 537 passed, 0 failed; 2 intentional fixture-generator ignores |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo xtask header-check` | 311 files passed |
| `cargo xtask gen-skill-reference` | Regenerated 21 files; the Tier 3 `api.md` gained the pairing methods |
| `cargo xtask gen-skill-reference --check` | 21 files match |
| `cargo xtask lint-spec-refs` | 37 rules resolve |
| `node --test crates/privatium-core/tests/js/*.test.mjs` | 36 passed, 11 of them new; portable Node 24.20.0 |
| `cargo deny check` | Passed; existing duplicate-`syn` warning |
| `bash .github/scripts/conformance.sh` | Every named Phase 1, identity, session and pairing test passed using Git Bash |

No app folder changed, so no targeted `privatium lint` run is needed; the reference-app
lint test and CI's lint step still run.

Security review: every message is bounded before it is parsed; a device ID is checked
against its key; a point is refused unless it is the one canonical encoding of a point
outside the small-order subgroup, and a shared point that is the identity is refused;
`cB` is verified before `cA` is sent and `cA` before anything is sealed; the confirmation
comparison is constant-time; a `kind` of `node` and a registered key are refused with
4403; the rate limit and the attempt cap are enforced before the PAKE runs; a refusal
carries none of the peer's bytes; no code, rendering or `w` appears in any message or any
audit row, which `test_spec_7_0_the_code_never_crosses_the_wire` and
`test_spec_7_5_every_attempt_writes_an_audit_row_without_the_code` hold; the browser
checks storage before it sends anything, so a device the node registers is one that kept
its keys. What remains open is `§7.7`'s first-load gap, as designed.

No page, control or app folder changed; the accessibility pass belongs to M19's screen.
Documentation received a source review for headings, tables and plain language; the
rendered checks remain a human check.

**Acceptance checklist** — check only after the named tests pass on all three platforms:

- [x] The code, both renderings and the parser:
  `test_spec_7_2_code_is_16_bits_rendered_as_four_glyphs_and_two_words`,
  `test_spec_7_2_word_input_is_case_and_punctuation_insensitive`,
  `test_spec_7_2_glyph_labels_are_accepted_as_input`,
  `test_spec_7_3_glyph_table_is_normative_and_keeps_variation_selectors`,
  `test_spec_7_2_word_list_has_256_distinct_words_with_unique_three_letter_prefixes`,
  `test_spec_7_2_parse_cases_and_renderings_match_rust` and
  `test_spec_7_2_word_list_and_glyph_table_match_the_spec` (JavaScript).
- [x] SPAKE2 agrees across the two languages and refuses what it must:
  `test_spec_7_4_spake2_matches_the_checked_in_vectors`,
  `test_spec_7_4_a_wrong_code_derives_nothing`,
  `test_spec_7_4_1_m_and_n_are_the_rfc_9382_edwards25519_points` (Rust unit), and in
  JavaScript `test_spec_7_4_spake2_matches_the_checked_in_vectors`,
  `test_spec_7_4_1_m_and_n_are_prime_order_points_distinct_from_g`,
  `test_spec_7_4_a_wrong_code_derives_nothing`,
  `test_spec_7_4_1_invalid_points_zero_scalars_and_bad_codes_are_refused`.
- [x] The six messages, what the device pins and the row the node writes:
  `test_spec_7_4_pairing_completes_and_writes_the_device_row`,
  `test_spec_7_4_the_client_pins_the_cluster_key_and_the_node_certificate`,
  `test_spec_7_4_2_the_transcript_matches_the_checked_in_vectors`,
  `test_spec_7_0_the_code_never_crosses_the_wire`, and in JavaScript
  `test_spec_7_4_2_client_transcript_matches_rust`,
  `test_spec_2_2_device_id_derivation_matches_rust`.
- [x] The window, its limits and its audit rows:
  `test_spec_7_1_pairing_is_closed_until_opened_and_closes_on_first_success`,
  `test_spec_7_5_code_expires_at_120s_and_five_attempts_issue_a_new_one`,
  `test_spec_7_5_attempts_are_rate_limited_per_source`,
  `test_spec_7_5_every_attempt_writes_an_audit_row_without_the_code`,
  `test_spec_app_contract_6_pair_opens_a_window_and_returns_the_code`.
- [x] Every refusal fails closed with its close code and nothing derived:
  `test_spec_7_4_a_node_kind_is_refused_naming_phase_3`,
  `test_spec_7_4_2_malformed_messages_and_a_mismatched_device_id_are_refused`,
  `test_spec_7_6_a_registered_device_key_cannot_pair_again`, and in JavaScript
  `test_spec_7_1_a_closed_node_and_a_wrong_code_send_nothing_further`,
  `test_spec_7_4_2_malformed_and_tampered_node_messages_are_refused_without_a_record`,
  `test_spec_7_6_no_storage_means_no_pairing_and_a_bad_code_is_refused_first`.

- `pair::code`: `Code(u16)` from the CSPRNG; `glyphs() -> [Glyph; 4]` from the normative
  table of `§7.3` stored as byte strings with their labels — index 8 and 9 keep U+FE0F
  and a test grep proves no normalization touched them; `words() -> [&'static str; 2]`
  from `spec/pairing-words.txt`, included at compile time and checked by a test for 256
  distinct lowercase words with unique three-letter prefixes; `Code::parse(&str)` accepts
  four glyphs (labels accepted too, so a screen reader's user can type "fox pizza
  lightning die") or two words, case-insensitive, ignoring spaces, hyphens and
  punctuation, and refuses anything else naming what it expected.
- `pair::spake2`: RFC 9382 §3 over edwards25519 with the RFC's `M` and `N` (§2.4). `Side
  A` and `Side B`, `start(w, identity) -> (State, Message)`, `finish(state, their_message)
  -> (Ke, Ka, confirm_to_send, confirm_expected)`; the transcript `TT` with eight-byte
  little-endian lengths as the RFC spells it. `w` from the code per §2.4. The vector file
  `tests/fixtures/pake-vectors.json` — code, both scalars, both messages, `TT`, `Ke`,
  both MACs — generated as M15's is, and read by `pake.test.mjs`.
- `pair::Pairing` — the window: `open(ttl, now) -> Pairing` with `code`, `expires_at`,
  `attempts`, `consumed_by`; `attempt(source_ip, now)` enforces one per two seconds per
  source and five per code; on the fifth failure the code is destroyed and a fresh one
  issued for the remaining window (`§7.5`), announced to the settings page and the CLI as
  a new code. Every attempt, success or failure, and every expiry is a `sys_audit` row —
  `pair.opened`, `pair.attempt`, `pair.success`, `pair.failed`, `pair.expired` — with the
  source's address in `detail` and never the code. The first success closes the window.
- `Node::pair(&mut self, ttl: Duration) -> Result<Pairing>` (§3 row 13) and
  `Node::pairing(&self) -> Option<&Pairing>`; `Node::close_pairing(&mut self)`.
- The `/ws/pair` handshake, as data in `pair::handshake` and as I/O in `wire::channel`
  (M17 wires the socket; this milestone drives the messages from a test):

  ```json
  → {"v":1,"id":"k7m2q9xf","pub":"<node ed25519 b64>","open":true}
  ← {"v":1,"dev":"b3nn8t2q","pub":"<device ed25519 b64>","kind":"browser","pA":"<b64>"}
  → {"pB":"<b64>","cB":"<b64>"}
  ← {"cA":"<b64>"}
  → sealed(K_pair, s2c) {"x25519":"<b64>","cert":"<b64>","cluster_id":"q4w8rt2n","cluster_pub":"<b64>"}
  ← sealed(K_pair, c2s) {"x25519":"<b64>","label":"Pixel 9","ua":"Mozilla/5.0 …"}
  ```

  `open: false` closes with 4404; a wrong `cA` closes with 4401 after the audit and the
  attempt count; an exhausted code with 4429. `K_pair` is `Ke`; the two sealed messages
  use M15's frame with keys `HKDF-Expand(HKDF-Extract("pv/1 pair", Ke), "pv/1 c2s" |
  "pv/1 s2c", 32)`. On the client's sealed message the node writes the `sys_device` row —
  `kind`, `replica = false` for a browser, `ed25519_pub`, `x25519_pub`, `paired_at`,
  `paired_via = 'lan'`, `user_agent`, `label` — marks the code consumed, audits
  `pair.success`, closes the window. `kind = "node"` is refused with 4403 naming Phase 3.
  The device ID the client sends is checked against its public key by the same
  derivation `NodeId::derive` uses.
- `assets/shell/pair.js`: the client role — key generation (`ed25519`, `x25519` from the
  vendored modules), the device ID derivation, the PAKE, the two sealed messages, and
  storage under `pv:device` (§2.9). No UI yet; M19's page drives it.

**Produces:** `pair::{Code, Glyph, GLYPHS, WORDS, Pairing, PairingSnapshot, handshake::{...}}`;
`Node::{pair, pairing, close_pairing}`; `sys::KIND_PAIR_*`; `spec/pairing-words.txt`;
`assets/shell/pair.js` exporting `pair(socket, code, options)`.

**Tests** (`tests/pair.rs`): `test_spec_7_2_code_is_16_bits_rendered_as_four_glyphs_and_two_words`,
`test_spec_7_2_word_input_is_case_and_punctuation_insensitive`,
`test_spec_7_2_glyph_labels_are_accepted_as_input`,
`test_spec_7_3_glyph_table_is_normative_and_keeps_variation_selectors`,
`test_spec_7_2_word_list_has_256_distinct_words_with_unique_three_letter_prefixes`,
`test_spec_7_4_spake2_matches_the_checked_in_vectors`,
`test_spec_7_4_a_wrong_code_derives_nothing` (the MACs differ; no key comes out),
`test_spec_7_0_the_code_never_crosses_the_wire` (neither the code bytes, their glyphs,
their words nor `w` appear in any message of a full run),
`test_spec_7_4_pairing_completes_and_writes_the_device_row` (a Rust client through the
message types; the row's every column checked; `replica` false),
`test_spec_7_1_pairing_is_closed_until_opened_and_closes_on_first_success`,
`test_spec_7_5_code_expires_at_120s_and_five_attempts_issue_a_new_one` (a fake clock),
`test_spec_7_5_attempts_are_rate_limited_per_source`,
`test_spec_7_5_every_attempt_writes_an_audit_row_without_the_code`,
`test_spec_7_4_a_node_kind_is_refused_naming_phase_3`,
`test_spec_app_contract_6_pair_opens_a_window_and_returns_the_code`. Under `node --test`:
`pake.test.mjs` (the vectors; a full exchange against the Rust vectors' `B` side),
`client.test.mjs` gains the device ID derivation against a Rust vector.

**Documentation:** `protocol.md §7.2, §7.4.1, §7.4.2`, `data-dictionary.md §3.3`,
`app-contract.md §6`, `spec/pairing-words.txt` and `NOTICE` are written (rows 1, 3, 6, 13,
18) and are edited only where the code proves them wrong.

---

### M17 — The LAN bind, the auth policy, and the channel

**Implementation status, 2026-09-05, branch `m17-channel-lan`:** the LAN listeners,
bootstrap policy, live pairing and encrypted channel are implemented. Browser modules
drive HTMX, the data API and full-page response handoffs. Section 3 rows 26–28 record the
security disclosure, approved navigation correction and resource-integrity clarification.
Discovery and the pairing screen remain M18 and M19 respectively.

The browser parses each authenticated full page in a fresh bootstrap document. That
document already carries the destination app's CSP, and browser navigation resets its
module map. HTMX's `beforeRequest` hook replaces that request's send operation while
preserving HTMX's parameter encoding, indicators, response handling and completion
callbacks. Its history cache is disabled on channel pages so it does not persist
decrypted page content. Forms outside HTMX use §8.3.1's response handoff.

Response slots are reserved before dispatch. Their bodies remain unpolled, are consumed
atomically by the same active device, and are dropped on expiry or release. Timer tasks
are cancelled when a response is consumed. Disconnects do not roll back writes or
re-execute a handler. Browser storage carries no form body or rendered response.

`tests/channel.rs` in the binary crate runs real WebSocket pairing and sessions through
a capturing loopback proxy with a TEST-NET peer. Its live JavaScript test runs the actual
browser modules against that core. Node.js must be on PATH for this test; the test-only
`PRIVATIUM_TEST_NODE` environment variable may name a portable executable.

Final verification results are recorded with the commit and PR; the checklist below is
ticked on the three-platform run recorded under "Phase 2 hardening".

The full workspace run exposed a probabilistic assertion in
`test_spec_7_0_the_code_never_crosses_the_wire`: random base64 could contain a short
word or hex rendering of the code. The test now validates the exact message fields,
canonical encodings and lengths, rejects an injected code field, and still checks
that the password scalar is absent. Pairing tests also separate their synthetic
window clock from certificates issued by `Node::open` on the real clock. Neither
correction changes pairing cryptography.

- `crates/privatium/src/lib.rs`: `bind(port)` opens `0.0.0.0:<port>` and `[::]:<port>`
  (the second best-effort), `announce` prints the default route's URL and the rest under
  `--verbose` (§2.2, §3 row 14), and the adapter still inserts `Peer` and nothing else.
- `http::auth`: the policy of §2.1. `AuthLayer` learns three things a request may carry:
  a `Peer`, an unforgeable `Session` (the authenticated device, node and static key),
  and the route class from `Router`. Hourly `last_seen_at` writes remain M19. Loopback with a
  loopback `Host`: this node, every route. Non-loopback without a session: the bootstrap
  set; the answer for a page path is the bootstrap page, for anything else 403 with a
  sentence naming pairing. A session: `Device(session.device)`, every route, with the
  grant resolution of `§3.5` still landing on `*+*`. The embedder's layer refuses a
  peerless request as before.
- `wire::channel`: `/ws` — the upgrade answered from `handle` with axum's
  `WebSocketUpgrade` (the `OnUpgrade` extension hyper attaches; an in-process caller
  without one gets 426), M15's handshake on the socket, then the frame loop: a `req` frame
  becomes a `Request` with `Peer`, `Session` and `Host` from the upgrade request, is given
  to `Handler::handle` on its own task, and its `Response` goes back as `res`, `chunk`s
  and `end` — the body streamed as it is produced, which is what carries SSE. `cancel`
  from the client drops the task. A revoked device's session is closed by the handler that
  revoked it (M19) and is refused at the next handshake. `/ws/pair`: M16's messages on the
  socket, with the pairing window read and written under the node lock.
- Frame plaintext: `[u32 BE json_len][json][payload]`; `json` is `{"id","kind"}` plus, for
  `req`, `method`, `path`, `headers` and, for `res`, `status`, `headers`. A `req` carries
  its whole body in one frame, bounded by `api.max_body`; `chunk` for a request is
  reserved for Phase 3's uploads and refused here.
- The bootstrap page (`http::pairing::bootstrap`): a document with the public node ID, the
  requested path in a `data-path` attribute, `<script type="module"
  src="/static/client.js">` with `integrity`, the `<noscript>` of §2.9, and no app data —
  `test_spec_9_2_unauthenticated_leaks_nothing` extends to it.
- `assets/shell/client.js`: on load, read `pv:device`; with none, the pairing screen
  (M19's UI, this milestone's plumbing); with one, open `/ws`, run M15's client handshake,
  verify the node's certificate against the pinned cluster key — a mismatch is `§8.1`'s
  full-screen refusal with the two fingerprints and no way past it but *forget this node
  and pair again*, which wipes `pv:device` — then fetch the requested path through the
  channel and put it in place of the bootstrap document. Scripts and stylesheets are
  re-created with integrity hashes under §2.1. HTMX keeps its lifecycle and swaps;
  its send operation uses the channel. Other forms use the response handoff, while
  ordinary links open fresh bootstrap documents. `pv.js` uses `window.__pv_channel`
  for calls and SSE, with no API surface change.
- `Route::Ws`, `Route::WsPair` in `wire::router` with `/ws` in `FRAMEWORK_PREFIXES` (§3
  row 9); the auth layer's route class comes from there.
- The page frame carries `integrity` on the framework's own script and stylesheet tags
  (`shell.rs`, `assets.rs` computing SHA-256 once at startup) so that a page fetched over
  the channel is pinned end to end without the client's help for the framework's files.

**Produces:** `http::auth::Session`; `wire::channel::{serve_ws, serve_ws_pair, Frame,
Kind}`; `http::pairing::bootstrap`; `assets/shell/client.js` exporting `channel()`,
`fetchThroughChannel()`; `pv.js` unchanged in surface.

**Tests** (`tests/channel.rs`, through `handle` with an inserted `Peer(192.0.2.10:4000)` —
TEST-NET, never loopback): `test_spec_8_4_plain_http_on_the_lan_serves_only_the_bootstrap_set`
(every path of `HOST_ROUTES` from `tests/wire.rs`, the answer classified),
`test_spec_9_2_bootstrap_page_carries_no_app_data`,
`test_channel_requests_reach_every_route_of_handle` (the decoder fed frames, each route of
`§9.1` answered), `test_channel_streams_a_response_body_frame_by_frame` (`/api/stream`
through the decoder: an append arrives as a `chunk` before the response ends),
`test_channel_requests_carry_the_session_device` (`req.device`, `/api/node`'s `dev`),
`test_spec_8_1_a_revoked_device_is_refused_at_the_handshake`,
`test_loopback_keeps_phase_1_semantics` (every Phase 1 wire test still passes with a
loopback peer), `test_spec_8_3_page_frame_scripts_carry_integrity`,
`test_channel_refuses_a_request_chunk_naming_phase_3`. In `crates/privatium/tests/`:
`test_adapter_binds_every_interface` (replaces `test_binds_loopback_only`),
`test_spec_8_2_lan_socket_carries_no_plaintext_app_data` — a listener served through a
service that inserts a TEST-NET `Peer` before `handle`, a capturing TCP proxy in front of
it, a Rust client that pairs and then reads `/a/hello/` and `/a/hello/api/schema` through
the channel, and an assertion that the proxy's bytes contain neither the page's `<h1>`
text nor a column name — the roadmap's Wireshark bullet, automated;
`test_spec_10_4_browser_client_holds_exactly_one_endpoint` (`client.test.mjs`: every URL
the client builds is on its own origin). `client.test.mjs` also drives the extension
against the `pv.test.mjs` harness with a fake channel.

**Acceptance checklist** — Windows runs are green; check after the named tests pass
on all three platforms:

- [x] LAN policy, owner loopback and IPv4/IPv6 bind:
  `test_spec_8_4_plain_http_on_the_lan_serves_only_the_bootstrap_set`,
  `test_loopback_keeps_phase_1_semantics`, `test_adapter_binds_every_interface`.
- [x] Live encrypted routing and streaming:
  `test_spec_8_2_lan_socket_carries_no_plaintext_app_data`,
  `test_channel_streams_a_response_body_frame_by_frame`,
  `test_spec_8_3_browser_client_against_live_core`.
- [x] Response ownership, capacity and expiry:
  `test_spec_8_3_1_handoff_survives_disconnect_without_repeating_a_write`,
  `test_spec_8_3_1_wrong_device_cannot_consume_or_release_a_response`,
  `test_spec_8_3_1_capacity_refuses_before_dispatch_and_release_frees_it`,
  `test_spec_8_3_1_idle_expiry_releases_body_without_another_request` (unit).
- [x] Destination policy and client integration:
  `test_spec_8_3_1_bootstrap_uses_destination_app_permissions`,
  `test_spec_8_3_integrity_uses_authenticated_bytes_and_refuses_unpinned_remote_code`,
  `test_spec_8_3_pv_uses_the_channel_for_requests_and_subscriptions`,
  `test_spec_8_3_htmx_keeps_its_lifecycle_and_never_sends_plaintext` (JavaScript).

**Documentation:** `protocol.md §7.7, §8, §8.3, §8.4, §9.1, §13`, `cli.md §2`,
`data-api.md`, `docs/security.md §3, §4`, `docs/architecture.md §2.6` and the security and
Tier 2 skills are written (rows 2, 7, 8, 9, 14, 17) and are edited only where the code
proves them wrong; `docs/deployment.md §4` says the Windows prompt now happens;
`apps/sketch/README.md`.

---

### M18 — Discovery: mDNS and UDP, together

**Implementation status, 2026-09-06, branch `m18-discovery`:** implemented. `discover::{Facts,
Discovered, Discovery, Options, Switch, Status, Outcome}`, `discover::txt` (the record
and its budget), `discover::mdns` (advertiser and browser on one `mdns-sd` daemon) and
`discover::udp` (the responder, the probe, the source check and the rate limit) are in
the core; `Node::{serve_discovery, discovery_status, discovered, discovery_facts,
publish_facts}` are the node's surface, and `run.rs` starts discovery on the port the
socket actually bound, so `p` is right under `--port 0`. The twelve tests of
`tests/discover.rs` and `test_cli_no_discovery_starts_nothing` pass on Windows,
including the real daemon browsing its own registration; the three-platform run
recorded under "Phase 2 hardening" passed it on every runner, which closes risk R10.

Four shapes differ from the sketch below. **Subtypes are computed and not advertised:**
`mdns-sd` keys registrations by full name and carries one subtype per `ServiceInfo`,
so a second subtype registration replaces the first, and registering each subtype
under its own instance name would break `§6.1`'s instance-name rule and multiply the
node's entries. `Facts::advertised` and `Facts::subtypes()` hold the rule — mounted,
`nav.advertise = true`, at most fifteen characters, with the load warning for a longer
slug — and the parent type carries the full TXT record; the SHOULD of `§6.1` waits on
a library that can carry several subtypes, which is recorded as risk R17. **No `watch`
channel:** the node hands new facts to the running mechanisms with `publish_facts`
after apps load and whenever a pairing window opens, closes, expires or is consumed;
the UDP responder reads the shared facts on every probe, and the mDNS registration
is replaced. **`pair` is an instant, not a flag:** the facts carry the window's
expiry, so an answer written after it says `0` with no call from the node, and the
browsing thread re-registers when the flag on the wire no longer matches. **`Discovery`
is not an error path:** `serve_discovery` fails only for the node's own trouble; a
mechanism the platform refuses is `Outcome::Failed` in `discovery_status`, printed by
the CLI and named in the `discovery.method` audit row, and a `discovery.*` setting
that is neither `true` nor `false` keeps its mechanism off with that reason.
`serve_discovery` is called after the bind rather than straight after `load_apps`,
for the port. `Node::discovered()` exists for M19's node page; nothing prints it yet.

The PR's macOS run exposed a latent flake in `pv.test.mjs`: two pages sharing one
storage minted outbox ids in the same millisecond, and each page's monotonic counter
knew nothing of the other's, so the replay order was the random tails'. `pv.js` now
takes the newest adopted entry as its floor when it reads the storage, so a page
mints past what another page queued; the test freezes the clock to make the case
certain rather than probable.

`mdns-sd` resolved to 0.21.2, the current patch of the 0.21.1 §5 planned; `default-features
= false` drops its `async` and `logging` features, so `flume` comes without its async
half and nothing logs. Its `if-addrs`, `socket2`, `mio` and `windows-sys` were already
in the graph; `flume`, `spin` and `socket-pktinfo` are new. `cargo deny check` is in
the PR's verification table.

The same branch carries the rule that a release binary never starts with an empty
launcher (§3 row 29): the three example apps are embedded in the core
(`app::examples`, 138 KB), written to `<data-dir>/apps/` whenever that folder holds no app and by
`privatium new --examples`, and found by `--from hello` without a checkout. A checkout
still mounts its own `apps/` as `bundled` and writes nothing, since a copy would shadow
the folder the developer is editing. `PRIVATIUM_TEST_NO_CHECKOUT` is the test-only
variable that lets the suite, which always runs from a checkout, exercise the release
binary's path.

- `discover::txt`: the record of `§6.1` — `v`, `id`, `cl`, `nm`, `apps`, `build`, `pair`,
  `p` — built from a `Facts` struct, `apps` truncated with `,…` to keep the whole under
  1300 bytes; instance name `sys_node.display_name` or the Node ID, ≤ 63 bytes.
- `discover::mdns`: register `_privatium._tcp.local.` with the TXT and one sub type per
  mounted app with `nav.advertise = true` and a slug of ≤ 15 characters — the M5 warning
  for longer slugs stays the surfacing `§6.1` requires; re-register when the facts change
  (a `watch::Receiver<Facts>` the node updates); browse the same type and keep
  `Discovered { id, cl, name, addrs, port, apps, pair, seen_at }` keyed by `id`, never by
  name.
- `discover::udp`: a socket on `0.0.0.0:52525`; a probe of `PVDISCO1` plus four nonce
  bytes from a private, link-local or loopback source (§3 row 11) is answered by unicast
  with the same prefix, the nonce and the TXT keys as a JSON object; one answer per source
  per second; anything else dropped. `probe(timeout) -> Vec<Discovered>` for the node's
  own use in Phase 3.
- `Node::serve_discovery(&mut self) -> Result<()>` starts both, per `discovery.mdns` and
  `discovery.udp` from `sys_setting`, at once and never in sequence (`§6.5`); a mechanism
  the platform refuses — no multicast interface — is a line on standard error, not a
  failure of the other. Both stop when the node drops. `discovery.method` audit (info)
  names what started.
- `--no-discovery` starts neither and says so; `run.rs` calls `serve_discovery` after
  `load_apps` unless it is set. `Node::discovered() -> Vec<Discovered>` for the settings
  page and `--verbose`.
- The `pair` key flips through the facts channel the moment a window opens or closes, and
  `/api/v1/manifest` reports the same flag from the same source.

**Produces:** `discover::{Facts, Discovered, txt::record, mdns::{Advertiser, Browser},
udp::{Responder, probe}}`; `Node::{serve_discovery, discovered, discovery_facts}`.

**Tests** (`tests/discover.rs`): `test_spec_6_1_txt_record_carries_the_full_key_set_and_stays_under_1300_bytes`,
`test_spec_6_1_apps_is_truncated_with_an_ellipsis_when_over_budget`,
`test_spec_6_1_subtypes_only_for_advertised_slugs_of_15_chars_or_less`,
`test_spec_6_1_instance_name_is_the_display_name_or_the_node_id`,
`test_spec_6_4_udp_probe_is_answered_with_the_txt_key_set` (over loopback),
`test_spec_6_4_udp_refuses_a_public_source_and_answers_once_a_second` (the source check
as a pure function over addresses, the rate limit over a fake clock),
`test_spec_6_5_mdns_and_udp_start_together_and_stop_together`,
`test_spec_6_1_pair_flag_flips_when_pairing_opens`,
`test_spec_6_1_two_nodes_with_one_name_are_distinct_by_id` (two registrations carrying
the same instance name and different `id`s browse as two entries — the roadmap's
"distinguishable by ID, not name"),
`test_discovery_settings_disable_each_mechanism`,
`test_spec_6_1_mdns_registration_is_browsable_and_keyed_by_id` — a real daemon browsing
its own registration; runs on every platform, and if a CI runner proves to have no
multicast, it is gated by `PRIVATIUM_TEST_MDNS=1` in the same PR and stays in the manual
pass, with the CI log as the evidence (risk R10). In `crates/privatium/tests/cli.rs`:
`test_cli_no_discovery_starts_nothing`.

**Documentation:** `protocol.md §6.4` is written (row 11); `docs/deployment.md §4.1` (UDP
5353 and 52525 both named); `docs/connectivity.md §1` (the browser row's "owner types the
IP" becomes "scans the QR").

**Acceptance checklist** — Windows runs are green; check after the named tests pass on
all three platforms:

- [x] The record, its budget, the instance name and the subtype rule:
  `test_spec_6_1_txt_record_carries_the_full_key_set_and_stays_under_1300_bytes`,
  `test_spec_6_1_apps_is_truncated_with_an_ellipsis_when_over_budget`,
  `test_spec_6_1_instance_name_is_the_display_name_or_the_node_id`,
  `test_spec_6_1_subtypes_only_for_advertised_slugs_of_15_chars_or_less`.
- [x] The UDP fallback and its refusals:
  `test_spec_6_4_udp_probe_is_answered_with_the_txt_key_set`,
  `test_spec_6_4_udp_refuses_a_public_source_and_answers_once_a_second`.
- [x] Together, stopped together, switched by settings, and the pair flag from one source:
  `test_spec_6_5_mdns_and_udp_start_together_and_stop_together`,
  `test_discovery_settings_disable_each_mechanism`,
  `test_spec_app_contract_6_serve_discovery_runs_from_the_nodes_facts`,
  `test_spec_6_1_pair_flag_flips_when_pairing_opens`.
- [x] Browsing keyed by ID, with a real daemon:
  `test_spec_6_1_two_nodes_with_one_name_are_distinct_by_id`,
  `test_spec_6_1_mdns_registration_is_browsable_and_keyed_by_id` (risk R10).
- [x] The CLI: `test_cli_no_discovery_starts_nothing`; the example apps:
  `test_spec_cli_2_first_run_writes_the_example_apps`,
  `test_spec_cli_4_new_examples_writes_all_three_and_never_overwrites`,
  `test_new_from_hello_works_without_a_checkout`,
  `test_spec_cli_4_embedded_examples_match_the_repository_apps`.

---

### M19 — The devices page, the pairing UI, `privatium pair`, and the documents

**Implementation status, 2026-09-06, branch `m19-devices-shell`:** implemented. The
node page carries the display-name form, the LAN URL beside the loopback one and the
nodes `Node::discovered()` has seen, by ID; the devices page lists every active device
with its label and user agent escaped, a label form and a revoke button per device, and
the *Open pairing* button; the code page (`/settings/devices/pairing`) shows the four
glyphs with their labels beneath, the two words, the QR code as an inline SVG image with
the URL as text beside it, the seconds remaining, *Close pairing* and the `§7.7`
sentence, and asks htmx to swap itself every five seconds while the window is open so a
replaced code appears without a reload and nothing auto-dismisses. `POST` and `GET
/api/v1/pair` answer the `PairingSnapshot` for the owner alone; `privatium pair` asks
them over a hand-written loopback HTTP/1.1 exchange (`crates/privatium/src/pair.rs`),
prints the code both ways and the QR code in block characters, polls every two seconds,
and exits 0 naming the device or 1 on expiry; `--open` on a bare run prints the QR code
of the LAN URL and, on a node whose `sys_device` holds no row but its own, opens one
window and prints the code beneath it. The bootstrap document now carries the pairing
screen's markup — sixteen labelled keys, the word field, the optional device name, the
`role="status"` region — hidden until `client.js` finds no `pv:device`; `client.js`
wires it (`pairingScreen`), says the three outcomes there, and its `§8.1` refusal screen
names the space and keeps one action and no dismiss. `req.device`, `pv.device()` and
`/api/node`'s `dev` were already the session's since M17; `peers` now counts active
`kind = 'node'` rows other than this node. `--version` claims `pv/1 (partial: phase 2)`.

Five shapes differ from the sketch below, none of them wire meaning. **Owner standing
for every act:** `§9.2` makes `/api/v1/pair` the owner's alone, and the same standing —
no channel session — is required to open or close pairing from the page, to name the
node, and to label or revoke a device; a paired session sees the devices page without
the forms. Fail closed; loosening it later is a one-line change. **A revocation closes
the channel at once** through a broadcast the `Handler` carries (`revoked`), which
every open channel listens to, rather than at the device's next frame. **The pairing
screen is server-rendered** inside the bootstrap so `tests/common/a11y.rs` holds its
markup; the client toggles and wires it. **No `pair.css`, no `http/auth.rs` change, no
`tests/pairing.rs`:** the styles are appended to `shell.css`, the owner check reads the
`Session` extension in `wire/owner.rs`, and the socket tests live in
`crates/privatium/tests/channel.rs` beside the fixture they share; the registry edits
live in `registry.rs` and the QR renderers in `pair/qr.rs`. **`last_seen_at` at the
handshake follows the hourly rule** (§3 row 31), since every full-page navigation opens
a channel. The QR code is drawn from `qrcode` 0.14.1's module matrix with its default
features off — no `image` crate — so the SVG carries `role="img"`, a `<title>` and
`focusable="false"` as `docs/icons.md` asks, and the terminal rendering is half-block
characters with the standard's quiet zone. `PRIVATIUM_TEST_NO_BROWSER` is the test-only
variable that keeps `--open` from launching a browser under test.

The shell's user-facing vocabulary calls a node a *space*, following the UI change that
merged just before this milestone; the spec, the code and this plan keep *node*.

**Verification on Windows:**

| Command | Outcome |
|---|---|
| `cargo test -p privatium-core --locked --test devices` | 7 passed |
| `cargo test -p privatium --locked --test channel --test cli` | 16 and 24 passed |
| `cargo test --workspace --locked` | 600 passed, 0 failed; 2 intentional fixture-generator ignores |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo xtask header-check` | 332 files passed |
| `cargo xtask gen-skill-reference` | Regenerated 21 files; the Tier 1, Tier 2 and Tier 3 references changed |
| `cargo xtask gen-skill-reference --check` | 21 files match |
| `cargo xtask lint-spec-refs` | 37 rules resolve |
| `node --test crates/privatium-core/tests/js/*.test.mjs` | 59 passed, 2 of them new; Node 26 on PATH |
| `cargo deny check` | Passed; advisories, bans, licences and sources OK with `qrcode` 0.14.1 added |
| `bash .github/scripts/conformance.sh` | Every named test passed, the `devices` binary's included, using Git Bash |
| `privatium lint apps/hello apps/animals apps/sketch` | 0 findings |

The checklist below is ticked on the three-platform run recorded under "Phase 2
hardening".

- Settings, node page: a display-name form (§3 row 16) → `sys_node.display_name`;
  "Listening" shows the LAN URL; "Nodes on this network" lists `Node::discovered()` by ID.
- Settings, devices page: every active device with kind, replica, label, paired, last
  seen; a label form and a **Revoke** button per device (`POST
  /settings/devices/<id>/revoke`, `csrf()`, a `put` with `revoked_at` and
  `revoked_reason`, never a `del` — `§3.2`; `device.revoked` audit; the device's open
  channel closed at once); **Open pairing** → `POST /settings/devices/pair` → the code
  page: the four glyphs with their labels beneath, the two words, the QR as inline SVG
  with the URL as text beside it, the seconds remaining, a *Close pairing* button, and
  the sentence `§7.7` asks for when the page is served over plain HTTP. `last_seen_at`
  written at the handshake and then at most hourly (§3 row 15).
- `POST /api/v1/pair` and `GET /api/v1/pair` (§3 row 5), this node's own device only.
- `privatium pair [--open] [--timeout 120]` (`crates/privatium/src/pair.rs`): POST to the
  running node on loopback, print the glyphs with labels, the words, the QR in Unicode
  blocks and the URL as text, poll `GET /api/v1/pair` every two seconds, exit 0 on success
  naming the device, 1 on expiry; `--open` opens the devices page. No node → a runtime
  error naming `privatium`. `--open` on a bare run per §2.8: on a node with no paired
  device the QR is printed with a code beneath it.
- The pairing screen in `client.js`: the emoji pad — sixteen buttons, glyph as text with
  its label beneath, `aria-label` the label, 44-pixel targets — and the word field, both
  visible at once, the node's name and ID above, the plain-HTTP sentence, and three
  outcomes said in a `role="status"` region: paired (then the requested path loads), wrong
  code with attempts left, closed or expired with what to do. A `<noscript>` never reaches
  here. Completable without reading (tap four glyphs) and without seeing (type two words,
  every control labelled), which `AGENTS.md` makes a requirement rather than a choice.
- `§8.1`'s refusal screen: full-page, both fingerprints, the node's name, no dismiss, one
  action — forget and pair again.
- `req.device`, `pv.device()` and `/api/node`'s `dev` are the session's device;
  `pv.node().peers` and `/api/node`'s `peers` count paired nodes (§2.11).
- `--version` claims `pv/1 (partial: phase 2)`; `protocol_claim()` in `main.rs` and the
  two CLI tests that read it.
- `xtask gen-skill-reference`: the sentence about the four methods now says two remain.

**Tests:** `test_settings_devices_lists_paired_devices_and_revokes_one`,
`test_spec_3_2_revocation_is_a_put_never_a_del`,
`test_spec_3_2_last_seen_at_is_written_at_most_hourly`,
`test_settings_node_display_name_is_set_by_the_owner_and_reaches_the_manifest`,
`test_spec_9_2_manifest_pair_flag_is_true_while_open`,
`test_spec_7_7_plain_http_pairing_page_discloses_the_gap`,
`test_spec_cli_5_pv4xx_pairing_and_devices_pages` (`tests/common/a11y.rs` over the code
page, the bootstrap page and the pairing screen's markup),
`test_spec_lua_3_4_device_and_peers_come_from_the_session`; in
`crates/privatium/tests/`: `test_spec_cli_8_pair_prints_the_code_and_exits_on_success`,
`test_spec_cli_8_pair_without_a_node_is_a_runtime_error`,
`test_spec_cli_2_open_prints_a_qr_and_the_lan_url`,
`test_spec_cli_2_open_on_an_unpaired_node_opens_one_window`,
`test_spec_8_1_a_reinitialized_node_is_refused_by_a_paired_client` (the Rust channel
client pairs, the node's `identity/` is deleted and the node restarted — a new node key
and a new cluster — and the client's handshake fails the certificate check with no path
past it; the roadmap's "changing the node key" bullet),
`test_spec_cli_1_version_qualifies_protocol` (now `phase 2`). Under `node --test`:
`client.test.mjs` — both renderings accepted, the pad and the field produce the same
sixteen bits, the refusal screen has no dismiss.

**Manual pass, recorded in the PR:** a phone on the LAN, from scan to first page under
twenty seconds; the word path with VoiceOver or TalkBack and images disabled; keyboard-only
through the devices page and the code page; 200 % zoom; Wireshark beside the automated
proxy test.

**Documentation:** `protocol.md §6.1, §7.1, §9.2`, `cli.md §2, §8`, `data-dictionary.md
§3.2`, `lua-api.md §3.4` and `data-api.md §4` are written (rows 5, 10, 12, 15, 16);
`lua-api.md §3.1`'s sentence on `device` drops its Phase 1 clause; `README.md` quick start
step 2 and the status paragraph; `docs/security.md §2.2`;
`skills/privatium-overview`, `-tier1-lua`, `-tier2-web`, `-tier3-rust`, `-security`,
`-accessibility` (the pairing screen's two paths, now real); `apps/*/README.md` where
they name Phase 2; `spec/protocol.md`'s status line.

**Acceptance checklist** — Windows runs are green; check after the named tests pass on
all three platforms:

- [x] The owner's pairing window and the manifest's flag; a session refused:
  `test_spec_9_2_manifest_pair_flag_is_true_while_open`,
  `test_spec_9_2_pair_route_refuses_a_session`.
- [x] The code page and the bootstrap disclose `§7.7`, and every new page meets the
  PV4xx rules: `test_spec_7_7_plain_http_pairing_page_discloses_the_gap`,
  `test_spec_cli_5_pv4xx_pairing_and_devices_pages`.
- [x] Devices listed, labelled and revoked with nothing lost, the channel closed at once,
  the mark hourly: `test_settings_devices_lists_paired_devices_and_revokes_one`,
  `test_spec_3_2_revocation_is_a_put_never_a_del`,
  `test_spec_3_2_revoking_a_device_closes_its_open_channel_at_once`,
  `test_spec_3_2_last_seen_at_is_written_at_most_hourly`.
- [x] The display name reaches the manifest and the discovery facts:
  `test_settings_node_display_name_is_set_by_the_owner_and_reaches_the_manifest`.
- [x] The session's device and the paired-node count:
  `test_spec_lua_3_4_device_and_peers_come_from_the_session`,
  `test_channel_requests_carry_the_session_device`.
- [x] A re-keyed node refused, the refusal screen without a dismiss, the pad and the
  field one code: `test_spec_8_1_a_reinitialized_node_is_refused_by_a_paired_client`,
  `test_spec_8_1_refusal_screen_has_no_dismiss`,
  `test_spec_7_2_pad_and_word_field_yield_the_same_sixteen_bits` (JavaScript).
- [x] The CLI: `test_spec_cli_8_pair_prints_the_code_and_exits_on_success`,
  `test_spec_cli_8_pair_without_a_node_is_a_runtime_error`,
  `test_spec_cli_2_open_prints_a_qr_and_the_lan_url`,
  `test_spec_cli_2_open_on_an_unpaired_node_opens_one_window`,
  `test_spec_cli_1_version_qualifies_protocol`.

**Manual pass still owed** (recorded here so it is not lost in the pull request): a
phone on the LAN from scan to first page under twenty seconds; the word path with
VoiceOver or TalkBack and images disabled; keyboard-only through the devices page and
the code page at 200 % zoom; Wireshark beside the automated proxy test (R16). What was
checked without a person: the PV4xx rules and the document checks over every new page
and the bootstrap, the `§7.7` sentence on both, the sixteen keys named by their labels
alone, the focus ring and the 44-pixel targets inherited from the shell's stylesheet.

---

### Hardening after M19

Phase 1 needed four rounds after its last milestone; expect at least one here. The review
reads every path a non-loopback peer can take through `auth.rs` and `channel.rs` against
OWASP ASVS 5.0 V2 and V9, the pairing state machine against `§7.5`'s three limits under
concurrency, and the client script against the CSP with the browser console open on each
of the three platforms' browsers plus a phone. Fix the spec in the same PR, as always.

**Phase 2 hardening, 2026-09-06, branch `phase2-hardening`:** the first round. The
three-platform run of `main` at `8ca618d` (CI run 34069110056) passed every named test
of M16–M19, and their checklists below are ticked on it; risk R10 closed on the same run,
since the real-daemon mDNS test passed on all three runners ungated. The review read
`auth.rs`, `channel.rs`, `owner.rs`, `handoff.rs`, the pairing and session modules, the
discovery modules, the registry, the CLI's `pair` client and `--open`, and the four
client scripts, against ASVS 5.0 V2 and V9. What it found, all fixed in this round with
the spec edited where it was silent (§3 rows 32–35):

- **An accepted `pA` outlived its code and its window.** The node's side of an attempt
  was not bound to the code generation, and `cA` was verified without looking at the
  window, so after five failures replaced the code — or after another device consumed
  the window, or after it expired — a `cA` computed against the old code still verified
  and the node sealed its message; a sealed message arriving after the window closed was
  opened before the window was checked. Now the exchange carries the generation, the
  confirm refuses 4429 after a replacement and 4404 after consumption or expiry before
  the message is read, the registration checks the window and the registry before the
  sealed message is opened, and each refusal is one audited failure —
  `test_spec_7_5_an_attempt_begun_before_a_rotation_cannot_finish_after_it`,
  `test_spec_7_4_a_second_attempt_cannot_finish_once_the_window_is_consumed_or_expired`.
- **The devices page put a device ID into an attribute unescaped.** The label and revoke
  form actions were built from `sys_device.id` as the log holds it, and the log is
  untrusted input (`AGENTS.md`). A row whose ID is not shaped as a Node ID now gets no
  form, and the ID is escaped in every attribute —
  `test_spec_3_2_devices_page_never_trusts_a_device_id_from_the_log`.
- **A `null` `revoked_at` read as a revocation.** `revoke_device` tested for the key's
  presence, so a row that spelled the null out — which `spec/data-dictionary.md §2.1`
  makes the same value as an absent key — could never be revoked. Fixed and held by
  `test_spec_3_2_last_seen_at_is_written_at_most_hourly`, which now also writes over a
  mark from the future and an unreadable one.
- **The UDP responder was an amplifier.** Its per-source table grew without bound for a
  minute under spoofed private-range sources, each answered once. A budget of 32 answers
  a second across every source and a cap of 1024 remembered sources, refusing strangers
  while the table is full of fresh entries and never a source already known —
  `test_spec_6_4_a_probe_storm_is_bounded_by_a_global_budget_and_a_source_cap`.
- **Records off the wire were kept unread.** `txt::read` accepted any `id`, `cl`, `nm`
  and `apps`. An ID or a cluster ID that is not shaped as one is not a record; the name
  is bounded as an instance name is; `apps` keeps slugs alone, at most 64 —
  `test_spec_6_1_a_record_off_the_wire_is_validated_and_bounded`. The mDNS tests now
  register IDs shaped as Node IDs, since the reader refuses everything else.
- **A silent pairing peer held a task for two minutes.** `/ws/pair` was bounded by the
  window's TTL alone, where `/ws` had a ten-second bound. Both bounds live in
  `ChannelTimeouts` on the handler — thirty seconds for pairing — and a test shortens
  them: `test_spec_7_4_a_silent_pairing_peer_is_closed_without_an_attempt`.
- **The node's own row could start a `/ws` handshake.** Its `sys_device` row carries an
  X25519 key since the identity amendment, and nothing refused `dev` equal to the node's
  ID; without the node's private static nothing could finish, but the check was after
  the hello rather than before. Refused before the hello —
  `test_spec_8_3_the_nodes_own_row_cannot_open_a_channel`.
- **The pairing screen mis-said a close.** `pair.js` rejected every close with "the node
  closed the connection", which the screen read as *pairing is closed*; an exhausted
  code at `pA` was therefore told to open pairing. Each close code of `§7.4.2` now has
  its own sentence and the screen says it —
  `test_spec_7_4_2_close_codes_are_said_to_the_person_without_the_nodes_words`,
  `test_spec_7_2_pad_and_word_field_yield_the_same_sixteen_bits`.

Reviewed and found sound, so recorded rather than re-argued: the auth layer's refusal
before a read on every non-loopback path, with the `Host` check on loopback and the
cross-site refusal on every route; the WebSocket `Origin` check; the frame decoder's
bounds and its header allowlist; the handoff's atomic single consumer and its expiry
timer; the revocation broadcast with the lagged-receiver fallback; the hourly mark
decided under one lock hold; the rate limit and attempt counter behind the node mutex
and never across an await (R15); `privatium pair` speaking to loopback alone with a
bounded answer; `--open` opening a window only while no device has ever paired. Accepted
risks are R18 and R19 in §8. Deferred as performance rather than security: the channel
re-checks the device's standing with two registry reads per outbound frame, which the
revocation broadcast already makes redundant in the common case.

The Windows portable zip: the release workflow has never run — v0.1 was published two
hours before it reached `main`, and its four assets were uploaded by hand, the portable
zip last. `release.yml` now also runs on demand against a named tag, so binaries can be
attached to an existing release without a new one, and the asset list lives in
`release_tools.py` under test. The README links the v0.1 assets by tag, which resolve;
the `latest` form does not while v0.1 is a prerelease. The first release the workflow
served, v0.19-beta1, was published under three minutes after its commit reached `main`,
while push CI for that commit was still running, and the gate refused it; the gate now
waits for a run in progress (`test_ci_still_running_is_pending_not_refused`,
`test_wait_for_ci_polls_until_the_run_completes_and_gives_up_at_the_deadline`) and
refuses only a run that failed, was cancelled, or never existed.

---

## 7. Conformance mapping

Phase 2 can satisfy these lines of `protocol.md §13`, and `.github/scripts/conformance.sh`
gains a `run` per binary naming the tests:

| Checklist item (§13 wording) | Milestone | Test |
|---|---|---|
| Advertises `_privatium._tcp` with the full TXT key set (§6.1) | M18 | `test_spec_6_1_txt_record_carries_the_full_key_set_and_stays_under_1300_bytes`, `…_mdns_registration_is_browsable_and_keyed_by_id` |
| Runs all configured discovery mechanisms concurrently, not chained (§6.5) | M18 | `test_spec_6_5_mdns_and_udp_start_together_and_stop_together` |
| UDP fallback refuses non-private source addresses (§6.2) | M18 | `test_spec_6_4_udp_refuses_a_public_source_and_answers_once_a_second` |
| Pairing requires explicit owner action (§7.1) | M16 | `test_spec_7_1_pairing_is_closed_until_opened_and_closes_on_first_success` |
| Pairing code is node-generated, 16-bit, 120s TTL, 5 attempts (§7.2, §7.5) | M16 | `test_spec_7_2_code_is_16_bits_…`, `test_spec_7_5_code_expires_at_120s_…` |
| Both emoji and word encodings accepted (§7.2) | M16 | `test_spec_7_2_word_input_is_case_and_punctuation_insensitive`, `…_glyph_labels_are_accepted_as_input` |
| Variation selectors preserved on glyphs 8 and 9 (§7.3) | M16 | `test_spec_7_3_glyph_table_is_normative_and_keeps_variation_selectors` |
| The pairing code is never transmitted as a bearer credential (§7.0) | M16 | `test_spec_7_0_the_code_never_crosses_the_wire` |
| Pairing state persists per origin; loss requires re-pairing with no bypass (§7.6) | M17 | `client.test.mjs` |
| Plain-HTTP pairing screens disclose the property-1 gap (§7.7) | M19 | `test_spec_7_7_plain_http_pairing_page_discloses_the_gap` |
| Pinned key mismatch has no override path (§8.1) | M17, M19 | `test_spec_8_1_a_static_key_that_is_not_the_pinned_one_fails_the_confirm`, `test_spec_8_1_a_reinitialized_node_is_refused_by_a_paired_client`, `client.test.mjs` |
| Session layer never skipped on plain HTTP (§8.2) | M17 | `test_spec_8_2_lan_socket_carries_no_plaintext_app_data` |
| Plain HTTP from a non-loopback peer serves only the bootstrap set (§8.4) | M17 | `test_spec_8_4_plain_http_on_the_lan_serves_only_the_bootstrap_set` |
| Framework-named scripts and stylesheets carry integrity through the channel (§8.3) | M17 | `test_spec_8_3_page_frame_scripts_carry_integrity` |
| Cluster private key never leaves nodes; devices receive the public key only (§2.3.3) | M14, M16 | `test_spec_2_3_3_cluster_private_key_is_absent_…`, `test_spec_7_4_pairing_completes_and_writes_the_device_row` |
| Node certificates expire at 180 days (§2.3.1) — the expiry half | M14 | `test_spec_2_3_1_certificate_verifies_…_expires_at_180_days` |
| Cluster-key mismatch has no override path (§2.3.2, §8.1) | M19 | `test_spec_8_1_a_reinitialized_node_is_refused_by_a_paired_client`, `test_spec_8_1_refusal_screen_has_no_dismiss` (`client.test.mjs`) |
| Browser clients hold exactly one endpoint (§10.4, §10.8) | M17 | `test_spec_10_4_browser_client_holds_exactly_one_endpoint` |
| `sys_device.replica` declared accurately (§10.7) — browsers | M16 | `test_spec_7_4_pairing_completes_and_writes_the_device_row` |
| No SAS confirmation step exists (§7.8) | M19 | Reviewed with M19: the pairing screen (`http/pairing.rs`, `client.js`) and `pair.js` carry no comparison step; `AGENTS.md` already forbids adding one |
| Pairing requires explicit owner action — the owner's standing alone opens it (§7.1, §9.2) | M19 | `test_spec_9_2_manifest_pair_flag_is_true_while_open`, `test_spec_9_2_pair_route_refuses_a_session` |

Phase 2 **cannot** claim: renewal on sync (§2.3.1), an unmet node trusted by a pinned
device (§2.3.2), discovery filtered by `cl` once paired (§6.1 — a browser cannot browse,
and the node's own filter needs a second node), and everything of §10 and §6.2–§6.3. Those
are Phase 3 and Phase 5.

---

## 8. Risks

**R9 — The PAKE.** Two implementations of RFC 9382 written here, in two languages, with no
third to check against. Mitigations: the vector file both sides read; a test that a wrong
code yields no key on either side; the RFC's security considerations read against the
transcript before M16 opens; and §2.4 flagged for a second pair of eyes. If a JavaScript
SPAKE2 or CPace with an audit appears before M16 starts, prefer it and re-decide.

**R10 — Multicast on CI.** GitHub's runners may drop mDNS. M18's real-daemon test runs
everywhere first; the CI log decides whether it is gated, and the gate is an environment
variable named in the test, not a silent skip.

**R11 — htmx's internal API.** The extension leans on `api.swap` and its neighbours, which
htmx documents for extension authors but does not promise as stable. Pin htmx at 2.0.9 as
`VENDOR.md` does; the extension is a hundred lines, and a change in htmx is a day, not a
redesign.

**R12 — Twenty seconds on a phone.** Scan, load a page with a hundred kilobytes of crypto
modules, tap four glyphs. The budget is real on a slow phone on a busy network. Measure
in M19's manual pass; if it fails, the first lever is caching `/static/*` with a long
`max-age` and `immutable` (the assets carry integrity and are versioned by build), not a
minifier.

**R13 — Windows network profiles.** A home Wi-Fi classified *Public* blocks the node after
the owner clicked Allow (`docs/deployment.md §4.1`). Phase 2 documents it in the quick
start and on the node page; the helper is Phase 6.

**R14 — Browser storage eviction.** iOS Safari clears storage a site has not used for a
week (`§10.7`). The device re-pairs; the plan says so on the pairing screen rather than
pretending otherwise.

**R15 — Rate limits and the lock.** `§7.5`'s per-source limit and the attempt counter are
touched from WebSocket tasks; they live in the node behind its mutex, taken for
microseconds, never across an await — the discipline `wire/mod.rs` already states.

**R17 — Per-app subtypes wait on the library.** `mdns-sd` 0.21 carries one subtype per
registration and keys registrations by full name, so `_<slug>._sub._privatium._tcp`
cannot be advertised for more than one app without breaking `§6.1`'s instance-name
rule. The parent type with the full TXT record — the MUST — is advertised; the subtype
SHOULD is computed (`Facts::subtypes`) and not on the wire. A client that browses for
one app filters the parent type's `apps` key instead, which the TXT record was designed
for. Revisit when the library grows a multi-subtype registration or when a second
implementation makes the fork worth owning.

**R16 — The Wireshark bullet is a manual claim.** The proxy test proves the socket carries
no known plaintext; a person with Wireshark still looks, in M19's manual pass, because a
test only finds the strings it was told to look for.

**R18 — Same-origin app code holds the owner's standing.** Apps share the framework's
origin (`AGENTS.md`, the CSP note), so a Tier 2 app's JavaScript running in the owner's
browser on loopback can call `POST /api/v1/pair` with a JSON body, or read the devices
page and its form token, and so open pairing or revoke a device as the owner. This is
the posture `docs/security.md §7` already states — an installed app folder is code run
on the owner's node — and not a gap the owner-standing rule was meant to close; the
hardening round records it rather than adding a second fence that per-app origins
(`docs/security.md §7`) would make redundant. Accepted.

**R19 — The browser console pass is still a person's.** The scripts were checked against
the default CSP by the tests under `node --test` and by the live-core test; the console
open in Chrome, Firefox, Edge and Safari, and on a phone, remains the manual pass M19
recorded, since no test here runs a browser. Accepted until that pass is written up.

---

## 9. PR sequence

| # | Branch | Depends on | Spec edits |
|---|---|---|---|
| 16 | `m14-cluster-identity` | Phase 1 | as found — §3 is written |
| 17 | `m15-session` | M14 | as found |
| 18 | `m16-pairing` | M15 | as found |
| 19 | `m17-channel-lan` | M16 | as found |
| 20 | `m18-discovery` | M17 | as found |
| 21 | `m19-devices-shell` | M18 | as found; roadmap: tick Phase 2 |
| 22 | `phase2-hardening` | M19 | as found |

---

Copyright © 2026 Gabriel Mongefranco
