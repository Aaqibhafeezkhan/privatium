// Project:  Privatium™  |  File: crates/privatium-core/tests/browser/swap-and-reflow.mjs
// Authors:  Gabriel Mongefranco (@gabrielmongefranco)
// Created:  2026-10-04  |  Modified: 2026-10-04
// Summary:  The browser checks of docs/compliance.md that a real layout engine can answer,
//           run in headless Firefox over WebDriver BiDi against a loopback node: animals'
//           swap navigation end to end (no fresh document, title, focus, Alpine, back,
//           the teach post's redirect, a /settings link leaving), the bar's reflow at 320
//           and 640 CSS pixels, sketch without the chrome, and pantry with it. Not part of
//           the gates, since it needs Firefox; it prints PASS or FAIL per check and exits 1
//           on any failure. See main README.md for full license information.
//
// Run from the workspace root, all in one shell so the three share a loopback:
//
//   cargo build -p privatium
//   D=$(mktemp -d); P=$(mktemp -d)
//   ./target/debug/privatium --data-dir "$D" --port 4005 --no-discovery &
//   firefox --headless --no-remote --profile "$P" --remote-debugging-port 9222 about:blank &
//   node --experimental-websocket crates/privatium-core/tests/browser/swap-and-reflow.mjs http://127.0.0.1:4005
//
// `--experimental-websocket` is for Node 20; Node 22 and later have WebSocket built in.

