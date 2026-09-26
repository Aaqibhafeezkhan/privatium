<!--
This file is part of Privatium
docs/decisions/0007-household-profiles.md
Author(s): Gabriel Mongefranco
Created: 2026-09-07
Last Modified: 2026-09-26
Summary: Decision record. Household profiles as a pv/1 partition, at-rest encryption declined, an
         ephemeral app channel for per-frame traffic, no profile merge, and the reservations
         that keep real multi-user reachable in pv/2. Status: DECIDED; the pv/2 model is
         DEFERRED.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# ADR 0007 — Household profiles, segmented data, and the pv/2 multi-user path

**Status: DECIDED. The `pv/2` multi-user model is DEFERRED, and the reservations of
section 5 are held open deliberately. D9 and D11 are withdrawn: there is no profile merge.**

**Scheduled as Phase 3c, planned as M27 of `docs/plans/phase-3.md`.** The six questions an
earlier revision left open are decided; section 7 says where each went. The milestone's
body can be written from this record.

**Nothing here is implemented by M22 through M26.** M20 and M21 have landed, so the sync
wire this record changes is shipped surface rather than surface being written: section 6's
rows are edits to what exists. An agent reading this while an earlier milestone is open
should change nothing but this file.

## Context

`pv/1` is single-owner by construction. `docs/architecture.md §8` lists multi-user
accounts as deliberately absent, and `docs/roadmap.md` lists multi-user sharing under
"Explicitly not on the roadmap". Both entries stand as descriptions of `pv/1`.

What changed is intent, not scope. The owner now intends real multi-user in `pv/2`, and
wants a household-facing feature sooner. Two things drive it: family members who each
want their own view of an app, and apps that exchange state between nodes many times a
second, which the log cannot carry. This ADR answers one question: **which `pv/1`
decisions would make either of those unreachable, and what does `pv/1` reserve so that it
does not?**

Nothing here bolts multi-user onto `pv/1`. Section 3 is a partition and a UI. Section 5 is
a short list of reservations that cost almost nothing now and cannot be retrofitted later,
because log lines are immutable, `pv/1` never rewrites a log, and `migrations/` is
reserved and unimplemented (`spec/data-dictionary.md §3.11`).

## 1. Terms, used precisely throughout

| Term | Meaning |
|---|---|
| **Profile** | A household member as `pv/1` models them. A partition key. |
| **Principal** | An identity that can be *authenticated and proven to a third party*. `pv/2`. |
| **Segment** | The on-disk subtree holding one profile's data for one app. |
| **Owner** | Unchanged from `spec/protocol.md §1`. The single human who controls the cluster. |

The distinction that matters: a partition key answers "which rows are hers." A principal
proves "this is her." Inside one trust domain the first is sufficient. Across trust
domains only the second works, and that is the entire content of the `pv/1` to `pv/2` gap.

## 2. What is being built, stated honestly

> Profiles keep household members out of each other's data *in the app*. They do not hide
> anything from anyone with access to the node's files. Everyone's data is backed up on
> every node equally, and data that reached a cluster stays in that cluster: leaving the
> household does not unwrite it.

That paragraph is normative UI copy, not commentary. Following D8 it is permanently true
rather than provisional pending encryption, and following D18 the last clause is true as
well; it should be written that way.

It is the same deal a family NAS or a shared media server offers, and it is defensible
precisely because it is stated rather than implied. Same disclosure posture as
`spec/protocol.md §7.7`, which requires an implementation to state the plain-HTTP
bootstrap exposure rather than imply it away, and `§4.6`, where a tombstone is openly not
a deletion.

## 3. Decisions — `pv/1`

### D1. Profiles are a partition. They are never called accounts.

A PIN-gated selector choosing a partition key is not an access-control boundary on a node
whose `data/` is plaintext JSONL and whose cluster key is held by every node. Calling it
an account makes a promise the storage layer does not keep.

### D2. Enforcement lives in the framework, not in apps. Default deny.

The query layer scopes every read to the active profile before app code sees a row. An app
that wants otherwise declares it (D14). Rationale: N app-level implementations means the
partition is only as strong as the worst one, and the failure is silent.

**This is not the first per-subject access check in the system, and it must not duplicate
the one that exists.** `sys_app_grant` (`spec/data-dictionary.md §3.5`) already answers
"which devices may access which apps", with `device_id` and `app_id` each accepting `*`,
resolution by most-specific match, and `write` as the default when no row matches. Its
stated purpose is this exact scenario — hiding the medication tracker from the kitchen
tablet. Profile scoping is a **row filter within an app**; `sys_app_grant` is **whether
the app opens at all**. They compose: the grant decides the mount, the segment decides the
rows. Any implementation that finds itself re-deriving grant resolution has taken a wrong
turn.

