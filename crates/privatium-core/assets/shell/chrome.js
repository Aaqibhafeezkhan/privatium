/* Project:  Privatium™  |  File: crates/privatium-core/assets/shell/chrome.js
 * Authors:  Gabriel Mongefranco (@gabrielmongefranco)
 * Created:  2026-10-04  |  Modified: 2026-10-04
 * Summary:  What the standard chrome does in the browser (spec/lua-api.md §4.1,
 *           spec/app-contract.md §5.2, spec/protocol.md §8.3.1). It writes the footer's
 *           status slot when the connection changes and when the outbox grows or drains,
 *           in the framework's own words, once per change and never on load. It listens for
 *           the browser's own online and offline events and for the events pv.js dispatches
 *           on the document, so one copy serves a page whether or not it loaded pv.js, and
 *           it never imports pv.js itself: an app's import and this one would be two
 *           instances otherwise. For an app with swap navigation it holds the scope rule,
 *           swaps the next page's main region into the frame, and moves focus to the new
 *           heading. Loaded as a module from every framed page; a document an app owns
 *           loads it with the inserted chrome. See main README.md for full license
 *           information.
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

/**
 * The framework's prefixes (spec/protocol.md §9.1). A path that begins with one is the
 * framework's in either mode, never a page of the app, and is reached by a fresh document.
 */
export const FRAMEWORK_PREFIXES = Object.freeze(['/settings', '/api', '/skills', '/static', '/ws']);

/** Beneath a mount, the data API and the app's own files: reached, never swapped in. */
const MOUNT_RESERVED = ['api', 'static'];

/** Whether `path` is `prefix` or lies beneath it. */
function beneath(path, prefix) {
  return path === prefix || path.startsWith(prefix + '/');
}

/**
 * The scope rule of spec/protocol.md §8.3.1: whether a request for `target` may swap its
 * page into a document whose app is mounted at `mount` (`/a/<slug>/` in host mode, `/`
 * in solo mode). `target` is a URL or a path, resolved against `base`, the document's
 * own URL. True only when the target is on the document's origin, beneath the mount,
 * outside every framework prefix, and outside the mount's own `api/` and `static/`.
 * Anything else needs a fresh document, under the destination's own policy. A missing
 * or malformed mount is never in scope.
 */
export function inScope(target, mount, base = globalThis.location?.href) {
  if (typeof mount !== 'string' || !mount.startsWith('/') || !mount.endsWith('/')) return false;
  let url, origin;
  try { origin = new URL(base).origin; url = new URL(target, base); } catch { return false; }
  if (url.origin !== origin) return false;
  const path = url.pathname;
  if (FRAMEWORK_PREFIXES.some(prefix => beneath(path, prefix))) return false;
  if (path !== mount.slice(0, -1) && !path.startsWith(mount)) return false;
  return !MOUNT_RESERVED.includes(path.slice(mount.length).split('/')[0]);
}

/** The frame's main region when its app swaps pages: `<main id="main" hx-boost="true">`. */
function swapMain(doc) {
  const main = doc?.getElementById?.('main');
  return main?.getAttribute?.('hx-boost') === 'true' ? main : null;
}

/**
 * Whether an htmx request, described by the `detail` of its event, is a page swap: boosted,
 * made from inside the frame's boosted main region, and in scope for the mount the body
 * names in `data-pv-mount`. client.js asks the same question before it bridges a request
 * over the channel, so both transports draw the line in the same place.
 */
export function isSwap(detail, doc = globalThis.document, base = globalThis.location?.href) {
  if (!detail?.boosted) return false;
  const main = swapMain(doc);
  // `requestConfig.elt` is the link or form; `elt` is whatever the event was fired on,
  // which for htmx:beforeSwap is the swap target, not the element that asked.
  const elt = detail.requestConfig?.elt ?? detail.elt;
  if (!main || !elt || !main.contains(elt)) return false;
  const path = detail.pathInfo?.finalRequestPath ?? detail.pathInfo?.requestPath;
  return inScope(path, doc.body?.dataset?.pvMount, base);
}

/**
 * After a swap, move focus where a fresh load of the page would have put it: the field the
 * page marks `autofocus`, or else its `<h1>`, or else the main region itself, so assistive
 * technology announces the page and the next Tab starts inside it. Returns the element
 * focused, or null when the page has no main region.
 */
