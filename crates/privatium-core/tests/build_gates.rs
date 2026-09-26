// This file is part of Privatium
// crates/privatium-core/tests/build_gates.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-09-26
// Summary: The build gates: each test proves an engine compiled from source — the bundled SQLite, the
//          vendored Lua 5.4 — links and runs on all three CI platforms, before anything is
//          built on it. They are named for the risk they close, not for a spec section,
//          because none of them enforces a normative MUST.
// Notes: See README file for documentation and full license information.
//
// Copyright © 2026 Gabriel Mongefranco
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

// AGENTS.md, Style: unwrap() is permitted in tests, and a test that hides a failure
// behind `?` is worse than one that panics with a line number.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ed25519_dalek::{Signer, SigningKey, Verifier};
use sha2::{Digest, Sha256};

/// R1 — the bundled SQLite C build compiles, links, and executes on this platform, and it
/// is a release the framework's SQL was written against (`NULLS LAST`, `pragma_table_info`,
/// the built-in JSON functions all need 3.38 or later).
#[test]
fn test_r1_sqlite_bundled_links() {
    let engines = privatium_core::linked_engines().unwrap();
    let mut parts = engines.sqlite.split('.');
    let major: u32 = parts.next().unwrap().parse().unwrap();
    let minor: u32 = parts.next().unwrap().parse().unwrap();
    assert_eq!(major, 3, "{}", engines.sqlite);
    assert!(minor >= 38, "{}", engines.sqlite);
}

/// R1 — the reason SQLite is enough (`docs/decisions/0006`): the exact `DECIMAL` the
/// dictionary needs is the framework's own, registered on every connection, and `DATE`
/// arithmetic is SQLite's own date functions over ISO 8601 text. 0.1 + 0.2 must be 0.3
/// here, which a float would make 0.30000000000000004.
#[test]
fn test_r1_sqlite_has_exact_decimal_and_date_arithmetic() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    privatium_core::store::decimal::register(&conn).unwrap();

    let sum: String = conn
        .query_row("SELECT decimal_add('0.1', '0.2')", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sum, "0.3");

    let due: String = conn
        .query_row("SELECT date('2026-08-28', '+30 days')", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(due, "2026-09-27");

    // ISO 8601 text compares as time, which is why the log's `ts` needs no other type.
    let ordered: bool = conn
        .query_row(
            "SELECT '2026-08-28T14:03:11.412Z' < '2026-09-04T00:00:00.000Z'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(ordered);
}

/// `AGENTS.md`, Language and stack — "no extension loading". The bundled build compiles
/// the JSON functions in, and `load_extension()` is refused at the API before the
/// sandbox authorizer ever sees it.
#[test]
fn test_r1_sqlite_json_is_built_in_and_loading_is_off() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();

    let value: String = conn
        .query_row("SELECT json_extract('{\"a\":\"b\"}', '$.a')", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(value, "b");

    let error = conn
        .execute_batch("SELECT load_extension('anything')")
        .unwrap_err()
        .to_string();
    assert!(error.contains("not authorized"), "{error}");
}

/// R2 — the vendored Lua C build compiles and links, and it is 5.4.
///
/// Not LuaJIT (iOS forbids JIT) and not Luau (a dialect fragments the documentation and
/// the assistance that `app-skills/` depends on). `AGENTS.md`, Language and stack.
#[test]
fn test_r2_mlua_vendored_links_and_is_lua_54() {
    let engines = privatium_core::linked_engines().unwrap();
    assert_eq!(engines.lua, "Lua 5.4");
}

/// Dependency skew, caught early: `ed25519-dalek`, `rand`, and `sha2` must agree on one
/// `rand_core` and one `digest`, or node identity (`spec/protocol.md §2.1`) cannot be
/// written at all. Generate, sign, verify, and hash — the four operations identity needs.
#[test]
fn test_m1_ed25519_and_sha256_agree_on_rand_core_and_digest() {
    let mut rng = rand::rng();
    let signing = SigningKey::generate(&mut rng);
    let verifying = signing.verifying_key();

    let message = b"privatium m0 build gate";
    let signature = signing.sign(message);
    assert!(verifying.verify(message, &signature).is_ok());

    // §2.1 derives the node ID from SHA-256 of the public key. This is not that derivation
    // — that has its own spec-named test — only proof that the digest crate and
    // the signature crate can be handed the same bytes in one build.
    let digest = Sha256::digest(verifying.as_bytes());
    assert_eq!(digest.len(), 32);
}
