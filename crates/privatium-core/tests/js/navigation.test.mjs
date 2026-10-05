// Project:  Privatium™  |  File: crates/privatium-core/tests/js/navigation.test.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  Encrypted HTMX dispatch, metadata-only document handoffs, and the scope rule
//           that decides when a boosted request swaps a page in place (§8.3.1).
//           See main README.md for full license information.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bridgeHtmx, formRequest, saveHandoff, takeHandoff, resourceIntegrity } from '../../assets/shell/client.js';
import { createHash } from 'node:crypto';
import { storage, framedPage, boosted } from './harness.mjs';
import { inScope, isSwap, installNavigation } from '../../assets/shell/chrome.js';

test('test_spec_8_3_integrity_uses_authenticated_bytes_and_refuses_unpinned_remote_code', async () => {
  globalThis.location = { origin: 'http://192.0.2.1' };
  const bytes = 'synthetic source', expected = 'sha256-' + createHash('sha256').update(bytes).digest('base64');
  let fetched = 0;
  const fetcher = async () => { fetched++; return new Response(bytes); };
  assert.equal(await resourceIntegrity(new URL('http://192.0.2.1/static/app.js'), 'untrusted hash', fetcher), expected);
  assert.equal(fetched, 1);
  assert.equal(await resourceIntegrity(new URL('http://192.0.2.1/a/hello/static/hello.css'), '', fetcher), expected);
  assert.equal(fetched, 2);
  assert.equal(await resourceIntegrity(new URL('https://example.invalid/app.js'), expected, fetcher), expected);
  await assert.rejects(resourceIntegrity(new URL('https://example.invalid/app.js'), '', fetcher));
  await assert.rejects(resourceIntegrity(new URL('https://example.invalid/app.js'), 'sha256-a', fetcher));
  await assert.rejects(resourceIntegrity(new URL('javascript:alert(1)'), expected, fetcher));
  assert.equal(fetched, 2);
});

test('test_spec_8_3_1_browser_stores_only_reference_and_consumes_before_attach', () => {
  globalThis.sessionStorage = storage();
  globalThis.location = { href: 'http://192.0.2.1/a/hello/', origin: 'http://192.0.2.1' };
  const id = '01J00000000000000000000000';
  saveHandoff(id, '/a/hello/name', 'synthetic');
  assert.deepEqual([...sessionStorage.map.values()].map(JSON.parse), [{ handoff: id, path: '/a/hello/name', node: 'synthetic' }]);
  assert.equal(takeHandoff('/a/hello/name', 'synthetic'), id);
  assert.equal(sessionStorage.length, 0);
  assert.equal(takeHandoff('/a/hello/name', 'synthetic'), null);
  saveHandoff(id, '/a/hello/name', 'synthetic');
  assert.throws(() => takeHandoff('/different', 'synthetic'));
  assert.equal(sessionStorage.length, 0);
});

test('test_spec_8_3_htmx_keeps_its_lifecycle_and_never_sends_plaintext', async () => {
  globalThis.location = { href: 'http://192.0.2.1/a/hello/', origin: 'http://192.0.2.1' };
  const xhr = new EventTarget(); let loaded = false, native = false;
  xhr.send = () => { native = true; };
  xhr.onload = () => { loaded = true; };
  xhr.onerror = () => assert.fail('channel should answer');
  let captured;
  bridgeHtmx({ xhr, pathInfo: { finalRequestPath: '/a/hello/name' }, requestConfig: { verb: 'post', headers: { 'HX-Request': 'true' } } }, async (path, init) => {
    captured = { path, init }; return new Response('<p>synthetic</p>', { headers: { 'HX-Trigger': 'saved' } });
  });
  await xhr.send('name=synthetic');
  assert.equal(native, false); assert.equal(loaded, true);
  assert.equal(captured.init.body, 'name=synthetic');
  assert.equal(xhr.status, 200); assert.equal(xhr.response, '<p>synthetic</p>');
  assert.equal(xhr.getResponseHeader('hx-trigger'), 'saved');
});

/** A form or button as the shell reads it: attributes, plus a property a control may shadow. */
function control(attributes, shadow = {}) {
  return {
    ...shadow,
    hasAttribute: name => name in attributes,
    getAttribute: name => attributes[name] ?? null,
  };
}

