<!--
This file is part of Privatium
spec/cli.md
Author(s): Gabriel Mongefranco
Created: 2026-08-30
Last Modified: 2026-10-04
Summary: NORMATIVE. The command-line interface, including the linter that makes the skills system
         enforceable rather than advisory.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Command-Line Interface — `pv/1`

One binary, `privatium`. Running it with no arguments starts a node; every other function is
a subcommand.

Referenced throughout `app-skills/` and `docs/skills.md`. This document is what those references
point at.

---

## 1. Global behaviour

```
privatium [--data-dir <path>] [--config <file>] [--verbose] [--version] [<command> [args]]
```

| Flag | Default |
|---|---|
| `--data-dir` | A `privatium-data` folder beside the executable when one exists; else `$XDG_DATA_HOME/privatium`, or the platform equivalent |
| `--config` | `<data-dir>/config.toml` |
| `--verbose` | off |
| `--version` | — |

**Where the data root is** (`spec/protocol.md §3`), most explicit first: `--data-dir` when
given; else the folder named `privatium-data` beside the running executable, when the
owner has created one there — *portable mode*, the whole installation in one folder; else
the platform data directory. The program never creates the portable folder itself, so a
zip run from Downloads and the same program installed behave the same until the owner
makes that folder. A portable folder that exists and cannot be written is a runtime error
naming it, never a silent fall-through to the platform directory. Every run of a node
prints the root it chose and which of the three rules chose it, and the settings data
page shows the same.

`--version` prints the build version and the protocol version it implements on its first
line, then the project's own facts: the product name, the author, the copyright, the
licence, the project URL and the author's site. An implementation that does not satisfy
every item in `spec/protocol.md §13` MUST qualify the protocol string rather than print a
bare `pv/1` — for example `pv/1 (partial: phase 1)`. The version is written the way the
release tags spell it, `major.minor.patch`, exactly as the workspace manifest has it. Only
the first line is normative; an implementation MUST print it, and the rest is what this
one prints:

```
privatium 0.3.2 pv/1 (partial: phase 2)
Privatium™ is a framework for building small, personal apps that sync across your devices seamlessly without cloud services
Product:       Privatium
Author:        Gabriel Mongefranco (@gabrielmongefranco)
Copyright:     © 2026 Gabriel Mongefranco
Licence:       GPL-3.0-or-later — see main README.md for full license information.
Project:       https://github.com/gabrielmongefranco/privatium
Author's site: https://gabriel.mongefranco.com
```

These are the same facts a platform's own file properties carry where it has somewhere to
put them. On Windows that is the `VS_VERSION_INFO` resource, which fills the Details tab
of the properties dialog. A Mach-O binary and an ELF binary have no equivalent section a
file manager reads, so `--version` is the whole answer on macOS and Linux; a `.app`
bundle's `Info.plist` would be the macOS counterpart and this specification does not
require one.

`--verbose` widens what a command reports on standard error from what failed to what
happened: the apps a node loaded and the maintenance it decided on. It changes no
behaviour. Failures and warnings are reported whether or not it is set.

Global flags may stand anywhere on the line; `--version` and `--help` end parsing where
they stand.

A data directory is one process's at a time (`spec/protocol.md §3.1`): every command that
opens the node — a bare run, `dev`, `snapshot`, `restore` — takes its lock and holds it to
the end, and a second on the same directory is a runtime error naming the lock file.
`new`, `lint` and `skill` open no node and take nothing.

**No subcommand requires elevated privileges.** The one exception is `privatium firewall
--apply`, which prints what it would run and requires explicit confirmation
(`docs/deployment.md §4.2`). Implementations MUST NOT elevate silently.

Exit codes: `0` success, `1` runtime error, `2` usage error, `3` lint findings present.

---

## 2. `privatium` — run a node

```
privatium [--port 8420] [--solo <slug>] [--no-discovery] [--open]
```

Starts the node, mounts every enabled app, begins discovery (`spec/protocol.md §6`), and
prints the LAN URL — the address of the interface the default route uses; every other
interface under `--verbose`. The node binds every interface on `[node] port`, and there
is no flag to choose one; a loopback request stays the owner's own, with no pairing
(`spec/protocol.md §8.4`). `--open` opens a browser on the node and prints a QR code of
the LAN URL beside the URL in text; on a node with no paired device it also opens one
pairing window as the node starts and prints the code beneath the QR code
(`spec/protocol.md §7.1`), and once any device is paired it does that no more.

