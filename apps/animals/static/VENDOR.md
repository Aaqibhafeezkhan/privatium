# Vendored assets

Everything a Privatium app loads is served from its own folder. No CDN, no
`node_modules`, no build step. `docs/frameworks.md §2` explains why; the short
version is that a CDN is a third party on the critical path and this project
does not have those.

| File | Package | Version | Source |
|---|---|---|---|
| `alpine-csp.min.js` | `@alpinejs/csp` | 3.17.1 | `dist/cdn.min.js` |

## Reproducing it

```bash
npm pack @alpinejs/csp@3.17.1
tar xzf alpinejs-csp-3.17.1.tgz
cp package/dist/cdn.min.js apps/animals/static/alpine-csp.min.js
```

## Reasons for the CSP build of Alpine

An app is served with `script-src 'self'` and no `'unsafe-eval'`
(`spec/app-contract.md §5.4`). Standard Alpine compiles attribute expressions
with the `Function` constructor, so `x-data="{ open: false }"` cannot run under
that policy. The CSP build removes inline expressions instead: `x-data` names a
component registered with `Alpine.data()`, and bindings reference its properties
and methods by key. See `static/animals.js`.

The alternative — setting `eval = true` in `app.toml` — would hand any injected
string a JavaScript engine in order to save some typing. Do not.

## Upgrading

Bump the version, re-run the commands above, and re-read
<https://alpinejs.dev/advanced/csp>: the CSP build supports a subset of Alpine's
syntax, and that subset has changed between releases.
