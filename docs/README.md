<!--
This file is part of Privatium
docs/README.md
Author(s): Gabriel Mongefranco
Created: 2026-09-06
Last Modified: 2026-09-27
Summary: Index of every Privatium document — guides for people running a node, guides for people
         building apps, the normative specification, and decision records.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Privatium documentation

Everything written about Privatium, grouped by what you are trying to do. Start at the
top if you are new; the specification near the bottom is the contract implementations
must satisfy.

## Using Privatium

For anyone running a node and using apps on it. No programming needed.

| Document | What it covers |
|---|---|
| [Backup and restore](backup-and-restore.md) | How to save your data and get it back. Written to be usable under stress, by someone who is not a developer. |
| [Connectivity](connectivity.md) | How each kind of device reaches your node, and what each route costs you. |
| [Deployment](deployment.md) | Running a node on an always-on machine, and what each operating system's firewall does. |
| [Security](security.md) | The threat model: what is protected, and what plainly is not. |
| [Command line](../spec/cli.md) | Every command and option the `privatium` program accepts. |

## Building apps

For anyone writing an app to run on a node — by hand or with an AI assistant.

| Document | What it covers |
|---|---|
| [Architecture](architecture.md) | How the system is put together, and why it is shaped that way. |
| [Frameworks and libraries](frameworks.md) | Which libraries, frameworks and game engines fit inside Privatium, and which do not. |
| [AI assistant guides](skills.md) | How assistant-written apps end up correct, accessible and secure, and how the linter enforces it. |
| [Sample app design](sample-app-design.md) | How the example apps keep their presentation small and responsive. |
| [Your app in its own repository](app-repository.md) | Starting an app in a repository of its own: the template, the assistant guides, the framework version pin, and how to run and share it. Includes a prompt for an AI assistant. |
| [Sketch app design](sketch-app-design.md) | The Tier 2 reference app in full: its coordinate model, its tools, and the rules its implementation is held to. |
| [Icons](icons.md) | The icon system: Bootstrap Icons, bundled and inlined server-side. |
| [Example apps](../apps/README.md) | The apps that ship with the program, and what each one demonstrates. |

## The specification

Normative. Where a document here and any other disagree, the specification wins.

| Document | What it defines |
|---|---|
| [Protocol](../spec/protocol.md) | Wire formats, the event log, discovery, pairing, session cryptography, sync. |
| [App contract](../spec/app-contract.md) | What an app is: the three tiers and the three deployment modes. |
| [Lua API](../spec/lua-api.md) | Tier 1 — the Lua application API and the LSP template engine. |
| [Data API](../spec/data-api.md) | The HTTP API that custom front ends build against. |
| [Data dictionary](../spec/data-dictionary.md) | System tables, the app index, type mappings, field definitions. |
| [Command line](../spec/cli.md) | The command-line interface, and the lint rules the assistant guides are held to. |

## Decision records

Why a choice was made, kept so it is not argued again from scratch.

| Record | Decision |
|---|---|
| [0001](decisions/0001-barracuda-evaluation.md) | Barracuda App Server evaluated as a foundation and declined. |
| [0002](decisions/0002-rust-core.md) | Rust as the core language, and the discovery and transport stack that follows. |
| [0003](decisions/0003-in-process-adapter.md) | One request and response interface in the core, three transports behind it. |
| [0004](decisions/0004-declined-alternatives.md) | Gun, RxDB, libp2p, SharkTrustX and Barracuda-in-Rust evaluated and declined. |
| [0005](decisions/0005-mobile-role.md) | What a phone is in a cluster: a full replica, never a server. |
| [0006](decisions/0006-sqlite-engine.md) | SQLite as the query engine, and where the guarantees DuckDB gave now live. |
| [0007](decisions/0007-household-profiles.md) | Household profiles as a partition, no at-rest encryption, and what is reserved so real multi-user stays reachable. |

## Project reference

| Document | What it covers |
|---|---|
| [Branding](branding.md) | The logo, colors, typography and asset pack, and how to use them. |
| [Project homepage](homepage.md) | Previewing, editing, translating, and hosting the single-page website and its screenshots. |
| [Naming](naming.md) | The name, the tagline, and the tokens that are load-bearing across the code and the wire format. |
| [Roadmap](roadmap.md) | What exists, what is planned, and the test that holds each finished item. |

---

Copyright © 2026 Gabriel Mongefranco
