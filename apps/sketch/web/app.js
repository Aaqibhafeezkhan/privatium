// This file is part of Privatium
// apps/sketch/web/app.js
// Author(s): Gabriel Mongefranco
// Created: 2026-08-28
// Last Modified: 2026-10-04
// Summary: Drawing, controls and event replay. Plain ES modules — no build step, no framework, no SQL.
//          The event log is used directly as a document store, and a mark is one event:
//          freehand keeps the original { points, color, width }, and the shapes, text, fills
//          and the page colour add a `kind`, so every log written before this redesign still
//          replays. A stroke holds the pointer's capture from down to up, so ending it off the
//          sheet still saves it; the keyboard draws too, and a live summary says what the
//          sheet holds. Which controls are on show is CSS's job (data-tool on <body>) — this
//          file owns behaviour, not layout.
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
import { batches, SketchHistory } from './history.js';
import { Sheet, SHEET_W, SHEET_H } from './sheet.js';
import {
  areaFilled, areaOf, bounds, covers, encloses, extentOf, hits, inBox, isShape,
  moveEvents, moved, resizeEvents, scaleTo, scaled
} from './strokes.js';
import { paintArea, paintMark } from './paint.js';
import { fromSvg, isOurs, toSvg } from './clip.js';
import { exitTarget, pointOut } from './exit.js';
import {
  BASIC, DASHED, DASHES, DEFAULT_SLOTS, INKING, INKS, SIZES, TOOLS, WIDTHED,
  colorName, inkOn, isHex, needsEdge, panelColor
} from './tools.js';

const $ = id => document.getElementById(id);
const pad = $('pad');
const ctx = pad.getContext('2d', { willReadFrequently: true });
const viewport = $('viewport');
const status = $('status');
const summary = $('summary');
const sheet = new Sheet(pad, viewport);

const history = new SketchHistory(events => pv.append(events), () => pv.ulid());
const marks = history.strokes;

const state = {
  tool: 'brush',
  other: 'fill',            // the fourth quick slot: whatever else was last used
  previous: 'brush',        // what the eyedropper hands back to
  color: DEFAULT_SLOTS[0],
  size: 6,
  dash: 'solid',
  blend: false,
  slots: DEFAULT_SLOTS.slice(),
  slot: 0,
  selection: [],            // ids, not positions
  selMode: 'stroke',
  editing: null,            // the id of a text mark being rewritten
  draft: null,              // the colour dialog's pending value
  hover: '',                // the pixel under the eyedropper
  clip: null,
  clipStamp: null
};

let drawing = null;         // the mark in progress
let marquee = null;
let dragging = null;
let resizing = null;
let panning = null;
let cycle = null;           // where the last click was, for stepping through overlaps
let longFired = false;

function say(text) { status.textContent = text; }

/* ---- the log, read and written ----------------------------------------- */

const layerOf = (id, mark) => mark.layer || id;

/** Marks in painting order, as [id, mark]. */
const ordered = () => history.entries();

function anchoredTo(ids) {
  return ordered().filter(([, mark]) => mark.kind === 'fill' && mark.anchor && ids.includes(mark.anchor));
}

/** The sheet's own colour: the last page mark laid down, or white. */
function pageColor() {
  let colour = '#FFFFFF';
  for (const [, mark] of ordered()) if (mark.kind === 'page' && mark.color) colour = mark.color;
  return colour;
}

/** The area a fill covers, given the marks as they are on screen right now. */
function areaOnScreen(mark, live) {
  const host = mark.anchor ? marks.get(mark.anchor) : null;
  const carried = live && (live.ids.has(mark.anchor) || live.fills.has(mark));
  const shown = carried ? live.apply(mark) : mark;
  const shownHost = host && carried ? live.apply(host) : host;
  return areaFilled(shown, shownHost);
}

// Anything that can name more marks than the node accepts in one batch — clearing a full
// sheet, a large paste, a move over a big selection — is written in ceiling-sized chunks
// rather than failing at the door.
/** Write one action. Each batch is one undo step, so a chunked action says how many. */
async function commitGroups(groups, message) {
  const parts = batches(groups);
  if (!parts.length) return true;
  let written = 0;
  try {
    let result;
    for (const part of parts) {
      result = await history.change(part);
      written++;
    }
    refresh();
    const note = parts.length > 1
      ? ' Written in ' + parts.length + ' parts, so undo takes ' + parts.length + ' presses.'
      : '';
    say((result?.queued ? 'Offline — ' + message.toLowerCase() + ' queued.' : message) + note);
    return true;
  } catch (error) {
    refresh();
    say(written
      ? 'Saved ' + written + ' of ' + parts.length + ' parts, then stopped. ' + error.message
      : 'Could not save that. ' + error.message);
    return false;
  }
}

/** One event per group: the ordinary case, where nothing has to travel with anything. */
function commit(events, message) {
  return commitGroups(events.map(event => [event]), message);
}

/* ---- painting ---------------------------------------------------------- */

function render() {
  if (!ctx) return;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = pageColor();
  ctx.fillRect(0, 0, pad.width, pad.height);
  sheet.applyTo(ctx);

  // A selection being moved or scaled is drawn as the finished change will look, colour
  // included. Both go through one transform, so a fill can never be given its own.
  const live = liveTransform();

  for (const [id, mark] of ordered()) {
    if (mark.kind === 'page') continue;                    // it is the surface, not a mark on it
    if (mark.kind === 'fill') {
      if (live && mark.anchor && live.ids.has(mark.anchor)) live.fills.add(mark);
      paintArea(ctx, areaOnScreen(mark, live), mark.color);
      continue;
    }
    paintMark(ctx, live && live.ids.has(id) ? live.apply(mark) : mark, sheet.k);
  }

  if (drawing) paintMark(ctx, drawing, sheet.k);
  if (state.tool === 'select') { outlineSelection(); drawHandle(); }
  if (marquee) outlineBox(marquee.a, marquee.b, '#241A22', [10, 7]);
  drawPen();
}

// A pointer reports more often than the display refreshes, and one repaint per frame is
// all the screen can show.
let frame = null;
function scheduleRender() {
  if (frame !== null) return;
  frame = requestAnimationFrame(() => { frame = null; render(); });
}

/** What is happening to the selection right now, as a transform every mark goes through. */
function liveTransform() {
  const ids = new Set(state.selection);
  if (dragging && (dragging.dx || dragging.dy)) {
    return { ids, fills: new Set(), apply: mark => moved(mark, dragging.dx, dragging.dy) };
  }
  if (resizing && (resizing.sx !== 1 || resizing.sy !== 1)) {
    return {
      ids, fills: new Set(),
      apply: mark => scaled(mark, resizing.ax, resizing.ay, resizing.sx, resizing.sy)
    };
  }
  return null;
}

/** The selected marks, in painting order. */
const selected = () => state.selection.map(id => marks.get(id)).filter(Boolean);

/** The box a resize measures and drags by: the selection's own geometry, unpadded. */
function selectionBox(live) {
  const list = selected();
  return extentOf(live ? list.map(mark => live.apply(mark)) : list);
}

function outlineSelection() {
  const live = liveTransform();
  for (const id of state.selection) {
    const mark = marks.get(id);
    const box = mark && bounds(live && live.ids.has(id) ? live.apply(mark) : mark);
    if (box) outlineBox({ x: box[0], y: box[1] }, { x: box[2], y: box[3] }, '#2459CF', [9, 6]);
  }
}

/** Half the handle's side, in sheet pixels, so it stays a 24 CSS pixel target at any zoom. */
const handleReach = () => 12 / Math.max(0.05, sheet.scale);

/** Where the resize handle sits: the bottom-right of the selection's own box. */
function handleAt() {
  if (state.tool !== 'select' || !state.selection.length) return null;
  const box = selectionBox(liveTransform());
  return box ? { x: box[2], y: box[3] } : null;
}

