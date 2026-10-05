// Project:  Privatium™  |  File: crates/privatium-core/assets/shell/channel.js
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-09-05
// Summary:  Origin-confined encrypted requests and streaming responses (§8.3).
//           See main README.md for full license information.

import { clientHandshake, decode64 } from './session.js';
import { sha256 } from './vendor/noble/hashes/sha2.js';
import { bytesToHex } from './vendor/noble/hashes/utils.js';

const utf8 = new TextEncoder(), text = new TextDecoder('utf-8', { fatal: true });
const fail = () => new Error('Encrypted channel failed. Reconnect and check before submitting again.');
const fields = new Set(['id', 'kind', 'method', 'path', 'status', 'headers', 'navigation', 'handoff']);
const reference = value => typeof value === 'string' && /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(value);

/** Encode a protocol header and raw body into one plaintext frame. */
export function encode(head, payload = new Uint8Array()) {
  const metadata = utf8.encode(JSON.stringify(head));
  if (metadata.length > 65536) throw fail();
  const bytes = new Uint8Array(4 + metadata.length + payload.length);
  new DataView(bytes.buffer).setUint32(0, metadata.length, false);
  bytes.set(metadata, 4); bytes.set(payload, 4 + metadata.length); return bytes;
}

/** Decode bounded metadata, refusing unknown fields and unsafe request IDs. */
export function decode(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 4) throw fail();
  const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0, false);
  if (!length || length > 65536 || length > bytes.length - 4) throw fail();
  const head = JSON.parse(text.decode(bytes.subarray(4, 4 + length)));
  if (!head || typeof head !== 'object' || Array.isArray(head) ||
      Object.keys(head).some(key => !fields.has(key)) || !Number.isSafeInteger(head.id) || head.id < 0 ||
      !['req', 'res', 'chunk', 'end', 'cancel', 'resume', 'release'].includes(head.kind)) throw fail();
  return { head, payload: bytes.subarray(4 + length) };
}

/** The current origin's sole WebSocket endpoint; never probes another address. */
export function endpoint() { return `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws`; }

/** Resolve a path only within this document's origin. Throws before any network I/O. */
export function localUrl(path) {
  const url = new URL(path, location.href);
  if (!['http:', 'https:'].includes(url.protocol) || url.origin !== location.origin || url.username || url.password) throw new Error('Request must stay on this origin.');
  return url;
}

