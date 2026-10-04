// Project:  Privatium™  |  File: crates/privatium-core/tests/js/client.test.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  Browser channel framing, streaming, cancellation and origin confinement (§8.3).
//           See main README.md for full license information.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Transport, encode, decode, endpoint, eventSource, channel, identityRefusal } from '../../assets/shell/channel.js';
import { Frame } from '../../assets/shell/session.js';

const utf8 = new TextEncoder();
function connection(limit) {
  globalThis.location = { href: 'http://192.0.2.1:8420/a/hello/', origin: 'http://192.0.2.1:8420', protocol: 'http:', host: '192.0.2.1:8420' };
  const key = new Uint8Array(32).fill(17);
  const incoming = new Frame(key, 1), outgoing = new Frame(key, 2);
  class Socket extends EventTarget {
    readyState = 1; sent = [];
    send(bytes) { this.sent.push(decode(incoming.open(bytes))); }
    close() { this.readyState = 3; this.dispatchEvent(new Event('close')); }
    reply(head, text = '') { this.dispatchEvent(new MessageEvent('message', { data: outgoing.seal(encode(head, utf8.encode(text))).buffer })); }
  }
  const socket = new Socket();
  const transport = new Transport(socket, { send: new Frame(key, 1), receive: new Frame(key, 2) }, limit);
  return { socket, transport };
}
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

test('test_spec_8_3_browser_closes_at_id_budget_so_next_session_can_reconnect', async () => {
  const { socket, transport } = connection(1);
  const first = transport.fetch('/api/node'); await tick();
  socket.reply({ id: 1, kind: 'res', status: 204, headers: {} }); socket.reply({ id: 1, kind: 'end' });
  await first;
  await assert.rejects(transport.fetch('/api/node'));
  assert.equal(transport.closed, true);
  assert.throws(() => connection(65537));
});

test('test_spec_8_1_refusal_names_node_and_both_public_fingerprints_without_peer_text', () => {
  const record = { node: { id: 'abcdefgh', ed25519: Buffer.alloc(32, 1).toString('base64') } };
  const hello = JSON.stringify({ cert: Buffer.from(JSON.stringify({ node_pub: Buffer.alloc(32, 2).toString('base64'), detail: 'untrusted synthetic text' })).toString('base64') });
  const error = identityRefusal(record, hello);
  assert.equal(error.identity.node, 'abcdefgh');
  assert.match(error.identity.pinned, /^[0-9a-f]{64}$/);
  assert.match(error.identity.presented, /^[0-9a-f]{64}$/);
  assert.notEqual(error.identity.pinned, error.identity.presented);
  assert.ok(!JSON.stringify(error).includes('untrusted synthetic text'));
});

test('test_spec_7_6_missing_or_invalid_pairing_never_opens_a_socket', async () => {
  let opened = 0;
  globalThis.WebSocket = class { constructor() { opened++; } };
  for (const value of [null, '{}', '{"v":2}', 'invalid']) {
    globalThis.localStorage = { getItem: () => value };
    await assert.rejects(channel());
  }
  globalThis.localStorage = { getItem: () => { throw new Error('disabled'); } };
  await assert.rejects(channel()); assert.equal(opened, 0);
});

test('test_spec_8_3_browser_bounds_unread_body_and_refuses_future_frames', async () => {
  const { socket, transport } = connection();
  const promise = transport.fetch('/api/events'); await tick();
  socket.reply({ id: 1, kind: 'res', status: 200, headers: {} });
  const response = await promise;
  for (let i = 0; i < 17; i++) socket.reply({ id: 1, kind: 'chunk' }, 'x'.repeat(65536));
  assert.equal(socket.sent.at(-1).head.kind, 'cancel');
  await assert.rejects(response.text()); transport.close();
});

test('test_spec_10_4_browser_client_holds_exactly_one_endpoint', async () => {
  const { socket, transport } = connection();
  assert.equal(endpoint(), 'ws://192.0.2.1:8420/ws');
  for (const path of ['http://192.0.2.2:8420/', '//example.invalid/', 'https://192.0.2.1:8420/', 'javascript:alert(1)']) {
    await assert.rejects(transport.fetch(path), /origin/);
  }
  assert.equal(socket.sent.length, 0); transport.close();
});

test('test_spec_8_3_browser_decodes_bounded_frames_and_refuses_invalid_metadata', () => {
  for (const raw of [new Uint8Array(), new Uint8Array([255,255,255,255]), encode({ id: -1, kind: 'end' }), encode({ id: 2**53, kind: 'end' }), encode({ id: 1, kind: 'end', extra: true })]) assert.throws(() => decode(raw));
  const frame = decode(encode({ id: 1, kind: 'chunk' }, new Uint8Array([0,255])));
  assert.deepEqual(frame.payload, new Uint8Array([0,255]));
});

