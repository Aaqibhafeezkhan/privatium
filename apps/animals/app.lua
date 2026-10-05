-- This file is part of Privatium
-- apps/animals/app.lua
-- Author(s): Gabriel Mongefranco
-- Created: 2026-08-28
-- Last Modified: 2026-10-04
-- Summary: The guess-the-animal game. Demonstrates multi-event atomic writes, recursive SQL, stored
--          session state, and the HTMX/Alpine boundary.
-- Notes: See README file for documentation and full license information.
--
-- Copyright © 2026 Gabriel Mongefranco
--
-- This program is free software: you can redistribute it and/or modify
-- it under the terms of the GNU General Public License as published by
-- the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
--
-- This program is distributed in the hope that it will be useful,
-- but WITHOUT ANY WARRANTY; without even the implied warranty of
-- MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
-- GNU General Public License for more details.
--
-- You should have received a copy of the GNU General Public License along
-- with this program. If not, see <https://www.gnu.org/licenses/>.

-- Lineage: the "Animal" guessing game from David H. Ahl's BASIC Computer Games (1973),
-- preserved at https://github.com/coding-horror/basic-computer-games (Unlicense).
-- Nothing is copied from that project. The classic implementations keep the tree in
-- memory and lose it on exit; here the tree IS the event log, which is the point.

local pv   = require 'privatium'
local tree = require 'tree'          -- lib/tree.lua

-- Where we are: the cursor if a round is in progress, otherwise the root.
local function here()
  local c = pv.query1('SELECT node_id FROM cursor WHERE id = ?', {'cursor'})
  local id = c and c.node_id or tree.root_id()
  if not id then return nil end
  return pv.get_row('node', id)
end

-- A game with no tree gets the starter one, so the first visit is already a question.
--
-- This is the one write a GET may cause, and it happens once per data folder: the three
-- events are real rows in the log like any taught animal, which keeps "the tree is the
-- event log" true from the first screen. Loading `sample/seed.jsonl` from the settings
-- page is only possible before this runs, because a seed fills an empty log or nothing.
local function ensure_tree()
  if tree.root_id() then return end
  pv.batch(function(tx) tree.plant(tx) end)
end

-- Answer a board request the way the caller asked for it.
--
-- HTMX sets HX-Request, so `req.is_htmx` is true and we return _board.lsp alone —
-- the browser swaps it into #board and keeps scroll position and focus. A plain
-- form post (no JavaScript, or a reader with it off) gets a redirect and a full
-- page, which is the same state by a slower route.
--
-- Both paths are load-bearing. The forms in _board.lsp carry `method` and
-- `action` as well as `hx-post` precisely so this function has something correct
-- to do in either case. Deleting the redirect branch would make the app depend on
-- JavaScript to record a guess, which is not a trade this framework makes.
local function board(req, extra)
  local ctx = { node = here(), stats = tree.stats() }
  for k, v in pairs(extra or {}) do ctx[k] = v end

  if req and req.is_htmx then return pv.render('_board', ctx) end
  if extra and extra.err then return pv.render('play', ctx) end
  return pv.redirect(url('/'))
end

pv.get('/', function()
  ensure_tree()
  return pv.render('play', { node = here(), stats = tree.stats() })
end)

-- Winning is a presentation state; starting over is the action that moves the cursor.
pv.get('/won', function(req)
  local node = here()
  if not node or node.kind ~= 'a' then return pv.redirect(url('/')) end
  return pv.render(req.is_htmx and '_board' or 'play', {
    node = node, stats = tree.stats(), won = true,
  })
end)

-- Start a fresh round at the root.
pv.post('/start', function(req)
  local root = tree.root_id()
  if root then
    pv.append('cursor', 'cursor', { node_id = root, started = pv.now() })
  end
  return board(req)
end)

-- Walk one step down the tree. The answer is an allowlist of two values; anything else
-- is a hand-built request, refused without a write.
pv.post('/answer', function(req)
  local node = here()
  if not node or node.kind ~= 'q' then return board(req) end

  local choice = req.form.choice
  if choice ~= 'yes' and choice ~= 'no' then
    return board(req, { err = 'Answer yes or no.' })
  end
  local next_id = choice == 'yes' and node.yes_id or node.no_id
  pv.append('cursor', 'cursor', { node_id = next_id, started = pv.now() })
  return board(req)
end)

pv.get('/teach', function()
  return pv.render('teach', { node = here() })
end)

-- The guess was wrong. Learn the new animal.
--
-- The classic trick: the leaf we landed on BECOMES the question, keeping its own
-- id, and gains two fresh leaves. The parent is never touched and never has to be
-- found, so this is three events rather than four — and every existing pointer
-- into the tree stays correct.
--
-- Note this route redirects rather than swapping a fragment, even under HTMX.
-- Teaching is a navigation: you came here from the board on a separate page and
-- you are going back to it. Swapping a fragment would leave the browser's history
-- pointing at a form the user has already submitted. Not every write wants HTMX.
-- With swap navigation the frame follows the redirect inside the document, and the
-- address bar shows the board, exactly as after a full-page post.
pv.post('/teach', function(req)
  local node     = here()
  local animal   = tree.clean(req.form.animal)
  local question = tree.clean(req.form.question)
  local yes_new  = req.form.answer == 'yes'

  if not (node and node.kind == 'a') then return pv.redirect(url('/')) end
  if not animal or not question then
    return pv.render('teach', { node = node, err = 'Both fields are required.' })
  end

  pv.batch(function(tx)
    local new_leaf = tx.append('node', { kind = 'a', text = animal })
    local old_leaf = tx.append('node', { kind = 'a', text = node.text })

    tx.append('node', node.id, {
      kind   = 'q',
      text   = question,
      yes_id = yes_new and new_leaf or old_leaf,
      no_id  = yes_new and old_leaf or new_leaf,
    })

    tx.delete('cursor', 'cursor')
  end)

  return pv.redirect(url('/'))
end)

pv.get('/knowledge', function()
  return pv.render('knowledge', { rows = tree.knowledge() })
end)

-- Forgetting is tombstones, never a rewrite. The log still holds every round you
-- ever played; only the materialized tree is emptied, and the starter tree is planted
-- again in the same batch so the game is never left without a question. The
-- confirmation step in views/knowledge.lsp is Alpine, because a confirmation is not data.
pv.post('/reset', function()
  pv.batch(function(tx)
    for _, n in ipairs(pv.query('SELECT id FROM node')) do tx.delete('node', n.id) end
    tx.delete('cursor', 'cursor')
    tree.plant(tx)
  end)
  return pv.redirect(url('/'))
end)
