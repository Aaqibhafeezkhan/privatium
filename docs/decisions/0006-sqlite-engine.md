<!--
This file is part of Privatium
docs/decisions/0006-sqlite-engine.md
Author(s): Gabriel Mongefranco
Created: 2026-09-03
Last Modified: 2026-09-03
Summary: Decision record. The query engine is SQLite, not DuckDB; what that costs, what it buys, and
         where the guarantees DuckDB used to give now live. Status: DECIDED. Supersedes ADR
         0001 §3 and ADR 0002's engine bullet.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# ADR 0006 — SQLite is the query engine

**Status: DECIDED. Supersedes the "SQLite reintroduces the original problem" finding of
ADR 0001 §3 and the DuckDB bullet of ADR 0002.**

## Decision

`privatium-core` materializes the event log into SQLite — `cache/<slug>.sqlite`, through
`rusqlite` with the bundled amalgamation — and app SQL runs on a read-only, authorizer-fenced
SQLite connection. DuckDB is gone from the build, the binary, the spec, and the docs.

## Context

DuckDB was chosen (ADR 0001 §3, ADR 0002) for three things: native `DATE`, `TIMESTAMPTZ`,
`INTERVAL` and `DECIMAL`; reading JSONL and Parquet in place; and a SQL engine that made the
materializer a `CREATE TABLE AS SELECT`. All three were real. Once the engine was in the build, so
were the costs, and they were not the costs the decision had weighed:

- **Build.** The bundled DuckDB is a multi-minute C++ compile per cargo variant — build,
  clippy, test, stable — each leaving a 1.85 GB directory cargo never removes. `target/`
  reached 90 GB at its worst and was still 53 GB when the engine was replaced; a dependency
  change re-hashed every variant at once.
  CI took over half an hour per platform, most of it compiling the engine.
- **Binary.** 34.5 MB, nearly all engine.
- **Sandbox.** DuckDB's `enable_external_access` and `lock_configuration` are instance-wide
  and the database file is locked exclusively, so `spec/app-contract.md §7`'s sandbox had to
  be a boundary *in time*: open privileged, materialize, seal, and drop the whole store to
  open a window for every snapshot and restore. That machinery was built
  (`reopen_privileged`/`reseal`); it worked, and it was the most delicate code in the crate.
- **Mobile.** ADR 0002's reason for Rust is one core on every platform. DuckDB on a phone is
  possible and heavy; SQLite is already there, and it runs in a browser as WASM with the
  same dialect the node speaks.

Two of the three original reasons turned out not to be load-bearing. Reading JSONL in place is
replaceable by a Rust reader in an afternoon — and was the *cause* of the sandbox complexity,
because `read_json()` needs filesystem access from SQL. Parquet was a nice-to-have. The third,
the types, survives in weaker but sufficient form, below.

## Costs of SQLite and where each is paid

| DuckDB gave | SQLite has | Where the gap is closed |
|---|---|---|
| A real `DECIMAL` | Type affinity; a `DECIMAL(18,2)` column stores a REAL | The materializer stores decimals as **text at the declared scale** under the framework's `decimal` collation, and registers `decimal_add`/`sub`/`mul`/`cmp`/`decimal_sum` on every connection (`store::decimal`). `pv.dec` is the same type. A lint rule catches `SUM` over a decimal column. |
| `DATE`, `TIMESTAMPTZ`, `INTERVAL` types | ISO 8601 text and SQLite's own `date()`, `datetime()`, `strftime()`, `julianday()`, modifiers | The spec already stores RFC 3339 UTC to the millisecond, which compares as time as text. `'2026-01-01' + 30` is `2056` in SQLite; `date(x, '+30 days')` is the spelling, and a lint rule catches the other one. |
| Engine-enforced column types | Affinity only | The framework is the only writer of the cache, so it types every value from the declared type on the way in (`materialize::typed`). A value that does not parse as its type materializes as NULL rather than failing the replay — the log cannot be corrected, and the data API validates before it appends. |
| `VARCHAR[]` | Nothing | A JSON text column (`Kind::Json`); `json_each` queries it. The dictionary's `multiselect` maps to it. |
| Parquet snapshots | Nothing | Tier 1 is a **SQLite file per table**, which any SQLite tool opens; tier 2 is CSV written and read by the framework (`store::csv`), typed from `schema.sql`, never inferred. |
| One instance, exclusive lock, GLOBAL settings | Many connections on one file | The sandbox is the **connection**: read-only at the file, `query_only`, an authorizer refusing every write, every `PRAGMA`, `ATTACH` and `load_extension()`. The framework's connection writes while an app reads. No window, no seal. |
| Columnar speed | A B-tree | Irrelevant at personal-data scale, and the honest trade if it ever is not. |

## Benefits

The engine compiles in about a minute, once. The binary loses most of its weight. The
sandbox is three lines of `OpenFlags` and one authorizer, and the `Store` API lost `seal`,
`is_sealed`, `reopen_privileged` and `reseal` with nothing put in their place. The same
dialect runs on the node, on a phone, and in a browser.

## Consequences

- `spec/data-dictionary.md §2` gains a storage column and the `JSON` logical type; `§2.1`
  says how each declared type is stored; `spec/app-contract.md §7` describes the read-only
  connection; `spec/protocol.md §3` names `cache/<slug>.sqlite`, and `§5` names
  `<table>.sqlite` beside `<table>.csv` with `sqlite_sha256` in the manifest and
  `engine: "sqlite <version>"`.
- `store::` is rewritten: events are read by `store::events`, ranked in Rust, and inserted
  through bound parameters; the three restore tiers and the snapshot writer work from that one
  reading. The store's property — incremental equals replay equals every tier — is unchanged
  and still tested.
- `sys.v_app_nav` and its siblings keep their dictionary spelling: an app's connection will
  see `cache/_sys.sqlite` attached read-only as `sys`. The framework's own connection is
  the file itself and names them bare.
- `apps/*/schema.sql` keep the dictionary's type names — SQLite accepts them as written — and
  lose `COMMENT ON`, which was DuckDB syntax.

## Would reopen if

An app's SQL needs analytical throughput SQLite cannot give — millions of rows, wide
aggregations — and the answer then is DuckDB *as a prebuilt shared library*, never the
bundled build, and never a return to the instance-wide sandbox.

---

Copyright © 2026 Gabriel Mongefranco
