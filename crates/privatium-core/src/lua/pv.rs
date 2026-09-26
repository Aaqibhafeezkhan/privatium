// This file is part of Privatium
// crates/privatium-core/src/lua/pv.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-03
// Last Modified: 2026-09-26
// Summary: The `pv` module of spec/lua-api.md §3: routing (§3.1), reading on the sandboxed connection
//          (§3.2), writing through the node's log as appends and batches (§3.3), and the rest
//          of §3.4. Routes and `pv.on` register while app.lua loads; reads and writes run only
//          inside a request, where a connection and the node are at hand.
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

use std::sync::PoisonError;

use mlua::{Function, Lua, Table, Value};

use crate::app::{Appended, Event};
use crate::lua::convert::{
    bind_params, json_to_lua, lua_object, lua_to_json, row_to_lua, sql_error,
};
use crate::lua::dec::Dec;
use crate::lua::{Phase, RouteSpec, VmData, sandbox};
use crate::store::query::column_types;

/// Registry key: the sequence of route handlers, in registration order (`§2.4`'s index).
pub(crate) const ROUTES_KEY: &str = "pv.routes";

/// Registry key: the `pv.on('append', …)` handlers, in registration order.
pub(crate) const ON_APPEND_KEY: &str = "pv.on_append";

/// Registry key: the `tx` table `pv.batch` hands its function.
const TX_KEY: &str = "pv.tx";

/// The field that marks a table returned by a handler as a response, and its kinds.
pub(crate) const RESPONSE_FIELD: &str = "pv_response";

/// The module's surface — every `pv.<name>` [`install`] registers, with the section of
/// `spec/lua-api.md` that specifies it. What `cargo xtask gen-skill-reference` writes
/// into `app-skills/privatium-tier1-lua/reference/pv-api.md`, and what a unit test holds
/// `install` to, so the reference cannot name a function the host lacks.
pub const SURFACE: &[(&str, &str)] = &[
    ("get", "3.1"),
    ("post", "3.1"),
    ("route", "3.1"),
    ("render", "3.1"),
    ("redirect", "3.1"),
    ("json", "3.1"),
    ("text", "3.1"),
    ("query", "3.2"),
    ("query1", "3.2"),
    ("get_row", "3.2"),
    ("dec", "3.2"),
    ("append", "3.3"),
    ("delete", "3.3"),
    ("batch", "3.3"),
    ("ulid", "3.4"),
    ("now", "3.4"),
    ("device", "3.4"),
    ("node", "3.4"),
    ("setting", "3.4"),
    ("log", "3.4"),
    ("on", "3.4"),
    ("url", "4.0"),
];

/// Build the module, register it for `require 'privatium'`, and return it.
pub(crate) fn install(lua: &Lua) -> mlua::Result<Table> {
    lua.set_named_registry_value(ROUTES_KEY, lua.create_table()?)?;
    lua.set_named_registry_value(ON_APPEND_KEY, lua.create_table()?)?;

    let tx = lua.create_table()?;
    tx.raw_set("append", lua.create_function(tx_append)?)?;
    tx.raw_set("delete", lua.create_function(tx_delete)?)?;
    lua.set_named_registry_value(TX_KEY, tx)?;

    let pv = lua.create_table()?;
    // §3.1
    pv.raw_set(
        "get",
        lua.create_function(|lua, (pattern, handler): (String, Function)| {
            register(lua, "GET", &pattern, handler)
        })?,
    )?;
    pv.raw_set(
        "post",
        lua.create_function(|lua, (pattern, handler): (String, Function)| {
            register(lua, "POST", &pattern, handler)
        })?,
    )?;
    pv.raw_set(
        "route",
        lua.create_function(
            |lua, (method, pattern, handler): (String, String, Function)| {
                register(lua, &method.to_ascii_uppercase(), &pattern, handler)
            },
        )?,
    )?;
    // §3.2
    pv.raw_set("query", lua.create_function(query)?)?;
    pv.raw_set("query1", lua.create_function(query1)?)?;
    pv.raw_set("get_row", lua.create_function(get_row)?)?;
    pv.raw_set(
        "dec",
        lua.create_function(|_, value: Value| Dec::coerce(&value).map(Dec))?,
    )?;
    // §3.3
    pv.raw_set("append", lua.create_function(append)?)?;
    pv.raw_set("delete", lua.create_function(delete)?)?;
    pv.raw_set("batch", lua.create_function(batch)?)?;
    // §3.4
    pv.raw_set("ulid", lua.create_function(|_, ()| Ok(crate::new_ulid()))?)?;
    pv.raw_set("now", lua.create_function(|_, ()| Ok(crate::log::now()))?)?;
    pv.raw_set("device", lua.create_function(device)?)?;
    pv.raw_set("node", lua.create_function(node)?)?;
    pv.raw_set("setting", lua.create_function(setting)?)?;
    pv.raw_set("log", lua.create_function(log)?)?;
    pv.raw_set("on", lua.create_function(on)?)?;
    // Responses (§3.1's table) and `url` (§4.0).
    pv.raw_set("render", lua.create_function(render)?)?;
    pv.raw_set("redirect", lua.create_function(redirect)?)?;
    pv.raw_set("json", lua.create_function(json)?)?;
    pv.raw_set("text", lua.create_function(text)?)?;
    pv.raw_set("url", lua.create_function(sandbox::url)?)?;

    lua.set_named_registry_value(sandbox::PV_KEY, pv.clone())?;
    Ok(pv)
}