**The grant gains a profile subject, and it gains it in `pv/1`.** A household's actual
request is "the children do not see the medication tracker", which is app visibility, not a
row filter — a segment cannot express it, because a segment only hides rows inside an app
already open. So `sys_app_grant` grows a `profile_id` alongside `device_id` and `app_id`,
each still accepting `*`, resolved by the same most-specific-match rule it uses today, with
`write` still the default when nothing matches. Where two rows tie on specificity the order
is **profile, then device, then app** — the profile is the more particular fact about who
is asking, and the shared tablet is exactly the case that needs it to win.

This is not the ACL that `spec/protocol.md §14` item 3 rules out. That item is about
sharing across trust domains, where the answer is a capability model; this is one owner's
own visibility table gaining the third column the household case needs. An earlier
revision reserved this shape rather than building it; section 5 records why the
reservation is gone.

**Tier 3 is explicitly out of scope for this enforcement.** A Tier 3 app links
`privatium-core` and is not sandboxed; it can already read `identity/node.key` and every
log file directly. Profile scoping there is a convenience of the core API, not a boundary,
and the Tier 3 skill must say so in the same terms it already uses for filesystem access.

The scope belongs on the calls that read and write, not on `Node::open`. `Node::open`
takes a data root (`spec/app-contract.md §2.3`) and embedded mode has no session to carry
a profile; `query`, `append` and `open_app` already take a slug, and the segment travels
beside it. A Tier 3 app that passes no segment sees everything, exactly as it does today.

### D3. Profile is a property of the session, and switching is the shell's job.

**Problem this resolves:** a solo-mode or Tier 2 app renders no framework chrome by
design, because `docs/architecture.md §7` promises solo mode is indistinguishable from a
purpose-built application. If profile display required a Privatium header, solo mode would
be dead and Tier 3 would be unreachable.

Resolution, in six parts, none of which mandates chrome:

1. **The profile is selected before the app loads.** A session carries exactly one active
   profile, chosen at the PIN screen. An app never chooses, and cannot change, the profile
   it runs under.
2. **The switcher lives under `/settings`, and adds no route prefix.** `spec/protocol.md
   §9.1` reserves exactly six framework prefixes and `/settings` is one of them; it
   survives solo mode, where a colliding app route is shadowed in favour of the framework
   with a warning at load. A switcher at `/settings/profile` therefore needs no change to
   §9.1 at all. **`pair` is not a precedent for a seventh prefix** — it is a reserved slug
   (§1.1) covering the mounts of `/ws/pair` and `/api/v1/pair`, and there is no top-level
   `/pair` route. Any app links the switcher with `url()` or `pv.url()`.
3. **`profile` is added to the reserved slug list** (`spec/protocol.md §1.1`) — not
   because the route design needs it, which under (2) it does not, but because a slug is a
   one-way door: reserving it now costs nothing, and reserving it after someone has
   installed an app called `profile` breaks that app. §1.1's closing sentence names the
   last two entries as route prefixes and must be reworded when the list grows.
4. **`GET /a/<slug>/api/node` and `pv.node()` gain the active profile's ID, display name,
   and role.** The response today carries `id`, `dev`, `name`, `app`, `solo`, `peers` and
   `restore_tier` (`spec/data-api.md §4`); the profile joins it as a sibling of `dev`, and
   is `null` on a node with no profiles configured. Apps MAY display it; the launcher and
   the native shells MUST.
5. **The node remembers the last profile per device, and a reconnect resumes it.**
   `spec/protocol.md §8.3` is explicit: "The session is the connection: no cookie carries
   it, and a new connection is a new handshake", and the handshake authenticates `dev`
   alone. A profile held only against the WebSocket would be lost every time the network
   blipped, which throws a child back to the PIN screen mid-game. Usability wins: the node
   holds the last profile chosen on each device and a new handshake resumes it, with no
   PIN. Selecting a *different* profile asks for that profile's PIN; resuming the one
   already chosen does not.

   That memory is **node-local**, in `local/state.jsonl` beside the peer hints, never an
   event. It is a convenience, not a fact about the household, and `AGENTS.md` forbids
   syncing `local/`. Two consequences to accept: the same device paired to a second node
   starts at that node's picker, and clearing `local/` returns every device to the picker
   — both harmless, both better than replicating a UI preference to every machine.

   Stated plainly because it is a real posture: **a device left unattended stays in its
   profile.** This is the television model, the same one `§7.6` already applies to pairing
   — pair once, trusted thereafter. The switcher of part 2 is the way out, and it is
   reachable from every app.

6. **An app a profile may not open says so, and offers the switcher.** When
   `sys_app_grant` refuses a profile, the app is still listed and still reachable; opening
   it renders an explanatory screen naming the profile that is active and carrying a link
   to `/settings/profile`. A D13 lockout uses the same screen with a countdown. Hiding it
   from `sys.v_app_nav` was considered and refused: an app that vanishes teaches a
   household member that the node is broken, and a message teaches them to switch.

