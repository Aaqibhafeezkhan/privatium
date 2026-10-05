// This file is part of Privatium
// apps/pantry/web/app.js
// Author(s): Gabriel Mongefranco
// Created: 2026-09-07
// Last Modified: 2026-10-04
// Summary: Boot, the queries, and what each control does. The screen is read two ways at once, and that
//          is the point of this app: the shelves, the batches and the tray come from named
//          views in schema.sql through pv.query, which say what is true now; the activity list
//          comes from the event log through pv.events and pv.subscribe, which says what
//          happened — including an undo, which a view can never show, because materialization
//          drops a tombstoned row entirely (spec/protocol.md §4.5). Every quantity stays a
//          string from the field to the column. Which shelf is open is this device's business
//          and lives in localStorage; it never reaches the log.
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

import { pv } from '/static/pv.js';
import {
  el, renderActivity, renderBatches, renderChecks, renderExpiring, renderShelves,
  renderSummary, renderTray,
} from './views.js';
import { amountProblem, clearErrors, readBatch, readShelf, resetBatch, showRefusal } from './forms.js';
import { addBatch, addShelf, moveBatch, putBack, takeOut, undoChange } from './writes.js';

const $ = id => document.getElementById(id);

/** Which shelf is open, per device. View state, never an event. */
const SHELF_KEY = 'pantry.shelf';
/** How far ahead "Expiring soon" looks, in days. Bound as text to the view's $days. */
const EXPIRING_DAYS = 30;
/** How far back the tray's Earlier toggle reaches, in days. */
const EARLIER_DAYS = 30;
/** How many activity entries are on screen at once. The log keeps every one of them. */
const ACTIVITY_SHOWN = 100;

const state = {
  shelves: [],
  open: null,
  earlier: false,
  /** Change id to `{ id, lam, d, undone }`, read from the log. */
  log: new Map(),
  /** Batch id to `{ name, unit }`, read from the log, so an activity row can name what
   * it changed and say it in the batch's own unit. */
  names: new Map(),
};

// ---------------------------------------------------------------------------------------
// Saying what happened
// ---------------------------------------------------------------------------------------

/**
 * Put one sentence in the footer's status line, the slot the framework's chrome renders
 * (spec/app-contract.md §5.2). The framework writes the connection wording there itself;
 * what goes through here is what happened to the pantry.
 * @param {string} message What to say, in the person's own terms.
 */
function say(message) {
  pv.status(message);
}

/**
 * Report a write the node refused after it was queued: the row moved while this page was
 * away, and a browser never writes over what it did not see (spec/data-api.md §6).
 * @param {object} entry The `rejected` event: `{ id, events, error }`.
 */
function sayRejection(entry) {
  const conflict = entry.error && entry.error.conflict;
  const batch = conflict && state.names.get(conflict.id);
  say(conflict
    ? `Not recorded: ${batch ? batch.name : 'that batch'} changed while you were offline, so your queued change was refused. Look at what is there now and record it again.`
    : `Not recorded: ${entry.error ? entry.error.message : 'the node refused the change'}.`);
}

/**
 * Say what a completed write did, and whether it is on disk yet.
 * @param {object} out What a write function resolved with.
 * @param {string} done The sentence for a write the node took.
 */
function sayWrite(out, done) {
  say(out && out.queued
    ? 'Queued. The node is not reachable; this will be written when it is.'
    : done);
}

// ---------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------

/** The `$since` the tray asks for: local midnight, or thirty days before it. */
function sinceStamp() {
  const when = new Date();
  when.setHours(0, 0, 0, 0);
  if (state.earlier) when.setDate(when.getDate() - EARLIER_DAYS);
  return when.toISOString();
}

/**
 * Run every view and draw what they say. Values already on screen are left alone when the
 * node cannot be reached, with the status line saying they are not current.
 */
