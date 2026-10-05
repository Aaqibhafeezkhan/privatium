// Project:  Privatium™  |  File: crates/privatium-core/tests/js/pv.test.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  pv.js against spec/data-api.md §5 and §6 and spec/protocol.md §10.6, under
//           `node --test`: the outbox queues while the node is unreachable and replays in
//           order when it is back; an empty replay leaves the helper able to replay later;
//           the node's own trouble keeps an entry and a refusal drops it; an append during
//           a replay is not lost; no storage is still a queue; a replay carries its mark
//           and each row's rank for the node to judge, and a landed or conflicting answer
//           is honoured; an entry queued for another app or another node is refused; two
//           pages share one storage without loss; and the outbox's length is reported
//           once per change (spec/app-contract.md §5.2).
//           See main README.md for full license information.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { page, storage, upNode, downNode, source } from './harness.mjs';

test('test_spec_8_3_pv_uses_the_channel_for_requests_and_subscriptions', async () => {
  const requests = []; let subscribed = false;
  globalThis.__pv_channel = {
    fetch: async path => { requests.push(path); return new Response(JSON.stringify({ id: 'synthetic', app: 'sketch' })); },
    eventSource: () => { subscribed = true; return { addEventListener() {}, close() {} }; },
  };
  try {
    const p = await page({ respond: () => { throw new Error('plaintext request'); } });
    await p.settle(); await p.pv.node();
    const stop = p.pv.subscribe(() => {}); stop();
    assert.ok(requests.includes('/a/sketch/api/node'));
    assert.ok(subscribed); assert.equal(p.requests.length, 0);
  } finally { delete globalThis.__pv_channel; }
});

/** The appends that reached the node; an attempt the network dropped is not one. */
const posts = requests => requests.filter(r => r.method === 'POST' && r.path.endsWith('/api/events') && !r.failed);

/** The outbox as storage holds it: one key per entry beneath the mount, in id order. */
const stored = (store, mount = '/a/sketch/') =>
  [...store.map.keys()].filter(k => k.startsWith('pv:outbox:' + mount + ':')).sort().map(k => JSON.parse(store.map.get(k)));

/** A node whose identity is `id`, otherwise `upNode`. */
const nodeCalled = (id, app = 'sketch') => {
  const up = upNode(app);
  return (m, path, body) => (path.endsWith('/api/node') ? { json: { id, dev: id, name: 'Other', app, solo: false, peers: 0, restore_tier: 3 } } : up(m, path, body));
};

test('spec/data-api.md §5: pv.js converts no DECIMAL', () => {
  const text = source();
  for (const conversion of ['parseFloat(', 'Number(', '+row', 'toFixed(']) assert.ok(!text.includes(conversion), conversion);
});

test('test_spec_5_2_outbox_event_reports_the_queue_length_on_change', async () => {
  const doc = new EventTarget(); doc.getElementById = () => null;
  const seen = [], dom = [];
  doc.addEventListener('pv:outbox', event => dom.push(event.detail.waiting));
  const connection = [];
  doc.addEventListener('pv:offline', () => connection.push('offline'));
  doc.addEventListener('pv:online', () => connection.push('online'));
  globalThis.document = doc;
  try {
    const p = await page({ respond: downNode(), online: false });
    p.pv.on('outbox', ({ waiting }) => seen.push(waiting));
    assert.equal(p.pv.waiting, 0);
    await p.pv.put('stroke', '01K4B0000000000000000000A1', { points: [] });
    await p.pv.put('stroke', '01K4B0000000000000000000A2', { points: [] });
    assert.deepEqual(seen, [1, 2]);
    assert.equal(p.pv.waiting, 2);
    p.respond(upNode());
    p.fire('online');
    await p.pv.flush(); await p.settle();
    assert.deepEqual(seen, [1, 2, 1, 0], 'one report per change, none for a repeat');
    assert.deepEqual(dom, seen, 'the same changes reach the document');
    assert.deepEqual(connection, ['online']);
    assert.equal(p.pv.waiting, 0);
  } finally { delete globalThis.document; }
});

test('spec/data-api.md §6: an empty replay at load leaves the outbox able to replay later', async () => {
  const p = await page({ respond: upNode() });
  await p.settle();
  assert.equal(posts(p.requests).length, 0, 'nothing waited');
  p.respond(downNode());
  const out = await p.pv.put('stroke', '01K4B0000000000000000000A1', { points: [[0, 0]] });
  assert.deepEqual(out, { queued: true, appended: 0, ids: ['01K4B0000000000000000000A1'] });
  assert.equal(p.pv.online, false);
  assert.equal(stored(p.store).length, 1);
  p.respond(upNode());
  p.fire('online');
  await p.pv.flush();
  assert.equal(posts(p.requests).length, 1, 'the queued write was sent');
  assert.equal(posts(p.requests)[0].body.events[0].id, '01K4B0000000000000000000A1');
  assert.equal(stored(p.store).length, 0, 'the queue is empty again');
});

