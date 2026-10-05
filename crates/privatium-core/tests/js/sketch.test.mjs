// Project: Privatium™ | File: crates/privatium-core/tests/js/sketch.test.mjs
// Authors: Gabriel Mongefranco (@gabrielmongefranco)
// Created: 2026-09-06 | Modified: 2026-10-04
// Summary: Stroke erasing respects segment edges and rejects malformed geometry; undo is
//          shared across windows and devices, batched, bounded, and never reuses an ID; the
//          way out of the app leads to the launcher under one and to Settings in solo mode.
//          See main README.md for full license information.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { hitsStroke } from '../../../../apps/sketch/web/strokes.js';
test('eraser hits segment interiors, endpoints and width boundary without mutation', () => {
  const stroke = { width: 4, points: [[0, 0], [100, 0]] };
  const before = JSON.stringify(stroke);
  assert.equal(hitsStroke(stroke, 50, 8), true);
  assert.equal(hitsStroke(stroke, 50, 8.01), false);
  assert.equal(hitsStroke(stroke, -8, 0), true);
  assert.equal(hitsStroke(stroke, 109, 0), false);
  assert.equal(JSON.stringify(stroke), before);
});
test('single points and malformed strokes are handled safely', () => {
  assert.equal(hitsStroke({width: 4, points: [[2, 2]]}, 2, 2), true);
  for (const stroke of [null, {}, {width: 4, points: []}, {width: -1, points: [[0,0]]}, {width:4, points:[[NaN,0]]}]) {
    assert.equal(hitsStroke(stroke, 0, 0), false);
  }
  assert.equal(hitsStroke({width:4, points:[[0,0]]}, Infinity, 0), false);
});

import { SketchHistory } from '../../../../apps/sketch/web/history.js';
let nextId = 0;
const makeHistory = write => new SketchHistory(write, () => `restored-${nextId++}`);
const stroke = (id, color = '#00274C') => ({op:'put', tbl:'stroke', id, d:{points:[[1,1],[2,2]], color, width:3}});
test('undo restores painting order and reverses new sketch, including replay', async () => {
 const writes=[]; const m=makeHistory(async e=>{writes.push(e);});
 await m.change([stroke('a'),stroke('b','#FFFFFF')]);
 await m.change([{op:'del',tbl:'stroke',id:'a'}]); await m.undo();
 assert.deepEqual(m.entries().map(([,s])=>s.color),['#00274C','#FFFFFF']);
 await m.change(m.entries().map(([id])=>({op:'del',tbl:'stroke',id})));
 assert.equal(m.strokes.size,0); await m.undo();
 const replay=makeHistory(async()=>{}); writes.flat().forEach(e=>replay.apply(e));
 assert.deepEqual(replay.entries(),m.entries());
});
test('failed writes and remote edits preserve undo and visible strokes', async()=>{
 let fail=false; const m=makeHistory(async()=>{if(fail)throw new Error('offline');});
 await m.change([stroke('a')]); fail=true;
 await assert.rejects(m.change([stroke('b')])); await assert.rejects(m.undo());
 assert.equal(m.strokes.has('b'),false); assert.equal(m.undoStack.length,1);
 fail=false;
 // A change to the same stroke from another window is the latest change, so undo
 // reverses it — back to what this tab drew — rather than refusing.
 m.apply({...stroke('a','#FFFFFF'), ts:'2026-09-07T10:00:00.000Z', dev:'k7m2q9xf'});
 assert.equal(m.undoStack.length,2);
 await m.undo();
 assert.equal(m.strokes.get('a').color,'#00274C');
 // A later change this history did not see — a stroke edited underneath an entry —
 // still makes that entry unavailable rather than overwritten.
 m.strokes.set('a', {...m.strokes.get('a'), color:'#333333'});
 await assert.rejects(m.undo(),/another window/);
 assert.equal(m.strokes.get('a').color,'#333333');
});
test('history is bounded and simultaneous actions are refused',async()=>{
 const m=makeHistory(async()=>{});
 for(let i=0;i<51;i++)await m.change([stroke(String(i))]);
 assert.equal(m.undoStack.length,50);
 let done; m.write=()=>new Promise(resolve=>{done=resolve;});
 const pending=m.change([stroke('pending')]);
 await assert.rejects(m.undo(),/Wait/); await assert.rejects(m.change([stroke('other')]),/Wait/);
 done(); await pending;
});

