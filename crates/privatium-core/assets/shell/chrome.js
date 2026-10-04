/* Project:  Privatium™  |  File: crates/privatium-core/assets/shell/chrome.js
 * Authors:  Gabriel Mongefranco (@gabrielmongefranco)
 * Created:  2026-10-04  |  Modified: 2026-10-04
 * Summary:  What the standard chrome does in the browser (spec/lua-api.md §4.1,
 *           spec/app-contract.md §5.2): it writes the footer's status slot when the
 *           connection changes and when the outbox grows or drains, in the framework's own
 *           words, once per change and never on load. It listens for the browser's own
 *           online and offline events and for the events pv.js dispatches on the document,
 *           so one copy serves a page whether or not it loaded pv.js, and it never imports
 *           pv.js itself: an app's import and this one would be two instances otherwise.
 *           Loaded as a module from every framed page; a document an app owns loads it
 *           with the inserted chrome. See main README.md for full license information.
 */

/** The slot's text when the node cannot be reached. Technical detail never goes here. */
export const OFFLINE = 'Offline. Changes are saved on this device and sent later.';
/** The slot's text when the node is reachable again and nothing waits. */
export const CONNECTED = 'Connected again.';
/** The slot's text when the last queued change has been sent. */
export const SENT = 'All changes sent.';

/** The slot's text while `count` queued changes wait for the node. */
export function waitingWords(count) {
  return count === 1 ? '1 change waiting to send.' : `${count} changes waiting to send.`;
}

/**
 * Write `text` to the status slot, replacing what was there. The slot is `#pv-status`,
 * a `role="status"` region the frame renders empty in the footer, so assistive
 * technology announces each write without moving focus. Returns false when the page
 * has no slot, which a document that declined the chrome does not.
 */
export function say(text, doc = globalThis.document) {
  const slot = doc?.getElementById?.('pv-status');
  if (!slot) return false;
  slot.textContent = text;
  return true;
}

/**
 * Wire the status slot to the connection and the outbox. `doc` is the document and
 * `win` the window; a test passes its own. Each state is written once when it is
 * entered: going offline, coming back, the queue growing to a new length, and the
 * queue draining. A page that loads while the browser already reports itself offline
 * shows the offline text at once, which is the state, not a change. Returns the
 * controller so a test can drive the states directly.
 */
export function install(doc = globalThis.document, win = globalThis) {
  const state = { offline: false, waiting: 0 };
  const offline = () => {
    if (state.offline) return;
    state.offline = true;
    say(OFFLINE, doc);
  };
  const online = () => {
    if (!state.offline) return;
    state.offline = false;
    say(state.waiting > 0 ? waitingWords(state.waiting) : CONNECTED, doc);
  };
  const outbox = count => {
    const waiting = Math.max(0, Number(count) || 0);
    if (waiting === state.waiting) return;
    const had = state.waiting;
    state.waiting = waiting;
    if (state.offline) return;                 // the offline text already says changes wait
    if (waiting > 0) say(waitingWords(waiting), doc);
    else if (had > 0) say(SENT, doc);
  };
  win?.addEventListener?.('offline', offline);
  win?.addEventListener?.('online', online);
  doc?.addEventListener?.('pv:offline', offline);
  doc?.addEventListener?.('pv:online', online);
  doc?.addEventListener?.('pv:outbox', event => outbox(event.detail?.waiting));
  doc?.addEventListener?.('pv:status', event => say(String(event.detail?.text ?? ''), doc));
  if (win?.navigator?.onLine === false) offline();
  return { offline, online, outbox, state };
}

if (globalThis.document?.getElementById) install();
