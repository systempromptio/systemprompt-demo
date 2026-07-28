import { HISTORY_MAX } from './pi-constants.js';
import { autogrow, echo } from './pi-terminal-dom.js';
import {
  hidePalette, paletteOpen, openPaletteAll, selectPalette, acceptPalette,
} from './pi-terminal-palette.js';

/** The composer: what the viewer types, and every key that means something. */

export async function send(el) {
  const message = el._input.value.trim();
  if (!message) return;
  el._input.value = '';
  autogrow(el);
  hidePalette(el);
  remember(el, message);
  echo(el, message);
  // Mid-turn input redirects the running turn instead of queueing a new one.
  await el._post(el._turnLive ? 'steer' : 'prompt', { message });
}

/**
 * Keyboard handling for the composer.
 *
 * Enter sends because this is a chat surface and that is what a chat surface
 * does; shift-enter is the escape hatch for a multi-line prompt. Escape closes
 * the palette if it is open and otherwise stops a running turn — the narrower
 * meaning wins, so escape never does something drastic while a dropdown is
 * covering the thing the viewer was looking at.
 */
export function onKey(el, e) {
  if (e.key === 'Escape') {
    if (paletteOpen(el)) {
      e.preventDefault();
      hidePalette(el);
    } else if (el._turnLive) {
      e.preventDefault();
      void el._post('abort', {});
    }
    return;
  }
  // The palette owns the arrows and the accept keys while it is open, and
  // recalling history under an open list would fight what the viewer is
  // reading. This block used to be a bare `return` with a comment saying the
  // palette owned these keys — nothing did, so the list was mouse-only.
  if (paletteOpen(el)) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectPalette(el, el._paletteAt + 1);
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectPalette(el, el._paletteAt - 1);
      return;
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      acceptPalette(el);
      return;
    }
  }
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    void send(el);
    return;
  }
  if (paletteOpen(el)) return;
  // ↓ on an empty composer opens the whole skill catalogue: the hint bar
  // promises "↑↓ skills", and a key that only worked once the list was
  // already open was a promise the keyboard could not actually redeem.
  if (e.key === 'ArrowDown' && !el._input.value) {
    e.preventDefault();
    openPaletteAll(el);
    return;
  }
  // History only when the caret cannot usefully move, so ↑ still navigates a
  // prompt the visitor is part-way through writing.
  if (e.key === 'ArrowUp' && caretAtStart(el)) {
    if (el._historyAt + 1 < el._history.length) {
      el._historyAt += 1;
      recall(el);
      e.preventDefault();
    }
    return;
  }
  if (e.key === 'ArrowDown' && caretAtEnd(el)) {
    if (el._historyAt > 0) {
      el._historyAt -= 1;
      recall(el);
      e.preventDefault();
    } else if (el._historyAt === 0) {
      el._historyAt = -1;
      el._input.value = '';
      autogrow(el);
      e.preventDefault();
    }
  }
}

function caretAtStart(el) {
  return el._input.selectionStart === 0 && el._input.selectionEnd === 0;
}

function caretAtEnd(el) {
  const n = el._input.value.length;
  return el._input.selectionStart === n && el._input.selectionEnd === n;
}

function recall(el) {
  el._input.value = el._history[el._historyAt] || '';
  autogrow(el);
  const n = el._input.value.length;
  el._input.setSelectionRange(n, n);
}

export function remember(el, message) {
  el._history.unshift(message);
  if (el._history.length > HISTORY_MAX) el._history.pop();
  el._historyAt = -1;
}