Discovery is mDNS and the UDP responder on port 52525, started together
(`spec/protocol.md §6.5`), on the port the node actually bound. `--no-discovery` starts
neither for this run and says so; the `discovery.mdns` and `discovery.udp` settings
(`spec/data-dictionary.md §3.6`) turn each off durably. A mechanism the platform refuses
— no multicast interface, the port taken — is one line on standard error, never a reason
for the node not to serve, and the `discovery.method` audit row names what started.

`--solo <slug>` overrides `[node] mode` from the config file for this run.

**First run.** When `<data-dir>/apps/` holds no app folder — it does not exist yet, or
nothing in it is a directory — the node writes the example apps the binary carries —
`hello`, `animals`, `sketch` and `pantry`, the folders of `apps/README.md`, unchanged —
into it before it loads anything, so the launcher is never empty. The rest of the data directory
is not consulted: a root used before the binary carried the examples gets them too. Once
any app folder is there, the owner's or an example's, nothing is written again, so an
example the owner edits or deletes stays edited or deleted; `privatium new --examples`
(`§4`) writes them again on request. A binary running from a source checkout already
mounts the checkout's `apps/` as `bundled` (`spec/app-contract.md §3.1`) and writes
nothing.

---

## 3. `privatium dev` — the development loop

```
privatium dev [--app <slug>] [--open]
```

Runs a node and opens it for editing. The reloading below is the host's own behaviour on
every run — a change is noticed by a stat on the next request, with no daemon and no flag
(`spec/lua-api.md §7`) — and `dev` is the front door to it, adding only its two flags.
`--app <slug>` names the app being edited: `dev` prints where its folder is and its URL,
`--open` opens that URL rather than the node's, and an app that did not load is a runtime
error naming the load failure. Without `--app`, `dev` is a node that reports what it
loaded. On change:

| Changed | Effect |
|---|---|
| `views/*.lsp` | Template chunk cache invalidated; next request recompiles |
| `app.lua`, `lib/*.lua` | App reloaded in place, routes re-registered |
| `static/*` | Served fresh; no action needed |
| `schema.sql` | Rematerialization from the logs (`spec/app-contract.md §4.5`) |
| `app.toml` | Routes and manifest re-read; data untouched |

**No restart, ever.** If a change requires restarting the node, that is a defect in the host
(`docs/architecture.md §2.4`).

Errors render in the browser with the Lua traceback and the offending template line, and are
also written to the terminal. A save that does not load is that error on every request
beneath the mount until the next save loads — never the code from before it.

---

## 4. `privatium new` — scaffold an app

```
privatium new <slug> [--tier lua|web|rust] [--from <existing-app>] [--scaffold <table>]
privatium new --examples
```

Creates `<data-dir>/apps/<slug>/` populated for the chosen tier. Defaults to `--tier lua`.
The slug is validated as `spec/app-contract.md §3` requires and a reserved one is a usage
error; the title is the slug's words capitalised, for the author to change.

- `--from hello` copies an example app and rewrites its slug and title. `<existing-app>`
  is an installed app's slug, an example app's — `hello`, `animals`, `sketch` or
  `pantry`, from the checkout's `apps/` or, on a binary with no checkout beside it, the
  copy the binary carries — or a folder holding an `app.toml`. What is rewritten is what names the app —
  the manifest's `slug` and `title`, the `apps/<old>` path in file headers and READMEs,
  the `privatium-app-<old>` skill name, a heading that is the bare slug, an HTML
  `<title>` equal to the old title — and prose is left alone. The tier is the copied
  app's; `--tier` beside `--from` must agree with it.
- `--examples` writes every example app under its own slug, exactly as the repository
  holds it, with nothing rewritten — what a first run writes (`§2`), on request. It takes
  no slug and no other flag; combining them is a usage error. Because `new` never
  overwrites, an example whose folder already exists is a runtime error naming it, before
  anything is written.
