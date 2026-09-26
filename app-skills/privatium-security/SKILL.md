---
name: privatium-security
description: Security rules for writing or reviewing any Privatium app, and what to tell an owner before they install one. Covers injection, escaping, sandbox limits, secrets, permissions, and the framework's honest threat model. Load alongside every tier skill and before recommending any third-party app.
---

# Privatium Security

Applies to every tier. Load alongside the tier skill.

## Never, in any tier

1. **Never build SQL by concatenation.** Bind parameters. The linter rejects concatenation
   and you should too.
2. **Never put a secret in `data/`.** Not keys, not pairing codes, not tokens, not API
   credentials. Logs are plain text, sync everywhere, and live in backups forever. Secrets
   go in the OS keyring or `identity/`.
3. **Never disable escaping.** `<?= ?>` in LSP escapes by default. `<?raw ?>` exists for the
   rare genuine case and every occurrence is a review trigger. The framework's own markup
   — `icon()`, `csrf()`, `render()` — is an HTML value that passes through `<?= ?>`; do
   not reach for `<?raw ?>` to emit it, and do not wrap data in it. In JS, use
   `textContent`, never `innerHTML`, with user data.
4. **Never omit `csrf()`** from a non-GET form. The host verifies the token on every
   non-GET request beneath the mount — the `_csrf` field or the `X-CSRF-Token` header —
   and answers 403 without it; the page frame gives htmx the header for free. The data
   API takes no token and needs none: a POST is read only as `application/json` and a
   request another site made (`Sec-Fetch-Site: cross-site`) is refused on every route
   (`spec/data-api.md §2.1`). Never add a CORS header to open it.
5. **Never trust an event's origin.** Events arrive via sync from other devices. Validate
   on read as well as on write.
6. **Never claim data is deleted.** `del` writes a tombstone; the original line stays in the
   log forever. That is deliberate. The only way to destroy data is to destroy `data/` and
   every synced copy — say so plainly in any UI that offers deletion.
7. **Never reuse a ULID.** Once the framework has minted an `id` for a row, that `id` names
   that row forever — deleted or not. Writing a different row under it merges two histories
   under last-write-wins, silently and across devices. Mint a new one. (A key *you* chose,
   like a `'cursor'` singleton, is a different thing: deleting and re-asserting it is an
   amendment to the same logical row and is fine.)

## Tier 1 (Lua)

`io`, `os.execute`, `os.exit`, `os.getenv`, `os.remove`, `os.rename`, `os.tmpname`,
`os.setlocale`, `debug`, `load`, `loadstring`, `dofile`, `loadfile`, `package.loadlib` and
`package.cpath` are removed from the sandbox. Do not attempt to reach them; do not suggest
a workaround. `require` is confined to the app's own `lib/` and `'privatium'` — no `../`,
no absolute path, no symlink out. `print` and `pv.log` write to the node's diagnostic
log, never to its standard output.

The SQLite connection your SQL runs on is read-only, `query_only`, and behind an
authorizer that refuses every write, every `PRAGMA`, `ATTACH` and extension loading. This
is not adjustable hardening — a connection that could `ATTACH` could read
`identity/node.key`. It is a connection of its own, opened for your request and closed
with it; the framework writes through a separate handle while yours reads. Nothing you
can write in SQL reaches the filesystem: no `ATTACH`, no `VACUUM INTO`, no
`load_extension()`. Read your data from your tables and views; a write is a Lua error
you may catch, and it changed nothing.

Watch the instruction, memory and wall-clock limits. An unbounded loop aborts the request
whether or not you `pcall` it — the host decides, not the handler — and a long statement
is interrupted on the same clock.

A global you assign in a handler lasts one request and is never seen by another; what
`app.lua` defines at load is the baseline every request starts from. Do not put a
request's data in a global expecting to find it later, and do not build a cache by
mutating a load-time table — that one does persist, per VM, and is what the linter warns
about.

## Tier 2 (Web)

Default CSP is `script-src 'self'` scoped to the app's path. Inline `<script>` does not run.
Put JavaScript in external files.

Every non-default permission is shown to the owner at install:

| Permission | Ask for it only if |
|---|---|
| `inline_script` | You genuinely cannot use an external file. Almost never. |
| `wasm` / `eval` | A WASM loader requires it |
| `sql` | The app needs ad-hoc queries rather than named views |
| `cross_origin_isolated` | Solo mode only; see `privatium-games` |
| `remote` | **The app phones out.** This is the one thing the project exists to avoid. Expect the owner to refuse. |

Vendor libraries into `web/vendor/`. A CDN is a `remote` permission, an offline failure, and
an IP leak.

## Tier 3 (Rust)

Not sandboxed. Full filesystem and network access, in the owner's session, on their data.
Say so when recommending one. No `unsafe` without a comment naming the invariant it upholds.

## Reviewing a third-party app

Before recommending an owner install anything:

- [ ] Read `app.toml`. Every non-default permission justified by something visible in the code?
- [ ] Any `remote` origins? What is sent, and why?
- [ ] Grep for `<?raw`, `innerHTML`, `eval`, string-concatenated SQL
- [ ] Does it read `sys.v_*` views beyond what its function needs?
- [ ] Tier 3? Then it is unsandboxed native code — treat accordingly
- [ ] `privatium lint` clean?