Consequence to accept rather than fix: a Tier 2 app owning its whole viewport can render
whatever it likes and is not obliged to tell the user which profile is active. It also
cannot read another profile's data, so this is a UX failure and not a security one.
`privatium lint` SHOULD warn when a Tier 2 app neither links `url('/settings/profile')`
nor calls `pv.node()` on a node with profiles configured. **There is no `fullscreen`
declaration in `app.toml` and this ADR does not add one** (`spec/app-contract.md §3`); the
real distinctions are the tier and the deployment mode, both of which already exist.

### D4. Household members SHOULD use thin clients. Member nodes remain supported.

A node holds the cluster private key and a complete plaintext replica
(`spec/protocol.md §2.3.3`). Admitting a member's laptop to the cluster gives that member
everything, on hardware the owner does not administer. Mechanically identical to running a
full node on a VPS, which `docs/deployment.md §2` already covers with its four-functions
table; the difference is that the VPS threat is compromise and this one is ordinary use.

**This is a recommendation, not a rule, and the documentation MUST present it as a
choice.** Households that share everything are the common case, and a second node is the
best answer for replication and availability. What the docs owe the owner is the
consequence stated plainly at the moment of admission, in the same register as §2's table:

| Member client | Holds | Can read | Replication contribution |
|---|---|---|---|
| Browser or PWA (`replica = false`) | nothing | only their own profile, via the node | none |
| Caching mobile | cached views, outbox | only their own profile | none |
| **Full node** | **complete plaintext replica, cluster key** | **everything, every profile** | **full** |

The admission flow (`spec/protocol.md §2.3.1`) MUST show the third row before the pairing
code is accepted. Recommend thin; do not forbid full. Which trade a household wants is not
the project's decision to make.

### D5. Every node replicates every segment. Sync is never scoped by profile.

Restricting sync inside a cluster buys nothing and costs durability. Every node holds the
cluster key and can request any log at any time, so a sync restriction is a preference
between machines that already fully trust each other, not a boundary.

Everyone's data backs up on every node equally. This is a stated project goal, not a side
effect.

### D6. Segment-per-profile on disk, not a column.

```
data/<slug>/<segment-id>/log/<device-id>.jsonl
data/<slug>/<segment-id>/snap/<snapshot-id>/
data/<slug>/_shared/log/<device-id>.jsonl
```

`_sys` is **not** segmented. It is the node's own registry — devices, apps, settings,
audit — and it belongs to the cluster rather than to a person.

Single-writer holds unchanged: the device is still the sole writer of its own file.

This decision was originally justified largely by per-segment encryption, which D8 now
declines. It was re-examined against a plain `profile` column and **stands**, on three
remaining grounds:

1. **Removal is otherwise structurally impossible.** `spec/protocol.md §4.6` states that
   the supported way to destroy data irrecoverably is to destroy `data/` — the whole
   directory, every app, every person. With a column, one person's rows are interleaved
   into files shared with everyone else's, so removing them would require rewriting a log
   and is therefore never possible at all. With a directory it is one path, and the
   append-only invariant is intact. **D18 bounds what this buys:** the act is local, no
   peer is ever told to follow suit, and a node that still holds the segment will hand it
   back. So this ground is the difference between *impossible* and *possible on a node the
   owner chooses*, which is real but smaller than it first appears, and grounds 2 and 3
   carry more of the weight than they did in an earlier revision.
2. **`pv/2` sharing is a subtree sync.** Sync is already a set union over files keyed by
   `(app, dev, seq)`. Scoping it to a subtree is natural; filtering every line by a field
   is a different algorithm.
3. **Isolation is verifiable with `ls`** rather than by reading the query planner.

Honest accounting of the cost, which is real: `seq` is already per `(app, device)`
(`spec/protocol.md §4.1`) and must become per `(app, segment, device)`, because
`spec/protocol.md §10.2` requires a receiver to refuse a gap in `seq`, and a device
writing to two segments of one app would otherwise produce gaps in both files. §10.2 also
requires app and device names to be validated before a path is constructed; the segment ID
becomes a third component under the same rule. That is a normative change to §10 and the
main complexity this decision buys.

**`lam` stays per app. It is not segmented.** `spec/protocol.md §4.3` gives each node one
Lamport counter per app, and that is correct as it stands. Segmenting it would leave
`(lam, ts, dev)` unable to order a `_shared` row against a private one — two counters that
never met, compared as though they had — and `§4.5` needs that ordering to resolve a
last-write-wins conflict on a shared table, which D14 makes routine rather than
theoretical. So `seq` is per `(app, segment, device)` and `lam` is per app, and the two
disagreeing about their grain is deliberate: `seq` names a position in one file, `lam`
orders events against each other across all of them. `§4.3` says so normatively.

The heads exchange carries `seq`, not `lam`, so it follows `seq`: `{slug: {dev: seq}}`
today becomes `{slug: {segment: {dev: seq}}}`. The segment gets its own level and is
**not** folded into the outer key as `"<slug>/<segment>"` — the reference receiver
validates a destination as `_sys` or a valid unreserved slug, `§1.1`'s pattern has no `/`,
and admitting one would loosen the guard that keeps a path separator out of a name used to
build a path, which is the opposite of what `§10.2`'s validation rule is for.