test('test_spec_8_3_1_form_destination_and_method_come_from_attributes_not_shadowed_properties', () => {
  globalThis.location = { href: 'http://192.0.2.1/a/hello/', origin: 'http://192.0.2.1' };
  const base = 'http://192.0.2.1/a/hello/';
  // A submit button named `action` makes form.action the button in WebKit; the same for
  // `method` and `enctype`. The attributes are what the markup said.
  const button = control({ type: 'submit', name: 'action', value: 'save' });
  const shadowed = control({ action: '/a/hello/items/new', method: 'post', enctype: 'multipart/form-data' },
    { action: button, method: button, enctype: button });
  const request = formRequest(shadowed, button, base);
  assert.equal(request.destination.href, 'http://192.0.2.1/a/hello/items/new');
  assert.equal(request.method, 'POST');
  assert.equal(request.encoding, 'multipart/form-data');

  // No action means the document's own URL; no method means GET; no enctype means URL-encoded.
  const bare = formRequest(control({}), null, base);
  assert.equal(bare.destination.href, base);
  assert.equal(bare.method, 'GET');
  assert.equal(bare.encoding, 'application/x-www-form-urlencoded');

  // A button's formaction, formmethod and formenctype win over the form's.
  const override = control({ formaction: 'items/42', formmethod: 'post', formenctype: 'multipart/form-data' });
  const overridden = formRequest(control({ action: '/elsewhere', method: 'get' }), override, base);
  assert.equal(overridden.destination.href, 'http://192.0.2.1/a/hello/items/42');
  assert.equal(overridden.method, 'POST');
  assert.equal(overridden.encoding, 'multipart/form-data');

  // An action that leaves the origin is refused before anything is sent.
  assert.throws(() => formRequest(control({ action: 'https://example.invalid/steal' }), null, base), /stay on this origin/);
  assert.throws(() => formRequest(control({ action: 'javascript:alert(1)' }), null, base), /stay on this origin/);
});

const BASE = 'http://192.0.2.1:8420/a/animals/';

/** Fire an htmx event on `doc` with `detail`, cancelable as htmx's are; returns the event. */
function fire(doc, name, detail) {
  const event = new Event(name, { cancelable: true });
  event.detail = detail;
  doc.dispatchEvent(event);
  return event;
}

test('test_spec_8_3_1_a_boosted_request_beneath_the_mount_stays_in_the_document', () => {
  for (const target of ['knowledge', '/a/animals/', '/a/animals', '/a/animals/teach?from=board', 'http://192.0.2.1:8420/a/animals/won', '/a/animals/knowledge#path-3']) {
    assert.equal(inScope(target, '/a/animals/', BASE), true, target);
  }
  // A link and a form inside the boosted main region are swaps; on the plain path the
  // request goes ahead for htmx to send, and the form's submit button is held until it ends.
  const page = framedPage();
  installNavigation(page.doc, page.win);
  assert.equal(isSwap(boosted(page.link, '/a/animals/knowledge'), page.doc, BASE), true);
  const get = fire(page.doc, 'htmx:beforeRequest', boosted(page.link, '/a/animals/knowledge'));
  assert.equal(get.defaultPrevented, false);
  assert.deepEqual(page.win.location.assigned, []);
  // The same request from a frame without swap navigation is not a swap.
  const plain = framedPage({ swap: false });
  assert.equal(isSwap(boosted(plain.link, '/a/animals/knowledge'), plain.doc, BASE), false);
  // Nor is a request that is not boosted: a fragment goes where its own attributes say.
  assert.equal(isSwap({ ...boosted(page.link, '/a/animals/knowledge'), boosted: false }, page.doc, BASE), false);
});