test('test_spec_8_3_browser_streams_interleaved_responses_and_cancels', async () => {
  const { socket, transport } = connection();
  const a = transport.fetch('/a/hello/api/events'), b = transport.fetch('/a/hello/api/node');
  await tick();
  assert.deepEqual(socket.sent.map(x => x.head.id), [1,2]);
  socket.reply({ id: 1, kind: 'res', status: 200, headers: {} });
  const response = await a, reader = response.body.getReader();
  socket.reply({ id: 1, kind: 'chunk' }, 'first');
  assert.equal(new TextDecoder().decode((await reader.read()).value), 'first');
  socket.reply({ id: 2, kind: 'res', status: 200, headers: {} });
  socket.reply({ id: 2, kind: 'chunk' }, 'second'); socket.reply({ id: 2, kind: 'end' });
  assert.equal(await (await b).text(), 'second');
  await reader.cancel();
  assert.equal(socket.sent.at(-1).head.kind, 'cancel');
  socket.reply({ id: 1, kind: 'chunk' }, 'already in flight');
  transport.close();
});

test('test_spec_8_3_1_browser_handoff_is_metadata_and_resume_has_no_original_body', async () => {
  const { socket, transport } = connection();
  const promise = transport.fetch('/a/hello/name', { method: 'POST', body: 'synthetic', navigation: true }); await tick();
  assert.equal(socket.sent[0].head.navigation, true);
  const handoff = '01J00000000000000000000000';
  socket.reply({ id: 1, kind: 'res', status: 200, headers: {}, handoff }); socket.reply({ id: 1, kind: 'end' });
  assert.equal((await promise).handoff, handoff);
  const resumed = transport.resume(handoff); await tick();
  assert.deepEqual(socket.sent.at(-1).head, { id: 2, kind: 'resume', handoff });
  assert.equal(socket.sent.at(-1).payload.length, 0);
  socket.reply({ id: 2, kind: 'res', status: 200, headers: {} }); socket.reply({ id: 2, kind: 'end' });
  await resumed; transport.close();
});

test('test_spec_8_3_sse_parser_preserves_events_across_utf8_and_crlf_boundaries', async () => {
  let controller;
  const stream = new ReadableStream({ start(c) { controller = c; } });
  const source = eventSource('/api/stream', async () => new Response(stream, { headers: { 'content-type':'text/event-stream' } }));
  const events = []; source.addEventListener('append', e => events.push(e.data));
  await tick();
  for (const byte of utf8.encode('event: append\r\ndata: synthetic 🐙\r\ndata: second line\r\n\r\n')) controller.enqueue(new Uint8Array([byte]));
  await tick(); assert.deepEqual(events, ['synthetic 🐙\nsecond line']); source.close();
});

test('test_spec_8_3_malformed_response_closes_all_pending_requests_without_plaintext_fallback', async () => {
  const { socket, transport } = connection();
  const promise = transport.fetch('/a/hello/api/node'); await tick();
  const refused = assert.rejects(promise, /channel/);
  socket.reply({ id: 1, kind: 'chunk' }, 'body before head');
  await refused; assert.equal(transport.closed, true);
});

// ---------------------------------------------------------------------------------------
// The pairing screen (§7.2) and the refusal screen (§8.1), over a document fake enough
// to hold what client.js does with one: elements with text, attributes, children,
// listeners and a hidden flag.
// ---------------------------------------------------------------------------------------

function element(tag) {
  const listeners = {}, attrs = {}, children = [];
  const el = {
    tagName: tag.toUpperCase(), textContent: '', value: '', hidden: false, disabled: false, dataset: {}, children,
    setAttribute(name, value) { attrs[name] = String(value); if (name === 'hidden') el.hidden = true; },
    getAttribute(name) { return attrs[name] ?? null; },
    addEventListener(name, fn) { (listeners[name] ||= []).push(fn); },
    fire(name, event = { preventDefault() {} }) { for (const fn of listeners[name] || []) fn(event); },
    append(...nodes) { children.push(...nodes); },
    insertBefore(node, before) { children.splice(Math.max(0, children.indexOf(before)), 0, node); },
    replaceChildren(...nodes) { children.splice(0, children.length, ...nodes); },
    focus() { el.focused = true; },
    querySelector(selector) { return el.children.find(c => selector === '.pv-glyph-label' && c.className === 'pv-glyph-label') || null; },
    get className() { return attrs.class || ''; }, set className(v) { attrs.class = v; },
    classList: {
      contains: name => (attrs.class || '').split(/\s+/).includes(name),
      remove: name => { attrs.class = (attrs.class || '').split(/\s+/).filter(c => c && c !== name).join(' '); },
    },
    get id() { return attrs.id || ''; }, set id(v) { attrs.id = v; },
    get type() { return attrs.type; }, set type(v) { attrs.type = v; },
    get tabIndex() { return attrs.tabindex; }, set tabIndex(v) { attrs.tabindex = v; },
  };
  return el;
}