test('spec/data-api.md §6: entries replay in order and carry the mark and the app they were queued under', async () => {
  const p = await page({ respond: upNode('sketch', { value: 40 }) });
  await p.settle();
  await p.pv.query('v_x').catch(() => {});                    // a 404 here; the mark comes from an append
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  assert.equal(p.pv.lam, 41);
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A2', { n: 2 });
  await p.pv.del('stroke', '01K4B0000000000000000000A1');
  const entries = stored(p.store);
  assert.equal(entries.length, 2);
  assert.equal(entries[0].lam, 41);
  assert.equal(entries[0].app, 'sketch');
  assert.equal(entries[0].node, 'k7m2q9xf', 'the node it was queued against');
  assert.match(entries[0].id, /^[0-9A-HJKMNP-TV-Z]{26}$/);
  p.respond(upNode('sketch', { value: 41 }));
  await p.pv.flush();
  const sent = posts(p.requests).slice(1);
  assert.equal(sent.length, 2);
  assert.equal(sent[0].body.events[0].id, '01K4B0000000000000000000A2');
  assert.equal(sent[1].body.events[0].op, 'del');
});

test('spec/data-api.md §6: a 5xx or 429 keeps the entry; a 4xx drops it and reports it', async () => {
  const p = await page({ respond: downNode(), online: false });
  const rejected = [];
  p.pv.on('rejected', e => rejected.push(e));
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  await p.pv.put('stroke', '01K4B0000000000000000000A2', { n: 2 });
  const up = upNode();
  p.respond((m, path, body) => (m === 'POST' && path.endsWith('/api/events') ? { status: 503, json: { error: '503' } } : up(m, path, body)));
  p.fire('online');
  await p.pv.flush();
  assert.equal(stored(p.store).length, 2, 'kept for next time');
  assert.equal(rejected.length, 0);
  p.respond((m, path, body) => (m === 'POST' && path.endsWith('/api/events') ? { status: 429, json: { error: '429' } } : up(m, path, body)));
  await p.pv.flush();
  assert.equal(stored(p.store).length, 2, 'rate limited: kept');
  p.respond((m, path, body) => (m === 'POST' && path.endsWith('/api/events') ? { status: 409, json: { error: '409 Conflict: reused id', index: 0 } } : up(m, path, body)));
  await p.pv.flush();
  assert.equal(stored(p.store).length, 0, 'refused: dropped');
  assert.equal(rejected.length, 2);
  assert.equal(rejected[0].error.status, 409);
  assert.equal(rejected[0].events[0].id, '01K4B0000000000000000000A1');
});

test('spec/data-api.md §6: an append during a replay is queued behind it, not lost', async () => {
  const p = await page({ respond: downNode(), online: false });
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const up = upNode();
  p.respond(async (m, path, body) => { if (m === 'POST') await gate; return up(m, path, body); });
  p.fire('online');
  const replay = p.pv.flush();
  await p.settle();
  const second = await p.pv.put('stroke', '01K4B0000000000000000000A2', { n: 2 });
  assert.equal(second.queued, true, 'queued behind the replay in progress');
  release();
  await replay;
  await p.pv.flush();
  assert.deepEqual(posts(p.requests).map(r => r.body.events[0].id), ['01K4B0000000000000000000A1', '01K4B0000000000000000000A2']);
  assert.equal(stored(p.store).length, 0);
});

test('spec/data-api.md §6: with no storage the queue lives for the page', async () => {
  const p = await page({ respond: downNode(), online: false, store: storage(true) });
  const out = await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  assert.equal(out.queued, true);
  p.respond(upNode());
  p.fire('online');
  await p.pv.flush();
  assert.equal(posts(p.requests).length, 1);
});

