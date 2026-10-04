// Project:  Privatium™  |  File: crates/privatium-core/assets/shell/client.js
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  Bootstrap rendering, the pairing screen (§7.2), the refusal screen (§8.1),
//           encrypted HTMX and fresh-document navigation (§8.3).
//           See main README.md for full license information.

import { channel, channelFetch, closeChannel, eventSource, localUrl } from './channel.js';
import { pair, parseCode } from './pair.js';
import { base64, decode64 } from './session.js';
import { sha256 } from './vendor/noble/hashes/sha2.js';

export { channel };
/** Fetch on the current authenticated origin without a plaintext fallback. */
export const fetchThroughChannel = channelFetch;
const HANDOFF = 'pv:response';
const uncertainty = () => new Error('The previous response is unavailable. The operation may have completed; check before submitting again.');

/** Save only a response reference and its destination, never form data or response HTML. */
export function saveHandoff(handoff, path, node) {
  if (!/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(handoff)) throw uncertainty();
  const url = localUrl(path);
  sessionStorage.setItem(HANDOFF, JSON.stringify({ handoff, path: url.pathname + url.search, node }));
}

/** Remove the reference before attaching; an interrupted attachment is never retried. */
export function takeHandoff(path, node) {
  const raw = sessionStorage.getItem(HANDOFF);
  if (raw === null) return null;
  sessionStorage.removeItem(HANDOFF);
  let value;
  try { value = JSON.parse(raw); } catch { throw uncertainty(); }
  if (value?.path !== path || value.node !== node || !/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(value.handoff)) throw uncertainty();
  return value.handoff;
}

/**
 * Replace only an HTMX request's send operation. HTMX retains parameter encoding,
 * validation, indicators, response headers, swapping and completion callbacks.
 */
export function bridgeHtmx(detail, fetcher = channelFetch) {
  const { xhr, requestConfig, pathInfo } = detail;
  const abort = new AbortController();
  xhr.abort = () => { abort.abort(); xhr.onabort?.(); };
  xhr.send = async body => {
    let timer;
    try {
      if (xhr.timeout) timer = setTimeout(() => { abort.abort(); xhr.ontimeout?.(); }, xhr.timeout);
      let response = await fetcher(pathInfo.finalRequestPath, { method: requestConfig.verb.toUpperCase(),
        headers: requestConfig.headers, body: body ?? undefined, signal: abort.signal });
      let destination = localUrl(pathInfo.finalRequestPath);
      for (let count = 0; response.status >= 300 && response.status < 400 && response.headers.has('location'); count++) {
        if (count >= 10 || ![301, 302, 303].includes(response.status)) throw uncertainty();
        destination = localUrl(new URL(response.headers.get('location'), destination));
        await response.body?.cancel(); response = await fetcher(destination.href, { headers: { 'HX-Request': 'true' }, signal: abort.signal });
      }
      let html = await response.text();
      if (globalThis.DOMParser) html = await pinMarkup(html, false, destination.href);
      for (const [name, value] of Object.entries({ status: response.status, statusText: response.statusText,
        response: html, responseText: html, responseURL: destination.href, readyState: 4 })) {
        Object.defineProperty(xhr, name, { configurable: true, value });
      }
      xhr.getResponseHeader = name => response.headers.get(name);
      xhr.getAllResponseHeaders = () => [...response.headers].map(([k, v]) => `${k}: ${v}\r\n`).join('');
      if (!abort.signal.aborted) xhr.onload?.();
    } catch { if (!abort.signal.aborted) xhr.onerror?.(); }
    finally { clearTimeout(timer); }
  };
}

/** Hash a same-origin resource through the channel, or require an authenticated remote hash. */
export async function resourceIntegrity(url, provided, fetcher = channelFetch) {
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('Cannot verify this resource URL. Check the app.');
  if (url.origin !== location.origin) {
    try {
      for (const token of (provided || '').trim().split(/\s+/)) {
        const match = token.match(/^sha(256|384|512)-(.+)$/);
        if (!match) throw new Error();
        decode64(match[2], Number(match[1]) / 8);
      }
    } catch { throw new Error('Cannot verify a remote app resource. The app must provide a valid integrity hash.'); }
    return provided;
  }
  const response = await fetcher(url.href);
  if (!response.ok) throw new Error('Cannot verify an app resource. Reload after the owner checks the app.');
  const hash = sha256.create();
  if (response.body) {
    const reader = response.body.getReader();
    try { for (;;) { const { done, value } = await reader.read(); if (done) break; hash.update(value); } }
    finally { reader.releaseLock(); }
  }
  return 'sha256-' + base64(hash.digest());
}