function drawHandle() {
  const at = handleAt();
  if (!at) return;
  const r = handleReach();
  ctx.save();
  ctx.fillStyle = '#FFFFFF';
  ctx.strokeStyle = '#2459CF';
  ctx.lineWidth = Math.max(2, r / 5);
  ctx.beginPath();
  ctx.rect(at.x - r / 2, at.y - r / 2, r, r);
  ctx.fill();
  ctx.stroke();
  ctx.restore();
}

/** Whether a point is on the handle. Generous, because it is a target for a fingertip. */
function onHandle(point) {
  const at = handleAt();
  if (!at) return false;
  const r = handleReach();
  return Math.abs(point.x - at.x) <= r && Math.abs(point.y - at.y) <= r;
}

function outlineBox(a, b, colour, dash) {
  ctx.save();
  ctx.setLineDash(dash);
  ctx.lineWidth = 2;
  ctx.strokeStyle = colour;
  ctx.strokeRect(Math.min(a.x, b.x), Math.min(a.y, b.y), Math.abs(b.x - a.x), Math.abs(b.y - a.y));
  ctx.restore();
}

const pen = { x: SHEET_W / 2, y: SHEET_H / 2, down: false };

function drawPen() {
  if (document.activeElement !== pad) return;
  const r = Math.max(10, state.size);
  ctx.save();
  ctx.setLineDash([6, 5]);
  ctx.lineWidth = 2;
  ctx.strokeStyle = pen.down ? '#B42335' : '#241A22';
  ctx.beginPath();
  ctx.arc(pen.x, pen.y, r, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.beginPath();
  ctx.moveTo(pen.x - r - 14, pen.y); ctx.lineTo(pen.x - r - 2, pen.y);
  ctx.moveTo(pen.x + r + 2, pen.y); ctx.lineTo(pen.x + r + 14, pen.y);
  ctx.moveTo(pen.x, pen.y - r - 14); ctx.lineTo(pen.x, pen.y - r - 2);
  ctx.moveTo(pen.x, pen.y + r + 2); ctx.lineTo(pen.x, pen.y + r + 14);
  ctx.stroke();
  ctx.restore();
}

function fit() {
  sheet.resize();
  render();
}

/* ---- what the sheet holds, in words ------------------------------------ */

function summarize() {
  const colours = new Set();
  let count = 0;
  for (const [, mark] of marks) {
    if (mark.kind === 'page') continue;                        // the surface, not a mark
    if (mark.color === '#FFFFFF' && !mark.kind) continue;      // eraser strokes
    count++;
    colours.add(mark.color);
  }
  summary.textContent = count
    ? count + ' mark' + (count === 1 ? '' : 's') + ' in ' + colours.size +
      ' colour' + (colours.size === 1 ? '' : 's') + ' on a 1600 by 1200 sheet.'
    : 'Empty sheet, 1600 by 1200.';
}

/* ---- the toolbars ------------------------------------------------------ */

const toolButtons = () => document.querySelectorAll('[data-tool]');
const inking = id => INKING.includes(id);

function chooseTool(id, quiet) {
  if (id === 'pick' && state.tool !== 'pick' && !inking(state.tool)) {
    say('The eyedropper needs a tool that draws in colour. Pick Brush, Fill, Line, Rectangle, Ellipse or Text first.');
    return;
  }
  if (id === 'pick' && state.tool !== 'pick') state.previous = state.tool;
  if (!BASIC.includes(id)) state.other = id;
  state.tool = id;
  state.selection = [];
  state.editing = null;
  state.hover = '';
  drawing = null;
  marquee = null;
  cycle = null;
  closeMenus();
  if (!quiet) say((TOOLS.find(t => t.id === id) || {}).label + ' selected.');
  refresh();
}

function refresh() {
  const tool = state.tool;
  document.body.dataset.tool = tool;
  document.body.dataset.sel = state.selMode;
  document.body.dataset.selection = state.selection.length ? 'yes' : 'no';
  document.body.dataset.editing = state.editing ? 'yes' : 'no';

  const other = TOOLS.find(t => t.id === state.other) || TOOLS.find(t => t.id === 'fill');
  const quickOther = $('quick-other');
  quickOther.dataset.tool = other.id;
  quickOther.querySelector('use').setAttribute('href', '#i-' + other.id);
  quickOther.querySelector('.sr').textContent = other.label;
  const otherCaret = quickOther.parentElement.querySelector('.more');
  otherCaret.dataset.more = other.id;
  otherCaret.setAttribute('aria-label', other.label + ' options');
  if (openFor && !hasOptions(openFor)) closeQuick(false);
  for (const caret of document.querySelectorAll('.more')) caret.hidden = !hasOptions(caret.dataset.more);

  for (const button of toolButtons()) {
    const id = button.dataset.tool;
    button.setAttribute('aria-pressed', String(id === tool));
    const off = id === 'pick' && tool !== 'pick' && !inking(tool);
    button.disabled = off;
    button.title = off ? 'Eyedropper needs a tool that draws in colour' : (TOOLS.find(t => t.id === id) || {}).label;
  }

  $('options-name').textContent = (TOOLS.find(t => t.id === tool) || {}).label;
  $('tool-note').textContent = {
    fill: 'Tap a shape to colour inside it, or the sheet to colour the sheet.',
    pick: 'Tap the sheet to pick up a colour, then it hands back.',
    pan: 'Drag the sheet, or use the arrow keys, to move around.'
  }[tool] || '';

  for (const button of document.querySelectorAll('.size'))
    button.setAttribute('aria-pressed', String(Number(button.dataset.size) === state.size));
  for (const button of document.querySelectorAll('.dash'))
    button.setAttribute('aria-pressed', String(button.dataset.dash === state.dash));
  for (const button of document.querySelectorAll('.selmode'))
    button.setAttribute('aria-pressed', String(button.dataset.sel === state.selMode));

  $('blend').setAttribute('aria-pressed', String(state.blend));

  const chosen = state.selection.length;
  $('bar-note').textContent = tool === 'select'
    ? (chosen ? chosen + ' selected — drag to move, corner to resize'
      : (state.selMode === 'rect' ? 'Drag a box to select' : 'Tap a mark to select, again to cycle'))
    : tool === 'eraser' ? 'The eraser paints white ink. Pick a width on the left.'
    : tool === 'pan' ? (sheet.pannable ? 'Drag the sheet to move around it.' : 'Zoom past the window to have something to pan.')
    : '';

  paintSlots();
  paintPickChip();

  for (const id of ['bar-copy', 'bar-cut', 'rail-copy', 'rail-cut', 'rail-delete',
    'rail-bigger', 'rail-smaller']) $(id).disabled = !chosen;
  $('bar-paste').disabled = !state.clip;
  $('rail-paste').disabled = !state.clip;
  $('rail-delete').querySelector('span').textContent = chosen > 1 ? 'Delete ' + chosen + ' marks' : 'Delete';

  $('undo').disabled = !history.canUndo;
  $('redo').disabled = !history.canRedo;
  $('zoom-level').textContent = sheet.percent + '%';

  summarize();
  render();
}

/* ---- the colour row ---------------------------------------------------- */

function paintSlots() {
  const panel = panelColor();
  for (const button of document.querySelectorAll('.slot')) {
    const index = Number(button.dataset.slot);
    const hex = state.slots[index];
    const chip = button.querySelector('.chip');
    button.setAttribute('aria-pressed', String(index === state.slot));
    button.setAttribute('aria-label', 'Colour ' + (index + 1) + ', ' + colorName(hex) + (index === state.slot ? ', selected' : ''));
    button.title = button.getAttribute('aria-label');
    chip.style.setProperty('--chip-color', hex || 'transparent');
    chip.style.setProperty('--chip-edge', needsEdge(hex, panel) ? '2px ' + (hex ? 'solid' : 'dashed') + ' var(--muted)' : '1px solid var(--line)');
    chip.textContent = hex ? '' : '+';
  }
  const current = state.slots[state.slot];
  const edit = $('edit-color');
  const chip = edit.querySelector('.chip');
  chip.style.setProperty('--chip-color', current || 'transparent');
  chip.style.setProperty('--chip-edge', needsEdge(current, panel) ? '2px ' + (current ? 'solid' : 'dashed') + ' var(--muted)' : '1px solid var(--line)');
  chip.style.setProperty('--chip-ink', current ? inkOn(current) : 'var(--muted)');
  edit.setAttribute('aria-label', 'Edit colour ' + (state.slot + 1) + ', now ' + colorName(current));
  edit.title = edit.getAttribute('aria-label');
}

function paintPickChip() {
  const chip = $('pick-chip');
  const hex = state.hover;
  chip.style.setProperty('--chip-color', hex || 'transparent');
  chip.style.setProperty('--chip-ink', hex ? inkOn(hex) : 'var(--ink)');
  chip.style.setProperty('--chip-edge', hex
    ? (needsEdge(hex, panelColor()) ? '2px solid var(--muted)' : '1px solid var(--line)')
    : '2px dashed var(--muted)');
  $('pick-hex').textContent = hex || 'Hover the sheet';
}

function chooseSlot(index) {
  state.slot = index;
  const hex = state.slots[index];
  if (hex) {
    state.color = hex;
    closeDialog();
    say('Colour ' + (index + 1) + ', ' + colorName(hex));
  } else {
    openDialog('Colour ' + (index + 1) + ' is empty. Pick one for it.');
  }
  refresh();
}

/* ---- the custom colour dialog ------------------------------------------ */

const dialog = $('color-dialog');
const wheel = $('wheel');

function hex2hsv(hex) {
  const n = parseInt(hex.slice(1), 16);
  const r = (n >> 16 & 255) / 255, g = (n >> 8 & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let h = 0;
  if (d) {
    if (max === r) h = 60 * (((g - b) / d) % 6);
    else if (max === g) h = 60 * ((b - r) / d + 2);
    else h = 60 * ((r - g) / d + 4);
  }
  return { h: (h + 360) % 360, s: max ? Math.round(d / max * 100) : 0, v: Math.round(max * 100) };
}

function hsv2hex(h, s, v) {
  const sat = s / 100, value = v / 100;
  const c = value * sat, sector = ((h % 360) + 360) % 360 / 60;
  const x = c * (1 - Math.abs(sector % 2 - 1)), m = value - c;
  const rgb = [[c, x, 0], [x, c, 0], [0, c, x], [0, x, c], [x, 0, c], [c, 0, x]][Math.floor(sector) % 6];
  return '#' + rgb.map(v2 => Math.round((v2 + m) * 255).toString(16).padStart(2, '0')).join('').toUpperCase();
}

/** The dialog edits a draft; the pen colour changes only when it is applied. */
function draftColor() { return state.draft || state.slots[state.slot] || state.color; }

function paintDialog() {
  const hex = draftColor();
  const hsv = hex2hsv(hex);
  wheel.style.setProperty('--dim', String((100 - hsv.v) / 100));
  wheel.style.setProperty('--mark-x', (50 + Math.sin(hsv.h * Math.PI / 180) * hsv.s / 2) + '%');
  wheel.style.setProperty('--mark-y', (50 - Math.cos(hsv.h * Math.PI / 180) * hsv.s / 2) + '%');
  $('bright').value = String(hsv.v);
  $('hex').value = hex;
  $('apply-color').disabled = !state.draft || state.draft === state.slots[state.slot];
  for (const button of $('inks').querySelectorAll('button'))
    button.setAttribute('aria-pressed', String(button.dataset.hex === hex));
}

function openDialog(message) {
  dialog.hidden = false;
  $('edit-color').setAttribute('aria-expanded', 'true');
  state.draft = null;
  paintDialog();
  if (message) say(message);
}

function closeDialog() {
  dialog.hidden = true;
  $('edit-color').setAttribute('aria-expanded', 'false');
  state.draft = null;
}

function setDraft(hex) {
  state.draft = hex;
  paintDialog();
  paintSlots();
}

/** The wheel is HSV: angle is hue, distance from the centre is saturation, and value is
 *  the slider's alone — reading it as HSL is what made the slider jump on every click. */
function wheelAt(event) {
  const r = wheel.getBoundingClientRect(), radius = r.width / 2;
  const dx = event.clientX - r.left - radius, dy = event.clientY - r.top - radius;
  const h = ((Math.atan2(dx, -dy) * 180 / Math.PI) + 360) % 360;
  const s = Math.min(100, Math.round(Math.hypot(dx, dy) / radius * 100));
  const current = hex2hsv(draftColor());
  setDraft(hsv2hex(h, s, current.v || 100));
}

for (const ink of INKS) {
  const button = document.createElement('button');
  button.type = 'button';
  button.dataset.hex = ink.hex;
  button.setAttribute('aria-label', ink.name);
  button.setAttribute('aria-pressed', 'false');
  button.title = ink.name;
  const chip = document.createElement('span');
  chip.className = 'chip';
  chip.style.setProperty('--chip-color', ink.hex);
  chip.style.setProperty('--chip-edge', needsEdge(ink.hex, panelColor()) ? '2px solid var(--muted)' : '1px solid var(--line)');
  button.append(chip);
  button.onclick = () => { setDraft(ink.hex); say(ink.name + ', ' + ink.hex); };
  $('inks').append(button);
}

wheel.addEventListener('pointerdown', event => {
  wheel.setPointerCapture(event.pointerId);
  wheel.dataset.drag = 'yes';
  wheelAt(event);
});
wheel.addEventListener('pointermove', event => { if (wheel.dataset.drag === 'yes') wheelAt(event); });
for (const end of ['pointerup', 'pointercancel', 'lostpointercapture'])
  wheel.addEventListener(end, () => { wheel.dataset.drag = 'no'; });

wheel.addEventListener('keydown', event => {
  const current = hex2hsv(draftColor());
  const step = event.shiftKey ? 12 : 4;
  let h = current.h, s = current.s;
  if (event.key === 'ArrowLeft') h -= step;
  else if (event.key === 'ArrowRight') h += step;
  else if (event.key === 'ArrowUp') s = Math.min(100, s + step);
  else if (event.key === 'ArrowDown') s = Math.max(0, s - step);
  else return;
  event.preventDefault();
  event.stopPropagation();
  const hex = hsv2hex(h, s, current.v || 100);
  setDraft(hex);
  say('Hue ' + Math.round(((h % 360) + 360) % 360) + ', saturation ' + s + ' per cent, ' + hex);
});

$('bright').addEventListener('input', () => {
  const current = hex2hsv(draftColor());
  setDraft(hsv2hex(current.h, current.s, Number($('bright').value)));
});

$('hex').addEventListener('change', () => {
  const value = $('hex').value.trim();
  if (!isHex(value)) {
    $('color-error').textContent = 'Enter # followed by six hexadecimal digits, such as #008080.';
    $('hex').setAttribute('aria-invalid', 'true');
    return;
  }
  $('color-error').textContent = '';
  $('hex').removeAttribute('aria-invalid');
  setDraft(value.toUpperCase());
});

$('edit-color').onclick = () => { if (dialog.hidden) openDialog(); else closeDialog(); refresh(); };
$('cancel-color').onclick = () => { closeDialog(); say('Custom colour discarded.'); refresh(); };
$('apply-color').onclick = () => {
  const hex = draftColor();
  state.slots[state.slot] = hex;
  state.color = hex;
  closeDialog();
  say('Colour ' + (state.slot + 1) + ' set to ' + hex);
  refresh();
};

/* ---- drawing ----------------------------------------------------------- */

const widthNow = event => (event && event.pointerType === 'pen' && event.pressure)
  ? Math.max(1, state.size * (0.35 + 1.3 * event.pressure))
  : state.size;

function startMark(point, event) {
  const tool = state.tool;
  const width = widthNow(event);
  const mark = { color: tool === 'eraser' ? '#FFFFFF' : state.color, width };
  if (state.blend && tool !== 'eraser') mark.blend = 'multiply';
  if (isShape({ kind: tool })) {
    mark.kind = tool;
    mark.a = { x: point.x, y: point.y };
    mark.b = { x: point.x, y: point.y };
    if (state.dash !== 'solid' && DASHED.includes(tool)) mark.dash = state.dash;
  } else {
    mark.points = [[point.x, point.y]];
    mark.varies = false;
  }
  return mark;
}

/** Filter the jitter out of a sample without lagging behind real movement: a big jump is
 *  the hand, a small one is noise. Samples closer than a pixel fold into the last. */
function addPoint(event) {
  const raw = sheet.at(event);
  const points = drawing.points;
  const last = points[points.length - 1];
  const width = widthNow(event);
  const distance = Math.hypot(raw.x - last[0], raw.y - last[1]);
  const factor = distance > 24 ? 1 : (distance > 8 ? 0.85 : 0.55);
  const x = last[0] + (raw.x - last[0]) * factor;
  const y = last[1] + (raw.y - last[1]) * factor;
  if (distance < 1.2) return;
  // Only a pressure-varying stroke stores a width per point; a mouse or a finger leaves
  // the log in the original two-number shape.
  if (Math.abs(width - drawing.width) > 0.5) drawing.varies = true;
  points.push(drawing.varies ? [x, y, width] : [x, y]);
}

function finishMark() {
  const mark = drawing;
  drawing = null;
  if (!mark) return;
  delete mark.varies;
  if (mark.points && mark.points.length === 1) mark.points.push([mark.points[0][0] + 0.7, mark.points[0][1]]);
  if (isShape(mark) && mark.a.x === mark.b.x && mark.a.y === mark.b.y) { render(); return; }
  commit([{ op: 'put', tbl: 'stroke', id: pv.ulid(), d: mark }], 'Mark saved.');
}

/**
 * Colour the inside of the shape under the pointer, or the sheet itself when there is no
 * shape there.
 *
 * A fill is the inside of one mark, and it carries that area in its own event. It is not
 * a seed replayed against the pixels: a seed has no owner, so it could not be moved,
 * selected or reasoned about, and it changed what it covered every time anything was
 * drawn near it. `anchor` names the mark the colour belongs to, which is how a move and a
 * deletion know to carry it along; nothing is looked up to draw it.
 */
function fillAt(point) {
  const host = ordered().slice().reverse()
    .find(([, mark]) => encloses(mark) && covers(mark, point.x, point.y));

  if (!host) {
    commit([{ op: 'put', tbl: 'stroke', id: pv.ulid(), d: { kind: 'page', color: state.color } }],
      'Sheet coloured ' + state.color + '. Undo puts it back.');
    return;
  }

  const area = areaOf(host[1]);
  if (!area) {
    say('That outline is too small to colour inside. Draw a larger one, or a thinner line.');
    return;
  }
  const mark = { kind: 'fill', shape: area.shape, color: state.color, anchor: host[0] };
  if (area.shape === 'free') mark.points = area.points;
  else { mark.a = area.a; mark.b = area.b; }
  commit([{ op: 'put', tbl: 'stroke', id: pv.ulid(), d: mark }],
    'Filled the shape. The colour moves with it.');
}

function placeText(point) {
  const words = $('words').value || 'Label';
  const mark = { kind: 'text', x: point.x, y: point.y, text: words, color: state.color, width: state.size };
  if (state.blend) mark.blend = 'multiply';
  commit([{ op: 'put', tbl: 'stroke', id: pv.ulid(), d: mark }], 'Text placed.');
}

function pickAt(point) {
  const device = sheet.toDevice(point);
  const data = ctx.getImageData(
    Math.max(0, Math.min(pad.width - 1, device.x)),
    Math.max(0, Math.min(pad.height - 1, device.y)), 1, 1).data;
  const hex = '#' + [data[0], data[1], data[2]].map(v => v.toString(16).padStart(2, '0')).join('').toUpperCase();
  state.slots[state.slot] = hex;
  state.color = hex;
  state.hover = '';
  const back = TOOLS.find(t => t.id === state.previous) || TOOLS[1];
  state.tool = back.id;
  if (!BASIC.includes(back.id)) state.other = back.id;
  say('Picked ' + hex + ' into colour ' + (state.slot + 1) + '. Back to ' + back.label + '.');
  refresh();
}

/* ---- selecting and moving ---------------------------------------------- */

/**
 * Every mark under a point, topmost first, so a repeat click can step through overlaps.
 *
 * A freehand mark is tested against the painted path, so a click inside a loose scribble
 * does not grab it. A freehand mark that holds a fill is a filled object, though, and is
 * grabbed anywhere inside it — otherwise a hand-drawn circle you have coloured in can
 * only be picked up by its outline.
 */
function hitsAt(point) {
  const filled = new Set();
  for (const [, mark] of ordered()) if (mark.kind === 'fill' && mark.anchor) filled.add(mark.anchor);
  const inside = (id, mark) => filled.has(id) && covers(mark, point.x, point.y);
  return ordered().slice().reverse()
    .filter(([id, mark]) => hits(mark, point.x, point.y) || inside(id, mark))
    .map(([id]) => id);
}

function selectAt(point) {
  const found = hitsAt(point);
  if (!found.length) {
    cycle = null;
    state.selection = [];
    say('Nothing there to select.');
    refresh();
    return;
  }
  const key = found.join(',');
  const near = cycle && cycle.key === key && Math.hypot(point.x - cycle.x, point.y - cycle.y) < 14;
  const index = near ? (cycle.index + 1) % found.length : 0;
  cycle = { x: point.x, y: point.y, key, index };
  state.selection = [found[index]];
  say(found.length > 1
    ? 'Mark ' + (index + 1) + ' of ' + found.length + ' here. Click again for the next one, or drag to move.'
    : 'Mark selected. Drag to move.');
  refresh();
}

/**
 * Move whole marks: a tombstone and a fresh put each, since an id is never reused, with
 * the original layer carried over so painting order survives. A fill anchored to a moved
 * shape is rewritten in the same batch to point at the shape's new id — the whole thing
 * is one batch, so one undo puts it all back.
 */
function moveSelection(dx, dy) {
  const ids = state.selection.slice();
  if (!ids.length || (!dx && !dy)) return;
  const { groups, fresh } = moveEvents(ordered(), ids, dx, dy, () => pv.ulid());
  state.selection = ids.map(id => fresh.get(id) || id);
  cycle = null;
  commitGroups(groups, 'Moved ' + ids.length + ' mark' + (ids.length === 1 ? '' : 's') + '.');
}

/**
 * Scale the selection about the top-left of its own box, so the corner being dragged is
 * the one that moves. Every mark and every colour inside one goes through the same
 * transform (`strokes.resizeEvents`), which is what keeps a fill with its shape.
 */
function resizeSelection(sx, sy) {
  const ids = state.selection.slice();
  const box = selectionBox(null);
  if (!ids.length || !box || (sx === 1 && sy === 1)) return;
  const { groups, fresh } = resizeEvents(ordered(), ids, box[0], box[1], sx, sy, () => pv.ulid());
  state.selection = ids.map(id => fresh.get(id) || id);
  cycle = null;
  const percent = Math.round(Math.max(sx, sy) * 100);
  commitGroups(groups, 'Resized ' + ids.length + ' mark' + (ids.length === 1 ? '' : 's') +
    ' to ' + percent + ' per cent.');
}

/** Grow or shrink the selection by a step, for the keyboard and for touch. */
function stepSize(factor) {
  if (!state.selection.length) { say('Select something first, then resize it.'); return; }
  const box = selectionBox(null);
  if (!box) return;
  const w = box[2] - box[0], h = box[3] - box[1];
  const { sx, sy } = scaleTo(box, box[0] + w * factor, box[1] + h * factor, true);
  resizeSelection(sx, sy);
}

function deleteSelection() {
  const ids = state.selection.slice();
  if (!ids.length) return;
  const events = ids.map(id => ({ op: 'del', tbl: 'stroke', id }));
  // A fill anchored to a deleted shape would never draw again; it goes with it.
  for (const [id] of anchoredTo(ids)) if (!ids.includes(id)) events.push({ op: 'del', tbl: 'stroke', id });
  state.selection = [];
  cycle = null;
  commit(events, 'Deleted ' + ids.length + ' mark' + (ids.length === 1 ? '' : 's') + '. Undo brings them back.');
}

/* ---- the clipboard ----------------------------------------------------- */

function copySelection(cut) {
  const ids = state.selection.slice();
  if (!ids.length) { say('Nothing selected to ' + (cut ? 'cut' : 'copy') + '.'); return; }
  // Each clipped mark remembers the id it came from, so a fill can be re-pointed at the
  // pasted copy of its shape rather than at the original.
  const picked = ids.map(id => [id, marks.get(id)]).filter(([, mark]) => mark);
  const fills = anchoredTo(ids).filter(([id]) => !ids.includes(id));
  state.clip = JSON.parse(JSON.stringify(picked.concat(fills).map(([id, mark]) => ({ ...mark, from: id }))));
  state.clipStamp = 'sk' + Date.now().toString(36);
  // Best effort only: pasting back into the sheet never depends on the system clipboard.
  try {
    const svg = toSvg(state.clip.map(mark => [mark.from, mark]), state.clipStamp).text;
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(svg);
  } catch { /* a clipboard the browser will not give us is not an error worth showing */ }
  if (cut) { deleteSelection(); return; }
  say('Copied ' + ids.length + ' mark' + (ids.length === 1 ? '' : 's') + '. Paste with Control or Command V.');
  refresh();
}

function pasteMarks(list, message) {
  if (!list || !list.length) { say('Nothing to paste.'); return; }
  const shifted = list.map(mark => moved(mark, 32, 32));
  const fresh = new Map();
  for (const mark of shifted) {
    const id = pv.ulid();
    mark.id = id;                                   // held here only, never written
    if (mark.from) fresh.set(mark.from, id);
  }
  const events = [];
  for (const mark of shifted) {
    const id = mark.id;
    const d = { ...mark };
    delete d.id;
    delete d.from;
    delete d.layer;                                 // a pasted mark is a new mark
    if (d.kind === 'fill' && d.anchor && fresh.has(d.anchor)) d.anchor = fresh.get(d.anchor);
    events.push({ op: 'put', tbl: 'stroke', id, d });
  }
  state.selection = events.filter(event => event.d.kind !== 'fill').map(event => event.id);
  state.tool = 'select';
  state.selMode = 'stroke';
  const fills = events.filter(event => event.d.kind === 'fill').length;
  const count = events.length - fills;
  commit(events, message || ('Pasted ' + count + ' mark' + (count === 1 ? '' : 's') +
    (fills ? ' with ' + (fills === 1 ? 'its fill' : fills + ' fills') : '') + '. Drag to place them.'));
}

document.addEventListener('paste', event => {
  const tag = (event.target && event.target.tagName) || '';
  if (tag === 'INPUT' || tag === 'TEXTAREA') return;
  const text = event.clipboardData && event.clipboardData.getData('text/plain');
  if ((isOurs(text, state.clipStamp) || !text) && state.clip) {
    event.preventDefault();
    pasteMarks(JSON.parse(JSON.stringify(state.clip)));
    return;
  }
  if (!text || !text.includes('<svg')) return;
  const parsed = fromSvg(text, state.color, state.size);
  if (!parsed) {
    say('That SVG has nothing this sheet can draw — straight lines, rectangles, ellipses and polylines only.');
    return;
  }
  event.preventDefault();
  const n = parsed.marks.length;
  pasteMarks(parsed.marks, 'Pasted ' + n + ' mark' + (n === 1 ? '' : 's') + ' from an SVG' +
    (parsed.skipped ? ', ' + parsed.skipped + (parsed.skipped === 1 ? ' element it cannot draw was' : ' elements it cannot draw were') + ' left out' : '') + '.');
});

/* ---- pointer input ----------------------------------------------------- */

pad.addEventListener('pointerdown', event => {
  if (history.busy || event.button !== 0) return;
  closeMenus();
  const point = sheet.at(event);
  const tool = state.tool;

  if (tool === 'pan') {
    try { pad.setPointerCapture(event.pointerId); } catch { /* a pointer we cannot capture still pans */ }
    panning = { x: event.clientX, y: event.clientY, left: viewport.scrollLeft, top: viewport.scrollTop };
    say(sheet.pannable ? 'Dragging the view.' : 'The whole sheet already fits — zoom in to have something to pan.');
    return;
  }
  try { pad.setPointerCapture(event.pointerId); } catch { /* as above */ }

  if (tool === 'pick') { pickAt(point); return; }
  if (tool === 'fill') { fillAt(point); return; }
  if (tool === 'text') { placeText(point); return; }

  if (tool === 'select') {
    if (state.selection.length && onHandle(point)) {
      const box = selectionBox(null);
      if (box) {
        resizing = { ax: box[0], ay: box[1], box, sx: 1, sy: 1 };
        say('Resizing. Hold Shift to keep the proportions.');
        return;
      }
    }
    if (state.selMode === 'rect') {
      if (state.selection.length && state.selection.some(id => {
        const box = bounds(marks.get(id));
        return box && point.x >= box[0] && point.x <= box[2] && point.y >= box[1] && point.y <= box[3];
      })) {
        dragging = { x: point.x, y: point.y, dx: 0, dy: 0 };
        return;
      }
      marquee = { a: point, b: point };
      state.selection = [];
      refresh();
      return;
    }
    selectAt(point);
    if (state.selection.length) dragging = { x: point.x, y: point.y, dx: 0, dy: 0 };
    return;
  }

  if (drawing) return;
  drawing = startMark(point, event);
});

pad.addEventListener('pointermove', event => {
  if (panning) {
    viewport.scrollLeft = panning.left - (event.clientX - panning.x);
    viewport.scrollTop = panning.top - (event.clientY - panning.y);
    return;
  }
  if (state.tool === 'pick' && !drawing) {
    const device = sheet.toDevice(sheet.at(event));
    const data = ctx.getImageData(
      Math.max(0, Math.min(pad.width - 1, device.x)),
      Math.max(0, Math.min(pad.height - 1, device.y)), 1, 1).data;
    const hex = '#' + [data[0], data[1], data[2]].map(v => v.toString(16).padStart(2, '0')).join('').toUpperCase();
    if (hex !== state.hover) { state.hover = hex; paintPickChip(); }
    return;
  }
  if (marquee) { marquee.b = sheet.at(event); scheduleRender(); return; }
  if (resizing) {
    const point = sheet.at(event);
    const next = scaleTo(resizing.box, point.x, point.y, event.shiftKey);
    resizing.sx = next.sx;
    resizing.sy = next.sy;
    scheduleRender();
    return;
  }
  if (dragging) {
    const point = sheet.at(event);
    dragging.dx = point.x - dragging.x;
    dragging.dy = point.y - dragging.y;
    scheduleRender();
    return;
  }
  if (!drawing) return;
  if (drawing.points) {
    const native = event;
    const batch = native.getCoalescedEvents ? native.getCoalescedEvents() : [];
    if (batch.length) for (const sample of batch) addPoint(sample);
    else addPoint(event);
  } else {
    drawing.b = sheet.at(event);
  }
  scheduleRender();
});

pad.addEventListener('pointerleave', () => { if (state.hover) { state.hover = ''; paintPickChip(); } });

function endPointer() {
  if (panning) { panning = null; return; }
  if (resizing) {
    const { sx, sy } = resizing;
    resizing = null;
    if (Math.abs(sx - 1) < 0.005 && Math.abs(sy - 1) < 0.005) { render(); return; }
    resizeSelection(sx, sy);
    return;
  }
  if (marquee) {
    const box = marquee;
    marquee = null;
    state.selection = ordered().filter(([, mark]) => inBox(mark, box.a, box.b)).map(([id]) => id);
    say(state.selection.length
      ? state.selection.length + ' mark' + (state.selection.length === 1 ? '' : 's') + ' selected. Drag to move.'
      : 'Nothing in that box.');
    refresh();
    return;
  }
  if (dragging) {
    const { dx, dy } = dragging;
    dragging = null;
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) { render(); return; }
    moveSelection(dx, dy);
    return;
  }
  finishMark();
}

pad.addEventListener('pointerup', endPointer);
for (const end of ['pointercancel', 'lostpointercapture']) pad.addEventListener(end, endPointer);

/** Double-click a text block with Select to rewrite it in the rail's own field. */
pad.addEventListener('dblclick', event => {
  if (state.tool !== 'select') return;
  const point = sheet.at(event);
  const found = hitsAt(point).find(id => (marks.get(id) || {}).kind === 'text');
  if (!found) return;
  state.editing = found;
  state.selection = [found];
  $('words').value = marks.get(found).text;
  refresh();
  $('words').focus();
  say('Editing text. Change the words and press Enter.');
});

$('words').addEventListener('keydown', event => {
  if (event.key !== 'Enter' || !state.editing) return;
  event.preventDefault();
  const id = state.editing;
  const mark = marks.get(id);
  if (!mark) { state.editing = null; refresh(); return; }
  const next = pv.ulid();
  state.editing = null;
  state.selection = [next];
  commit([
    { op: 'del', tbl: 'stroke', id },
    { op: 'put', tbl: 'stroke', id: next, d: { ...mark, text: $('words').value || 'Label', layer: layerOf(id, mark) } }
  ], 'Text rewritten.');
});

/* ---- keyboard ---------------------------------------------------------- */

// A drawing must not need a pointer (WCAG 2.5.7, and the accessibility skill's rule that
// no drag has no single-key alternative). Shortcuts stay out of the way of text fields.
document.addEventListener('keydown', event => {
  const tag = (event.target && event.target.tagName) || '';
  if (tag === 'INPUT' || tag === 'TEXTAREA') {
    if (event.key === 'Escape' && state.editing) { state.editing = null; refresh(); pad.focus(); }
    return;
  }
  const key = event.key.toLowerCase();
  const mod = event.metaKey || event.ctrlKey;

  if (mod && key === 'z') { event.preventDefault(); event.shiftKey ? doRedo() : doUndo(); return; }
  if (mod && key === 'c') { event.preventDefault(); copySelection(false); return; }
  if (mod && key === 'x') { event.preventDefault(); copySelection(true); return; }
  // A drag of the handle needs a path that is not a drag (WCAG 2.5.7).
  if (mod && (event.key === 'ArrowRight' || event.key === 'ArrowDown')) {
    event.preventDefault();
    stepSize(event.shiftKey ? 1.5 : 1.1);
    return;
  }
  if (mod && (event.key === 'ArrowLeft' || event.key === 'ArrowUp')) {
    event.preventDefault();
    stepSize(event.shiftKey ? 1 / 1.5 : 1 / 1.1);
    return;
  }
  if (mod) return;                                  // leave every other shortcut alone

  const tool = TOOLS.find(t => t.key.toLowerCase() === key);
  if (tool) { event.preventDefault(); chooseTool(tool.id); return; }

  if (key === '+' || key === '=') { event.preventDefault(); sheet.step(1); fit(); refresh(); return; }
  if (key === '-') { event.preventDefault(); sheet.step(-1); fit(); refresh(); return; }
  if (key === '0') { event.preventDefault(); sheet.setZoom(0); fit(); refresh(); return; }

  if ((event.key === 'Delete' || event.key === 'Backspace') && state.selection.length) {
    event.preventDefault();
    deleteSelection();
    return;
  }
  if (event.key === 'Escape') {
    if (!dialog.hidden) { closeDialog(); refresh(); return; }
    if (resizing) { resizing = null; say('Resize abandoned.'); render(); return; }
    if (drawing) { drawing = null; say('Stroke discarded.'); render(); return; }
    if (state.selection.length) { state.selection = []; cycle = null; say('Selection cleared.'); refresh(); return; }
  }

  if (document.activeElement !== pad) return;

  const step = event.shiftKey ? 60 : 8;
  let dx = 0, dy = 0;
  if (event.key === 'ArrowLeft') dx = -step;
  else if (event.key === 'ArrowRight') dx = step;
  else if (event.key === 'ArrowUp') dy = -step;
  else if (event.key === 'ArrowDown') dy = step;
  else if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    if (history.busy) return;
    const point = { x: pen.x, y: pen.y };
    if (state.tool === 'pick') { pickAt(point); return; }
    if (state.tool === 'fill') { fillAt(point); return; }
    if (state.tool === 'text') { placeText(point); return; }
    if (state.tool === 'select') {
      if (state.selMode === 'rect') { say('Rectangle selection needs a pointer. Switch to Stroke mode to select from the keyboard.'); return; }
      selectAt(point);
      return;
    }
    if (pen.down) { pen.down = false; finishMark(); return; }
    drawing = startMark(point, null);
    pen.down = true;
    say('Pen down. Move with the arrow keys, press Space to lift.');
    render();
    return;
  } else return;

  event.preventDefault();

  if (state.tool === 'pan') { sheet.panBy(dx * 7.5, dy * 7.5); return; }

  if (state.tool === 'select' && state.selection.length) { moveSelection(dx, dy); return; }

  pen.x = Math.max(0, Math.min(SHEET_W, pen.x + dx));
  pen.y = Math.max(0, Math.min(SHEET_H, pen.y + dy));
  if (state.tool === 'pick') {
    const device = sheet.toDevice(pen);
    const data = ctx.getImageData(
      Math.max(0, Math.min(pad.width - 1, device.x)),
      Math.max(0, Math.min(pad.height - 1, device.y)), 1, 1).data;
    state.hover = '#' + [data[0], data[1], data[2]].map(v => v.toString(16).padStart(2, '0')).join('').toUpperCase();
    paintPickChip();
  }
  if (pen.down && drawing) {
    if (drawing.points) drawing.points.push([pen.x, pen.y]);
    else drawing.b = { x: pen.x, y: pen.y };
  }
  render();
});