### D7. Reserve `usr` in the event envelope now.

`pv/1` stamps it and otherwise ignores it. `spec/protocol.md §4.2` already requires readers
to accept and preserve unknown top-level fields, and names this as the mechanism by which
a `pv/1` node and a `pv/2` node share a log without either losing information. So this is
backward compatible by construction.

Named `usr` rather than `user` because every envelope key is short and lowercase — `seq`,
`lam`, `ts`, `dev`, `app`, `op`, `tbl`, `id` — and a four-letter outlier reads as a
different kind of field. `spec/data-dictionary.md §5` separately forbids a *column* named
`user` as a reserved word, which is corroboration rather than the reason: envelope keys
are not columns.

Rationale: `dev` answers "which device wrote this." Once two people share a node, nothing
answers "which person," and no later change can answer it for lines already written.

**This is the only identity reservation `pv/1` needs.** An earlier draft of this ADR also
required an Ed25519 identity keypair per profile from day one. That was wrong and is
withdrawn. `pv/1` does not sign individual events at all — attribution rests on the
authenticated session and the single-writer file, for devices as much as for profiles — so
a profile key would prove nothing retroactive that device keys do not already fail to
prove. `pv/2` can mint a keypair then and bind it to the profile ULID that `usr` already
records.

### D8. No at-rest encryption. Not optional, not later.

**Rejected because it breaks a hard project constraint: copying `data/` must be a complete
backup that a non-technical person can perform.** An encrypted segment copies fine and
restores to nothing, because the key is deliberately not in `data/` and therefore not in
the backup. Fixing that means the restore instructions grow a key-management step, which
is the constraint failing. `docs/backup-and-restore.md` is the page that would have to
carry it, and `AGENTS.md` names that page as the bar for a document an owner reads under
stress. Usability wins.

Two consequences worth stating so nobody re-derives them:

- Every open question in the previous revision concerning locked segments, key escrow,
  escrow visibility, owner-role key transfer, snapshotting ciphertext, and where an
  unwrapped key lives in memory is **closed by deletion**. Five of ten went away.
- The honest statement in section 2 is now permanent rather than provisional. Profiles are
  never a boundary against anyone with the node's files, at any protocol version.

**One factual correction for the record**, because it arose in discussion and should not
survive into the repository: small or highly structured plaintext does **not** weaken
modern authenticated encryption. AES-256-GCM and ChaCha20-Poly1305 are not meaningfully
easier to attack on a 40-byte JSON line than on a 40 GB file, and known plaintext structure
is not an attack on either. What small structured records genuinely leak, even under
correct encryption, is *metadata*: file sizes, line counts, and write timing expose
activity patterns. That was never the reason to decline encryption, and this decision does
not rest on it. The backup constraint does all the work on its own.

### D9. **Withdrawn.** There is no profile merge, and no alias.

An earlier revision merged two profiles with a `sys_profile_alias` event that asserted
segment B belonged to the same person as segment A, unioned at read time. It is removed:
the feature bought a rare convenience and charged for it in every read path, every
snapshot, and every sync head, and the question of how far the union reached had no
obvious answer.

Two profiles that turn out to be one person stay two profiles. If their owner wants the
rows together, an app can read both and show them together, which is an app's decision and
costs the framework nothing.

**What the withdrawal does not change:** rewriting `usr` on existing events was never an
option and still is not. Sync is a set union over `(app, dev, seq)`
(`spec/protocol.md §10.1`), correct only because those three fields determine a line's
content; rewrite one on a node and two nodes hold different bytes for the same identity,
with no conflict-resolution rule anywhere in the protocol because the design deliberately
has none. `§10.2` forbids a receiver to re-serialize or normalize, peers already hold the
old lines, and snapshot manifests carry SHA-256 of the materialized output (`§5.2`) and
would mismatch. That reasoning is recorded here because it is the first thing anyone
proposing a merge will try.

Retiring a profile needs no new mechanism. `sys_profile` is an ordinary row, so an
amendment marks it retired under `§4.5`, its segment stays where it is, and nothing is
unwritten — which is what D18 says happens to it.

### D10. Profile ID is a ULID. Display name is mutable and may collide.

`[node]-[user]` is rejected: it bakes a mutable label into an identifier that appears in
immutable log paths, so renaming a node would orphan a segment. The same lesson is already
recorded for `sys_app.id` being the slug, where renaming an app is replacing an app.

Display collisions are a UI problem. The picker shows "Ana (desktop)" and "Ana (laptop)".

### D11. **Withdrawn.** There is no ceremony for moving a profile between nodes.

It fell with D9, and it was already thinner than it looked. D5 replicates every segment to
every node, so a profile is on all of them the moment it exists; there was never anything
to move. What the earlier revision described was authorizing an alias across two nodes,
and there is no alias.