async function pinMarkup(html, full, base) {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  for (const element of doc.querySelectorAll('script[src], link[rel~="stylesheet"][href]')) {
    const attribute = element.tagName === 'SCRIPT' ? 'src' : 'href';
    const url = new URL(element.getAttribute(attribute), base);
    element.setAttribute('integrity', await resourceIntegrity(url, element.getAttribute('integrity')));
    element.setAttribute(attribute, url.href);
    if (url.origin !== location.origin) element.setAttribute('crossorigin', 'anonymous');
  }
  if (!full) return doc.head.innerHTML + doc.body.innerHTML;
  const config = doc.querySelector('meta[name="htmx-config"]') || doc.createElement('meta');
  const settings = config.content ? JSON.parse(config.content) : {};
  config.name = 'htmx-config';
  config.content = JSON.stringify({ ...settings, historyCacheSize: 0, refreshOnHistoryMiss: true }); doc.head.prepend(config);
  return '<!doctype html>' + doc.documentElement.outerHTML;
}

/**
 * Replace the document with a failure screen. A pinned-identity refusal (§8.1) is the
 * full-page form: the space's name and ID, both key fingerprints, no way past it, and
 * one action — forget this pairing and pair again. `doc` and `storage` are the
 * document and localStorage, replaceable by a test.
 */
export function showFailure(error, doc = document, storage = globalThis.localStorage) {
  const main = doc.createElement('main'); main.id = 'main'; main.tabIndex = -1;
  const title = doc.createElement('h1'); title.textContent = 'Cannot open this app';
  const message = doc.createElement('p'); message.setAttribute('role', 'alert');
  message.textContent = error.message || 'Connection failed. Reconnect and check before submitting again.';
  main.append(title, message);
  if (error.message?.includes('pinned identity')) {
    title.textContent = 'Space identity could not be verified';
    const text = doc.createElement('p'); text.textContent = 'Connection stopped. There is no override and no way past this screen. Check the space with its owner before forgetting this pairing.';
    const forget = doc.createElement('button'); forget.type = 'button'; forget.className = 'pv-btn pv-btn-danger'; forget.textContent = 'Forget this pairing and pair again';
    forget.addEventListener('click', () => {
      try { storage.removeItem('pv:device'); sessionStorage.removeItem(HANDOFF); location.reload(); }
      catch { message.textContent = 'Cannot clear pairing. Clear this site’s stored data in your browser settings.'; }
    });
    main.append(text, forget);
    if (error.identity) {
      const list = doc.createElement('dl'); list.className = 'pv-card';
      const name = doc.body?.dataset?.name;
      const rows = [['Space', name && name !== error.identity.node ? `${name} (${error.identity.node})` : error.identity.node],
        ['Pinned key fingerprint', error.identity.pinned], ['Presented key fingerprint', error.identity.presented]];
      for (const [label, value] of rows) {
        const term = doc.createElement('dt'), description = doc.createElement('dd');
        term.textContent = label; description.textContent = value; list.append(term, description);
      }
      main.insertBefore(list, forget);
    }
  }
  doc.body.replaceChildren(main); main.focus();
}

/** The sixteen-bit code four pad presses spell, big-endian nibbles (§7.2). */
function padCode(presses) {
  return (presses[0] << 12) | (presses[1] << 8) | (presses[2] << 4) | presses[3];
}

/** What a browser device suggests as its label: the platform, for the owner to rename. */
function suggestedLabel() {
  const agent = globalThis.navigator?.userAgent || '';
  for (const [needle, name] of [['iPhone', 'iPhone'], ['iPad', 'iPad'], ['Android', 'Android phone'], ['Windows', 'Windows browser'], ['Mac', 'Mac browser'], ['Linux', 'Linux browser']]) {
    if (agent.includes(needle)) return name;
  }
  return 'Browser';
}

/**
 * Wire the pairing screen the bootstrap carries (§7.2): the sixteen pad keys, the two
 * undo buttons, the word field and the form. Four presses on the pad or two words in
 * the field are the same sixteen bits; the Pair button sends whichever is complete.
 * The three outcomes — paired, wrong code, closed — are said in the status region.
 * `options.pair(code, label)` performs the exchange (the real one opens /ws/pair);
 * `options.doc` is the document. Returns the controller, for a test.
 */