pad.addEventListener('focus', () => {
  say('Sheet focused. Arrow keys move the pen, Space puts it down.');
  render();
});
pad.addEventListener('blur', () => { pen.down = false; render(); });

/* ---- controls ---------------------------------------------------------- */

function closeMenus() {
  for (const open of document.querySelectorAll('#actions[open]')) open.open = false;
  closeQuick(false);
}

/** Apply one of a tool's options and return what to say about it. The rail's buttons and
 *  the top bar's menus are two ways to the same setting, so both come through here. */
function applyOption(kind, value) {
  if (kind === 'size') {
    state.size = Number(value);
    return (SIZES.find(s => s.px === state.size) || {}).label + ', ' + state.size + ' pixels.';
  }
  if (kind === 'dash') {
    state.dash = value;
    return (DASHES.find(d => d.id === value) || {}).label + ' line style.';
  }
  if (kind === 'sel') {
    state.selMode = value;
    state.selection = [];
    return (value === 'rect' ? 'Rectangle' : 'Stroke') + ' selection.';
  }
  return '';
}

/* ---- the quick tools' option menus -------------------------------------- */

// Each quick tool is a split button: the tool, and a corner caret that opens its own
// options. A long press and a right click reach the same menu, but the caret is a control
// of its own, so nothing depends on holding a finger down (WCAG 2.5.7) — and every option
// in here is also a full-size button in the rail.
const LONG_PRESS = 450;
const quickMenu = $('quick-menu');
let openFor = null;
let pressTimer = null;
let pressedLong = false;