export function focusPage(doc = globalThis.document) {
  const main = doc?.getElementById?.('main');
  if (!main) return null;
  const field = main.querySelector('[autofocus]');
  const target = field ?? main.querySelector('h1') ?? main;
  if (!field && !target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1');
  target.focus({ preventScroll: true });
  return target;
}

/** Whether a response body is a page with the frame's main region to take. */
function hasMain(body) {
  return typeof body === 'string' && /<main\b[^>]*\bid\s*=\s*["']?main["'\s>]/i.test(body);
}

/** The submit controls of a form, which a swap holds while the form's request is out. */
function submitControls(form) {
  return form?.tagName === 'FORM'
    ? [...form.querySelectorAll('button:not([type]), button[type="submit"], input[type="submit"], input[type="image"]')]
    : [];
}

/**
 * Leave the document for `detail`'s destination with a fresh one, as a link or form does
 * without htmx. A form is submitted natively, which fires no submit event, so htmx does
 * not take it back; the pressed button's name, value and form overrides are carried over,
 * as the browser would have sent them.
 */
function leave(detail, doc, win) {
  const config = detail.requestConfig ?? {};
  const destination = new URL(detail.pathInfo?.finalRequestPath ?? '', win.location.href).href;
  const form = config.elt?.closest?.('form');
  if ((config.verb ?? 'get') === 'get' || !form) { win.location.assign(destination); return; }
  const submitter = config.triggeringEvent?.submitter;
  if (submitter) {
    for (const [own, attribute] of [['formaction', 'action'], ['formmethod', 'method'], ['formenctype', 'enctype']]) {
      if (submitter.hasAttribute(own)) form.setAttribute(attribute, submitter.getAttribute(own));
    }
    if (submitter.name) {
      const field = doc.createElement('input');
      field.type = 'hidden'; field.name = submitter.name; field.value = submitter.value;
      form.append(field);
    }
  }
  // The prototype's method, because a control named `submit` shadows the form's own.
  win.HTMLFormElement.prototype.submit.call(form);
}

/**
 * Wire swap navigation on a framed page (spec/protocol.md §8.3.1). A boosted request that
 * is a swap holds its form's submit controls while it is out, then takes the response's
 * main region in place of the frame's, scrolls to the top, takes the response's title,
 * and focuses the new heading; the menu's app list arrives out of band with it. A
 * boosted request that is not a swap leaves for a fresh document. Over the channel
 * client.js makes that choice before the request is sent, so here only the swap itself
 * is done. `doc` and `win` are the document and window; a test passes its own. Returns
 * the state, for a test.
 */
export function installNavigation(doc = globalThis.document, win = globalThis) {
  const state = { swapping: false, form: null, held: [] };
  const release = () => {
    for (const control of state.held) control.disabled = false;
    state.form = null;
    state.held = [];
  };
  doc.addEventListener('htmx:beforeRequest', event => {
    const detail = event.detail;
    if (event.defaultPrevented || !detail?.boosted) return;
    if (isSwap(detail, doc, win.location?.href)) {
      release();
      state.form = detail.requestConfig?.elt ?? detail.elt;
      state.held = submitControls(state.form).filter(control => !control.disabled);
      for (const control of state.held) control.disabled = true;
      return;
    }
    if (win.__pv_channel || !doc.body?.dataset?.pvMount) return;
    event.preventDefault();
    leave(detail, doc, win);
  });
  for (const name of ['htmx:afterRequest', 'htmx:sendError', 'htmx:sendAbort']) {
    doc.addEventListener(name, event => { if (event.detail?.elt === state.form) release(); });
  }
  doc.addEventListener('htmx:beforeSwap', event => {
    const detail = event.detail;
    if (!isSwap(detail, doc, win.location?.href)) return;
    if (!hasMain(detail.serverResponse)) {
      // Not a page — a download, a bare error, nothing at all. Swapping it would empty
      // the main region, so the frame stays as it is and the browser shows it instead.
      detail.shouldSwap = false;
      if (detail.xhr?.status !== 204) {
        if ((detail.requestConfig?.verb ?? 'get') === 'get') win.location.assign(new URL(detail.pathInfo?.finalRequestPath ?? '', win.location.href).href);
        else say('The page could not be shown. Reload to see the current state.', doc);
      }
      return;
    }
    detail.target = swapMain(doc);
    detail.selectOverride = '#main';
    detail.swapOverride = 'outerHTML show:window:top';
    // A page the node answered with an error status is still a page, with its own
    // heading saying what went wrong, and says so better than a link that does nothing.
    detail.shouldSwap = true;
    detail.isError = false;
    state.swapping = true;
  });
  doc.addEventListener('htmx:afterSettle', () => {
    if (!state.swapping) return;
    state.swapping = false;
    focusPage(doc);
  });
  return state;
}

if (globalThis.document?.getElementById && !globalThis.document.body?.hasAttribute('data-pv-bootstrap')) {
  install();
  installNavigation();
}
