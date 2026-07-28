import { getJson } from './pi-transport.js';
import { autogrow } from './pi-terminal-dom.js';

/**
 * The slash-command palette.
 *
 * Discovery only. A leading `/` already works without it — pi expands skill
 * commands itself on the `prompt` utterance — so nothing here parses or
 * rewrites what the viewer typed.
 */

/**
 * Load the skills this session can run. Failure is silent on purpose: the
 * palette is a convenience, and a terminal that refuses to start because a
 * dropdown could not be populated is a worse outcome than no dropdown.
 */
export async function loadCommands(el) {
  const url = el._endpoint + '/commands/' + encodeURIComponent(el._conversationId)
    + '?token=' + encodeURIComponent(el._token);
  el._commands = (await getJson(url)) || [];
}

export function refreshPalette(el) {
  const value = el._input.value;
  if (!el._commands || !el._commands.length || value[0] !== '/') {
    hidePalette(el);
    return;
  }
  const needle = value.toLowerCase();
  const hits = el._commands.filter((c) => c.command.toLowerCase().startsWith(needle));
  if (!hits.length) {
    hidePalette(el);
    return;
  }
  renderPalette(el, hits);
}

/** The whole catalogue, unfiltered — what ↓ on an empty composer opens. */
export function openPaletteAll(el) {
  if (!el._commands || !el._commands.length) return;
  renderPalette(el, el._commands);
}

function renderPalette(el, hits) {
  el._paletteEl.textContent = '';
  hits.forEach((hit, n) => {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'pi-palette-row';
    row.id = 'pi-cmd-' + n;
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', 'false');
    row.tabIndex = -1;
    const name = document.createElement('span');
    name.className = 'pi-palette-cmd';
    name.textContent = hit.command;
    const desc = document.createElement('span');
    desc.className = 'pi-palette-desc';
    desc.textContent = hit.description;
    row.append(name, desc);
    row.dataset.command = hit.command;
    // Pointer and keyboard resolve to the same call, so the two can never
    // drift into doing subtly different things.
    row.addEventListener('click', () => acceptPalette(el, row));
    row.addEventListener('mousemove', () => selectPalette(el, n));
    el._paletteEl.append(row);
  });
  el._paletteEl.hidden = false;
  el._input.setAttribute('aria-expanded', 'true');
  // Preselect the first hit, so `/` then Enter is a complete keyboard flow.
  selectPalette(el, 0);
}

/** Move the palette's selection. Wraps, so ↑ from the top reaches the end. */
export function selectPalette(el, n) {
  const rows = paletteRows(el);
  if (!rows.length) return;
  const at = (n + rows.length) % rows.length;
  rows.forEach((row, i) => {
    const on = i === at;
    row.classList.toggle('is-selected', on);
    row.setAttribute('aria-selected', on ? 'true' : 'false');
    if (on) {
      row.scrollIntoView({ block: 'nearest' });
      el._input.setAttribute('aria-activedescendant', row.id);
    }
  });
  el._paletteAt = at;
}

/** Take the highlighted command into the composer. */
export function acceptPalette(el, row) {
  const target = row || paletteRows(el)[el._paletteAt];
  if (!target) return;
  el._input.value = target.dataset.command + ' ';
  hidePalette(el);
  el._input.focus();
  autogrow(el);
}

function paletteRows(el) {
  return Array.from(el._paletteEl.querySelectorAll('.pi-palette-row'));
}

export function hidePalette(el) {
  el._paletteEl.hidden = true;
  el._paletteEl.textContent = '';
  el._paletteAt = 0;
  el._input.setAttribute('aria-expanded', 'false');
  el._input.removeAttribute('aria-activedescendant');
}

export function paletteOpen(el) {
  return !el._paletteEl.hidden;
}