test('spec/protocol.md §10.6: a replay carries the mark it was queued at and is judged by the node — an entry already in the log is dropped, never sent again', async () => {
  const p = await page({ respond: upNode('sketch', { value: 7 }) });
  await p.settle();
  await p.pv.put('stroke', '01K4B0000000000000000000A0', { n: 0 });   // the mark is 8 now
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { points: [[1, 2]], color: '#00274C' });
  await p.pv.del('stroke', '01K4B0000000000000000000A0');
  const up = upNode('sketch', { value: 9 });
  p.respond((m, path, body) => {
    // Both landed before their responses were lost: the node says so and appends nothing.
    if (m === 'POST' && path.endsWith('/api/events')) return { json: { appended: 0, lam: 10, ids: body.events.map(e => e.id) } };
    return up(m, path, body);
  });
  p.fire('online');
  await p.pv.flush();
  const replayed = posts(p.requests).slice(1);
  assert.equal(replayed.length, 2, 'each entry is sent once, with its mark, for the node to judge');
  assert.deepEqual(replayed.map(r => r.body.since), [8, 8]);
  assert.equal(replayed[1].body.events[0].base.lam, 8, 'the row the page wrote carries the rank it wrote');
  assert.equal(p.requests.filter(r => r.method === 'GET' && r.path.includes('/api/events')).length, 0, 'the helper reads nothing itself');
  assert.equal(p.pv.lam, 10);
  assert.equal(stored(p.store).length, 0);
  // After a landed replay the helper knows no rank for the row; the next edit carries the mark alone.
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 3 });
  assert.equal(stored(p.store)[0].events[0].base, undefined);
});

test('spec/protocol.md §10.6: a row that moved since the entry was queued is a conflict the node refuses — reported, never written over', async () => {
  const p = await page({ respond: downNode(), online: false });
  const rejected = [];
  p.pv.on('rejected', e => rejected.push(e));
  // Queued offline: an edit of A, and a brand-new row B.
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 2 });
  await p.pv.put('stroke', '01K4B0000000000000000000B1', { n: 9 });
  const up = upNode();
  p.respond((m, path, body) => {
    // Another device edited A after the mark; nobody has touched B.
    if (m === 'POST' && path.endsWith('/api/events') && body.events[0].id === '01K4B0000000000000000000A1') {
      return { status: 409, json: { error: '409 Conflict: events[0]: stroke/01K4B0000000000000000000A1 changed after this write was queued', index: 0, conflict: { tbl: 'stroke', id: '01K4B0000000000000000000A1' } } };
    }
    return up(m, path, body);
  });
  p.fire('online');
  await p.pv.flush();
  assert.equal(rejected.length, 1);
  assert.equal(rejected[0].error.status, 409);
  assert.deepEqual(rejected[0].error.conflict, { tbl: 'stroke', id: '01K4B0000000000000000000A1' });
  assert.match(rejected[0].error.message, /stroke\/01K4B0000000000000000000A1 changed after/);
  const sent = posts(p.requests);
  assert.deepEqual(sent.map(r => r.body.events[0].id), ['01K4B0000000000000000000A1', '01K4B0000000000000000000B1'], 'each entry was put to the node once');
  assert.equal(sent[0].body.since, 0, 'queued before the page saw anything');
  assert.equal(stored(p.store).length, 0);
});

test('spec/data-api.md §5: a queued edit of a row the page read carries the rank it saw, and one of a row it never saw carries none', async () => {
  const p = await page({ respond: upNode() });
  await p.settle();
  const up = upNode();
  p.respond((m, path, body) => {
    if (m === 'GET' && path.endsWith('/api/row/save/01K4B0000000000000000000A1')) return { text: JSON.stringify({ seq: 7, lam: 7, ts: '2026-09-05T11:00:00.000Z', dev: 'k7m2q9xf', op: 'put', tbl: 'save', id: '01K4B0000000000000000000A1', d: { level: 1 } }) };
    return up(m, path, body);
  });
  await p.pv.get('save', '01K4B0000000000000000000A1');
  p.respond(downNode());
  await p.pv.put('save', '01K4B0000000000000000000A1', { level: 2 });
  await p.pv.put('save', '01K4B0000000000000000000A2', { level: 9 });
  const entries = stored(p.store);
  assert.deepEqual(entries[0].events[0].base, { lam: 7, ts: '2026-09-05T11:00:00.000Z', dev: 'k7m2q9xf' });
  assert.equal(entries[1].events[0].base, undefined);
  // The replay carries them to the node as they are.
  p.respond(upNode());
  p.fire('online');
  await p.pv.flush();
  const sent = posts(p.requests);
  assert.deepEqual(sent[0].body.events[0].base, { lam: 7, ts: '2026-09-05T11:00:00.000Z', dev: 'k7m2q9xf' });
  assert.equal(sent[1].body.events[0].base, undefined);
});