/** What a tool offers, in the groups a menu draws. Empty for a tool with no settings. */
function optionsFor(tool) {
  const groups = [];
  if (WIDTHED.includes(tool)) {
    groups.push({
      name: tool === 'eraser' ? 'Eraser width' : 'Size',
      kind: 'size',
      items: SIZES.map(size => ({
        value: size.px, label: size.label, note: size.px + ' px',
        chip: 'dot dot-' + size.px, on: state.size === size.px
      }))
    });
  }
  if (DASHED.includes(tool)) {
    groups.push({
      name: 'Line style',
      kind: 'dash',
      items: DASHES.map(dash => ({
        value: dash.id, label: dash.label,
        chip: 'rule rule-' + dash.id, on: state.dash === dash.id
      }))
    });
  }
  if (tool === 'select') {
    groups.push({
      name: 'Selection',
      kind: 'sel',
      items: [
        { value: 'stroke', label: 'Stroke', note: 'one mark', icon: 'i-select', on: state.selMode === 'stroke' },
        { value: 'rect', label: 'Rectangle', note: 'a box', icon: 'i-marquee', on: state.selMode === 'rect' }
      ]
    });
  }
  return groups;
}

const hasOptions = tool => optionsFor(tool).length > 0;

// Cloned from the page rather than built in script: an <svg> needs its namespace, and
// naming that URL here would read as an external origin the app reaches for.
function menuIcon(id) {
  const svg = $('icon-template').content.firstElementChild.cloneNode(true);
  svg.querySelector('use').setAttribute('href', '#' + id);
  return svg;
}

