<!--
This file is part of Privatium
docs/deployment.md
Author(s): Gabriel Mongefranco
Created: 2026-08-28
Last Modified: 2026-09-27
Summary: Topologies, the always-on node, and per-OS firewall behaviour.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Deployment

Current build: one node serves paired browser clients over its encrypted LAN channel.
It listens on `0.0.0.0` and, where available, `[::]`, using `[node] port` (8420 by
default). Startup prints a LAN URL and a separate loopback URL for the owner; `--open`
uses loopback. `--verbose` lists other interface addresses. No bind flag is needed.

The node advertises itself on the local network over mDNS and answers UDP probes on port
52525 (`spec/protocol.md §6`); `--no-discovery` turns both off for one run, and the
`discovery.mdns` and `discovery.udp` settings turn each off durably. A phone pairs by
scanning the QR code `--open` or `privatium pair` prints, or the one on the devices
page. A second node is admitted with one pairing (§3.1). Admitted nodes sync over the
LAN on discovery, on a timer, and one second after writes settle. Remote transports,
tunnels and certificates below remain planned; each is marked where it appears.

## 1. Topologies

Pick by what the owner actually has, not by what is most impressive.

| Topology | Nodes | Remote access | Setup |
|---|---|---|---|
| **Single** | one desktop | none, or a tunnel | trivial |
| **Household** | desktop + laptop | tunnel or VPN | pair the second node once |
| **Household + always-on** | desktop + laptop + VPS | free, always | ~$5/month |
| **Backup-only** | one node, file sync on `data/` | none | install Syncthing |

Nodes are peers. There is no primary and no server role
(`spec/protocol.md §10.3`). For which client kinds work under which network conditions, see
`docs/connectivity.md`.

## 2. The always-on machine — four separable jobs

A cheap VPS is useful, but "run a node on it" is only one of four things it can do, and it is
the only one that touches your data. Enable them independently.

| # | Function | Holds data? | Can decrypt? | Solves |
|---|---|---|---|---|
| 1 | **Relay** | No | No | Hole punching failures (~10%), CGNAT, symmetric NAT |
| 2 | **pkarr relay / DNS** | No | No | Discovery on networks that block the DHT |
| 3 | **Certificate host** | No | No | Browsers and PWAs needing a real HTTPS origin |
| 4 | **Full node** | **Yes — complete plaintext replica** | Yes | Reachability when every machine at home is off |

### 2.1 Start with 1–3, think hard about 4

Functions 1–3 are pure infrastructure. They forward, resolve, and terminate TLS; they store
nothing and can read nothing.

Function 4 puts your entire medication history in plaintext on rented hardware, indefinitely.
It buys exactly one thing the others do not: a peer that is reachable when your desktop and
laptop are both off.

**For health data, running 1–3 and declining 4 is the better default.** This inverts the
usual advice — a relay is a safer thing to place on someone else's hardware than a node,
because a node is the only one of the four that can be read if the machine is compromised.

### 2.2 What each function needs

| Function | Requires |
|---|---|
| Relay | The binary, a public address, one open port |
| pkarr relay / DNS | The above, plus port 53 |
| Certificate host | A domain and an ACME certificate |
| Full node | Admission to the cluster by pairing (`spec/protocol.md §2.3.1`) |

Only the fourth involves pairing, because only the fourth is a cluster member. The first
three never see the cluster key.

### 2.3 Alternatives

All legitimate, all retained, none removed by peer-to-peer:

| Option | Cost |
|---|---|
| **Peer-to-peer** (pkarr + hole punching) | **none** — no account, no domain, no payment. Native clients only. |
| Mesh VPN | account, client on every device |
| Tunnel service | account; a domain for a stable name |
| DDNS + port forward + DNS-01 certificate | router configuration, a domain. Required for browsers and PWAs. |
| Syncthing on `data/` | an external tool; global discovery servers and a relay pool. Backup, not a transport. |
| Nothing | LAN works; remote is offline-cached with an outbox |

