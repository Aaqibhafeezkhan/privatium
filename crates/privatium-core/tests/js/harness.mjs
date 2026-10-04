// Project:  Privatium™  |  File: crates/privatium-core/tests/js/harness.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  What pv.js needs of a browser, faked for `node --test`: a location, a
//           navigator, a localStorage that can be told to fail, an EventSource that goes
//           nowhere, and a fetch that answers from a script and records every request. Each
//           test imports a fresh copy of the module through a unique query string.
//           See main README.md for full license information.

import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';

const PV = new URL('../../assets/shell/pv.js', import.meta.url);

/** The text of pv.js, for assertions about what it does and does not contain. */
export function source() { return readFileSync(fileURLToPath(PV), 'utf8'); }

/** An in-memory localStorage; `broken` makes every call throw, as a private window can. */
export function storage(broken = false) {
  const map = new Map();
  const fail = () => { if (broken) throw new Error('storage is unavailable'); };
  return {
    getItem(k) { fail(); return map.has(k) ? map.get(k) : null; },
    setItem(k, v) { fail(); map.set(k, String(v)); },
    removeItem(k) { fail(); map.delete(k); },
    key(i) { fail(); return [...map.keys()][i] ?? null; },
    get length() { fail(); return map.size; },
    get map() { return map; },
  };
}

/**
 * Stand up the browser globals and import a fresh pv.js.
 *
 * `respond(method, path, body)` is the node: return `{ status, json }` or `{ status, text }`,
 * or throw a TypeError to be unreachable. Every request is pushed to `requests`.
 */
export async function page({ pathname = '/a/sketch/', online = true, store = storage(), respond } = {}) {
  const requests = [];
  const listeners = {};
  globalThis.location = { pathname };
  Object.defineProperty(globalThis, 'navigator', { value: { onLine: online }, configurable: true, writable: true });
  Object.defineProperty(globalThis, 'localStorage', { value: store, configurable: true, writable: true });
  globalThis.addEventListener = (name, fn) => { (listeners[name] ||= []).push(fn); };
  globalThis.EventSource = class { constructor() { this.listeners = {}; } addEventListener() {} close() {} };
  let answer = respond;
  globalThis.fetch = async (path, init = {}) => {
    const body = init.body ? JSON.parse(init.body) : undefined;
    const entry = { method: init.method || 'GET', path, body, failed: false };
    requests.push(entry);
    let out;
    try { out = await answer(entry.method, path, body); }
    catch (e) { entry.failed = true; throw e; }     // never reached the node
    const status = out.status ?? 200;
    const text = out.text ?? JSON.stringify(out.json ?? null);
    return new Response(text, { status, headers: { 'content-type': out.text != null ? 'application/x-ndjson' : 'application/json' } });
  };
  const module = await import(PV.href + '?fresh=' + Math.random());
  return {
    pv: module.pv,
    PvOffline: module.PvOffline,
    requests,
    store,
    /** Swap the node's behaviour mid-test. */
    respond(fn) { answer = fn; },
    /** Fire the browser's own online/offline event. */
    fire(name) { for (const fn of listeners[name] || []) fn(); },
    /** Let every pending microtask and timer of this tick settle. */
    settle: () => new Promise(resolve => setTimeout(resolve, 0)),
  };
}

/** A node that is up, serves app `app`, and appends whatever it is sent. */
export function upNode(app = 'sketch', lam = { value: 10 }) {
  return (method, path, body) => {
    if (path.endsWith('/api/node')) return { json: { id: 'k7m2q9xf', dev: 'k7m2q9xf', name: 'Study', app, solo: false, peers: 0, restore_tier: 3 } };
    if (method === 'POST' && path.endsWith('/api/events')) { lam.value += body.events.length; return { json: { appended: body.events.length, lam: lam.value, ts: '2026-09-05T12:00:00.000Z', dev: 'k7m2q9xf', ids: body.events.map(e => e.id) } }; }
    if (method === 'GET' && path.includes('/api/events')) return { text: '' };
    return { status: 404, json: { error: '404 Not Found' } };
  };
}

/** A node nobody can reach. */
export function downNode() {
  return () => { throw new TypeError('fetch failed'); };
}