/** The bootstrap's pairing markup as objects: sixteen keys, the fields, the regions. */
function pairingDocument() {
  const byId = {};
  const make = (id, tag = 'div') => { const el = element(tag); el.id = id; byId[id] = el; return el; };
  const keys = [];
  for (let index = 0; index < 16; index++) {
    const key = element('button'); key.className = 'pv-pad-key'; key.dataset.glyph = String(index);
    const label = element('span'); label.className = 'pv-glyph-label'; label.textContent = ['Unicorn', 'Headphones', 'Pizza', 'UFO', 'Guitar', 'Mushroom', 'Diamond', 'Fox', 'Lightning', 'Hot Pepper', 'Flamingo', 'Artist Palette', 'Pineapple', 'Maple Leaf', 'Game Die', 'Strawberry'][index];
    key.append(label); keys.push(key);
  }
  const section = make('pv-pair', 'section'); section.hidden = true;
  make('pv-chosen', 'output'); make('pv-pair-status', 'p'); make('pv-words', 'input'); make('pv-label', 'input');
  make('pv-pair-form', 'form'); make('pv-pair-submit', 'button'); make('pv-undo', 'button'); make('pv-clear', 'button');
  const heading = make('pv-heading', 'h1'); heading.className = 'pv-quiet';
  const connecting = make('pv-connecting', 'p'); connecting.className = 'pv-quiet';
  const body = element('body'); body.dataset = { name: 'Study', node: 'k7m2q9xf' };
  const quiet = () => [heading, connecting].filter(el => el.classList.contains('pv-quiet'));
  return { body, keys, byId, getElementById: id => byId[id] || null, querySelectorAll: selector => selector === '.pv-quiet' ? quiet() : keys, createElement: element };
}

test('test_spec_7_2_pad_and_word_field_yield_the_same_sixteen_bits', async () => {
  const { pairingScreen } = await import('../../assets/shell/client.js');
  const { encodeCode } = await import('../../assets/shell/pair.js');
  Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'Synthetic/1.0 (Android)' }, configurable: true, writable: true });
  const sent = [];
  const doc = pairingDocument();
  const screen = pairingScreen({ doc, pair: async (code, label) => { sent.push({ code, label }); return {}; }, onPaired() {} });
  assert.equal(doc.byId['pv-pair'].hidden, false, 'the screen shows');
  assert.equal(doc.byId['pv-connecting'].getAttribute('hidden'), '');
  assert.equal(doc.byId['pv-label'].value, 'Android phone');
  // Four taps on the pad — Fox, Strawberry, Pizza, Game Die — are 0x7F2E, exactly as
  // typing the two words that render the same code.
  const code = 0x7F2E, words = encodeCode(code).words.join(' ');
  for (const index of [7, 15, 2, 14]) doc.keys[index].fire('click');
  assert.equal(doc.byId['pv-chosen'].textContent, 'Fox, Strawberry, Pizza, Game Die (4 of 4)');
  assert.equal(screen.code(), code);
  doc.keys[0].fire('click');
  assert.equal(screen.code(), code, 'a fifth tap changes nothing');
  screen.undo(); screen.undo();
  assert.equal(doc.byId['pv-chosen'].textContent, 'Fox, Strawberry (2 of 4)');
  assert.equal(screen.code(), null, 'two taps and an empty field send nothing');
  doc.byId['pv-words'].value = words.toUpperCase().replace(' ', '-');
  assert.equal(screen.code(), code, 'the field is case- and punctuation-insensitive');
  screen.clear();
  assert.equal(doc.byId['pv-chosen'].textContent, 'none yet');
  doc.byId['pv-label'].value = '  Pixel 9 ';
  assert.equal(await screen.submit(), true);
  assert.deepEqual(sent, [{ code, label: 'Pixel 9' }]);
  assert.match(doc.byId['pv-pair-status'].textContent, /^Paired/);

  // The three outcomes are said in the status region; a wrong code re-enables Pair.
  for (const [thrown, expected] of [
    ['pairing is closed on the node; open it there and try again', /closed on the space/],
    ['the pairing code did not match; check the code on the node and try again', /did not match.*five attempts/],
    ['too many attempts, or the code was replaced; read the code on the node now and try again in a moment', /too many tries, or a new code/],
    ['cannot pair: the node ended the connection (1006)', /^cannot pair: the space ended the connection \(1006\)$/],
    ['cannot pair: this browser will not keep the pairing; enable site storage and try again', /site storage/],
  ]) {
    const failing = pairingScreen({ doc, pair: async () => { throw new Error(thrown); } });
    doc.byId['pv-words'].value = words;
    assert.equal(await failing.submit(), false);
    assert.match(doc.byId['pv-pair-status'].textContent, expected);
    assert.equal(doc.byId['pv-pair-submit'].disabled, false);
  }
  const empty = pairingScreen({ doc, pair: async () => ({}) });
  doc.byId['pv-words'].value = '';
  assert.equal(await empty.submit(), false);
  assert.match(doc.byId['pv-pair-status'].textContent, /Tap the four emoji/);
  doc.byId['pv-words'].value = 'not a code at all';
  assert.equal(await empty.submit(), false);
  assert.match(doc.byId['pv-pair-status'].textContent, /not recognized/);
});