// ---------------------------------------------------------------------------------------
// Access to the VM's state
// ---------------------------------------------------------------------------------------

fn data(lua: &Lua) -> mlua::Result<mlua::AppDataRef<'_, VmData>> {
    lua.app_data_ref::<VmData>()
        .ok_or_else(|| mlua::Error::runtime("pv: no app is loaded in this state"))
}

fn data_mut(lua: &Lua) -> mlua::Result<mlua::AppDataRefMut<'_, VmData>> {
    lua.app_data_mut::<VmData>()
        .ok_or_else(|| mlua::Error::runtime("pv: no app is loaded in this state"))
}

fn outside_request(what: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "pv.{what} is only available while handling a request, not while app.lua loads"
    ))
}

fn outside_load(what: &str) -> mlua::Error {
    mlua::Error::runtime(format!(
        "pv.{what} registers while app.lua loads, not inside a handler"
    ))
}

// ---------------------------------------------------------------------------------------
// §3.1 — routing
// ---------------------------------------------------------------------------------------

fn register(lua: &Lua, method: &str, pattern: &str, handler: Function) -> mlua::Result<()> {
    let spec = RouteSpec::parse(method, pattern).map_err(mlua::Error::runtime)?;
    {
        let mut data = data_mut(lua)?;
        if data.phase != Phase::Loading {
            return Err(outside_load("get/post/route"));
        }
        data.routes.push(spec);
    }
    let routes: Table = lua.named_registry_value(ROUTES_KEY)?;
    routes.raw_seti(routes.raw_len() + 1, handler)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// §3.2 — reading
// ---------------------------------------------------------------------------------------

fn query(lua: &Lua, (sql, params): (String, Option<Table>)) -> mlua::Result<Table> {
    let data = data(lua)?;
    let ctx = data.ctx.as_ref().ok_or_else(|| outside_request("query"))?;
    let mut statement = ctx.conn.prepare(&sql).map_err(sql_error)?;
    let columns = column_types(&statement, &data.schema);
    let params = bind_params(params)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(params))
        .map_err(sql_error)?;
    let out = lua.create_table()?;
    let mut index = 1;
    while let Some(row) = rows.next().map_err(sql_error)? {
        out.raw_seti(index, row_to_lua(lua, row, &columns)?)?;
        index += 1;
    }
    Ok(out)
}

fn query1(lua: &Lua, (sql, params): (String, Option<Table>)) -> mlua::Result<Value> {
    let data = data(lua)?;
    let ctx = data.ctx.as_ref().ok_or_else(|| outside_request("query1"))?;
    let mut statement = ctx.conn.prepare(&sql).map_err(sql_error)?;
    let columns = column_types(&statement, &data.schema);
    let params = bind_params(params)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(params))
        .map_err(sql_error)?;
    match rows.next().map_err(sql_error)? {
        Some(row) => Ok(Value::Table(row_to_lua(lua, row, &columns)?)),
        None => Ok(Value::Nil),
    }
}