function buildQuickMenu(tool) {
  quickMenu.textContent = '';
  for (const group of optionsFor(tool)) {
    const heading = document.createElement('p');
    heading.className = 'menu-head';
    heading.textContent = group.name;
    quickMenu.append(heading);
    for (const item of group.items) {
      const button = document.createElement('button');
      button.type = 'button';
      button.setAttribute('role', 'menuitemradio');
      button.setAttribute('aria-checked', String(item.on));
      button.dataset.kind = group.kind;
      button.dataset.value = String(item.value);
      if (item.icon) {
        button.append(menuIcon(item.icon));
      } else {
        const chip = document.createElement('span');
        chip.className = item.chip;
        button.append(chip);
      }
      const label = document.createElement('span');
      label.textContent = item.label;
      button.append(label);
      if (item.note) {
        const note = document.createElement('span');
        note.className = 'px';
        note.textContent = item.note;
        button.append(note);
      }
      quickMenu.append(button);
    }
  }
}

function openQuick(tool, wrapper) {
  if (!hasOptions(tool)) return false;
  closeQuick(false);
  buildQuickMenu(tool);
  wrapper.append(quickMenu);
  quickMenu.hidden = false;
  openFor = tool;
  // The menu hangs off its own button, which puts the rightmost one past the edge of a
  // narrow window; shift it back rather than letting the page scroll sideways.
  quickMenu.style.setProperty('--shift', '0px');
  const room = document.documentElement.clientWidth - 8 - quickMenu.getBoundingClientRect().right;
  if (room < 0) quickMenu.style.setProperty('--shift', Math.floor(room) + 'px');
  const caret = wrapper.querySelector('.more');
  if (caret) caret.setAttribute('aria-expanded', 'true');
  const first = quickMenu.querySelector('[aria-checked="true"]') || quickMenu.querySelector('button');
  if (first) first.focus();
  say((TOOLS.find(t => t.id === tool) || {}).label +
    ' options. Up and down choose, Enter applies, Escape closes.');
  return true;
}

