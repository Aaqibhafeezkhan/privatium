// This file is part of Privatium
// apps/sketch/web/exit.js
// Author(s): Gabriel Mongefranco
// Created: 2026-10-04
// Last Modified: 2026-10-04
// Summary: Where the way out of the app leads. Sketch declines the framework's bar, so its
//          own two exits — the mark in the rail and the Apps entry in its menu — point where
//          this says: the launcher when the app is mounted under one, and Settings when the
//          app is the node's front page and there is no launcher to return to (spec/cli.md
//          §2). A pure function, so the test can hold it without a document.
// Notes: See README file for documentation and full license information.
//
// Copyright © 2026 Gabriel Mongefranco
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

/**
 * The destination and accessible name of a link out of the app.
 * @param {string} mount The app's mount: `/` in solo mode, `/a/<slug>/` under a launcher.
 * @param {(path: string) => string} url `pv.url`, which resolves a path beneath the mount.
 * @returns {{href: string, label: string}} Where the link goes and what it is called.
 */
export function exitTarget(mount, url) {
  const solo = mount === '/';
  return {
    href: url(solo ? 'settings' : '../../'),
    label: solo ? 'Settings' : 'Apps',
  };
}

/**
 * Point `link` out of the app: its `href`, `aria-label` and `title`, and the text of the
 * `<span>` inside it when it has one. Clears `hidden`, so a link written hidden in the
 * page shows once it knows where it goes.
 * @param {HTMLAnchorElement} link The link to fill in.
 * @param {{href: string, label: string}} target From {@link exitTarget}.
 */
export function pointOut(link, target) {
  link.href = target.href;
  link.setAttribute('aria-label', target.label);
  link.title = target.label;
  const text = link.querySelector('span');
  if (text) text.textContent = target.label;
  link.hidden = false;
}