export function pairingScreen(options = {}) {
  const doc = options.doc || document;
  const section = doc.getElementById('pv-pair');
  const chosen = doc.getElementById('pv-chosen'), status = doc.getElementById('pv-pair-status');
  const words = doc.getElementById('pv-words'), label = doc.getElementById('pv-label');
  const form = doc.getElementById('pv-pair-form'), submit = doc.getElementById('pv-pair-submit');
  const keys = [...doc.querySelectorAll('.pv-pad-key')];
  const presses = [];
  const say = text => { status.textContent = text; };
  const show = () => {
    const labels = presses.map(index => keys[index]?.querySelector('.pv-glyph-label')?.textContent || String(index));
    chosen.textContent = labels.length ? `${labels.join(', ')} (${labels.length} of 4)` : 'none yet';
  };
  const perform = options.pair || (async (code, name) => {
    const socket = new WebSocket(`${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws/pair`);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('cannot pair: the space did not answer; check that it is running and that you are on its network')), { once: true }); });
    return pair(socket, code, { label: name, userAgent: globalThis.navigator?.userAgent });
  });
  const controller = {
    press(index) { if (presses.length < 4) { presses.push(index); show(); } },
    undo() { presses.pop(); show(); },
    clear() { presses.length = 0; show(); },
    /** The code the screen would send: the pad when it holds four presses, else the field. */
    code() {
      if (presses.length === 4) return padCode(presses);
      const typed = words.value.trim();
      return typed ? parseCode(typed) : null;
    },
    async submit() {
      let code;
      try { code = controller.code(); } catch (error) { say(error.message); return false; }
      if (code === null) { say('Tap the four emoji shown on the space, or type its two words, then press Pair.'); return false; }
      submit.disabled = true; say('Pairing…');
      try {
        await perform(code, label.value.trim() || suggestedLabel());
        say('Paired. Opening the page…');
        options.onPaired ? options.onPaired() : location.reload();
        return true;
      } catch (error) {
        const text = error.message || '';
        if (text.includes('pairing is closed')) say('Pairing is closed on the space. Open it there — Settings › Devices › Open pairing, or privatium pair — and press Pair again.');
        else if (text.includes('did not match')) say('The code did not match. Check the code on the space and try again; a code allows five attempts before a new one is shown there.');
        else if (text.includes('too many attempts')) say('The space refused this attempt: too many tries, or a new code is on its screen now. Read the code there again and press Pair in a moment.');
        else say(text.replace(/\bnode\b/g, 'space') || 'Could not pair. Try again.');
        submit.disabled = false;
        return false;
      }
    },
  };
  keys.forEach((key, index) => key.addEventListener('click', () => controller.press(Number(key.dataset.glyph ?? index))));
  doc.getElementById('pv-undo').addEventListener('click', controller.undo);
  doc.getElementById('pv-clear').addEventListener('click', controller.clear);
  form.addEventListener('submit', event => { event.preventDefault(); controller.submit(); });
  if (!label.value) label.value = suggestedLabel();
  doc.getElementById('pv-connecting')?.setAttribute('hidden', '');
  // The bootstrap keeps its own text out of sight for a moment so a paired device sees
  // no interstitial between pages; a device that has to pair needs the heading now.
  for (const quiet of doc.querySelectorAll('.pv-quiet')) quiet.classList.remove('pv-quiet');
  section.hidden = false;
  say('This device is not paired yet.');
  return controller;
}

function formFailure(error) {
  if (error.identity) { showFailure(error); return; }
  let notice = document.getElementById('pv-channel-error');
  if (!notice) {
    notice = document.createElement('p'); notice.id = 'pv-channel-error'; notice.tabIndex = -1;
    notice.setAttribute('role', 'alert'); (document.querySelector('main') || document.body).prepend(notice);
  }
  notice.textContent = error.message || 'Request failed. Check whether it completed before submitting again.';
  notice.focus();
}

function installNavigation(node) {
  let leaving = false;
  window.addEventListener('pv:identity-refused', event => showFailure(event.detail));
  window.addEventListener('pagehide', () => { leaving = true; closeChannel(); }, { once: true });
  // The channel closed under the page: the footer's status slot says so, in the frame's
  // words, unless the page is on its way out anyway.
  window.addEventListener('pv:channel-closed', () => { if (!leaving) { const slot = document.getElementById('pv-status'); if (slot) slot.textContent = 'Connection lost.'; } });
  window.addEventListener('pageshow', event => { if (event.persisted) location.reload(); });
  document.addEventListener('htmx:beforeRequest', event => {
    if (event.detail.boosted) {
      event.preventDefault();
      const config = event.detail.requestConfig;
      if (config.verb === 'get') location.assign(localUrl(event.detail.pathInfo.finalRequestPath).href);
      else submit(config.elt.closest('form'), config.triggeringEvent?.submitter, node).catch(formFailure);
    } else bridgeHtmx(event.detail);
  });
  document.addEventListener('submit', event => {
    const form = event.target;
    if (event.defaultPrevented) return;
    event.preventDefault(); submit(form, event.submitter, node).catch(formFailure);
  });
  document.addEventListener('htmx:sendError', () => formFailure(uncertainty()));
}