/** Own one session, its counters, and bounded response streams. Never retries a request. */
export class Transport {
  #socket; #crypto; #next = 0; #pending = new Map(); #cancelled = new Set(); #closed = false; #limit;
  /** A lower ID budget permits boundary tests; the protocol resource ceiling cannot rise. */
  constructor(socket, crypto, limit = 65536) {
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 65536) throw fail();
    this.#limit = limit;
    this.#socket = socket; this.#crypto = crypto; socket.binaryType = 'arraybuffer';
    socket.addEventListener('message', event => {
      try {
        if (!(event.data instanceof ArrayBuffer) || event.data.byteLength > 131092) throw fail();
        this.#receive(decode(crypto.receive.open(new Uint8Array(event.data))));
        if (crypto.receive.closed) this.close();
      } catch { this.close(); }
    });
    // A socket the page did not close is a lost connection: said once, for the status
    // slot (spec/app-contract.md §5.2). A close this side asked for says nothing.
    const lost = () => { const unexpected = !this.#closed; this.close(); if (unexpected) globalThis.dispatchEvent?.(new CustomEvent('pv:channel-closed')); };
    socket.addEventListener('close', lost);
    socket.addEventListener('error', lost);
  }
  get closed() { return this.#closed; }
  #send(head, payload) {
    if (this.#closed || this.#socket.readyState !== 1 || this.#socket.bufferedAmount > 1048576) throw fail();
    this.#socket.send(this.#crypto.send.seal(encode(head, payload)));
    if (this.#crypto.send.closed) this.close();
  }
  #cancel(id) {
    const pending = this.#pending.get(id);
    if (!pending) return;
    this.#pending.delete(id); this.#cancelled.add(id); pending.cleanup();
    try { this.#send({ id, kind: 'cancel' }); } catch { this.close(); }
    pending.reject(fail()); pending.controller?.error(fail());
  }
  #request(head, payload, signal) {
    if (this.#next >= this.#limit) this.close();
    if (this.#closed || this.#pending.size >= 64) return Promise.reject(fail());
    const id = ++this.#next;
    return new Promise((resolve, reject) => {
      const abort = () => this.#cancel(id);
      this.#pending.set(id, { resolve, reject, head, controller: null,
        cleanup: () => signal?.removeEventListener('abort', abort) });
      signal?.addEventListener('abort', abort, { once: true });
      try { this.#send({ id, ...head }, payload); if (signal?.aborted) abort(); }
      catch { this.close(); }
    });
  }
  /** Fetch an origin-local resource through this session, exposing its body as a stream. */
  async fetch(path, init = {}) {
    const url = localUrl(path), method = (init.method || 'GET').toUpperCase();
    const request = new Request(url, { ...init, method });
    const payload = request.body ? new Uint8Array(await request.arrayBuffer()) : new Uint8Array();
    const head = { kind: 'req', method, path: url.pathname + url.search, headers: Object.fromEntries(request.headers) };
    if (init.navigation) {
      if (['GET', 'HEAD'].includes(method)) throw fail();
      head.navigation = true;
    }
    return this.#request(head, payload, init.signal);
  }
  /** Consume an already-produced response; carries no original request body. */
  resume(handoff) { if (!reference(handoff)) return Promise.reject(fail()); return this.#request({ kind: 'resume', handoff }); }
  /** Drop a retained response without altering the operation that produced it. */
  release(handoff) { if (!reference(handoff)) return Promise.reject(fail()); return this.#request({ kind: 'release', handoff }); }
  #receive({ head, payload }) {
    if (this.#cancelled.has(head.id)) return;
    const pending = this.#pending.get(head.id);
    if (!pending || Object.keys(head).some(key => !['id', 'kind', 'status', 'headers', 'handoff'].includes(key))) throw fail();
    if (head.kind === 'res') {
      if (pending.started || payload.length || !Number.isInteger(head.status) || head.status < 200 || head.status > 599 ||
          !head.headers || typeof head.headers !== 'object' || Array.isArray(head.headers) ||
          Object.values(head.headers).some(value => typeof value !== 'string') ||
          (head.handoff !== undefined && (!pending.head.navigation || !reference(head.handoff)))) throw fail();
      pending.started = true;
      pending.empty = pending.head.method === 'HEAD' || [204, 205, 304].includes(head.status) || head.handoff !== undefined;
      const stream = pending.empty ? null : new ReadableStream({
        start: controller => { pending.controller = controller; }, cancel: () => this.#cancel(head.id),
      }, { highWaterMark: 1048576, size: chunk => chunk.byteLength });
      const response = new Response(stream, { status: head.status, headers: head.headers });
      if (head.handoff) Object.defineProperty(response, 'handoff', { value: head.handoff });
      pending.resolve(response);
    } else if (head.kind === 'chunk' && pending.started && !pending.empty) {
      if (Object.keys(head).length !== 2 || payload.length > 65536) throw fail();
      if (pending.controller.desiredSize < payload.length) { this.#cancel(head.id); return; }
      pending.controller.enqueue(payload);
    } else if (head.kind === 'end' && pending.started && !payload.length && Object.keys(head).length === 2) {
      pending.controller?.close(); pending.cleanup(); this.#pending.delete(head.id);
    } else throw fail();
  }
  /** Permanently close the session and fail every outstanding stream without retrying. */
  close() {
    if (this.#closed) return;
    this.#closed = true; this.#crypto.send.close(); this.#crypto.receive.close();
    for (const pending of this.#pending.values()) { pending.cleanup(); pending.reject(fail()); pending.controller?.error(fail()); }
    this.#pending.clear(); this.#socket.close();
  }
}

let current, opening;

/** Dispose the current connection without opening another during page teardown. */
export function closeChannel() { current?.close(); }

/** Public fingerprints for a refusal screen, with no peer-controlled error text (§8.1). */
export function identityRefusal(record, hello) {
  const error = new Error('Cannot establish session: pinned identity verification failed. Explicitly re-pair to proceed.');
  const fingerprint = value => { try { return bytesToHex(sha256(decode64(value, 32))); } catch { return 'Unavailable'; } };
  let presented;
  try {
    if (typeof hello !== 'string' || utf8.encode(hello).length > 8192) throw fail();
    const reply = JSON.parse(hello);
    const cert = JSON.parse(text.decode(decode64(reply.cert)));
    presented = cert.node_pub;
  } catch { /* An invalid certificate has no usable public key to fingerprint. */ }
  error.identity = { node: /^[0-9a-hjkmnp-tv-z]{8}$/.test(record.node.id) ? record.node.id : 'Unknown node',
    pinned: fingerprint(record.node.ed25519), presented: fingerprint(presented) };
  return error;
}

/** Authenticate a fresh session using the device record produced by pairing (§7.6). */
export async function channel() {
  if (current && !current.closed) return current;
  if (opening) return opening;
  opening = (async () => {
    let record;
    try { record = JSON.parse(localStorage.getItem('pv:device')); } catch { throw new Error('Cannot read pairing. Enable browser storage and pair this device.'); }
    if (record?.v !== 1) throw new Error('This browser is not paired. Pair this device before opening an app.');
    const secret = decode64(record.x25519.secret, 32);
    let handshake;
    try { handshake = clientHandshake(record.dev, secret, { id: record.node.id, cluster: decode64(record.cluster.pub, 32), x25519: decode64(record.node.x25519, 32) }); }
    finally { secret.fill(0); }
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(endpoint()); socket.binaryType = 'arraybuffer';
      const timer = setTimeout(refuse, 10000);
      function cleanup() { clearTimeout(timer); socket.removeEventListener('message', hello); socket.removeEventListener('error', refuse); socket.removeEventListener('close', refuse); }
      function refuse() { cleanup(); handshake.close(); socket.close(); reject(fail()); }
      function hello(event) {
        try {
          const crypto = handshake.finish(event.data); cleanup();
          current = new Transport(socket, crypto); socket.send(crypto.confirm); resolve(current);
        } catch (error) {
          cleanup(); handshake.close(); socket.close();
          if (error.message?.includes('pinned identity')) {
            error = identityRefusal(record, event.data);
            globalThis.dispatchEvent?.(new CustomEvent('pv:identity-refused', { detail: error }));
          }
          reject(error);
        }
      }
      socket.addEventListener('open', () => socket.send(handshake.hello), { once: true });
      socket.addEventListener('message', hello); socket.addEventListener('error', refuse); socket.addEventListener('close', refuse);
    });
  })();
  try { return await opening; } finally { opening = null; }
}

/** Fetch on the authenticated current-origin session. A failed request is never replayed. */
export async function channelFetch(path, init) { return (await channel()).fetch(path, init); }

/** EventSource-shaped reader over an encrypted response; caller controls reconnection. */
export function eventSource(path, fetcher = channelFetch) {
  const source = new EventTarget(), abort = new AbortController();
  let reader, closed = false;
  source.close = () => { closed = true; abort.abort(); reader?.cancel().catch(() => {}); };
  const emit = event => { source.dispatchEvent(event); source['on' + event.type]?.(event); };
  (async () => {
    try {
      const response = await fetcher(path, { signal: abort.signal, headers: { accept: 'text/event-stream' } });
      if (!response.ok || !response.headers.get('content-type')?.startsWith('text/event-stream')) throw fail();
      reader = response.body.getReader(); if (closed) { await reader.cancel(); return; }
      emit(new Event('open'));
      const decoder = new TextDecoder(); let line = '', data = [], kind = '', cr = false, size = 0;
      function consume() {
        if (!line) { if (data.length) emit(new MessageEvent(kind || 'message', { data: data.join('\n') })); data = []; kind = ''; size = 0; }
        else if (!line.startsWith(':')) {
          const at = line.indexOf(':'), field = at < 0 ? line : line.slice(0, at);
          let value = at < 0 ? '' : line.slice(at + 1); if (value.startsWith(' ')) value = value.slice(1);
          if (field === 'data') data.push(value); else if (field === 'event') kind = value;
        }
        line = '';
      }
      while (!closed) {
        const { done, value } = await reader.read(); if (done) throw fail();
        for (const char of decoder.decode(value, { stream: true })) {
          if (cr && char === '\n') { cr = false; continue; }
          cr = char === '\r';
          if (char === '\r' || char === '\n') consume(); else { line += char; if (++size > 1048576) throw fail(); }
        }
      }
    } catch { if (!closed) { source.close(); emit(new Event('error')); } }
  })();
  return source;
}