async function refresh() {
  try {
    state.shelves = await pv.query('v_shelf');
    if (!state.shelves.some(shelf => shelf.id === state.open)) {
      state.open = state.shelves.length > 0 ? state.shelves[0].id : null;
    }
    const [batches, outs, checkBatch, checkOut, expiring, summary] = await Promise.all([
      state.open ? pv.query('v_batch', { shelf: state.open }) : [],
      pv.query('v_out', { since: sinceStamp() }),
      pv.query('v_check_batch'),
      pv.query('v_check_out'),
      pv.query('v_expiring', { days: EXPIRING_DAYS }),
      pv.query('v_stock_by_unit'),
    ]);
    for (const row of batches) state.names.set(row.id, { name: row.name, unit: row.unit });

    const bare = state.shelves.length === 0;
    renderShelves($('shelves'), state.shelves, state.open, openShelf);
    $('shelves-empty').toggleAttribute('hidden', !bare);
    $('add-shelf').open = $('add-shelf').open || bare;
    fillShelfChoice();

    // A batch has to go on a shelf, so until there is one the batch half is not shown at
    // all: an empty shelf list under an empty table is two dead ends instead of one start.
    $('batches').toggleAttribute('hidden', bare);
    const open = state.shelves.find(shelf => shelf.id === state.open);
    $('shelf-name').textContent = open ? open.name : 'this shelf';
    renderBatches($('batch-rows'), batches, state.shelves, { take: onTake, move: onMove });
    $('batches-empty').toggleAttribute('hidden', batches.length > 0);
    $('batch-table').toggleAttribute('hidden', batches.length === 0);
    if (batches.length === 0 && !bare) $('add-batch').open = true;

    const outCount = renderTray($('out-cards'), outs, onPutBack);
    $('tray-empty').toggleAttribute('hidden', outCount > 0);

    const checks = renderChecks($('check-list'), checkBatch, checkOut);
    $('checks-empty').toggleAttribute('hidden', checks > 0);

    renderExpiring($('expiring-rows'), expiring);
    renderSummary($('summary-rows'), summary);
  } catch (error) {
    if (error.name === 'PvOffline') {
      // The chrome has already said the node is offline; this is the app's part of it.
      say('What is on screen was read a moment ago and may not be current.');
      return;
    }
    say(`Could not read: ${error.message}`);
  }
}

/**
 * Read one table's whole log, a page at a time.
 * @param {string} tbl The table.
 * @param {Function} note Called with each envelope, oldest first.
 */
async function readTable(tbl, note) {
  const page = 500;
  let offset = 0;
  for (;;) {
    let count = 0;
    for await (const event of pv.events({ tbl, limit: page, offset })) {
      note(event);
      count += 1;
    }
    if (count < page) return;
    offset += count;
  }
}

/**
 * Fold one envelope into the activity list. A `del` marks the entry undone rather than
 * removing it: the log kept the change, and so does this list.
 * @param {object} event One event envelope.
 */
function noteChange(event) {
  const held = state.log.get(event.id);
  if (event.op === 'del') {
    if (held) held.undone = true;
    return;
  }
  state.log.set(event.id, { id: event.id, lam: event.lam, d: event.d, undone: false });
}

/** Read both logs this page shows: the changes, and the batch names that go with them. */
async function readLog() {
  try {
    await readTable('batch', event => {
      if (event.op === 'put') state.names.set(event.id, { name: event.d.name, unit: event.d.unit });
    });
    await readTable('quantity_change', noteChange);
    drawActivity();
  } catch (error) {
    if (error.name !== 'PvOffline') say(`Could not read the log: ${error.message}`);
  }
}

/** Draw the activity list from what the log said, newest first. */
function drawActivity() {
  const entries = Array.from(state.log.values())
    .sort((a, b) => (a.d.at === b.d.at ? b.lam - a.lam : (a.d.at < b.d.at ? 1 : -1)))
    .slice(0, ACTIVITY_SHOWN);
  renderActivity($('activity-rows'), entries, state.names, onUndo);
  $('activity-empty').toggleAttribute('hidden', entries.length > 0);
  $('activity-table').toggleAttribute('hidden', entries.length === 0);
}

// ---------------------------------------------------------------------------------------
// Live updates
// ---------------------------------------------------------------------------------------

let pending = null;

/** Re-read once for a burst of events rather than once per event. */
function scheduleRefresh() {
  if (pending) return;
  pending = setTimeout(() => {
    pending = null;
    drawActivity();
    void refresh();
  }, 80);
}

/**
 * One event from any device, including this page's own write coming back.
 * @param {object} event The envelope the stream carried.
 */
function onStream(event) {
  if (event.tbl === 'batch' && event.op === 'put') {
    state.names.set(event.id, { name: event.d.name, unit: event.d.unit });
  }
  if (event.tbl === 'quantity_change') noteChange(event);
  scheduleRefresh();
}

// ---------------------------------------------------------------------------------------
// What the controls do
// ---------------------------------------------------------------------------------------

/**
 * Open a shelf. This is view state and stays on this device.
 * @param {string} id The shelf's id.
 */
function openShelf(id) {
  state.open = id;
  try {
    localStorage.setItem(SHELF_KEY, id);
  } catch {
    // A browser with storage switched off still works; it just forgets the open shelf.
  }
  void refresh();
}

/** Keep the add-batch form's shelf list in step with the map, holding the choice. */
function fillShelfChoice() {
  const select = $('b-shelf');
  const chosen = select.value || state.open;
  select.replaceChildren();
  for (const shelf of state.shelves) {
    const option = el('option', { value: shelf.id, text: shelf.name });
    if (shelf.id === chosen) option.setAttribute('selected', 'selected');
    select.append(option);
  }
}

/**
 * Take some out of one batch.
 * @param {object} row The `v_batch` row.
 * @param {HTMLInputElement} input The amount field.
 * @param {HTMLElement} error The paragraph beneath it.
 */