function closeQuick(refocus) {
  if (!openFor) return;
  const tool = openFor;
  openFor = null;
  quickMenu.hidden = true;
  for (const caret of document.querySelectorAll('.more')) caret.setAttribute('aria-expanded', 'false');
  if (!refocus) return;
  const button = [...document.querySelectorAll('.quick')].find(b => b.dataset.tool === tool);
  if (button) button.focus();
}

quickMenu.addEventListener('click', event => {
  const item = event.target.closest('button[role="menuitemradio"]');
  if (!item) return;
  const tool = openFor;
  closeQuick(true);
  let message = '';
  if (tool && tool !== state.tool) {
    chooseTool(tool, true);
    message = (TOOLS.find(t => t.id === tool) || {}).label + ' selected. ';
  }
  message += applyOption(item.dataset.kind, item.dataset.value);
  refresh();
  say(message);
});

// The menu owns the keyboard while it is open, so a tool letter does not fire underneath it.
quickMenu.addEventListener('keydown', event => {
  const items = [...quickMenu.querySelectorAll('button[role="menuitemradio"]')];
  const at = items.indexOf(document.activeElement);
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    closeQuick(true);
    return;
  }
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    event.stopPropagation();
    const step = event.key === 'ArrowDown' ? 1 : -1;
    const next = items[(at + step + items.length) % items.length];
    if (next) next.focus();
    return;
  }
  if (event.key === 'Home' || event.key === 'End') {
    event.preventDefault();
    event.stopPropagation();
    const next = event.key === 'Home' ? items[0] : items.at(-1);
    if (next) next.focus();
    return;
  }
  if (event.key === 'Tab') { closeQuick(false); return; }
  event.stopPropagation();
});

