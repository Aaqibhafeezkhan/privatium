// Project:  Privatium™  |  File: crates/privatium-core/tests/js/chrome.test.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-10-04  |  Modified: 2026-10-04
// Summary:  chrome.js against spec/app-contract.md §5.2 and spec/lua-api.md §4.1, under
//           `node --test`: the footer's status slot is written once per connection change
//           and never on load, the outbox count is reported as it changes, pv.status()
//           and the pv:status event write task wording, and a page without the slot is
//           left alone. See main README.md for full license information.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { page, downNode } from './harness.mjs';

/** A document with the frame's status slot, counting every write to it. */
function framed() {
  const doc = new EventTarget();
  const writes = [];
  const slot = { get textContent() { return writes.at(-1) ?? ''; }, set textContent(text) { writes.push(text); } };
  doc.getElementById = id => (id === 'pv-status' ? slot : null);
  const win = new EventTarget();
  win.navigator = { onLine: true };
  return { doc, win, writes, slot };
}

test('test_spec_5_2_pv_status_writes_the_slot_and_connection_changes_announce_once', async () => {
  const { install, OFFLINE, CONNECTED, SENT, waitingWords, say } = await import('../../assets/shell/chrome.js');
  const { doc, win, writes } = framed();
  install(doc, win);
  assert.deepEqual(writes, [], 'nothing is said on load');

  // The browser and pv.js both report the same loss; it is said once.
  win.dispatchEvent(new Event('offline'));
  doc.dispatchEvent(new CustomEvent('pv:offline'));
  assert.deepEqual(writes, [OFFLINE]);
  // Changes queued while offline do not replace the offline text.
  doc.dispatchEvent(new CustomEvent('pv:outbox', { detail: { waiting: 2 } }));
  assert.deepEqual(writes, [OFFLINE]);
  // Back, with two waiting: the count, then the drain, then nothing more for a repeat.
  doc.dispatchEvent(new CustomEvent('pv:online'));
  win.dispatchEvent(new Event('online'));
  assert.deepEqual(writes, [OFFLINE, waitingWords(2)]);
  doc.dispatchEvent(new CustomEvent('pv:outbox', { detail: { waiting: 2 } }));
  doc.dispatchEvent(new CustomEvent('pv:outbox', { detail: { waiting: 1 } }));
  doc.dispatchEvent(new CustomEvent('pv:outbox', { detail: { waiting: 0 } }));
  assert.deepEqual(writes, [OFFLINE, waitingWords(2), '1 change waiting to send.', SENT]);
  // A loss with nothing queued comes back as plain reconnection.
  doc.dispatchEvent(new CustomEvent('pv:offline'));
  doc.dispatchEvent(new CustomEvent('pv:online'));
  assert.deepEqual(writes.slice(-2), [OFFLINE, CONNECTED]);
  // Task wording from the app, by event and by the helper.
  doc.dispatchEvent(new CustomEvent('pv:status', { detail: { text: 'Saved.' } }));
  assert.equal(writes.at(-1), 'Saved.');
  assert.equal(say('Three items added.', doc), true);
  assert.equal(writes.at(-1), 'Three items added.');
  for (const text of writes) assert.doesNotMatch(text, /fetch|xhr|socket|\d{3}\b/, 'no technical detail');

  // pv.js writes the same slot through pv.status(), and its outbox reaches the slot
  // through the document without chrome.js importing it.
  globalThis.document = doc;
  try {
    const p = await page({ respond: downNode(), online: false });
    p.pv.status('Counting…');
    assert.equal(writes.at(-1), 'Counting…');
    await p.pv.put('stroke', '01K4B0000000000000000000A1', { points: [] });
    assert.equal(p.pv.waiting, 1);
    assert.equal(writes.at(-1), '1 change waiting to send.');
  } finally { delete globalThis.document; }

  // A page that declined the chrome has no slot and nothing to say.
  const bare = new EventTarget(); bare.getElementById = () => null;
  assert.equal(say('anything', bare), false);
  const quiet = install(bare, new EventTarget());
  quiet.offline(); quiet.online();
  assert.equal(quiet.state.offline, false);
});

test('spec/lua-api.md §4.1: a page that loads while the browser is offline shows the state at once', async () => {
  const { install, OFFLINE } = await import('../../assets/shell/chrome.js?offline');
  const { doc, win, writes } = framed();
  win.navigator.onLine = false;
  install(doc, win);
  assert.deepEqual(writes, [OFFLINE]);
  win.dispatchEvent(new Event('offline'));
  assert.deepEqual(writes, [OFFLINE], 'the same state is not said twice');
});