Moving a profile between *clusters* — one household to another — is a different problem
that needs identities two strangers can prove to each other. That is `pv/2`'s, and section
1's principal-versus-partition distinction is exactly why.

### D12. `role` is `owner` or `member`.

Needed so D14's `shared-readonly` has a subject to name, and so D3's shell UI knows what to
offer. It is not the `owner` of `spec/protocol.md §8.4`, which is a request's standing —
loopback, this machine's own addresses, or an in-process call — and is unrelated to which
person is at the keyboard. The spec text must keep the two apart by name.

### D13. PIN policy mirrors pairing.

Translated from `spec/protocol.md §7.5` rather than copied, since a PIN is long-lived and a
pairing code is not:

| Rule | Value |
|---|---|
| Failed attempts before lockout | 5 |
| Lockout duration | 120 seconds, then reset |
| Rate limit | no more than 1 attempt per 2 seconds per source |
| Audit | every attempt, success or failure, writes a `sys_audit` event |
| Replication | lockout state is node-local; audit events replicate |

A PIN has no TTL, because unlike a pairing code it is not single-use. §7.5's 120 seconds
becomes the lockout window rather than the code's lifetime, and §7.5's "the code is
destroyed and a new one issued" has no analogue: a PIN survives its own lockout.

`sys_audit.kind` is a **normative closed list** (`spec/data-dictionary.md §3.10`), so this
adds to it rather than reusing `pair.*`: `profile.created`, `profile.attempt`,
`profile.failed`, `profile.locked`, `profile.switched`, `profile.retired`. The PIN itself
never reaches `detail`, which is the general rule in `AGENTS.md` about secrets and row
contents, not a special case for this table.

### D14. Shared tables are declared in the manifest.

```toml
[tables]
recipe = "shared"            # all profiles read, all profiles write
chores = "shared-readonly"   # all profiles read, role = owner writes
meds   = "private"           # default, per segment
```

- Default is `private`. Sharing is explicit, shown to the owner at install like any other
  permission (`spec/app-contract.md §5.4` is the established pattern), and `privatium lint`
  flags an unscoped read in an app that declared nothing.
- Shared data lives in the reserved `_shared` segment (D6). Single-writer still holds.
- `shared` inherits last-write-wins at row granularity by `(lam, ts, dev)`
  (`spec/protocol.md §4.5`). Two people editing one row moves from theoretical to routine,
  so apps using `shared` should model rows small. §4.5's existing instruction — an app
  needing field-level merge MUST model each field as its own row — stops being an edge case
  here.

### D15. One node per OS user stays documented as the hard-isolation option.

It already works with no new code: `spec/cli.md §1` provides `--data-dir` and `§2` provides
`--port`. Two OS users on one desktop already get two independent nodes today, and the
isolation is enforced by the kernel rather than by our code. `spec/protocol.md §3.1`'s
`local/lock` makes the separation total: one process per data root, refused otherwise.

With D8 declining encryption, this is now the **only** configuration in which one household
member's data is genuinely unreadable by another. It should be documented that way, plainly.

| | Profiles on one node | One node per OS user |
|---|---|---|
| Isolation | convention, enforced by the framework | kernel |
| Unreadable by other members | no | yes |
| Shared apps and data | native | only via cross-cluster sharing, `pv/2` |
| Works on a shared tablet | yes | no |
| Backup | one `data/` | N of them |
| Path to multiplayer | yes | none |

These are the two ends of a slider, not competing designs.

### D16. Real-time app traffic is an ephemeral channel, not the log.

Not implemented in `pv/1`, and not in Phase 3. Recorded here because it is half the reason
the profile work matters and because its shape constrains D6 and D7.

Per-frame state MUST NOT be appended. Four players at 20 Hz for three minutes is roughly
14,000 permanent, replicated, replayed-forever lines, each one syncing to every device.
`app-skills/privatium-games/SKILL.md` already says to save on meaningful boundaries — level
complete, checkpoint, quit — and that guidance is the shape of this decision, not a
placeholder for it.

The eventual shape: app-scoped messages riding the authenticated session, never stamped,
never appended, never snapshotted, never synced. This violates no invariant, because
`AGENTS.md` invariant 1 says JSONL is the only *truth* and a position hint is not truth.

**The shape is the data API, not a new transport.** A new SSE event type on
`GET /a/<slug>/api/stream` outbound, and one small POST inbound. It is deferred, but it is
decided, because the alternative was costing more than it bought:

- **It needs no change to `spec/protocol.md §8.3` at all.** §8.3 defines five frame kinds
  and says the channel "defines no route of its own"; a sixth kind looked unavoidable and
  is not. Outbound, SSE already rides `res` plus interleaved `chunk`s — §8.3 says so in
  as many words, naming `spec/data-api.md §3` as the reason answers to different ids may
  interleave. Inbound, a small JSON POST is an ordinary `req`. Only a *streamed request
  body* is reserved and refused in `pv/1`, and an ephemeral message is not one.