for (const wrapper of document.querySelectorAll('.quick-wrap')) {
  const quick = wrapper.querySelector('.quick');
  const caret = wrapper.querySelector('.more');

  caret.addEventListener('click', event => {
    event.preventDefault();
    event.stopPropagation();
    const tool = caret.dataset.more;
    if (openFor === tool) closeQuick(true);
    else openQuick(tool, wrapper);
  });

  quick.addEventListener('pointerdown', event => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    pressedLong = false;
    clearTimeout(pressTimer);
    pressTimer = setTimeout(() => {
      pressTimer = null;
      pressedLong = openQuick(quick.dataset.tool, wrapper);
    }, LONG_PRESS);
  });
  for (const end of ['pointerup', 'pointerleave', 'pointercancel']) {
    quick.addEventListener(end, () => { clearTimeout(pressTimer); pressTimer = null; });
  }

  quick.addEventListener('contextmenu', event => {
    if (!hasOptions(quick.dataset.tool)) return;
    event.preventDefault();
    openQuick(quick.dataset.tool, wrapper);
  });

  quick.addEventListener('keydown', event => {
    if (event.key !== 'ArrowDown' && !(event.shiftKey && event.key === 'F10')) return;
    if (!openQuick(quick.dataset.tool, wrapper)) return;
    event.preventDefault();
    event.stopPropagation();
  });
}

// Anywhere else closes it, before the sheet's own handler sees the press.
document.addEventListener('pointerdown', event => {
  if (!openFor) return;
  if (event.target.closest('#quick-menu') || event.target.closest('.quick-wrap')) return;
  closeQuick(false);
}, true);

document.addEventListener('click', event => {
  // A long press has already opened the menu; the click that ends it is not a choice.
  if (pressedLong) { pressedLong = false; return; }
  // Only a control, never an ancestor that happens to carry the attribute: <body> holds
  // data-tool for the CSS, and matching it here made every click on the sheet re-choose
  // the current tool, which drops the selection the click had just made.
  const button = event.target.closest('button[data-tool], .size, .dash, .selmode, .slot');
  if (!button || button.disabled) return;
  if (button.dataset.tool) { chooseTool(button.dataset.tool); return; }
  if (button.classList.contains('slot')) { chooseSlot(Number(button.dataset.slot)); return; }
  const kind = button.classList.contains('size') ? 'size'
    : button.classList.contains('dash') ? 'dash' : 'sel';
  say(applyOption(kind, button.dataset.size ?? button.dataset.dash ?? button.dataset.sel));
  refresh();
});

$('blend').onclick = () => {
  state.blend = !state.blend;
  say(state.blend
    ? 'Translucent ink on. New marks darken where they overlap.'
    : 'Translucent ink off. New marks cover what is under them.');
  refresh();
};