test('test_spec_7_7_pairing_screen_lifts_the_bootstraps_quiet_delay', async () => {
  const { pairingScreen } = await import('../../assets/shell/client.js');
  const doc = pairingDocument();
  assert.equal(doc.byId['pv-heading'].classList.contains('pv-quiet'), true);
  pairingScreen({ doc, pair: async () => {} });
  // A device that has to pair reads the heading now, and the connecting status gives way.
  assert.equal(doc.byId['pv-heading'].classList.contains('pv-quiet'), false);
  assert.equal(doc.byId['pv-connecting'].classList.contains('pv-quiet'), false);
  assert.equal(doc.byId['pv-connecting'].getAttribute('hidden'), '');
  assert.equal(doc.byId['pv-pair'].hidden, false);
});

test('test_spec_8_1_refusal_screen_has_no_dismiss', async () => {
  const { showFailure } = await import('../../assets/shell/client.js');
  const { identityRefusal } = await import('../../assets/shell/channel.js');
  const record = { node: { id: 'abcdefgh', ed25519: Buffer.alloc(32, 1).toString('base64') } };
  const hello = JSON.stringify({ cert: Buffer.from(JSON.stringify({ node_pub: Buffer.alloc(32, 2).toString('base64') })).toString('base64') });
  const error = identityRefusal(record, hello);
  const doc = pairingDocument();
  doc.body.dataset = { name: 'Study', node: 'abcdefgh' };
  const removed = [];
  showFailure(error, doc, { removeItem: key => removed.push(key) });
  const [main] = doc.body.children;
  assert.equal(doc.body.children.length, 1, 'the whole document is the refusal');
  assert.equal(main.focused, true);
  const [title, message, text, list, forget] = main.children;
  assert.equal(title.textContent, 'Space identity could not be verified');
  assert.equal(message.getAttribute('role'), 'alert');
  assert.match(text.textContent, /no override/);
  const buttons = main.children.filter(c => c.tagName === 'BUTTON');
  assert.equal(buttons.length, 1, 'one action and no dismiss');
  assert.equal(forget, buttons[0]);
  assert.match(forget.textContent, /Forget this pairing and pair again/);
  assert.ok(!main.children.some(c => /continue|dismiss|ignore|anyway/i.test(c.textContent)));
  const terms = list.children.filter((_, i) => i % 2 === 0).map(c => c.textContent);
  const values = list.children.filter((_, i) => i % 2 === 1).map(c => c.textContent);
  assert.deepEqual(terms, ['Space', 'Pinned key fingerprint', 'Presented key fingerprint']);
  assert.equal(values[0], 'Study (abcdefgh)', 'the space is named');
  assert.match(values[1], /^[0-9a-f]{64}$/);
  assert.match(values[2], /^[0-9a-f]{64}$/);
  assert.notEqual(values[1], values[2]);
  globalThis.sessionStorage = { removeItem() {} };
  globalThis.location = { reload() { doc.reloaded = true; } };
  forget.fire('click');
  assert.deepEqual(removed, ['pv:device'], 'forgetting wipes the pairing and nothing else');
  assert.equal(doc.reloaded, true);
});