- **The LAN case is the channel case, which is the same code path.** On a plain-HTTP LAN
  origin the data API is reachable only through the channel (`spec/data-api.md §5`,
  `spec/protocol.md §8.4`), and this is expected to be where the feature lives. There is no
  second implementation for LAN and remote: the same two endpoints work on loopback, in a
  native shell, on an HTTPS origin, and framed inside the channel.
- **The security fences come for free.** `spec/data-api.md §2.1` already refuses a POST
  that is not `application/json`, and refuses any request a browser marks
  `Sec-Fetch-Site: cross-site`, on every route before anything is read. An ephemeral
  endpoint inherits both and adds no new surface. It carries no token, for the reason §2.1
  gives.
- **It is node-local.** Messages fan out to sessions on this node and are not relayed to
  peers. The ordinary household shape is one node and several paired devices, so this
  covers the case that motivates it; cross-node relay is deferred with the rest of `pv/2`.
- **There is no direct browser-to-browser path anywhere in the stack, and this ADR does
  not add one.** No WebRTC, no data channel. The route is device → node → device. On a LAN
  that is fine. Anyone modelling a twitch game across two houses should measure before
  designing, and `AGENTS.md` invariant 9 forbids solving it with a primary node.

Two things it must not do quietly. `spec/data-api.md §2` ends with **"Nothing else"** —
there is no framework-defined action endpoint, and `POST /a/<slug>/api/x/<action>` was
removed with the declarative tier. An ephemeral endpoint is not that: it invokes no named
server-side action and runs no app logic, it hands a message to the other sessions of the
same app. The spec edit has to say so in that paragraph, or a future session correctly
reading "Nothing else" will delete it — the same failure D17 guards against. And it needs
its own rate setting beside `spec/data-api.md §7`'s, because none of the existing ones
bounds it: 20 Hz per player is well under `api.sql_rate` and unrelated to
`api.max_batch`. One SSE stream per player sits comfortably inside `api.max_streams`
(8 per device).

The honest cost of the cheap shape: inbound is request/response, so each message is a
`req`/`res` pair rather than a fire-and-forget push. On a LAN that is affordable. A c2s
push frame is the optimization to reach for if measurement ever demands it — an
optimization, not a prerequisite, and not a reason to design for it now.

**Address it to a profile pair, not to a cluster.** In `pv/1` that resolves to profiles on
one node and is nearly a no-op; in `pv/2` the same API means two people, and no app has to
be rewritten. This is why D16 belongs in this record rather than a separate one.

### D17. Amend `docs/architecture.md §8` **and** `docs/roadmap.md`.

Two documents say multi-user is not happening, and both must be corrected together or the
tree contradicts itself:

- `docs/architecture.md §8`'s "deliberately absent" entry for multi-user accounts becomes
  deferred to `pv/2` **with the reservations of section 5 held open**, listing them.
- `docs/roadmap.md`'s "Explicitly not on the roadmap" list opens with multi-user sharing.
  The honest correction is narrower than deletion: multi-user *sharing as a hosted
  service* stays off the roadmap, which is what the paragraph's own reason says — each
  item there "turns a personal tool into a service". Household profiles do not, and they
  are scheduled as **Phase 3c**, planned as M27 of `docs/plans/phase-3.md`.

Without this, a future session correctly following the project's own no-invented-spec rule
will find unused machinery, conclude it is scope creep, and delete it. `docs/decisions/0004`
records the inverse failure (an invented `--bind` flag, an invented `kind = "console"`);
this is the same class of error running the other way.

### D18. No node is ever instructed to delete data. Leaving the household does not unwrite anything.

The obvious next move after D6 is a replicated `sys_segment_removed` marker: delete a
segment, tell the peers, and have a node that was offline honour the marker instead of
handing the segment back at its next pass. It is rejected.

That marker would be the first thing in the system that instructs a node to *not* hold data
it is entitled to, and it sits directly against `spec/protocol.md §10.2`, which requires a
receiver to refuse a gap rather than skip it. A removed segment is a gap every peer must
agree to stop asking about — a new invariant, load-bearing, and wrong the first time
somebody's node honours a marker it should not have.

**So the rule is the plain one: data that reached a cluster stays in that cluster.** A
member who leaves can have their segment deleted from any node the owner is willing to
delete it on, and it will come back from any peer that still holds it. That is the price of
sharing a cluster, and it is the same price `§4.6` already charges — a tombstone is not a
deletion, and destroying `data/` is the supported way to destroy data.

Consequences, all of which belong in the UI copy rather than in a footnote:

- Deleting a segment is a local act with no cluster-wide guarantee behind it. The interface
  MUST NOT describe it as removing someone's data, because on any other node it has not.
- Every backup still holds it, which `docs/backup-and-restore.md` already implies and this
  makes concrete.
- Section 2's honest statement grows one sentence, below.

This is the household analogue of D8: the boundary that does not exist is stated rather
than implied, and no mechanism is built that would imply otherwise.