- `--scaffold <table>` reads the app's own `schema.sql` — the one `--from` copied, or the
  one already in the folder — and emits `app.lua` plus `views/*.lsp` giving list, detail,
  create, and edit screens for that table. Structured columns (`JSON`, `VARCHAR[]`) are
  shown and not edited. It is the one form of `new` that accepts an existing folder.

**`new` never overwrites a file.** Within one invocation a later source replaces an earlier
one — `--from hello --scaffold profile` copies hello, then the scaffold's `app.lua` and
views stand in for hello's — but a file already on disk is a runtime error naming it,
before anything is written.

**The generator has no runtime presence.** It writes ordinary source files you then edit,
delete, or rewrite. Implementations MUST NOT introduce a config format that describes a UI
(`spec/app-contract.md §1`).

---

## 5. `privatium lint` — the enforcement mechanism

```
privatium lint [<path>...] [--format text|json] [--severity error|warn|info] [--fix]
```

With no path, lints every installed app plus the node configuration.

A `<path>` is an app folder — one holding `app.toml` — a folder of app folders, which is
searched to three levels so the corpus of `§5.4` is one path, or a file inside an app,
which lints that app and reports the findings in that file. A path with no app under it
is one `PV101` finding. Installed apps are the folders the node would mount
(`spec/app-contract.md §3.1`): the owner's `apps/` and, in a checkout, the repository's.
"The node configuration" is `config.toml` as `--data-dir` and `--config` name it: its
mode decides `PV502` and `PV506`, and a configuration that does not load is a runtime
error, since the node would not start on it either. `--severity` is a floor — `warn` is
warnings and errors — and the exit code is `3` when anything at or above it remains
(`§1`), whatever the format. `--format text` is one line per finding, `<file>:<line>:
<id> <severity>: <message>`, with the fix and the section after it; a count goes to
standard error.

Advice an assistant can ignore is worth little. The linter is what makes `app-skills/`
enforceable rather than advisory, which is why it ships with the first working node.

### 5.1 Rule classes

Rule IDs are stable. Removing or renumbering one is a breaking change to the skills.

**Contract — `PV1xx`**

| ID | Rule |
|---|---|
| `PV101` | `app.toml` parses and carries `slug`, `title`, `version`, `api`, `tier` |
| `PV102` | Slug matches `^[a-z][a-z0-9-]{1,30}$` and is not reserved (`spec/protocol.md §1.1`) |
| `PV103` | `api` does not exceed the framework's supported version |
| `PV104` | Slug directory name matches `app.slug` |
| `PV105` | Tier-required files present — `app.lua` for `lua`, `web/index.html` for `web` — and every `app.lua`, `lib/*.lua` and `views/*.lsp` parses |
| `PV106` | Every table in `schema.sql` has `id VARCHAR PRIMARY KEY` |
| `PV107` | `schema.sql` contains only `CREATE TABLE`, `CREATE VIEW`, `CREATE INDEX` and comments |
| `PV108` | No `UNIQUE` constraint or index beyond `id`'s primary key (`spec/app-contract.md §4.5`) |
| `PV109` | A document that takes the standard chrome — a Tier 2 HTML document, or the document a `layout()` owns — has `</head>`, `<body>`, `</body>` and a main region with `id="main"` (`spec/app-contract.md §5`) |
| `PV110` | Every `[ui]` reference resolves: `scripts` and `styles` name files under `static/` and belong to a Tier 1 app, whose frame loads them; every menu item has a label and a mount-relative path (`spec/app-contract.md §3`) |

**Security — `PV2xx`**

| ID | Rule | Severity |
|---|---|---|
| `PV201` | No string-concatenated SQL — parameters must be bound | error |
| `PV202` | Every `<?raw ?>` use is reported for review | warn |
| `PV203` | No banned Lua global — `io`, `os.execute`, `os.getenv`, `debug`, `load`, `dofile`, `package.loadlib`, and the rest of `spec/lua-api.md §5`'s closed list | error |
| `PV204` | Every non-GET form contains `csrf()` | error |
| `PV205` | Declared `[permissions]` beyond the defaults carry a justifying comment | warn |
| `PV206` | No `innerHTML` with non-literal data in Tier 2 JavaScript | error |
| `PV207` | No external origin referenced without a matching `permissions.remote` entry | error |
| `PV208` | No apparent secret in `schema.sql`, `app.toml`, or sample data | error |

