-- This file is part of Privatium
-- apps/animals/lib/tree.lua
-- Author(s): Gabriel Mongefranco
-- Created: 2026-08-28
-- Last Modified: 2026-09-26
-- Summary: Queries over the decision tree. Kept out of app.lua so the routes stay readable — the same
--          split any growing app should make.
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

local pv = require 'privatium'
local M  = {}

-- Trim and reject empty input. Returns nil when there is nothing usable.
function M.clean(s)
  s = (s or ''):gsub('^%s+', ''):gsub('%s+$', '')
  return s ~= '' and s or nil
end

-- The tree every fresh game starts from: one question and one animal on each side. Small on
-- purpose, so the first round already teaches something, and the first wrong guess grows it.
M.STARTER = {
  question = 'Does it have legs?',
  yes      = 'dog',
  no       = 'fish',
}

-- Write the starter tree through `tx`, inside a batch the caller owns. The leaves are
-- appended first because the question references their minted ids; nothing points at the
-- question, which is what makes it the root.
function M.plant(tx)
  local yes = tx.append('node', { kind = 'a', text = M.STARTER.yes })
  local no  = tx.append('node', { kind = 'a', text = M.STARTER.no })
  tx.append('node', { kind = 'q', text = M.STARTER.question, yes_id = yes, no_id = no })
end

-- The root is the only node nobody points at.
function M.root_id()
  local r = pv.query1([[
    SELECT n.id FROM node n
    WHERE NOT EXISTS (SELECT 1 FROM node p WHERE p.yes_id = n.id OR p.no_id = n.id)
    LIMIT 1
  ]])
  return r and r.id
end

-- Every animal with the questions that lead to it.
--
-- `id` is selected as well as `text` because views/knowledge.lsp needs a stable,
-- unique value for the `aria-controls` / `id` pair on each collapsible path. Two
-- animals can share a name in a tree that has been reset and rebuilt; ULIDs
-- cannot, so the accessible name never points at the wrong element.
function M.knowledge()
  return pv.query([[
    WITH RECURSIVE walk(id, depth, path) AS (
      SELECT n.id, 0, ''
      FROM node n
      WHERE NOT EXISTS (SELECT 1 FROM node p WHERE p.yes_id = n.id OR p.no_id = n.id)

      UNION ALL

      SELECT c.id,
             w.depth + 1,
             w.path || CASE WHEN w.path = '' THEN '' ELSE ' -> ' END
                    || p.text || ': ' || t.answer
      FROM walk w
      JOIN node p ON p.id = w.id AND p.kind = 'q'
      CROSS JOIN (SELECT 'yes' AS answer UNION ALL SELECT 'no') AS t
      JOIN node c ON c.id = CASE WHEN t.answer = 'yes' THEN p.yes_id ELSE p.no_id END
    )
    SELECT w.depth, w.path, n.id AS animal_id, n.text AS animal
    FROM walk w JOIN node n ON n.id = w.id
    WHERE n.kind = 'a'
    ORDER BY w.depth, n.text
  ]])
end

function M.stats()
  return pv.query1([[
    SELECT count(*) FILTER (WHERE kind = 'a') AS animals,
           count(*) FILTER (WHERE kind = 'q') AS questions
    FROM node
  ]])
end

return M