## 3. Cluster growth

Adding a second node is one pairing, and it buys more than a second copy:

- The laptop keeps serving when the desktop is off. Your phone reaches it over mDNS with no
  reconfiguration, because it pinned the **cluster** key, not the node key.
- Discovery already returns multiple instances; clients key on Node ID and filter on the
  `cl` TXT field, so a LAN carrying several households' nodes still shows you only your own.

Do not add nodes for performance. Add them for availability.

### 3.1 Admitting a node

Two commands, one on each machine, in whichever order the network allows:

1. On one machine, open a window for a node: `privatium pair --node`, or **Settings ›
   Devices › Admit a space**. It prints a code — four emoji with their labels, and two
   words — and the address to use, and waits up to two minutes.
2. On the other, run `privatium pair --join http://<that address>` and type the code
   when asked, or open **Settings › Space › Join a cluster** and enter the address and
   the code there.

Which machine joins which is decided by the exchange, not by which command you ran: a
machine that has paired nothing and admitted nobody joins the other's cluster and gives
up the empty cluster it founded; two machines that both already have devices are
refused, since two clusters are never merged. So on a home network it does not matter
which of the desktop and the laptop opens the window. A machine behind a router — a
laptop on hotel Wi-Fi, a rented server — can dial home and cannot be dialed, so it runs
the `--join` half.

Both sides prove they hold their keys before anything is handed over, and the cluster
private key crosses the network once, encrypted under the pairing code's key, to the
machine being admitted. The admitting node lists the new one on its devices page, with
a **Revoke** button that removes it from the cluster on every node that hears of it
(`docs/security.md §8`). Each node holds every device's log after synchronization;
one pairing lets a phone use either node once their system logs have crossed.

Joining keeps every node's data, and sync is a union. Admission replaces three identity
files and nothing under `data/`. After the first pass both nodes hold both devices'
logs. What you see depends on the app: Sketch has one shared canvas, so drawings started
separately on desktop and laptop are merged on one canvas after joining, each stroke
intact and undo reversing the latest change from either. Nothing is lost, and the
drawings are not kept apart.

## 4. Firewalls

The current LAN bind can trigger Windows Defender's incoming-connection prompt. The
node does not change firewall rules or require elevation. Declining the prompt leaves
loopback access available. The optional firewall helper remains planned; a successful
bind alone does not prove another device can reach the port.

**Only a node needs inbound connectivity.** Clients need none. Outbound is effectively never
blocked on any supported platform, so certificate issuance and tunnels work regardless.

| OS | Default state | Consequence |
|---|---|---|
| **Fedora Workstation** | firewalld, `FedoraWorkstation` zone | Easiest of all. Ports 1025–65535 TCP and UDP are open by default — a deliberate decision so that novice users can run network-facing applications — and `mdns` is among the zone's allowed services. Nothing to configure. |
| **Ubuntu / Debian desktop** | ufw present but inactive | Works out of the box. Server installs vary. |
| **macOS** | Application Firewall off by default | Usually works. If enabled, prompts to allow incoming connections; unsigned applications are flagged, so notarization materially improves this. Recent macOS versions add a **separate local-network privacy prompt**. |
| **Windows** | Defender Firewall, inbound blocked | An elevation prompt appears on the first non-loopback bind. Request the **Private** profile only, never Public. |
| **openSUSE** | firewalld, `public` zone | Restrictive. Requires an explicit rule. |

### 4.1 Two traps

**Windows network classification.** Home Wi-Fi is frequently classified as *Public* rather
than *Private*, which blocks the application even after the owner clicked Allow. Expect this
to be the most common support question. Detect the profile and say so in plain language;
an owner who is not told will conclude the software is broken.

**mDNS needs its own rule.** UDP 5353 inbound is separate from the TCP application port.
Opening one and forgetting the other produces "it works by IP but not by name," which looks
like a discovery bug and is not. The UDP fallback for networks that filter multicast is a
third rule, UDP 52525 (`spec/protocol.md §6.4`).

