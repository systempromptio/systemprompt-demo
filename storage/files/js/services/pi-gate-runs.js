import { soleOpen } from './pi-gate-parts.js';
import { append, nudge } from './pi-terminal-dom.js';

/**
 * Gate records, aggregated per run.
 *
 * A model that issues six tool calls in a row leaves six records behind, and
 * nothing between them to divide one from the next — the transcript stops
 * being a conversation and becomes a filing cabinet. But the records still
 * matter: what happened cannot be dropped just because it repeated.
 *
 * So a run of them collapses into at most two cards, one per verdict: the
 * calls that got through, and the calls that did not. The tally is the glance
 * version, the individual records are inside it, and the split is by outcome
 * because that is the only distinction worth a second card — a blocked call
 * should never be one row down a list of successes.
 *
 * A run ends when something divides it: assistant prose, or the next prompt.
 * A run of one is left as the bare record — a group of one is a wrapper around
 * nothing.
 */

const KIND_LABEL = { ok: 'cleared', blocked: 'blocked' };
const KIND_MARK = { ok: '✓', blocked: '✗' };

/** Add one record to the current run, grouping it once a second of its kind arrives. */
export function gateRecord(el, kind, node, toolName) {
  const run = el._gateRun || (el._gateRun = {});
  const slot = run[kind];
  if (!slot) {
    run[kind] = { first: node, tools: [toolName], group: null };
    append(el, node);
    return;
  }
  slot.tools.push(toolName);
  if (!slot.group) {
    slot.group = groupEl(kind);
    slot.group.lastChild.append(slot.first);
  }
  slot.group.lastChild.append(node);
  syncHead(slot, kind);
  // Moved to the tail on every addition: tool rows land between records, and a
  // tally stranded above half the calls it counts reads as a tally of
  // something else.
  el._body.append(slot.group);
  nudge(el);
}

/** Something divided the run — the next record starts a fresh one. */
export function endGateRun(el) {
  el._gateRun = null;
}

function groupEl(kind) {
  const group = document.createElement('details');
  group.className = 'pi-gate-group';
  group.dataset.kind = kind;

  const head = document.createElement('summary');
  head.className = 'pi-gate-group-head';
  const mark = document.createElement('span');
  mark.className = 'pi-gate-group-mark';
  mark.textContent = KIND_MARK[kind];
  mark.setAttribute('aria-hidden', 'true');
  const count = document.createElement('strong');
  count.className = 'pi-gate-group-count';
  const tools = document.createElement('span');
  tools.className = 'pi-gate-group-tools';
  const more = document.createElement('span');
  more.className = 'pi-gate-group-more';
  more.setAttribute('aria-hidden', 'true');
  head.append(mark, count, tools, more);

  const body = document.createElement('div');
  body.className = 'pi-gate-group-body';
  group.append(head, body);
  soleOpen(group);
  return group;
}

function syncHead(slot, kind) {
  const n = slot.tools.length;
  slot.group.querySelector('.pi-gate-group-count').textContent
    = n + ' calls ' + KIND_LABEL[kind];
  slot.group.querySelector('.pi-gate-group-tools').textContent = tally(slot.tools);
}

/**
 * `render_artifact ×4, read_file` — which tools, and how many of each. Names
 * beyond the third are counted rather than listed, because a summary line that
 * wraps is no longer a summary line.
 */
function tally(tools) {
  const counts = new Map();
  tools.forEach((t) => counts.set(t, (counts.get(t) || 0) + 1));
  const named = [...counts.entries()].slice(0, 3)
    .map(([name, n]) => (n > 1 ? name + ' ×' + n : name));
  const rest = counts.size - named.length;
  return named.join(', ') + (rest > 0 ? ' +' + rest + ' more' : '');
}