test('undo never reuses tombstoned IDs and earlier undo follows restored strokes',async()=>{
 const deleted=new Set();
 const m=makeHistory(async events=>{
  for(const ev of events) {
   if(ev.op==='del')deleted.add(ev.id);
   else assert.equal(deleted.has(ev.id),false,'a deleted ID cannot be reused');
  }
 });
 await m.change([stroke('original')]);
 await m.change([{op:'del',tbl:'stroke',id:'original'}]);
 await m.undo();
 assert.equal(m.strokes.has('original'),false);
 assert.equal(m.strokes.size,1);
 await m.undo();
 assert.equal(m.strokes.size,0);
});

test('changes from other windows and devices join the shared undo, batches as one', async () => {
 const writes = []; const m = makeHistory(async e => { writes.push(e); });
 // Two strokes from another device, one batch: undone together.
 m.apply({...stroke('r1'), ts: '2026-09-07T10:00:00.000Z', dev: 'k7m2q9xf'});
 m.apply({...stroke('r2', '#FFFFFF'), ts: '2026-09-07T10:00:00.000Z', dev: 'k7m2q9xf'});
 assert.equal(m.undoStack.length, 1);
 assert.equal(m.strokes.size, 2);
 await m.undo();
 assert.equal(m.strokes.size, 0);
 assert.deepEqual(writes.at(-1).map(e => [e.op, e.id]), [['del', 'r2'], ['del', 'r1']]);
 // A remote erasure: undo restores the stroke under a fresh ID and its original layer.
 m.apply({...stroke('r3'), ts: '2026-09-07T10:00:01.000Z', dev: 'k7m2q9xf'});
 m.apply({op: 'del', tbl: 'stroke', id: 'r3', ts: '2026-09-07T10:00:02.000Z', dev: 'b3nn8t2q'});
 assert.equal(m.strokes.size, 0);
 await m.undo();
 assert.equal(m.strokes.size, 1);
 const [[id, restored]] = m.entries();
 assert.notEqual(id, 'r3');
 assert.equal(restored.layer, 'r3');
});

test('a tab\'s own write echoed by the stream and a replay of what is drawn are not remembered twice', async () => {
 const m = makeHistory(async () => {});
 await m.change([stroke('a')]);
 assert.equal(m.undoStack.length, 1);
 // The stream echoes the write; nothing changes, nothing is remembered.
 m.apply({...stroke('a'), ts: '2026-09-07T10:00:00.000Z', dev: 'as3nn9tm'});
 assert.equal(m.undoStack.length, 1);
 // A tombstone for a stroke that is not there changes nothing either.
 m.apply({op: 'del', tbl: 'stroke', id: 'never', ts: '2026-09-07T10:00:03.000Z', dev: 'as3nn9tm'});
 assert.equal(m.undoStack.length, 1);
 // A replay at load fills the history in log order, oldest first.
 const replay = makeHistory(async () => {});
 for (let i = 0; i < 3; i++) {
  replay.apply({...stroke(`s${i}`), ts: `2026-09-07T10:00:0${i}.000Z`, dev: 'as3nn9tm'});
 }
 assert.equal(replay.undoStack.length, 3);
 await replay.undo();
 assert.deepEqual([...replay.strokes.keys()], ['s0', 's1']);
});

import { exitTarget, pointOut } from '../../../../apps/sketch/web/exit.js';
test('test_spec_5_the_mark_links_to_apps_in_host_mode_and_settings_in_solo_mode', () => {
  // `pv.url` as pv.js resolves it: a path beneath the mount, `..` segments and all.
  const url = mount => path => new URL(path, `http://node${mount}`).pathname;
  assert.deepEqual(exitTarget('/a/sketch/', url('/a/sketch/')), { href: '/', label: 'Apps' });
  assert.deepEqual(exitTarget('/', url('/')), { href: '/settings', label: 'Settings' });
  // Both exits — the mark in the rail and the menu's entry — are filled from one target.
  const link = (span) => {
    const attrs = {};
    return {
      hidden: true, href: '', title: '',
      setAttribute: (k, v) => { attrs[k] = v; },
      getAttribute: k => attrs[k],
      querySelector: () => span,
    };
  };
  const text = { textContent: 'Apps' };
  const menu = link(text);
  pointOut(menu, exitTarget('/', url('/')));
  assert.equal(menu.href, '/settings');
  assert.equal(menu.getAttribute('aria-label'), 'Settings');
  assert.equal(menu.title, 'Settings');
  assert.equal(text.textContent, 'Settings');
  assert.equal(menu.hidden, false);
  const mark = link(null);
  pointOut(mark, exitTarget('/a/sketch/', url('/a/sketch/')));
  assert.equal(mark.href, '/');
  assert.equal(mark.getAttribute('aria-label'), 'Apps');
  assert.equal(mark.hidden, false);
});