### 4.2 Never require administrator

The node MUST run as an ordinary user. Elevation is only ever needed to open a firewall
port, and that MUST be an optional helper the owner can decline — with the manual commands
shown so they can inspect what would be run.

```
Fedora / openSUSE   sudo firewall-cmd --permanent --add-port=8420/tcp \
                    --add-port=5353/udp --add-port=52525/udp && sudo firewall-cmd --reload
Ubuntu / Debian     sudo ufw allow 8420/tcp && sudo ufw allow 5353/udp && sudo ufw allow 52525/udp
Windows             netsh advfirewall firewall add rule name="Privatium" \
                    dir=in action=allow protocol=TCP localport=8420 profile=private
```

Ports must be ≥ 1024 (`spec/protocol.md §6.3`) — required for sandboxed packaging, and the
default 8420 also sits inside the range Fedora already permits.

## 5. Network transitions

Switching Wi-Fi to cellular mid-session is ordinary, not exceptional. Two separate concerns:

**Reachability** is handled by endpoint failover (`spec/protocol.md §10.4`): short connect
timeouts, re-attempt on network-change events, offline mode with an outbox when nothing
answers. Queued writes replay idempotently because they carry ULIDs.

**Origin** is the subtler one (`§10.8`). A browser reaching the node at a LAN address and
then at a public name is on two different origins, with two service workers and two storage
buckets — the app does not degrade, it becomes a different installation. Native clients
sidestep this entirely. For browsers, one resolvable name across every path is the answer.

## 6. Packaging

Download the archive for your operating system from a published GitHub release or
from an individual artifact in a successful CI run. Extract it once; there is no
containing folder inside the archive.

| Operating system | Archive | Contents |
|---|---|---|
| macOS | `privatium-mac.zip` | `privatium` |
| Windows | `privatium-windows.zip` | `privatium.exe` |
| Windows, portable | `privatium-windows-portable.zip` | `privatium.exe`, a `README.txt`, and a `privatium-data` folder holding the three example apps, so everything stays in one folder (`spec/cli.md §1`) |
| Linux | `privatium-linux.tar.gz` | `privatium` |

These are native builds on GitHub's `macos-latest`, `windows-latest` and
`ubuntu-latest` runners, respectively. The shorter names do not imply universal
architecture support. The binary includes Lua, SQLite and the three example apps, which
it writes into an empty `apps/` folder on its first run; the portable zip carries them
as files beside the program instead. Installers, AppImage, Flatpak and signed or
notarized packages are not built yet.

### 6.1 Publishing binaries

1. Let CI pass on the commit you intend to release on `main`. CI runs on pushes to
   `main` and pull requests; release builds require a run for the exact commit that was
   either a push to `main` or started by hand. Creating a release tag does not repeat CI.
   If the commit never had a run, because the CI workflow was disabled or the run was
   lost, start **CI** from the Actions tab on `main` while that commit is its head, or
   run `gh workflow run ci.yml --ref main`. A pull request run does not count, since it
   tested a merge preview rather than the commit itself.
2. Publish a GitHub release for that commit. The **Release binaries** workflow also
   handles prereleases. Pushing a tag alone does not create a release.
3. The workflow checks the latest matching CI run, builds the three binaries
   with the pinned Rust toolchain and `Cargo.lock`, smoke-tests them, packages the four
   archives above, and attaches them. It does not repeat the full test suite.

If matching CI is missing, pending or unsuccessful, the release remains published
without new binaries. Let CI pass, then run **Release binaries** again from Actions:
either rerun the failed run, or start the workflow by hand and give it the release tag,
which is also how binaries are attached to a release that was published before the
workflow existed. Either way replaces only the four named assets; release notes and
other assets stay as they are. The workflow runs from the default branch, so it must be
on `main` when the release is published or the workflow is started.

---

Copyright © 2026 Gabriel Mongefranco