/**
 * Where a form goes, how, and in what encoding, read from the markup's attributes
 * alone. The `action`, `method` and `enctype` properties are not read, because a
 * control named `action` (or `method`, `enctype`) shadows the property of the same name
 * in WebKit, and the property then yields the element instead of the attribute. A
 * missing `action` means the document's own URL, as the HTML form submission
 * algorithm has it. `base` is the URL a relative action resolves against.
 */
export function formRequest(form, button, base = document.baseURI) {
  const action = button?.hasAttribute('formaction') ? button.getAttribute('formaction') : form.getAttribute('action');
  const destination = localUrl(new URL(action ?? '', base).href);
  const method = (button?.getAttribute('formmethod') || form.getAttribute('method') || 'get').toUpperCase();
  const encoding = button?.getAttribute('formenctype') || form.getAttribute('enctype') || 'application/x-www-form-urlencoded';
  return { destination, method, encoding };
}

async function submit(form, button, node) {
  if (!form) throw new Error('Cannot submit this action. Use a form with an accessible submit button.');
  const { destination, method, encoding } = formRequest(form, button);
  const values = new FormData(form, button);
  if (method === 'GET') {
    destination.search = new URLSearchParams([...values].map(([key, value]) => [key, typeof value === 'string' ? value : value.name]));
    location.assign(destination.href); return;
  }
  // Check storage before dispatch; a failed handoff must never replay a form (§8.3.1).
  try { sessionStorage.setItem('pv:storage-check', ''); sessionStorage.removeItem('pv:storage-check'); }
  catch { throw new Error('Form was not sent. Enable per-tab browser storage before submitting.'); }
  const body = encoding === 'multipart/form-data' ? values : new URLSearchParams([...values].map(([key, value]) => [key, typeof value === 'string' ? value : value.name]));
  const connection = await channel();
  const response = await connection.fetch(destination.href, { method, body, navigation: true });
  if ([301, 302, 303].includes(response.status) && response.headers.has('location')) {
    await response.body?.cancel(); location.assign(localUrl(new URL(response.headers.get('location'), destination)).href); return;
  }
  if (!response.handoff) {
    await response.body?.cancel();
    if (response.status === 429) throw new Error('Form was not sent. Finish another navigation before submitting.');
    throw uncertainty();
  }
  try { saveHandoff(response.handoff, destination.href, node); }
  catch { await connection.release(response.handoff); throw uncertainty(); }
  location.assign(destination.href);
}

async function start() {
  const { path, node } = document.body.dataset;
  globalThis.__pv_channel = Object.freeze({ fetch: channelFetch, eventSource });
  // No pairing in storage means the pairing screen, not a refusal (§7.6): a browser
  // that never paired, or one whose storage was cleared, pairs again here.
  let record = null;
  try { record = JSON.parse(localStorage.getItem('pv:device')); } catch { /* storage unavailable or unreadable: pair again */ }
  if (record?.v !== 1) { pairingScreen(); return; }
  const connection = await channel();
  const handoff = takeHandoff(path, node);
  const response = handoff ? await connection.resume(handoff) : await connection.fetch(path, { headers: { accept: 'text/html' } });
  if ([301, 302, 303, 307, 308].includes(response.status) && response.headers.has('location')) {
    await response.body?.cancel(); location.replace(localUrl(new URL(response.headers.get('location'), location.href)).href); return;
  }
  if (!response.headers.get('content-type')?.startsWith('text/html')) {
    const main = document.createElement('main'), heading = document.createElement('h1'), pre = document.createElement('pre');
    if (response.headers.get('content-type') === 'application/zip') {
      const url = URL.createObjectURL(await response.blob());
      const link = document.createElement('a'); link.href = url; link.download = 'privatium-skills.zip';
      link.textContent = 'Download skill bundle'; link.className = 'pv-btn'; heading.textContent = 'Skill bundle ready';
      main.append(heading, link); document.body.replaceChildren(main);
      window.addEventListener('pagehide', () => URL.revokeObjectURL(url), { once: true }); return;
    }
    heading.textContent = response.ok ? 'Response' : 'Request could not be completed';
    pre.textContent = await response.text(); main.append(heading, pre); document.body.replaceChildren(main); return;
  }
  const html = await pinMarkup(await response.text(), true, location.href);
  // The bootstrap is already a fresh navigation with the destination app's policy.
  // Parsing once preserves script ordering and the app's DOMContentLoaded lifecycle.
  document.open(); document.write(html); installNavigation(node); document.close();
}

if (globalThis.document?.body?.hasAttribute('data-pv-bootstrap')) start().catch(showFailure);
