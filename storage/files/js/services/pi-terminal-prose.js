import { markdown } from './pi-markdown.js';
import { approxTokens, countFences } from './pi-format.js';
import { append, nudge, working, workingSync } from './pi-terminal-dom.js';
import { orphanRail } from './pi-terminal-rail.js';

/**
 * Assistant prose, buffered and never shown raw.
 *
 * Markdown cannot be parsed token by token — a fence is not a fence until its
 * closing line lands, and a table is not a table until its separator row does.
 * The old behaviour streamed the raw buffer into the transcript and only
 * rendered it at the end of the turn, which meant the viewer read literal
 * `## heading` and `**bold**` for the entire time an answer took to arrive.
 *
 * So nothing is written until it is renderable. Deltas accumulate, the working
 * indicator carries the fact that something is happening, and every *complete*
 * top-level block is revealed as prose the moment it closes.
 */
export function delta(el, text, thinking) {
  if (!text) return;
  if (thinking) {
    el._thinkBuf += text;
    if (!el._thinkEl) el._thinkEl = thinkBlock(el);
    el._thinkEl.body.textContent = el._thinkBuf;
    el._thinkEl.count.textContent = approxTokens(el._thinkBuf) + ' tokens';
    nudge(el);
    return;
  }
  el._streamBuf += text;
  working(el, true);
  // Coalesced onto a frame: a blank-line scan per delta is wasted work when
  // twenty of them land between two paints.
  if (!el._raf) {
    el._raf = requestAnimationFrame(() => {
      el._raf = 0;
      revealComplete(el);
      workingSync(el);
      nudge(el);
    });
  }
}

/**
 * Reveal every block that has finished, keep the rest buffered.
 *
 * Waiting for the whole turn would leave a long answer as nothing but a
 * spinner for as long as it takes to write, which trades one bad experience
 * for another. A blank line is markdown's own block separator, so everything
 * before the last one is complete by definition and can be rendered now.
 *
 * The exception is an open code fence: an odd number of ``` means the reader
 * is mid-block and its blank lines separate nothing, so the whole buffer waits.
 */
export function revealComplete(el) {
  const cut = el._streamBuf.lastIndexOf('\n\n');
  if (cut === -1) return;
  const head = el._streamBuf.slice(0, cut);
  if (!head.trim()) return;
  if (countFences(head) % 2 !== 0) return;
  renderProse(el, head);
  el._streamBuf = el._streamBuf.slice(cut + 2);
}

/** Render one finished unit of markdown into the transcript. */
export function renderProse(el, md) {
  orphanRail(el);
  const host = document.createElement('div');
  host.className = 'pi-prose pi-reveal';
  host.append(markdown(md));
  append(el, host);
}

/** End of a block: render whatever is left, however it ends. */
export function flushStream(el) {
  if (el._raf) {
    cancelAnimationFrame(el._raf);
    el._raf = 0;
  }
  if (el._streamBuf.trim()) {
    renderProse(el, el._streamBuf);
  }
  el._streamBuf = '';
  working(el, false);
}

/** Chain-of-thought, collapsed. Interesting, but not the answer. */
export function thinkBlock(el) {
  const details = document.createElement('details');
  details.className = 'pi-think';
  const summary = document.createElement('summary');
  const label = document.createElement('span');
  label.textContent = 'thinking';
  const count = document.createElement('span');
  count.className = 'pi-think-count';
  summary.append(label, count);
  const body = document.createElement('div');
  body.className = 'pi-think-body';
  details.append(summary, body);
  append(el, details);
  return { details, body, count };
}