**Correctness — `PV3xx`**

| ID | Rule | Severity |
|---|---|---|
| `PV301` | No literal `/a/<slug>/` path — use `url()` or `pv.url()` (breaks solo mode) | error |
| `PV302` | No `tonumber()` or JavaScript `Number()` applied to a `DECIMAL` or `BIGINT` column | error |
| `PV303` | No `INSERT`, `UPDATE`, or `DELETE` in app SQL — writes are appends | error |
| `PV304` | Client code does not set `seq`, `lam`, `ts`, `dev`, or `app` on an event | error |
| `PV305` | No outbox dedupe table, transaction ID, or acknowledgement protocol | warn |
| `PV306` | Multi-event writes that must land together use `pv.batch` | warn |
| `PV307` | No global assigned in a handler expecting persistence, and no load-time table mutated from one — a global lives one request, and a VM's baseline is never shared (`spec/lua-api.md §5`) | warn |
| `PV308` | No `SUM()` over a `DECIMAL` column and no `+` or `-` on a `DATE` column in app SQL — `decimal_sum()` and `date(x, '+30 days')` (`spec/data-dictionary.md §2`) | error |

**Accessibility — `PV4xx`**

| ID | Rule | Severity |
|---|---|---|
| `PV401` | No icon-only control without a label argument or `aria-label` | error |
| `PV402` | Every form input has an associated `<label for>` | error |
| `PV403` | Radio and checkbox groups are wrapped in `fieldset`/`legend` | warn |
| `PV404` | Heading levels do not skip; exactly one `<h1>` per rendered page — a view with its partials inside the page frame, or the document a `layout()` owns; a fragment answering an htmx request is judged by the element it replaces, not on its own | warn |
| `PV405` | No status conveyed by colour alone | warn |
| `PV406` | Declared colour tokens meet 4.5:1 body / 3:1 large and UI | warn |
| `PV407` | Tabular data uses `<table>` with `<th scope>`, not a grid of divs | warn |
| `PV408` | No second way back: under the standard chrome the app draws no link of its own to the launcher or the settings pages — an `href` of `/` or `/settings…`, or a path that climbs out of the mount in browser code (`spec/app-contract.md §5`) | warn |

The `PV4xx` rules are the framework's reading of WCAG 2.2 AA (`AGENTS.md`, Accessibility
target), and this table is the document they enforce: `PV401` is 1.1.1 Non-text Content
and 4.1.2 Name, Role, Value; `PV402` 1.3.1 Info and Relationships and 3.3.2 Labels or
Instructions; `PV403` and `PV407` 1.3.1; `PV404` 1.3.1 and 2.4.6 Headings and Labels;
`PV405` 1.4.1 Use of Color; `PV406` 1.4.3 Contrast (Minimum) and 1.4.11 Non-text Contrast;
`PV408` 3.2.3 Consistent Navigation.
`PV401`'s icon requirements — `aria-hidden` beside text, a label when the icon is the only
content, `focusable="false"` always — are `docs/icons.md`'s, which is what its findings cite.

**Portability — `PV5xx`**

| ID | Rule | Severity |
|---|---|---|
| `PV501` | Slug ≤ 15 characters when `nav.advertise = true` (DNS-SD label limit) | error |
| `PV502` | `permissions.cross_origin_isolated` only in solo mode (`docs/frameworks.md §5.4`) | error |
| `PV503` | Icon names exist in the vendored Bootstrap Icons set | warn |
| `PV504` | No CDN reference — libraries are vendored under `web/vendor/` | error |
| `PV505` | No absolute filesystem path, and nothing written beside the binary | error |
| `PV506` | No app route matching a framework prefix — shadowed in solo mode (`spec/protocol.md §9.1`) | warn |