const [, , base] = process.argv;
const ws = new WebSocket('ws://127.0.0.1:9222/session');
let id = 0;
const pending = new Map();
ws.onmessage = event => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
};
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const n = ++id;
  pending.set(n, msg => (msg.type === 'error' ? reject(new Error(`${method}: ${msg.error} ${msg.message}`)) : resolve(msg.result)));
  ws.send(JSON.stringify({ id: n, method, params }));
});
await send('session.new', { capabilities: {} });
const { contexts } = await send('browsingContext.getTree', {});
const context = contexts[0].context;
const go = url => send('browsingContext.navigate', { context, url: base + url, wait: 'complete' });
const run = async expression => {
  const out = await send('script.evaluate', { expression, target: { context }, awaitPromise: true, resultOwnership: 'none' });
  if (out.type === 'exception') throw new Error(out.exceptionDetails.text);
  return out.result.value;
};
const sleep = ms => new Promise(r => setTimeout(r, ms));
const until = async (expression, what) => {
  for (let i = 0; i < 50; i++) {
    try { if (await run(expression)) return; } catch { /* the document may be mid-navigation */ }
    await sleep(100);
  }
  throw new Error('timed out waiting for ' + what);
};
const results = [];
const check = (name, ok, detail = '') => { results.push({ name, ok, detail }); console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ' — ' + detail : ''}`); };
const state = () => run(`JSON.stringify({ path: location.pathname, marker: window.__marker ?? null, title: document.title,
  h1: document.querySelector('#main h1')?.textContent?.trim() ?? null, active: document.activeElement?.tagName + '#' + (document.activeElement?.id || ''),
  headers: document.querySelectorAll('header.pv-header').length, mains: document.querySelectorAll('main').length,
  cloaked: document.querySelectorAll('[x-cloak]').length, menu: document.querySelectorAll('#pv-app-menu').length,
  disabled: [...document.querySelectorAll('#main button[disabled]')].length })`).then(JSON.parse);

await send('browsingContext.setViewport', { context, viewport: { width: 1280, height: 900 } });

// 1. A link inside the main region swaps the next page in, with no fresh document.
await go('/a/animals/');
await run('window.__marker = 1; true');
await run(`document.querySelector('#main a[href$="/knowledge"]').click(); true`);
await until(`location.pathname.endsWith('/knowledge') && document.querySelector('#main h1')?.textContent.includes('What I know')`, 'knowledge');
await sleep(300);
let s = await state();
check('link swap keeps the document', s.marker === 1, JSON.stringify(s));
check('swapped page title', /Animals/.test(s.title), s.title);
check('focus on the new h1', s.active.startsWith('H1'), s.active);
check('one header, one main, one menu list', s.headers === 1 && s.mains === 1 && s.menu === 1, JSON.stringify(s));
check('Alpine started the swapped components (x-cloak removed)', s.cloaked === 0, String(s.cloaked));
const scroll = await run('window.scrollY');
check('scrolled to the top', scroll === 0, String(scroll));

// 2. Back is a fresh load of the previous page.
await run('history.back(); true');
await until(`location.pathname === '/a/animals/' && document.readyState === 'complete' && window.__marker === undefined`, 'back reload');
s = await state();
check('back button reloads the page', s.marker === null && s.path === '/a/animals/', JSON.stringify(s));

// 3. Play to a wrong guess, then the No link swaps the teach form in, focus on its autofocus field.
await run('window.__marker = 2; true');
await run(`document.querySelector('#board form button[type=submit]').click(); true`);
await until(`!!document.querySelector('#board a[href$="/teach"]')`, 'a guess on the board');
await run(`document.querySelector('#board a[href$="/teach"]').click(); true`);
await until(`location.pathname.endsWith('/teach') && !!document.querySelector('#animal')`, 'teach page');
await sleep(300);
s = await state();
check('teach link swaps in place', s.marker === 2, JSON.stringify(s));
check('focus on the autofocus field', s.active === 'INPUT#animal', s.active);

// 4. The teach post follows its redirect inside the document.
await run(`document.querySelector('#animal').value = 'cat';
  document.querySelector('#question').value = 'Does it purr?';
  const yes = document.querySelector('input[name=answer][value=yes]'); if (yes) yes.checked = true;
  document.querySelector('#main form[action$="/teach"]').requestSubmit(); true`);
await until(`location.pathname === '/a/animals/' && !!document.querySelector('#board')`, 'board after teaching');
await sleep(300);
s = await state();
check('teach post lands on the board in the same document', s.marker === 2 && s.path === '/a/animals/', JSON.stringify(s));
check('no submit button left disabled', s.disabled === 0, String(s.disabled));
const taught = await run(`fetch('${base}/a/animals/knowledge').then(r => r.text()).then(t => t.includes('<td>cat</td>'))`);
check('the taught animal was recorded once', taught === true);

// 5. A link to /settings inside the main region is a full navigation.
await run(`const a = document.createElement('a'); a.href = '/settings'; a.id = 'away'; a.textContent = 'Settings';
  document.querySelector('#main').append(a); htmx.process(a); a.click(); true`);
await until(`location.pathname === '/settings' && document.readyState === 'complete'`, 'settings');
s = await state();
check('a /settings link leaves for a fresh document', s.marker === null && s.path === '/settings', JSON.stringify(s));

// 6. Reflow: no horizontal scroll and the bar's controls inside the viewport.
for (const width of [320, 640]) {
  await send('browsingContext.setViewport', { context, viewport: { width, height: 800 } });
  for (const page of ['/', '/a/hello/', '/a/animals/', '/a/animals/knowledge', '/a/pantry/', '/settings']) {
    await go(page);
    await sleep(200);
    const m = JSON.parse(await run(`JSON.stringify({ sw: document.documentElement.scrollWidth, iw: innerWidth,
      controls: document.querySelector('nav.pv-controls')?.getBoundingClientRect().right ?? null,
      title: document.querySelector('.pv-app-title')?.getBoundingClientRect().right ?? null })`));
    check(`${width}px ${page}: no horizontal scroll, controls in view`, m.sw <= m.iw && (m.controls === null || m.controls <= m.iw) && (m.title === null || m.title <= m.iw), JSON.stringify(m));
  }
}

// 7. Sketch declines the chrome; its mark is a link with a 44-pixel target.
await send('browsingContext.setViewport', { context, viewport: { width: 1280, height: 900 } });
await go('/a/sketch/');
await sleep(300);
const sk = JSON.parse(await run(`JSON.stringify({ header: !!document.querySelector('header.pv-header'), footer: !!document.querySelector('footer.pv-footer'),
  mark: (() => { const r = document.querySelector('#mark')?.getBoundingClientRect(); return r ? [r.width, r.height] : null; })(),
  href: document.querySelector('#mark')?.href })`));
check('sketch has no bar and no footer', !sk.header && !sk.footer, JSON.stringify(sk));
check('sketch mark is a link of at least 44 by 44', sk.mark && sk.mark[0] >= 44 && sk.mark[1] >= 44 && sk.href === base + '/', JSON.stringify(sk));

// 8. Pantry: bar and footer, one h1, status written to the footer slot.
await go('/a/pantry/');
await sleep(300);
const pa = JSON.parse(await run(`JSON.stringify({ header: document.querySelectorAll('header.pv-header').length, h1: document.querySelectorAll('h1').length,
  slot: !!document.querySelector('footer #pv-status[role=status]'), main: !!document.querySelector('main#main') })`));
check('pantry wears the bar, footer slot and one h1', pa.header === 1 && pa.h1 === 1 && pa.slot && pa.main, JSON.stringify(pa));

const failed = results.filter(r => !r.ok).length;
console.log(`${results.length - failed} passed, ${failed} failed`);
await send('session.end', {}).catch(() => {});
ws.close();
process.exit(failed ? 1 : 0);
