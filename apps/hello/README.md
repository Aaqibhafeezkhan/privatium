# hello — the reference Tier 1 app

The smallest thing that is still an app. Read it before writing your own.

## Features

Asks for your name. Greets you with it. Lets you change it.

## The whole thing

| File | Lines | Job |
|---|---|---|
| `app.toml` | ~18 | Manifest |
| `app.lua` | ~25 | Three routes |
| `schema.sql` | 4 | One table, one column |
| `views/index.lsp` | ~15 | The greeting |
| `views/edit.lsp` | ~18 | The form |

No JavaScript. No build step. No restart. Save a file, refresh the browser.

## Copy this to start a new app

```bash
privatium new myapp --from hello
# or by hand:
cp -r apps/hello ~/.local/share/privatium/apps/myapp
```

Change `slug` and `title` in `app.toml`. The slug must match the folder name.

## Points to notice

**Nothing is stored in the table.** Change your name three times and the table still holds
one row — but `data/hello/log/<device>.jsonl` holds three lines. The table is derived. Open
that file and read your own history.

**`pv.append` reuses the existing id.** That is what makes an edit an amendment rather than
a second person. Row-granularity last-write-wins does the rest.

**`<?= ?>` escapes.** Type `<script>alert(1)</script>` as your name. It displays; it does
not run. There is no flag to turn that off — `<?raw ?>` exists for the rare case, and every
use is flagged by the linter.

**`url()` everywhere.** Never `/a/hello/edit`. The app works unchanged in solo mode because
of it.

**The logic is in the template.** Morning/afternoon/evening is three lines of Lua inside the
LSP. A real app moves that into `app.lua` or `lib/`, and everything else stays this size.

## Try this

```bash
# See your history
cat data/hello/log/*.jsonl

# Break the cache; nothing is lost
rm -rf cache/ data/hello/snap/
# restart — the greeting is still there

# Append an event by hand, then reload the page
echo '{"seq":4,"lam":4,"ts":"2026-08-28T20:00:00.000Z","dev":"<your-id>","app":"hello","op":"put","tbl":"profile","id":"<the-ulid>","d":{"display_name":"Someone Else"}}' \
  >> data/hello/log/<your-id>.jsonl
```

That last one is the whole architecture in one command. Two numbers matter. Use the next
unused `seq` for your device — `4` if the log holds three lines — because a writer must
emit `seq` gaplessly (`spec/protocol.md §4.1`); reading tolerates a gap, syncing does not.
And give the line a `lam` above the highest one in the log — `4` after three writes —
because `lam` is what decides the merge (`§4.5`): a hand-written line with a lower `lam`
than the row it amends is durable, and loses. Read the last line of the file first; the
node picks both counters up from it on the next request, so the name you save afterwards
takes `seq` 5 and `lam` 5.

---

Copyright © 2026 Gabriel Mongefranco