test("spec/data-api.md §5: a page's own write, and a row it read through pv.events, are rows it saw", async () => {
  const p = await page({ respond: upNode('sketch', { value: 40 }) });
  await p.settle();
  const out = await p.pv.append([
    { op: 'put', tbl: 'stroke', id: '01K4B0000000000000000000A1', d: { n: 1 } },
    { op: 'put', tbl: 'stroke', id: '01K4B0000000000000000000A2', d: { n: 2 } },
  ]);
  assert.equal(out.lam, 42);
  const up = upNode('sketch', { value: 42 });
  p.respond((m, path, body) => {
    if (m === 'GET' && path.includes('/api/events')) return { text: JSON.stringify({ seq: 5, lam: 5, ts: '2026-09-05T10:00:00.000Z', dev: 'other000', op: 'put', tbl: 'stroke', id: '01K4B0000000000000000000B1', d: { n: 7 } }) + '\n' };
    return up(m, path, body);
  });
  for await (const ev of p.pv.events({ tbl: 'stroke' })) assert.equal(ev.tbl, 'stroke');
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 10 });
  await p.pv.put('stroke', '01K4B0000000000000000000A2', { n: 20 });
  await p.pv.put('stroke', '01K4B0000000000000000000B1', { n: 70 });
  const entries = stored(p.store);
  // The batch's ranks: contiguous lam from the response's, one ts, this node.
  assert.deepEqual(entries[0].events[0].base, { lam: 41, ts: '2026-09-05T12:00:00.000Z', dev: 'k7m2q9xf' });
  assert.deepEqual(entries[1].events[0].base, { lam: 42, ts: '2026-09-05T12:00:00.000Z', dev: 'k7m2q9xf' });
  assert.deepEqual(entries[2].events[0].base, { lam: 5, ts: '2026-09-05T10:00:00.000Z', dev: 'other000' });
});

test('spec/data-api.md §6: in solo mode an entry queued before the app was known is refused; in host mode the mount names it', async () => {
  // Solo mount, first load with the node down and nothing cached: the app is unknown.
  const solo = await page({ pathname: '/', respond: downNode(), online: false });
  const rejected = [];
  solo.pv.on('rejected', e => rejected.push(e));
  await solo.pv.put('save', '01K4B0000000000000000000A1', { level: 7 });
  assert.equal(stored(solo.store, '/')[0].app, null);
  solo.respond(upNode('mygame'));
  solo.fire('online');
  await solo.pv.flush();
  assert.equal(posts(solo.requests).length, 0, 'never replayed into whichever app owns / now');
  assert.equal(rejected.length, 1);
  assert.match(rejected[0].error.message, /before the app at this mount was known/);

  // A host-mode mount carries the app in its path, node or no node.
  const host = await page({ pathname: '/a/sketch/', respond: downNode(), online: false });
  await host.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  assert.equal(stored(host.store)[0].app, 'sketch');
  host.respond(upNode('sketch'));
  host.fire('online');
  await host.pv.flush();
  assert.equal(posts(host.requests).length, 1);
});

test('spec/data-api.md §6: an entry queued for another app at this mount is refused, never replayed; a one-list queue from an earlier helper is carried over', async () => {
  const store = storage();
  store.setItem('pv:app:/', JSON.stringify('other'));
  store.setItem('pv:outbox:/', JSON.stringify([{ id: '01K4B0000000000000000000B1', lam: 3, app: 'other', events: [{ op: 'put', tbl: 'save', id: '01K4B0000000000000000000A1', d: { level: 7 } }] }]));
  const p = await page({ pathname: '/', store, respond: upNode('mygame') });
  const rejected = [];
  p.pv.on('rejected', e => rejected.push(e));
  assert.equal(store.map.has('pv:outbox:/'), false, 'the one-list key is carried over and removed at load');
  await p.settle();
  await p.pv.flush();
  assert.equal(posts(p.requests).length, 0);
  assert.equal(rejected.length, 1);
  assert.match(rejected[0].error.message, /queued for app other/);
  assert.equal(JSON.parse(store.map.get('pv:app:/')), 'mygame', 'the app served now is remembered');
  assert.equal(stored(store, '/').length, 0);
});

test('spec/data-api.md §5: a put with no id is minted one before it is sent or queued', async () => {
  const p = await page({ respond: downNode(), online: false });
  const out = await p.pv.append([{ op: 'put', tbl: 'stroke', d: { n: 1 } }]);
  assert.equal(out.queued, true);
  assert.match(out.ids[0], /^[0-9A-HJKMNP-TV-Z]{26}$/);
  assert.equal(stored(p.store)[0].events[0].id, out.ids[0]);
});