**What the linter reads.** Lua — `app.lua`, `lib/*.lua`, and the code inside a template's
tags — is judged over a syntax tree, never by pattern-matching the text. A template is
read through the same front end that compiles it, so a `<? if ?>` is a branch and each
branch is a state of the page; the HTML between tags is parsed as it would render, with
`icon()` and `csrf()` standing in for what they emit. `schema.sql` is judged by the
engine: `PV106` and `PV108` ask the catalog, and `PV107` prepares each statement and
classifies it by the actions SQLite reports, never by its first word. The SQL literals handed to
`pv.query`, `pv.query1` and `pv.sql`, and the bodies of `CREATE VIEW`, are tokenized for
`PV303` and `PV308`. A Tier 2 app's JavaScript is lexed — strings, template literals,
comments, identifiers — not parsed, which is enough for the rules that read it and no more.

### 5.2 Output

`--format json` emits one object per finding so an assistant can read its own failures and
iterate:

```json
{"id":"PV301","severity":"error","file":"apps/meds/app.lua","line":42,
 "message":"Literal mount path '/a/meds/' breaks solo mode",
 "fix":"Use url('/') instead","spec":"spec/app-contract.md §2.2"}
```

Every finding MUST carry a `spec` reference. A rule that cannot cite the document it
enforces does not belong in the linter.

### 5.3 `--fix`

Applies only unambiguous mechanical corrections — literal mount paths to `url()`, missing
`focusable="false"` on inline icons. It MUST NOT touch SQL, Lua control flow, or anything
where the intent is inferred. Everything else is reported for a human.

A mount path is rewritten only where the literal is the whole value — a Lua string that
is exactly the path, an attribute whose value is exactly the path — and only for the
app's own slug; a path to another app has no mechanical answer. After fixing, the linter
lints again and reports what remains, so the exit code describes the files as they now
stand; the files it rewrote are named on standard error.

### 5.4 In CI

`privatium lint` runs against every app in this repository. The reference apps are the
linter's test corpus, and a rule without a passing and a failing case in `apps/` is not
considered implemented.

The cases live under `apps/_lint/pass/<rule>/<slug>/` and `apps/_lint/fail/<rule>/<slug>/`
— the rule directory holds the app rather than being it, since `PV104` compares the slug
to the folder name and a rule ID is not a slug. A `pass` app is clean under every rule; a
`fail` app trips its own. A rule directory may hold a `config.toml`, the node
configuration the fixture is linted under (`pass/PV502` needs solo mode), which the test
suite reads and the command line takes as `--config`. The loader never mounts anything
under `_lint/`: a folder whose name starts with `_` is not an app.

The `PV4xx` rules bind the framework's own pages too — the launcher, the settings pages,
the error pages, and the page frame a Tier 1 view renders inside — not only apps. Those
pages have no template for the linter to read, so the framework's own test suite holds
their *rendered* HTML to `PV401`–`PV407` on every run, and a change to the shell that
fails one of them fails CI exactly as an app would.

---

## 6. `privatium skill` — skills for an assistant

```
privatium skill list
privatium skill export [<name>...] [--out <dir>]
```

Writes the skill folders matching **the running version** to disk, so an owner on v1.2 hands
their assistant v1.2's contract rather than whatever a search engine returned
(`docs/skills.md §6`). `list` prints each skill's folder name and, indented beneath it,
the `description` of its front matter. `export` writes each named folder — every folder
and `README.md` when none is named — under `--out`, which defaults to `skills/` in the
working directory, replacing files already there: a re-export after an upgrade is the
new version. A name this build does not ship is a usage error listing what it does.

A running node also serves them at `/skills/<name>.md` — `app-skills/<name>/SKILL.md` — and
`/skills/bundle.zip`, which holds every file under `app-skills/` at its path relative to
that folder: `README.md`, each `<name>/SKILL.md`, and each skill's `reference/`. Extracting
it into a `skills/` folder reproduces the tree `export` writes, which is what the `curl`
line in `app-skills/README.md` does. The entries are stored, not compressed: the bundle is
a few hundred kilobytes of Markdown and every extractor reads a stored zip.

---

## 7. `privatium snapshot` and `privatium restore`

```
privatium snapshot [--app <slug>] [--verify]
privatium restore --from <path> [--app <slug>] [--dry-run]
```

`snapshot` writes a SQLite + CSV + `schema.sql` set (`spec/protocol.md §5`) for `--app`,
or for `_sys` and every loaded app. `--verify` writes nothing: it recomputes the checksums
of every existing snapshot of those apps against its `MANIFEST.json`, records a match as
`sys_snapshot.verified_at`, and exits non-zero on any mismatch.