## 4. Rejected

| Option | Why |
|---|---|
| At-rest encryption of any segment | Breaks the "copy `data/` is a complete backup" constraint (D8) |
| A per-profile data key, and escrow | Follows from the above |
| A per-profile identity key in `pv/1` | Proves nothing retroactive; mintable in `pv/2` against the ULID `usr` already records (D7) |
| Per-profile sync scoping | No security gain inside a cluster; costs durability (D5) |
| A `profile` column instead of a segment directory | Re-examined after D8 and D18 and still rejected: makes per-person removal structurally impossible rather than merely local, and makes `pv/2` sharing a line filter rather than a subtree sync (D6) |
| Enforcement in each app | Partition is only as strong as the worst app, and it fails silently (D2) |
| A second access-resolution table beside `sys_app_grant` | It already resolves device × app with `*` and most-specific-match, and grows a profile subject rather than a sibling; segments filter rows within an app and compose with it (D2) |
| A seventh framework route prefix for `/profile` | `/settings` already survives solo mode and is already shadow-resolved in the framework's favour; `pair` is a reserved slug, not a top-level route (D3) |
| A `fullscreen` key in `app.toml` | No such key exists, and a config key describing a UI is what `AGENTS.md` forbids; tier and deployment mode already carry the distinction (D3) |
| **Forbidding** household members from running nodes | Not the project's call. Recommend thin, state the consequence, let the owner decide (D4) |
| Rewriting `usr` on existing events and re-syncing | Breaks the `(app, dev, seq)` set-union property; silent divergence with no resolution rule (D9) |
| Merging two profiles, by alias or otherwise | Withdrawn to simplify: it charged every read, snapshot and sync head for a rare convenience, and how far the union reached had no obvious answer. An app may read two profiles and show them together (D9) |
| A ceremony for moving a profile between nodes | D5 already replicates every segment to every node, so there is nothing to move (D11) |
| A replicated `sys_segment_removed` marker | Would be the first thing instructing a node not to hold data it is entitled to, and stands against `§10.2`'s refusal of a gap. Data that reached a cluster stays there (D18) |
| Hiding an app a profile may not open | An app that vanishes teaches a household member the node is broken; a message teaches them to switch (D3) |
| Re-asserting the profile on every reconnect | Correct and unusable: a network blip returns a child to the PIN screen mid-game. The node remembers per device, node-local (D3) |
| Segmenting `lam` alongside `seq` | `(lam, ts, dev)` could then not order a `_shared` row against a private one, which D14 makes routine (D6) |
| `[node]-[user]` as a profile ID | Mutable label inside an immutable path (D10) |
| A mandatory framework header for profile display | Kills solo mode and cannot reach Tier 3 (D3) |
| Appending per-frame state, at any rate | 20 Hz is thousands of permanent replicated lines per session (D16) |
| A sixth channel frame kind for D16 | SSE already rides `res` and interleaved `chunk`s, and an inbound message is an ordinary `req`; §8.3 needs no change (D16) |
| A direct browser-to-browser transport for D16 | Not in the stack, and not justified by one deferred feature (D16) |
| Cross-node relay of ephemeral messages in `pv/1` | The household case is one node and several devices; relay is `pv/2`'s (D16) |
| Calling any of this an "account" | Promises a boundary the storage layer does not provide, at any version (D1, D8) |

## 5. Reservations that keep `pv/2` reachable

Four. D7 and D8 removed two, `sys_audit` added one, and D2 cashed R4 in `pv/1` rather
than reserving it.

| # | Reservation | Cost if skipped |
|---|---|---|
| R1 | `usr` in the envelope | Historical events are permanently unattributable to a person |
| R2 | Segment directories rather than a column | `pv/2` sharing becomes a line filter rather than a subtree sync; removing one person's data becomes structurally impossible rather than merely local (D18) |
| R3 | A segment scope parameter on the sync endpoints, even if `pv/1` always passes `*` | `/api/v1/sync/heads`, `pull` and `push` are keyed by `app` and `dev` alone and assume total trust; the shape becomes a one-way door |
| R4 | `sys_audit.actor` documented as a subject, not as "device ID or `system`" | Every security-relevant act in the household is permanently attributed to a machine rather than a person |

**The former R4 is gone because D2 spends it.** An earlier revision reserved
`sys_app_grant` resolution written against a subject rather than a device lookup, against
the day a profile needed one. D2 gives the profile its column in `pv/1`, so the shape is
built rather than reserved. `spec/protocol.md §14` item 3 still stands and is still not
reopened: multi-owner sharing across trust domains needs a capability model, and a third
column in one owner's own visibility table is not that.

The ephemeral channel's profile-pair addressing (D16) is not listed, because nothing in
`pv/1` implements the channel and therefore nothing can foreclose it. Its outbound event
type is additive to `spec/data-api.md §3` and its inbound endpoint is additive to `§2`;
neither touches the wire, so both are reachable from HEAD at any time.

