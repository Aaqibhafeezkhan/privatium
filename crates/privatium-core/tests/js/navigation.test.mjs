// Project:  Privatium™  |  File: crates/privatium-core/tests/js/navigation.test.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-09-05  |  Modified: 2026-10-04
// Summary:  Encrypted HTMX dispatch and metadata-only document handoffs (§8.3.1).
//           See main README.md for full license information.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bridgeHtmx, formRequest, saveHandoff, takeHandoff, resourceIntegrity } from '../../assets/shell/client.js';
import { createHash } from 'node:crypto';
import { storage } from './harness.mjs';

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