`restore --from <path>` takes a backup — a `data/` folder as `spec/protocol.md §3` lays it
out, or a data root containing one (`docs/backup-and-restore.md §1`) — and brings it into
this node's data root, for `--app` or for every app the backup holds, before rebuilding.
A log file is copied when this node has none of that name, or when this node's copy is a
byte-for-byte prefix of the backup's; it is kept when identical or when this node's copy
is the longer one; and a file that is neither — two writers, or an edited backup — refuses
the whole restore before anything is written, since a device's log is one writer's
(`§3.1`). Snapshot directories are copied when absent. `local/` and `cache/` are never
read from a backup. A copy never overwrites: a log this node lacks is written beside its
destination and renamed into place, a log this node holds a prefix of grows by the
backup's suffix, appended and synced, and each file is decided again against the disk at
the moment it is written; the root's lock (`§1`, `spec/protocol.md §3.1`) is held from
before the plan is read until the rebuild is done, so a node that is running is refused
rather than raced. Then each app's cache is rebuilt by `§5.3`'s three tiers, and
`restore` reports which tier it used per app and exits non-zero if it fell through to tier
3 unexpectedly. An app the backup holds but this node has no folder for keeps its data and
is not rebuilt. `--dry-run` prints what would be copied and, for each app, the tier the
rebuild would use *as the node stands* — a prediction over the logs and snapshots already
present, since nothing is copied.

Neither is required for normal operation — snapshots are written automatically and restore
is ordinarily "copy the folder back" (`docs/backup-and-restore.md`).

---

## 8. `privatium pair`

```
privatium pair [--open] [--timeout 120] [--node] [--join <url>]
```

Opens pairing mode and prints the code as four emoji with their labels, two words, and a
QR code of the node's URL — never of the code — with the URL in text beside it. Closes
after the timeout or the first success (`spec/protocol.md §7`).

`pair` opens no node of its own: a data directory is one process's (§1). It asks the
running node over loopback — `POST /api/v1/pair` to open the window, `GET /api/v1/pair`
to follow it (`spec/protocol.md §9.2`) — reports the device that paired and exits 0, or
the expiry and exits 1; with no node running it is a runtime error saying to start one.
`--open` opens the devices page in a browser.

`--node` opens the window **for a node** instead of for devices (`spec/protocol.md
§7.1`): the code is printed the same way, beside the command to run on the other
machine, and the command reports the node that was admitted and exits 0. A window of
the other kind already open is reported and exits 1.

`--join <url>` is the other machine's half: it prompts on the terminal for the code the
first machine shows — the two words, or the four glyph labels — and asks the running
node over loopback to join, `POST /api/v1/join` (`spec/protocol.md §9.2`, `§2.3.1`).
Which node joins which is decided by the exchange, not by which command ran; the
command says which it was, names the cluster and the peer, and exits 0, or names the
refusal and exits 1 with this node's identity untouched. The code is read from standard
input, never taken as an argument, so it appears in no shell history and no process
list. `--join` with `--node` or with `--open` is a usage error.

Pairing mode MUST NOT open without this command, its equivalent in the settings UI, or
the first-run window of `§2` (`spec/protocol.md §7.1`).

---

## 9. `privatium firewall`

```
privatium firewall [--apply]
```

Detects the platform and prints the exact command that would open the node's TCP port and
UDP 5353 (`docs/deployment.md §4.2`). `--apply` runs it after showing it and asking for
confirmation.

Never runs implicitly, never elevates silently, and the node runs correctly without it on
any network that does not block inbound connections.

---

## 10. What is deliberately absent

| Not doing | Why |
|---|---|
| `doctor` / `diagnose` | Detect and explain failures where they occur, in context, rather than in a separate command nobody runs |
| `serve` | Running a node is the default; a subcommand for it is ceremony |
| `migrate` | Schema changes rematerialize automatically (`spec/app-contract.md §4.5`) |
| `install <app>` from a registry | There is no registry (`spec/app-contract.md §9`) |
| `login` / `account` | There are no accounts anywhere in this system |

---

Copyright © 2026 Gabriel Mongefranco