$('bar-copy').onclick = $('rail-copy').onclick = () => copySelection(false);
$('bar-cut').onclick = $('rail-cut').onclick = () => copySelection(true);
$('bar-paste').onclick = $('rail-paste').onclick = () => pasteMarks(JSON.parse(JSON.stringify(state.clip || [])));
$('rail-delete').onclick = () => deleteSelection();
$('rail-bigger').onclick = () => stepSize(1.1);
$('rail-smaller').onclick = () => stepSize(1 / 1.1);

async function doUndo() {
  if (drawing) { drawing = null; render(); say('Stroke in progress discarded.'); return; }
  try {
    const result = await history.undo();
    state.selection = [];
    refresh();
    say(result?.queued ? 'Offline — undo queued.' : 'Change undone.');
  } catch (error) { say(error.message); }
}
async function doRedo() {
  try {
    const result = await history.redo();
    state.selection = [];
    refresh();
    say(result?.queued ? 'Offline — redo queued.' : 'Change redone.');
  } catch (error) { say(error.message); }
}
$('undo').onclick = doUndo;
$('redo').onclick = doRedo;

$('contrast').onclick = () => {
  const on = document.documentElement.dataset.contrast === 'on';
  if (on) delete document.documentElement.dataset.contrast;
  else document.documentElement.dataset.contrast = 'on';
  $('contrast').setAttribute('aria-pressed', String(!on));
  say(on ? 'High contrast off.' : 'High contrast on.');
  refresh();
};

$('zoom-out').onclick = () => { sheet.step(-1); fit(); refresh(); };
$('zoom-in').onclick = () => { sheet.step(1); fit(); refresh(); };
$('zoom-fit').onclick = () => { sheet.setZoom(0); fit(); refresh(); say('Fit to view.'); };

$('zoom-level').onclick = () => {
  const input = $('zoom-input');
  $('zoom-level').hidden = true;
  input.hidden = false;
  input.value = String(sheet.percent);
  input.focus();
  input.select();
};
function leaveZoomInput() {
  $('zoom-input').hidden = true;
  $('zoom-level').hidden = false;
}
$('zoom-input').addEventListener('blur', leaveZoomInput);
$('zoom-input').addEventListener('keydown', event => {
  event.stopPropagation();
  if (event.key === 'Escape') { event.preventDefault(); leaveZoomInput(); say('Zoom unchanged.'); return; }
  if (event.key !== 'Enter') return;
  event.preventDefault();
  const value = parseFloat(String($('zoom-input').value).replace('%', ''));
  if (!isFinite(value) || value < 5 || value > 400) { say('Enter a zoom between 5 and 400 per cent.'); return; }
  leaveZoomInput();
  sheet.setZoom(value / 100);
  fit();
  refresh();
});

$('dismiss-hint').onclick = () => { $('rotate-hint').hidden = true; };

/* ---- export ------------------------------------------------------------ */

// The sheet at its own resolution, whatever the zoom, on an opaque white surface and
// without the keyboard crosshair.
$('png').onclick = () => {
  closeMenus();
  const output = document.createElement('canvas');
  output.width = SHEET_W;
  output.height = SHEET_H;
  const target = output.getContext('2d');
  target.fillStyle = pageColor();
  target.fillRect(0, 0, SHEET_W, SHEET_H);
  target.lineCap = target.lineJoin = 'round';
  for (const [, mark] of ordered()) {
    if (mark.kind === 'page') continue;
    if (mark.kind === 'fill') {
      paintArea(target, areaOnScreen(mark, null), mark.color);
      continue;
    }
    paintMark(target, mark, 1);
  }
  output.toBlob(blob => {
    if (!blob) { say('Could not create the PNG. Try again.'); return; }
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'sketch.png';
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 60000);
    say('PNG saved at 1600 by 1200, whatever the zoom.');
  }, 'image/png');
};

$('svg').onclick = () => {
  closeMenus();
  const { text, skipped } = toSvg(ordered(), null, pageColor());
  const url = URL.createObjectURL(new Blob([text], { type: 'image/svg+xml' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = 'sketch.svg';
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
  say(skipped
    ? 'SVG saved. ' + skipped + ' fill' + (skipped === 1 ? '' : 's') +
      ' left out — a fill on the open page is pixels, not a shape.'
    : 'SVG saved.');
};

$('clear').onclick = () => {
  closeMenus();
  drawing = null;
  state.selection = [];
  state.slots = DEFAULT_SLOTS.slice();
  state.slot = 0;
  state.color = DEFAULT_SLOTS[0];
  const events = [...marks.keys()].map(id => ({ op: 'del', tbl: 'stroke', id }));
  if (!events.length) { refresh(); say('The sheet is already empty.'); return; }
  commit(events, 'New sheet. Colours are back to their defaults; undo restores the drawing.');
};

/* ---- live updates ------------------------------------------------------ */

// Every mark drawn in another window on this node arrives here, and so does every mark
// from a paired device — including ones that landed while this tab was closed. Each joins
// the undo history like a mark drawn here: the sheet is shared, so undo reverses the
// latest change to it whichever device made it.
pv.subscribe(ev => {
  if (ev.tbl !== 'stroke') return;
  history.apply(ev);
  refresh();
});

pv.on('offline', () => say('Offline — your marks are queued.'));
pv.on('online', () => say(''));
// A refused write is the person's work: say so rather than dropping it silently.
// The payload is `{ id, events, error }` (spec/data-api.md §6): the node's own refusal
// is on `error`, and a conflict names the row it lost to.
pv.on('rejected', report => {
  const error = report && report.error;
  const conflict = error && error.conflict;
  say('A change could not be saved: ' +
    (error && error.message ? error.message : 'it conflicted with a newer one.') +
    (conflict ? ' Another change to that mark arrived first.' : ''));
});

/* ---- boot -------------------------------------------------------------- */

// Two exits, one destination: the mark in the rail and the Apps entry in the menu. The
// app declines the framework's bar, so these are the only way back it has.
const out = exitTarget(pv.mount, pv.url);
pointOut($('exit'), out);
pointOut($('mark'), out);

/** An empty sheet opens ready to draw; a sheet that already holds marks opens ready to
 *  pick one. Runs after the log has been read, and never overrides a tool already chosen. */
let touched = false;
function openingTool() {
  if (touched) return;
  const next = marks.size ? 'select' : 'brush';
  state.tool = next;
  if (next === 'select') say('This sheet already has marks. Select is ready — pick Brush to draw.');
  refresh();
}

// The log in order: a put is a mark, a del takes it back, and the last fifty changes are
// what undo can reverse from the start. The same read serves a resync, which is the node
// saying its cache was rebuilt underneath us — and a reconnect, since a Lamport cursor
// cannot replay a synced event that arrived while this tab was away.
async function load() {
  marks.clear();
  history.order.clear();
  history.undoStack.length = 0;
  history.redoStack.length = 0;
  for await (const ev of pv.events({ tbl: 'stroke' })) history.apply(ev);
  openingTool();
  refresh();
}
/** The read, and what to do when it cannot happen: an unreachable node leaves the page
 *  working, since a mark drawn now is queued and the log is read again on reconnect. */
async function reload() {
  try {
    await load();
  } catch (error) {
    refresh();
    say('Could not read the sheet: ' + error.message + ' Marks you draw now are queued.');
  }
}
pv.on('resync', reload);
pv.on('online', reload);

document.addEventListener('pointerdown', () => { touched = true; }, { once: true, capture: true });
document.addEventListener('keydown', () => { touched = true; }, { once: true, capture: true });

await reload();
fit();
refresh();

if (window.matchMedia) {
  // The swatch boundaries are decided against the panel colour, which the scheme changes.
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => refresh());
}

// Help, the status line and a wrapping toolbar can all resize the sheet without a window
// resize, and the viewport's own size decides the fit scale.
new ResizeObserver(fit).observe(viewport);
window.addEventListener('resize', fit);

// A portrait phone sees the 4:3 sheet small; say why once, and let it be dismissed.
if (window.matchMedia('(max-width: 51.25rem) and (orientation: portrait)').matches) $('rotate-hint').hidden = false;