/// `pv.get_row(tbl, id)`: the row, or `nil` when it is absent — including when its winning
/// event is a tombstone, since `§4.5` materializes no row for one.
fn get_row(lua: &Lua, (tbl, id): (String, String)) -> mlua::Result<Value> {
    if !is_table_name(&tbl) {
        return Err(mlua::Error::runtime(format!(
            "pv.get_row: {tbl:?} is not a table name"
        )));
    }
    let sql = format!(
        "SELECT * FROM {} WHERE id = ?",
        crate::store::materialize::quote_ident(&tbl)
    );
    let params = lua.create_table()?;
    params.raw_seti(1, id)?;
    query1(lua, (sql, Some(params)))
}

fn is_table_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// ---------------------------------------------------------------------------------------
// §3.3 — writing
// ---------------------------------------------------------------------------------------

/// The two arities of `append` (`§3.3`): `(tbl, data)` and `(tbl, id, data)` with a `nil`
/// id allowed in the second. Returns the id, minted when none was given.
fn change_of(what: &str, tbl: String, second: Value, third: Option<Value>) -> mlua::Result<Event> {
    if !is_table_name(&tbl) {
        return Err(mlua::Error::runtime(format!(
            "{what}: {tbl:?} is not a table name"
        )));
    }
    let (id, data) = match (second, third) {
        (Value::Table(data), None | Some(Value::Nil)) => (None, data),
        (Value::Nil, Some(Value::Table(data))) => (None, data),
        (Value::String(id), Some(Value::Table(data))) => (Some(id.to_str()?.to_string()), data),
        _ => {
            return Err(mlua::Error::runtime(format!(
                "{what}: call it as {what}(tbl, data) or {what}(tbl, id, data), where data \
                 is a table and id is a string or nil (spec/lua-api.md §3.3)"
            )));
        }
    };
    let id = match id {
        Some(id) => check_id(what, id)?,
        None => crate::new_ulid(),
    };
    Ok(Event {
        tbl,
        id,
        d: Some(serde_json::Value::Object(lua_object(&data)?)),
    })
}

fn check_id(what: &str, id: String) -> mlua::Result<String> {
    if id.is_empty() || id.chars().any(char::is_control) {
        return Err(mlua::Error::runtime(format!(
            "{what}: an id is a non-empty string without control characters"
        )));
    }
    Ok(id)
}

fn append(lua: &Lua, (tbl, second, third): (String, Value, Option<Value>)) -> mlua::Result<String> {
    let change = change_of("pv.append", tbl, second, third)?;
    {
        let data = data(lua)?;
        if data.ctx.is_none() {
            return Err(outside_request("append"));
        }
        if data.batch.is_some() {
            return Err(mlua::Error::runtime(
                "pv.append inside pv.batch: use tx.append so the batch stays atomic",
            ));
        }
    }
    let id = change.id.clone();
    write(lua, vec![change])?;
    Ok(id)
}

fn delete(lua: &Lua, (tbl, id): (String, String)) -> mlua::Result<()> {
    if !is_table_name(&tbl) {
        return Err(mlua::Error::runtime(format!(
            "pv.delete: {tbl:?} is not a table name"
        )));
    }
    let id = check_id("pv.delete", id)?;
    {
        let data = data(lua)?;
        if data.ctx.is_none() {
            return Err(outside_request("delete"));
        }
        if data.batch.is_some() {
            return Err(mlua::Error::runtime(
                "pv.delete inside pv.batch: use tx.delete so the batch stays atomic",
            ));
        }
    }
    write(lua, vec![Event { tbl, id, d: None }])
}

/// `pv.batch(function(tx) … end)`: every event or none, contiguous `seq`, one `ts`.
fn batch(lua: &Lua, build: Function) -> mlua::Result<()> {
    {
        let mut data = data_mut(lua)?;
        if data.ctx.is_none() {
            return Err(outside_request("batch"));
        }
        if data.batch.is_some() {
            return Err(mlua::Error::runtime("pv.batch does not nest"));
        }
        data.batch = Some(Vec::new());
    }
    let tx: Table = lua.named_registry_value(TX_KEY)?;
    let outcome = build.call::<()>(tx);
    let staged = data_mut(lua)?.batch.take().unwrap_or_default();
    // An error anywhere in the function means nothing reaches the log.
    outcome?;
    if staged.is_empty() {
        return Ok(());
    }
    write(lua, staged)
}