Tell the owner in plain terms: **installing an app means running someone else's code on
your data.** It is sandboxed from the filesystem, but within its own scope it sees
everything. Treat it like a script a stranger emailed you.

## Three properties — do not conflate them

The node provides live `/ws/pair` and `/ws` transports. Non-loopback clients need a
paired channel for application data; only bootstrap documents and public assets are
served without it. Pairing opens only with the owner's standing — the devices page,
`privatium pair`, or `--open` on a node no device has paired with — never from a paired
session (`spec/protocol.md §9.2`); the same holds for labelling or revoking a device and
naming the node. A revoked device's open channel is closed at once. Session transports
must use fresh ephemerals on reconnect and close the connection on every frame error.
Never reconstruct a frame counter under an existing key (`spec/protocol.md §8, §8.3`).

| # | Property | Mechanism | Missing anywhere? |
|---|---|---|---|
| 1 | Program authenticity | Signature / notarization / CA chain | **Yes** — browser on plain-HTTP LAN |
| 2 | Device authentication | PAKE, then pinned keys | No |
| 3 | Transport security | Derived session key | No |

**The PAKE does 1 job, not 2.** Authentication and key agreement are one operation —
deriving a key is the proof. Never write code that runs a PAKE for the channel and then
sends the code as a bearer token to log in; that is strictly weaker.

A guest on the Wi-Fi is blocked by property 2, on every path including plain HTTP. Pairing
mode is closed by default, and the code is 16 bits with 5 attempts in 120 seconds.

Pairing persists: the client stores its keypair and the pinned cluster key under the origin
(browser) or the OS keyring (native). Lost storage means re-pairing — never write a recovery
path that skips it.

## The framework's honest gap

Every load over plain HTTP can be replaced by an active on-path attacker, including
later visits after pairing. Replacement JavaScript can read the stored device keys
under the page origin, impersonate the device and read its data. Pairing need not be
open. The bootstrap and its integrity hashes can both be replaced. Never claim pairing
prevents this replacement, limits it to first pairing, or makes this equivalent to an
installed SSH client (`spec/protocol.md §7.7`, `docs/security.md §4`).

With genuine client code, pairing authenticates devices, pinned keys refuse a
substituted node, and the channel protects application data from passive listeners.
Integrity pins same-origin external scripts and stylesheets to channel bytes. A remote
resource allowed by existing app permissions needs a hash in authenticated HTML;
imported framework and app modules lack that protection. None authenticates a later
bootstrap. Describe the exposure as every visit, including stored device keys.

Full-page navigation uses a fresh bootstrap with the destination app's CSP. Form
responses may remain briefly as bounded, unpolled streams in node RAM. Only an opaque
reference and destination metadata cross in per-tab storage; never persist form bodies
or decrypted HTML for this handoff. Same-device attachment consumes the response once
without running the request again. Expiry or loss does not undo a committed write;
show the uncertainty and require checking before resubmission (`protocol.md §8.3.1`).
This is not an outbox acknowledgement or an event deduplication mechanism.

The plain-HTTP path remains supported without a domain, account or certificate setup.
A signed native client or authenticated transport on every visit closes the bootstrap
gap; protecting only initial pairing does not. Do not add a verification string: a
substituted client can render any comparison it chooses.

## Cluster keys

The keys and verified certificate in `identity/` select this installation's node and
cluster (`spec/protocol.md §2.3`, `spec/data-dictionary.md §3.1, §3.1b`). Restore can bring
other nodes' and clusters' public records. Preserve them; never tombstone a cluster just
because this installation lacks its key. Query the local node or cluster by its ID,
never by the first row or an assumption that the whole registry has one row. A public
record alone grants no cluster trust and cannot replace pairing, admission or pinned-key
verification.

A node renews its own certificate at startup only while it is unexpired and fewer than
ninety days remain (`spec/protocol.md §2.3.1`). An expired certificate requires
re-admission; do not bypass expiry by signing a replacement. Node admission is not built
yet, so the current build refuses an expired certificate.

The cluster private key lives in `identity/cluster.key` on nodes only. It MUST NOT be sent to
a phone, tablet, or browser, and MUST NOT appear in any event, log, snapshot, or backup
export. Devices receive the cluster *public* key and pin it.

Consequence to state when asked: **compromising any one node compromises the cluster**, since
every node holds the key. That is the accepted trade — the alternative is node admission
failing whenever one particular machine is off. Nodes are machines the owner physically
controls; devices are not.

Revocation is bounded by the 180-day certificate lifetime, not instant. An owner needing a
hard cut must rotate the cluster and re-pair everything.

## Discovery and relays

| Component | Leaks | Reads data |
|---|---|---|
| Relay | addresses, timing, volume | **no** — ciphertext only |
| Mainline DHT (pkarr) | that a key published an address | no |
| pkarr DNS server | which keys are resolved | no |

- pkarr exposure equals dynamic DNS: whoever has the key resolves the address. Not worse.
- **BEP44 mutable items, never BEP5 infohash announcements.** A node must never look like a
  torrent peer.
- Publishing is optional and separately disableable. A LAN-only node should not publish.
- Records are under 1000 bytes and carry no application data, ever.
- When recommending rented hardware: **relay yes, full node think twice.** A relay stores
  nothing; a node stores a complete plaintext replica.

## Reporting

Suspected vulnerabilities go to <privatium@mongefranco.com>. Never a public issue.

Detail: `docs/security.md`, `spec/protocol.md §7–9`.