test('spec/data-api.md §5: a row read with pv.get moves the mark, so an edit of that row is not a false conflict', async () => {
  const p = await page({ respond: upNode() });
  await p.settle();
  const up = upNode();
  p.respond((m, path, body) => {
    if (m === 'GET' && path.endsWith('/api/row/save/01K4B0000000000000000000A1')) return { text: JSON.stringify({ seq: 7, lam: 7, op: 'put', tbl: 'save', id: '01K4B0000000000000000000A1', d: { level: 1 } }) };
    return up(m, path, body);
  });
  const row = await p.pv.get('save', '01K4B0000000000000000000A1');
  assert.equal(row.d.level, 1);
  assert.equal(p.pv.lam, 7, 'the mark follows the row the page saw');
  const missing = await p.pv.get('save', '01K4B0000000000000000000A2');
  assert.equal(missing, null);
});

test('spec/data-api.md §6: an entry is bound to the node it was queued against; a replay asks who serves the origin now and refuses another node', async () => {
  // A page that learned node k7m2q9xf, queued while it was unreachable, and finds a
  // different data root answering on the same port afterwards.
  const p = await page({ respond: upNode() });
  await p.settle();
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  assert.equal(stored(p.store)[0].node, 'k7m2q9xf');
  const rejected = [];
  p.pv.on('rejected', e => rejected.push(e));
  p.respond(nodeCalled('newnode0'));
  p.fire('online');
  await p.pv.flush();
  assert.equal(posts(p.requests).length, 0, 'never replayed into another node');
  assert.equal(rejected.length, 1);
  assert.match(rejected[0].error.message, /queued for node k7m2q9xf; this origin now serves newnode0/);
  assert.equal(JSON.parse(p.store.map.get('pv:node:/a/sketch/')), 'newnode0', 'the node served now is remembered');
  assert.equal(stored(p.store).length, 0);

  // An entry that matches replays, and the POST names the node and the app it is for, so
  // the node can refuse a mismatch itself.
  p.respond(downNode());
  await p.pv.put('stroke', '01K4B0000000000000000000A2', { n: 2 });
  assert.equal(stored(p.store)[0].node, 'newnode0');
  p.respond(nodeCalled('newnode0'));
  p.fire('online');
  await p.pv.flush();
  const sent = posts(p.requests);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].body.node, 'newnode0');
  assert.equal(sent[0].body.app, 'sketch');
});

test('spec/data-api.md §6: two pages over one storage keep both queues — one key per entry — and either replays both', async () => {
  const store = storage();
  const a = await page({ store, respond: downNode(), online: false });
  await a.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
  const b = await page({ store, respond: downNode(), online: false });
  await b.pv.put('stroke', '01K4B0000000000000000000B1', { n: 2 });
  assert.equal(stored(store).length, 2, `both pages' entries are stored: ${[...store.map.keys()]}`);
  // Page a closed while offline; page b comes back and replays both, oldest first.
  b.respond(upNode());
  b.fire('online');
  await b.pv.flush();
  assert.deepEqual(posts(b.requests).map(r => r.body.events[0].id), ['01K4B0000000000000000000A1', '01K4B0000000000000000000B1']);
  assert.equal(stored(store).length, 0, 'the queue is empty again');
});

test('spec/data-api.md §6: two pages queueing in the same millisecond still replay oldest first — the second page mints past the entries it adopted', async () => {
  // A fast machine puts both entries inside one millisecond. Each page has its own
  // counter, so without the adopted entries as a floor the order would be the random
  // tails' — a coin toss. The clock is frozen to make the case certain.
  const store = storage();
  const realNow = Date.now;
  const frozen = realNow();
  Date.now = () => frozen;
  let a, b;
  try {
    a = await page({ store, respond: downNode(), online: false });
    await a.pv.put('stroke', '01K4B0000000000000000000A1', { n: 1 });
    b = await page({ store, respond: downNode(), online: false });
    await b.pv.put('stroke', '01K4B0000000000000000000B1', { n: 2 });
  } finally {
    Date.now = realNow;
  }
  const [first, second] = stored(store);
  assert.equal(first.id.slice(0, 10), second.id.slice(0, 10), 'both minted in the frozen millisecond');
  assert.ok(first.id < second.id, `${first.id} < ${second.id}`);
  b.respond(upNode());
  b.fire('online');
  await b.pv.flush();
  assert.deepEqual(posts(b.requests).map(r => r.body.events[0].id), ['01K4B0000000000000000000A1', '01K4B0000000000000000000B1']);
});