test('test_spec_8_3_1_a_boosted_request_leaving_the_mount_or_a_framework_prefix_navigates', () => {
  for (const target of ['/', '/a/other/', '/a/animalsx/', '/settings', '/settings/devices', '/static/htmx.min.js',
    '/api/v1/node', '/skills/privatium.zip', '/ws', '/a/animals/api/events', '/a/animals/static/animals.css',
    '/a/animals/../settings', '/a/animals/%2e%2e/%2e%2e/settings', 'https://example.invalid/a/animals/', '//example.invalid/a/animals/',
    'http://192.0.2.1:9999/a/animals/']) {
    assert.equal(inScope(target, '/a/animals/', BASE), false, target);
  }
  for (const mount of [undefined, '', 'a/animals/', '/a/animals']) assert.equal(inScope('/a/animals/x', mount, BASE), false, String(mount));

  // On the plain path the frame makes the fresh document itself: a GET is a navigation.
  const page = framedPage();
  installNavigation(page.doc, page.win);
  const away = fire(page.doc, 'htmx:beforeRequest', boosted(page.link, '/settings'));
  assert.equal(away.defaultPrevented, true);
  assert.deepEqual(page.win.location.assigned, ['http://192.0.2.1:8420/settings']);
  // A boosted element outside the main region never swaps, even beneath the mount.
  fire(page.doc, 'htmx:beforeRequest', boosted(page.outside, '/a/animals/knowledge'));
  assert.equal(page.win.location.assigned.at(-1), 'http://192.0.2.1:8420/a/animals/knowledge');
  // A form posting out of scope is submitted natively, with the pressed button's value.
  const post = boosted(page.button, '/a/other/teach', 'post');
  post.requestConfig.elt = page.form;
  post.requestConfig.triggeringEvent = { submitter: page.button };
  assert.equal(fire(page.doc, 'htmx:beforeRequest', post).defaultPrevented, true);
  assert.deepEqual(page.win.submitted, [page.form]);
  assert.ok(page.form.children.some(field => field.attributes.type === 'hidden' && field.name === 'answer' && field.value === 'yes'));

  // Over the channel, client.js has already chosen; the frame does not navigate twice.
  const channel = framedPage({ channel: true });
  installNavigation(channel.doc, channel.win);
  assert.equal(fire(channel.doc, 'htmx:beforeRequest', boosted(channel.link, '/settings')).defaultPrevented, false);
  assert.deepEqual(channel.win.location.assigned, []);
  // A document an app owns has no data-pv-mount; its own boosting is left to it.
  const owned = framedPage({ mount: null, swap: false });
  installNavigation(owned.doc, owned.win);
  assert.equal(fire(owned.doc, 'htmx:beforeRequest', boosted(owned.link, '/settings')).defaultPrevented, false);
});

test('test_spec_8_3_1_solo_mode_scope_excludes_the_framework_prefixes_only', () => {
  const base = 'http://192.0.2.1:8420/';
  for (const target of ['/', '/knowledge', '/teach?x=1', '/settingsx', '/apiary', '/a/animals/']) {
    assert.equal(inScope(target, '/', base), true, target);
  }
  for (const target of ['/settings', '/settings/apps', '/api/events', '/api/v1/node', '/skills/x', '/static/animals.css', '/ws', 'https://example.invalid/']) {
    assert.equal(inScope(target, '/', base), false, target);
  }
});

test('test_spec_8_3_1_a_boosted_post_follows_its_redirect_with_the_boosted_header', async () => {
  globalThis.location = { href: BASE, origin: 'http://192.0.2.1:8420' };
  const calls = [];
  const fetcher = async (path, init) => {
    calls.push({ path, init });
    if (calls.length === 1) return new Response(null, { status: 303, headers: { location: '/a/animals/' } });
    return new Response('<title>Animals — Privatium</title><main id="main"><h1>Is it a mammal?</h1></main>');
  };
  const page = framedPage();
  const detail = boosted(page.form, '/a/animals/teach', 'post');
  const xhr = new EventTarget(); let loaded = false;
  xhr.onload = () => { loaded = true; };
  detail.xhr = xhr;
  bridgeHtmx(detail, fetcher);
  await xhr.send('animal=wombat');
  assert.equal(loaded, true);
  assert.equal(calls[0].init.method, 'POST');
  assert.equal(calls[0].init.body, 'animal=wombat');
  assert.equal(calls[1].path, 'http://192.0.2.1:8420/a/animals/');
  assert.deepEqual(calls[1].init.headers, { 'HX-Request': 'true', 'HX-Boosted': 'true' });
  // htmx pushes the address it was answered from, so the address bar shows the redirect's target.
  assert.equal(xhr.responseURL, 'http://192.0.2.1:8420/a/animals/');

  // A fragment's redirect stays a fragment request.
  calls.length = 0;
  const fragment = { xhr: new EventTarget(), pathInfo: { finalRequestPath: '/a/animals/answer' }, requestConfig: { verb: 'post', headers: { 'HX-Request': 'true' } } };
  bridgeHtmx(fragment, fetcher);
  await fragment.xhr.send('a=yes');
  assert.deepEqual(calls[1].init.headers, { 'HX-Request': 'true' });

  // While the boosted post is out its submit button is held, so a second press sends nothing;
  // the end of the request releases it.
  installNavigation(page.doc, page.win);
  fire(page.doc, 'htmx:beforeRequest', boosted(page.form, '/a/animals/teach', 'post'));
  assert.equal(page.button.disabled, true);
  fire(page.doc, 'htmx:afterRequest', { elt: page.link });
  assert.equal(page.button.disabled, true, 'another element finishing releases nothing');
  fire(page.doc, 'htmx:afterRequest', { elt: page.form });
  assert.equal(page.button.disabled, false);
});