async function onTake(row, input, error) {
  const amount = input.value.trim();
  const wrong = amountProblem(amount);
  if (wrong) {
    error.textContent = wrong;
    error.removeAttribute('hidden');
    input.setAttribute('aria-invalid', 'true');
    input.focus();
    return;
  }
  error.setAttribute('hidden', 'hidden');
  input.removeAttribute('aria-invalid');
  try {
    const out = await takeOut(row.id, amount);
    input.value = '';
    sayWrite(out, `Took out ${amount} ${row.unit} of ${row.name}.`);
  } catch (failure) {
    error.textContent = failure.message;
    error.removeAttribute('hidden');
  }
}

/**
 * Move a batch to another shelf: the same row, under the same id, on a new shelf.
 * @param {object} row The `v_batch` row.
 * @param {string} shelfId Where it goes.
 * @param {HTMLElement} error The paragraph beneath the row's controls.
 */
async function onMove(row, shelfId, error) {
  const shelf = state.shelves.find(one => one.id === shelfId);
  try {
    const out = await moveBatch(row.id, {
      name: row.name,
      icon: row.icon,
      unit: row.unit,
      shelf_id: shelfId,
      stored_on: row.stored_on,
      expires_on: row.expires_on,
    });
    sayWrite(out, `Moved ${row.name} to ${shelf ? shelf.name : 'another shelf'}.`);
  } catch (failure) {
    error.textContent = failure.message;
    error.removeAttribute('hidden');
  }
}

/**
 * Put some of a withdrawal back.
 * @param {object} row The `v_out` row.
 * @param {HTMLInputElement} input The amount field.
 * @param {HTMLElement} error The paragraph beneath it.
 */
async function onPutBack(row, input, error) {
  const amount = input.value.trim();
  const wrong = amountProblem(amount);
  if (wrong) {
    error.textContent = wrong;
    error.removeAttribute('hidden');
    input.setAttribute('aria-invalid', 'true');
    input.focus();
    return;
  }
  error.setAttribute('hidden', 'hidden');
  input.removeAttribute('aria-invalid');
  try {
    const out = await putBack(row, amount);
    input.value = '';
    sayWrite(out, `Put back ${amount} ${row.unit} of ${row.batch_name}.`);
  } catch (failure) {
    error.textContent = failure.message;
    error.removeAttribute('hidden');
  }
}

/**
 * Undo one recorded change.
 * @param {string} id The change's id.
 */
async function onUndo(id) {
  try {
    const out = await undoChange(id);
    sayWrite(out, 'Undone. The change is gone from the tables and still in the log.');
  } catch (failure) {
    say(`Could not undo: ${failure.message}`);
  }
}

/**
 * Add a shelf from the new-shelf form.
 * @param {Event} event The form's submit.
 */
async function onAddShelf(event) {
  event.preventDefault();
  const shelf = readShelf();
  if (!shelf) return;
  try {
    const out = await addShelf(shelf.name, (state.shelves.length + 1) * 10);
    $('s-name').value = '';
    sayWrite(out, `Added the shelf ${shelf.name}.`);
  } catch (failure) {
    say(showRefusal('shelf-form', failure));
  }
}

/**
 * Add a batch and the stock it came with, in one batch of the log.
 * @param {Event} event The form's submit.
 */
async function onAddBatch(event) {
  event.preventDefault();
  const entry = readBatch();
  if (!entry) return;
  try {
    const out = await addBatch(entry.batch, entry.amount);
    resetBatch();
    sayWrite(out, `Added ${entry.amount} ${entry.batch.unit} of ${entry.batch.name}.`);
  } catch (failure) {
    say(showRefusal('batch-form', failure));
  }
}

/** The tray's Earlier toggle: the same view, a wider `$since`. */
function onEarlier() {
  state.earlier = !state.earlier;
  const button = $('earlier');
  button.setAttribute('aria-pressed', state.earlier ? 'true' : 'false');
  button.lastElementChild.textContent = state.earlier ? 'Today only' : 'Show earlier';
  void refresh();
}

// ---------------------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------------------

async function boot() {
  try {
    state.open = localStorage.getItem(SHELF_KEY);
  } catch {
    state.open = null;
  }
  $('b-stored').value = new Date().toISOString().slice(0, 10);

  $('shelf-form').addEventListener('submit', onAddShelf);
  $('batch-form').addEventListener('submit', onAddBatch);
  $('earlier').addEventListener('click', onEarlier);
  for (const field of document.querySelectorAll('#batch-form input, #batch-form select')) {
    field.addEventListener('input', () => clearErrors('batch-form'));
  }

  pv.on('resync', () => {
    say('The node rebuilt its cache. Reading everything again.');
    state.log.clear();
    void readLog();
    void refresh();
  });
  // The chrome says when the node is offline and back; the app only re-reads on return.
  pv.on('online', () => void refresh());
  pv.on('rejected', sayRejection);

  await refresh();
  await readLog();
  pv.subscribe(onStream);
}

void boot();