/// `tx.append`: staged, and the ULID is minted now so later events can reference it.
fn tx_append(
    lua: &Lua,
    (tbl, second, third): (String, Value, Option<Value>),
) -> mlua::Result<String> {
    let change = change_of("tx.append", tbl, second, third)?;
    let id = change.id.clone();
    stage(lua, change)?;
    Ok(id)
}

fn tx_delete(lua: &Lua, (tbl, id): (String, String)) -> mlua::Result<()> {
    if !is_table_name(&tbl) {
        return Err(mlua::Error::runtime(format!(
            "tx.delete: {tbl:?} is not a table name"
        )));
    }
    let id = check_id("tx.delete", id)?;
    stage(lua, Event { tbl, id, d: None })
}

fn stage(lua: &Lua, change: Event) -> mlua::Result<()> {
    let mut data = data_mut(lua)?;
    match data.batch.as_mut() {
        Some(staged) => {
            staged.push(change);
            Ok(())
        }
        None => Err(mlua::Error::runtime(
            "tx is only valid inside the function passed to pv.batch",
        )),
    }
}

/// Append `changes` through the node, apply them to the cache, then fire `pv.on('append')`
/// for each in this VM.
fn write(lua: &Lua, changes: Vec<Event>) -> mlua::Result<()> {
    let (node, slug) = {
        let data = data(lua)?;
        let ctx = data.ctx.as_ref().ok_or_else(|| outside_request("append"))?;
        (ctx.node.clone(), data.slug.clone())
    };
    let appended = {
        let mut node = node.lock().unwrap_or_else(PoisonError::into_inner);
        node.append_batch(&slug, changes)
    }
    .map_err(|error| mlua::Error::runtime(format!("pv.append: {error}")))?;
    fire_append(lua, &appended)
}

