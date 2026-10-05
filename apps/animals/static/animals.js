/* Project:  Privatium™  |  File: apps/animals/static/animals.js
 * Authors:  Gabriel Mongefranco (@gabrielmongefranco)
 * Created:  2026-08-31  |  Modified: 2026-10-04
 * Summary:  Game focus management and Alpine components for interactions that are purely
 *           visual. Everything that changes data is HTMX, not this file. Loaded once, from
 *           [ui] scripts, by the frame's head: pages swap in under it, so every listener is
 *           bound to the document here and nothing assumes a fresh page.
 *           See main README.md for full license information.
 *
 * WHY THIS FILE EXISTS AT ALL, RATHER THAN INLINE x-data
 * -----------------------------------------------------
 * A Privatium app is served with `script-src 'self'` and no 'unsafe-eval'
 * (spec/app-contract.md §5.4). The *standard* Alpine build compiles attribute
 * expressions with the Function constructor, so `x-data="{ open: false }"` and
 * `@click="open = !open"` cannot run under that policy at all.
 *
 * The CSP build (vendored as alpine-csp.min.js — see VENDOR.md) removes that
 * requirement by removing inline expressions: every x-data must name a component
 * registered here, and every binding must reference one of its properties or
 * methods *by key*. That is more verbose and it is the correct trade. Setting
 * `eval = true` in app.toml to keep the shorter syntax would hand any injected
 * string a JavaScript engine, which is the whole thing the policy prevents.
 *
 * Rule of thumb for anything added here:
 *
 *     If losing it on refresh loses data, it is HTMX.
 *     If losing it on refresh is fine, it is Alpine.
 *
 * Nothing in this file writes an event, and nothing in this file should.
 */

// A swapped form removes its focused button. Start at the new question so the next
// Tab reaches its answers, and assistive technology can announce the question.
document.addEventListener('htmx:afterSwap', event => {
  if (event.detail.target.id !== 'board') return;
  const heading = event.detail.target.querySelector('h1');
  if (heading) {
    heading.tabIndex = -1;
    heading.focus();
  }
});

document.addEventListener('alpine:init', () => {
  /* Used on: views/knowledge.lsp (each question path), views/teach.lsp (help text).
   *
   * A show/hide toggle. Nothing is persisted, because nothing here is worth
   * persisting — reopening the page with everything collapsed is the correct
   * behaviour, not a bug. Long question paths are the reason this exists:
   * collapsed by default keeps the table scannable, which matters more than
   * usual for the dyslexia-friendly target in docs/architecture.md. */
  Alpine.data('disclosure', () => ({
    open: false,

    toggle() {
      this.open = !this.open;
    },

    /* The CSP build cannot evaluate `x-text="open ? 'Hide' : 'Show'"`, so the
     * conditional lives here and the template references `label` by key. */
    get label() {
      return this.open ? 'Hide' : 'Show';
    },
  }));

  /* Used on: views/knowledge.lsp (Forget everything).
   *
   * Replaces `onsubmit="return confirm(...)"`. That attribute was a real bug,
   * not a style preference: an inline event handler is script, and this app's
   * CSP does not allow 'unsafe-inline', so it would simply never have fired.
   *
   * This is deliberately *not* a data-safety mechanism. Reset writes tombstones
   * and the log keeps every round ever played (see README), so the worst case is
   * an inconvenience. It is here because a destructive button deserves a second
   * step, and that step is ephemeral UI. */
  Alpine.data('confirmable', () => ({
    asking: false,

    ask() {
      this.asking = true;
    },

    cancel() {
      this.asking = false;
    },
  }));
});
