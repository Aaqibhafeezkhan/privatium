// Project:  Privatium™  |  File: crates/privatium-core/tests/js/harness.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  What pv.js needs of a browser, faked for `node --test`: a location, a
//           navigator, a localStorage that can be told to fail, an EventSource that goes
//           nowhere, and a fetch that answers from a script and records every request. Each
//           test imports a fresh copy of the module through a unique query string. Also a
//           framed document small enough to drive chrome.js's swap navigation: a body
//           naming its mount, a main region, links, forms and their buttons.
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

/**
 * A fake element: a tag, attributes and children, with the few DOM methods chrome.js's
 * navigation uses. `querySelector('h1')` matches by tag; the submit-control selector
 * matches buttons without a type or of type submit, and inputs of type submit or image.
 */
export function element(tag, attributes = {}, children = []) {
  const el = {
    tagName: tag.toUpperCase(), attributes: { ...attributes }, children: [], parent: null, disabled: false,
    getAttribute(name) { return name in this.attributes ? this.attributes[name] : null; },
    setAttribute(name, value) { this.attributes[name] = String(value); },
    hasAttribute(name) { return name in this.attributes; },
    get name() { return this.attributes.name ?? ''; },
    set name(text) { this.attributes.name = String(text); },
    get value() { return this.attributes.value ?? ''; },
    set value(text) { this.attributes.value = String(text); },
    get type() { return this.attributes.type ?? ''; },
    set type(text) { this.attributes.type = String(text); },
    append(...nodes) { for (const node of nodes) { node.parent = this; this.children.push(node); } },
    contains(other) { for (let node = other; node; node = node.parent) if (node === this) return true; return false; },
    descendants() { return this.children.flatMap(child => [child, ...child.descendants()]); },
    querySelectorAll(selector) {
      if (selector === '[autofocus]') return this.descendants().filter(node => node.hasAttribute('autofocus'));
      if (selector.includes('button')) {
        return this.descendants().filter(node => (node.tagName === 'BUTTON' && ['', 'submit'].includes(node.type))
          || (node.tagName === 'INPUT' && ['submit', 'image'].includes(node.type)));
      }
      return this.descendants().filter(node => node.tagName === selector.toUpperCase());
    },
    querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; },
    closest(selector) { for (let node = this; node; node = node.parent) if (node.tagName === selector.toUpperCase()) return node; return null; },
    focus() { el.ownerDocument.activeElement = el; },
  };
  el.append(...children);
  return el;
}

/**
 * A framed page of the app at `mount`, as the frame renders it under swap navigation
 * (`swap`, the default) or page navigation: a body with `data-pv-mount`, a header outside
 * the main region, and `<main id="main">` holding a link and a form with a submit button.
 * `win` records every full navigation in `assigned` and every native form submission in
 * `submitted`. `__pv_channel` is set on it when `channel` is true.
 */
export function framedPage({ mount = '/a/animals/', href = 'http://192.0.2.1:8420/a/animals/', swap = true, channel = false } = {}) {
  const doc = new EventTarget();
  const own = node => { node.ownerDocument = doc; node.descendants().forEach(child => { child.ownerDocument = doc; }); return node; };
  const link = element('a', { href: 'knowledge' });
  const button = element('button', { type: 'submit', name: 'answer', value: 'yes' });
  const form = element('form', { method: 'post', action: 'teach' }, [element('input', { type: 'text', name: 'animal' }), button]);
  const outside = element('a', { href: '/settings' });
  const main = own(element('main', swap ? { id: 'main', 'hx-boost': 'true' } : { id: 'main' }, [element('h1'), link, form]));
  own(outside);
  doc.body = { dataset: mount ? { pvMount: mount } : {} };
  doc.main = main;
  doc.activeElement = null;
  doc.getElementById = id => (id === 'main' ? doc.main : null);
  doc.createElement = tag => own(element(tag));
  const win = new EventTarget();
  win.location = { href, origin: new URL(href).origin, assigned: [], assign(url) { this.assigned.push(url); } };
  win.submitted = [];
  win.HTMLFormElement = { prototype: { submit() { win.submitted.push(this); } } };
  if (channel) win.__pv_channel = {};
  return { doc, win, main, link, form, button, outside };
}

/**
 * The `detail` htmx gives its request and swap events for a boosted request from `elt`
 * to `path` with `verb`.
 */
export function boosted(elt, path, verb = 'get', extra = {}) {
  return { boosted: true, elt, pathInfo: { requestPath: path, finalRequestPath: path }, requestConfig: { verb, elt, headers: { 'HX-Request': 'true', 'HX-Boosted': 'true' } }, ...extra };
}