/// Call every `pv.on('append')` handler with each event's envelope, in order.
pub(crate) fn fire_append(lua: &Lua, appended: &Appended) -> mlua::Result<()> {
    let handlers: Table = lua.named_registry_value(ON_APPEND_KEY)?;
    if handlers.raw_len() == 0 {
        return Ok(());
    }
    let mut functions = Vec::with_capacity(handlers.raw_len());
    for handler in handlers.sequence_values::<Function>() {
        functions.push(handler?);
    }
    for (offset, change) in appended.events.iter().enumerate() {
        let offset = offset as u64;
        let event = lua.create_table()?;
        event.raw_set("seq", appended.seq.saturating_add(offset))?;
        event.raw_set("lam", appended.lam.saturating_add(offset))?;
        event.raw_set("ts", appended.ts.as_str())?;
        event.raw_set("dev", appended.dev.as_str())?;
        event.raw_set("app", appended.app.as_str())?;
        event.raw_set("op", if change.d.is_some() { "put" } else { "del" })?;
        event.raw_set("tbl", change.tbl.as_str())?;
        event.raw_set("id", change.id.as_str())?;
        if let Some(d) = &change.d {
            event.raw_set("d", json_to_lua(lua, d)?)?;
        }
        for handler in &functions {
            handler.call::<()>(event.clone())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// §3.4 — the rest
// ---------------------------------------------------------------------------------------

fn device(lua: &Lua, (): ()) -> mlua::Result<String> {
    let data = data(lua)?;
    let ctx = data.ctx.as_ref().ok_or_else(|| outside_request("device"))?;
    Ok(ctx.device.clone())
}

/// `pv.node()`: `{ id, name, solo, peers, restore_tier }`. `peers` is the number of paired
/// nodes other than this one — a paired phone or browser does not count
/// (`spec/lua-api.md §3.4`); `restore_tier` is `nil` for an app this node has not
/// materialized.
fn node(lua: &Lua, (): ()) -> mlua::Result<Table> {
    let facts = {
        let data = data(lua)?;
        let ctx = data.ctx.as_ref().ok_or_else(|| outside_request("node"))?;
        ctx.facts.clone()
    };
    let table = lua.create_table()?;
    table.raw_set("id", facts.id)?;
    table.raw_set("name", facts.name)?;
    table.raw_set("solo", facts.solo)?;
    table.raw_set("peers", i64::try_from(facts.peers).unwrap_or(i64::MAX))?;
    if let Some(tier) = facts.restore_tier {
        table.raw_set("restore_tier", i64::from(tier))?;
    }
    Ok(table)
}

/// `pv.setting(key, default)`: the JSON-decoded `sys_setting.value`, or `default` when the
/// key is unset.
fn setting(lua: &Lua, (key, default): (String, Value)) -> mlua::Result<Value> {
    let node = {
        let data = data(lua)?;
        let ctx = data
            .ctx
            .as_ref()
            .ok_or_else(|| outside_request("setting"))?;
        ctx.node.clone()
    };
    let raw = {
        let node = node.lock().unwrap_or_else(PoisonError::into_inner);
        node.setting_value(&key)
    }
    .map_err(|error| mlua::Error::runtime(format!("pv.setting: {error}")))?;
    match raw {
        Some(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => json_to_lua(lua, &value),
            Err(_) => Ok(Value::String(lua.create_string(text)?)),
        },
        None => Ok(default),
    }
}

/// `pv.log(level, message)`: the diagnostic log, never stdout.
fn log(lua: &Lua, (level, message): (String, Value)) -> mlua::Result<()> {
    if !matches!(level.as_str(), "debug" | "info" | "warn" | "error") {
        return Err(mlua::Error::runtime(format!(
            "pv.log: level {level:?} is not one of debug, info, warn, error"
        )));
    }
    let tostring: Function = lua.globals().get("tostring")?;
    let message: String = tostring.call(message)?;
    sandbox::diagnostic(lua, &level, &message);
    Ok(())
}

/// `pv.on('append', fn)`: fires for every event this node appends — a handler's
/// `pv.append`, `pv.batch`, or the owner loading `sample/seed.jsonl`.
fn on(lua: &Lua, (event, handler): (String, Function)) -> mlua::Result<()> {
    if event != "append" {
        return Err(mlua::Error::runtime(format!(
            "pv.on: {event:?} is not an event; 'append' is the one there is"
        )));
    }
    if data(lua)?.phase != Phase::Loading {
        return Err(outside_load("on"));
    }
    let handlers: Table = lua.named_registry_value(ON_APPEND_KEY)?;
    handlers.raw_seti(handlers.raw_len() + 1, handler)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------------------

fn response(lua: &Lua, kind: &str) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set(RESPONSE_FIELD, kind)?;
    Ok(table)
}

fn render(lua: &Lua, (view, ctx): (String, Option<Table>)) -> mlua::Result<Table> {
    if !crate::lua::lsp::is_view_name(&view) {
        return Err(mlua::Error::runtime(format!(
            "pv.render: {view:?} is not a view name; views/<name>.lsp is named by letters, \
             digits, '_' and '-'"
        )));
    }
    let table = response(lua, "render")?;
    table.raw_set("view", view)?;
    if let Some(ctx) = ctx {
        table.raw_set("ctx", ctx)?;
    }
    Ok(table)
}

fn redirect(lua: &Lua, location: String) -> mlua::Result<Table> {
    let table = response(lua, "redirect")?;
    table.raw_set("location", location)?;
    Ok(table)
}

fn json(lua: &Lua, value: Value) -> mlua::Result<Table> {
    let body = serde_json::to_string(&lua_to_json(&value)?)
        .map_err(|error| mlua::Error::runtime(format!("pv.json: {error}")))?;
    let table = response(lua, "json")?;
    table.raw_set("body", body)?;
    Ok(table)
}

fn text(lua: &Lua, body: mlua::LuaString) -> mlua::Result<Table> {
    let table = response(lua, "text")?;
    table.raw_set("body", body)?;
    Ok(table)
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// `SURFACE` is what the reference generator writes; `install` is what apps get. The
    /// two are one list.
    #[test]
    fn the_surface_is_what_install_registers() {
        let lua = sandbox::new_state(&crate::config::LuaConfig::default()).unwrap();
        let pv = install(&lua).unwrap();
        let mut registered: Vec<String> = pv
            .pairs::<String, Value>()
            .map(|pair| pair.unwrap().0)
            .collect();
        registered.sort();
        let mut listed: Vec<String> = SURFACE.iter().map(|(name, _)| (*name).to_owned()).collect();
        listed.sort();
        assert_eq!(registered, listed);
    }
}
