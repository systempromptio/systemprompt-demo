import { MAX_LINES, TRIM_BATCH, INPUT_MAX_ROWS, INPUT_ROW_PX } from './pi-constants.js';
import { approxTokens } from './pi-format.js';
import { endGateRun } from './pi-gate-runs.js';

/**
 * How the transcript grows, scrolls, and stops growing.
 *
 * Every renderer in the terminal ends up here, which is the point: appending,
 * trimming, and the follow-or-offer decision are one implementation so they
 * cannot disagree about how many lines exist or whether the viewer is pinned.
 */

export function echo(el, message) {
  // A new prompt divides one run of tool calls from the next, so the records
  // either side of it are counted separately.
  endGateRun(el);
  const line = document.createElement('div');
  // pi-turn-user marks the line as an act boundary: every exchange opens with
  // a prompt, so a rule above each one is all the grouping the transcript needs.
  line.className = 'terminal-line pi-turn-user';
  const p = document.createElement('span');
  p.className = 'prompt';
  p.textContent = '>';
  const c = document.createElement('span');
  c.className = 'command';
  c.textContent = ' ' + message;
  line.append(p, c);
  append(el, line);
}

export function line(el, cls, text) {
  const row = document.createElement('div');
  row.className = 'terminal-line';
  const span = document.createElement('span');
  span.className = cls;
  span.textContent = text;
  row.append(span);
  append(el, row);
  return span;
}

/** Append, trim, and scroll — the one place the transcript grows. */
export function append(el, node) {
  el._body.append(node);
  el._lines += 1;
  trim(el);
  // Counted here and not in nudge: a streaming turn calls that once per
  // frame, and "↓ 400 new" for one paragraph would be worse than no badge.
  if (!el._pinned) el._unseen += 1;
  nudge(el);
}

/**
 * Cap the transcript.
 *
 * A long session would otherwise hold every line for the tab's lifetime. The
 * marker is not decoration: the server's own replay buffer is capped too, and
 * both places say so rather than letting a gap look like silence.
 */
export function trim(el) {
  if (el._lines <= MAX_LINES) return;
  const marked = el._body.querySelector('.pi-trimmed');
  for (let i = 0; i < TRIM_BATCH; i += 1) {
    // Drop from the head, keeping the marker itself at the top.
    const victim = marked ? marked.nextSibling : el._body.firstChild;
    if (!victim) break;
    victim.remove();
    el._lines -= 1;
  }
  if (!marked) {
    const mark = document.createElement('div');
    mark.className = 'terminal-line pi-trimmed';
    mark.textContent = '── earlier output trimmed ──';
    el._body.prepend(mark);
  }
}

/** Follow the output, or offer to. */
export function nudge(el) {
  if (el._pinned) {
    el._body.scrollTop = el._body.scrollHeight;
    return;
  }
  if (!el._unseen) return;
  el._jumpBtn.hidden = false;
  el._jumpBtn.textContent = '↓ ' + el._unseen + ' new';
}

export function clearUnseen(el) {
  el._unseen = 0;
  el._jumpBtn.hidden = true;
}

/**
 * The working indicator.
 *
 * This replaced a blinking caret that was appended to the transcript body. A
 * caret in the transcript claims the transcript is an input — it is not, and
 * the one that is sat several hundred pixels below it, so the page gave no
 * honest answer to "where do I type?". The composer owns the caret now.
 *
 * What belongs here instead is the fact that something is running and how
 * much of it has arrived, which is exactly what a viewer waiting on a block
 * wants to know while the raw markdown is deliberately withheld.
 */
export function working(el, on) {
  if (on && !el._workEl) {
    const row = document.createElement('div');
    row.className = 'pi-working';
    row.setAttribute('aria-hidden', 'true');
    const dot = document.createElement('i');
    dot.className = 'pi-working-dot';
    const label = document.createElement('span');
    label.className = 'pi-working-label';
    label.textContent = 'working';
    const count = document.createElement('span');
    count.className = 'pi-working-count';
    row.append(dot, label, count);
    el._body.append(row);
    el._workEl = { row, count };
  } else if (!on && el._workEl) {
    el._workEl.row.remove();
    el._workEl = null;
  }
}

/**
 * Keep the indicator last and its count current.
 *
 * Revealed blocks are appended to the body, so without this the indicator
 * would be stranded above the prose it is supposedly still producing.
 */
export function workingSync(el) {
  if (!el._workEl) return;
  el._workEl.count.textContent = el._streamBuf
    ? approxTokens(el._streamBuf) + ' tokens' : '';
  el._body.append(el._workEl.row);
}

/** Grow with the prompt, up to a ceiling. */
export function autogrow(el) {
  el._input.style.height = 'auto';
  el._input.style.height
    = Math.min(el._input.scrollHeight, INPUT_MAX_ROWS * INPUT_ROW_PX) + 'px';
}