## 6. Spec changes this implies

**M27's work.** The rows naming `spec/protocol.md §10` and `§4.1` change surface M21 has
already shipped — the heads shape and three sync routes — and M25 adds three more routes
with the same keying, so six routes and the heads shape move together or not at all.

| File | Change |
|---|---|
| `spec/protocol.md §1.1` | `profile` added to reserved slugs; the "last two" sentence reworded (D3) |
| `spec/protocol.md §2.3.1` | Node admission shows the full-replica consequence (D4) |
| `spec/protocol.md §3` | Segment level in the storage layout; `_sys` exempt (D6) |
| `spec/protocol.md §4.1` | `usr` in the envelope; `seq` per `(app, segment, device)` (D6, D7) |
| `spec/protocol.md §4.3` | `lam` stays per app and is not segmented (D6) |
| `spec/protocol.md §10` | Heads, pull and push scoped by segment; the segment name validated before a path is built (D6, R3) |
| `spec/protocol.md §14` | The open questions below appended |
| `spec/data-api.md §2` | The inbound ephemeral endpoint, and why "Nothing else" still holds (D16) |
| `spec/data-api.md §3` | An ephemeral SSE event type carrying no `seq` (D16) |
| `spec/data-api.md §7` | A rate setting for ephemeral messages; no existing `api.*` limit bounds them (D16) |
| `spec/data-api.md §4` | `/api/node` returns the active profile; `pv.node()` likewise (D3) |
| `spec/data-dictionary.md §3.5` | `sys_app_grant` gains `profile_id`; resolution restated over a subject, with the profile-device-app tie-break (D2) |
| `spec/data-dictionary.md §3.10` | Six `profile.*` audit kinds; `actor` restated as a subject (D13, R4) |
| `spec/data-dictionary.md §3` | `sys_profile` and `role`; no alias table (D9, D10, D12) |
| `spec/app-contract.md §3` | `[tables]` sharing declarations in `app.toml` (D14) |
| `spec/cli.md §5.1` | New rules appended, never renumbered: `PV1xx` for an undeclared `[tables]` entry, `PV2xx` for an unscoped read, `PV3xx` for a profile affordance a Tier 2 app never offers (D2, D3, D14) |
| `docs/architecture.md §8` | Rewrite the absent-features entry (D17) |
| `docs/roadmap.md` | Narrow the not-on-roadmap entry; add profiles and D16 to the open questions (D17) |
| `docs/deployment.md §2` | Member nodes get the same treatment as a VPS full node, framed as a choice (D4) |
| `docs/security.md §1` | A household member on this node joins the adversary table (D1, D8) |
| `docs/security.md §9` | Deleting a segment is local, no peer is told, and a peer that holds it hands it back (D18) |
| `app-skills/privatium-security/SKILL.md` | The above, plus Tier 3 exempt from profile scoping (D2) |
| `app-skills/privatium-tier3-rust/SKILL.md` | The core's segment scope is a convenience, not a boundary (D2) |
| `app-skills/privatium-games/SKILL.md` | The ephemeral channel, once it exists, beside the save-on-boundaries rule (D16) |

`AGENTS.md` makes the last three rows mandatory rather than optional: a change to `spec/`
that is not reflected in `app-skills/` is an incomplete change, and every skill's `reference/`
is regenerated by `cargo xtask gen-skill-reference`.

## 7. Open questions

**None outstanding.** The six this record carried are decided, and each is written into the
decision it belongs to rather than left here:

| Was | Now | Where |
|---|---|---|
| `lam` under segmented logs | Stays per app; only `seq` is segmented, and heads nest rather than compounding the key | D6 |
| How deep an alias union goes | Withdrawn — there is no merge and no alias | D9 |
| Whether `sys_app_grant` gains a profile subject in `pv/1` or `pv/2` | `pv/1`, as a `profile_id` column with a profile-device-app tie-break | D2 |
| What a member sees when a profile may not open an app | The app stays listed; opening it explains and offers the switcher | D3 part 6 |
| Per-person removal in practice | No peer is ever told to delete; deletion is local and a peer that holds the segment hands it back | D18 |
| Where the active profile lives across a reconnect | The node remembers it per device, node-local, and a new handshake resumes it | D3 part 5 |

Two things are deliberately left to the milestone rather than decided here, because both
are values rather than shapes and neither forecloses anything: the default rate limit for
D16's ephemeral endpoint, and the PIN's own length and character rules. `spec/data-api.md
§7`'s table is where the first lands.

## Would reopen if

- Household use never materializes, or the multiplayer motivation is dropped. R2 and R3
  carry real `pv/1` complexity justified only by `pv/2` being genuinely intended.
- The backup story gains a documented, non-technical key step. That is the only thing that
  would reopen D8, and it should be reopened for that reason and no other.
- A direct peer transport between two clients arrives for another reason. D16's relayed
  shape is a consequence of what the stack has, not a preference.

---

Copyright © 2026 Gabriel Mongefranco
