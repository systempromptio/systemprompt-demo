import { TOOL_ICON } from './pi-constants.js';
import { summarise, pretty } from './pi-format.js';
import { approvalCard } from './pi-gate-cards.js';
import { autoApprovedCard, blockedRow } from './pi-gate-records.js';
import { append, line, nudge } from './pi-terminal-dom.js';
import { railEl } from './pi-terminal-rail.js';
import { flushStream } from './pi-terminal-prose.js';

/** Tool calls and the approvals that gate them — the rows the rail attaches to. */

export function toolStart(el, f) {
  // Claim the held chain before flushing: renderProse orphans anything still
  // unclaimed, and this call is exactly the thing that was going to claim it.
  const held = el._railFor;
  const heldDecision = el._railDecision;
  el._railFor = null;
  el._railDecision = null;
  flushStream(el);
  el._railFor = held;
  el._railDecision = heldDecision;
  const row = toolRow(el, f.tool_name, summarise(f.tool_input), f.tool_input);
  // tool_use_id is nullable; fall back to a per-row key so two concurrent
  // calls of the same tool cannot collide.
  el._toolRows.set(f.tool_use_id || 'anon:' + el._lastSeq, row);
}

/**
 * One tool call, expandable.
 *
 * The summary is the glance version; the arguments are one click away. Keying
 * is on dataset.tool rather than on the rendered text, so relabelling a row
 * cannot break the lookup that pairs it with its tool_end.
 */
export function toolRow(el, name, arg, input) {
  const details = document.createElement('details');
  details.className = 'pi-tool';
  details.dataset.tool = name;
  details.dataset.state = 'pending';

  const summary = document.createElement('summary');
  const icon = document.createElement('span');
  icon.className = 'pi-tool-icon';
  icon.textContent = TOOL_ICON.pending;
  icon.setAttribute('aria-hidden', 'true');
  const label = document.createElement('span');
  label.className = 'pi-tool-name';
  label.textContent = name;
  const argEl = document.createElement('span');
  argEl.className = 'pi-tool-arg';
  argEl.textContent = arg || '';
  const state = document.createElement('span');
  state.className = 'pi-tool-state';
  state.textContent = 'awaiting the gate';
  summary.append(icon, label, argEl, state);
  if (el._railFor) {
    if (el._railFor.some((st) => st.result === 'fail')) {
      details.classList.add('is-denied');
    }
    summary.append(railEl(el, el._railFor, true));
    el._railFor = null;
    el._railDecision = null;
  }

  const body = document.createElement('pre');
  body.className = 'pi-tool-body';
  // A tool with no parameters is a fact worth stating, not a `{}` to decode.
  const empty = input == null || (typeof input === 'object' && !Array.isArray(input)
    && !Object.keys(input).length);
  if (empty) body.classList.add('pi-tool-body--empty');
  body.textContent = empty ? 'no arguments' : pretty(input);
  details.append(summary, body);

  append(el, details);
  return { details, icon, state };
}

export function toolEnd(el, f) {
  const row = takeRow(el, f.tool_use_id, f.tool_name);
  if (!row) return;
  if (row.details.dataset.state === 'blocked') return;
  row.details.dataset.state = f.ok ? 'ok' : 'failed';
  row.icon.textContent = f.ok ? TOOL_ICON.ok : TOOL_ICON.blocked;
  row.state.textContent = f.ok ? 'ran' : 'failed';
}

export function toolBlocked(el, f) {
  const row = takeRow(el, f.tool_use_id, f.tool_name);
  if (row) {
    row.details.dataset.state = 'blocked';
    row.icon.textContent = TOOL_ICON.blocked;
    row.state.textContent = 'blocked';
  }
  append(el, blockedRow(f));
}

export function promptBlocked(el, f) {
  line(el, 'output-warn', 'Prompt blocked'
    + (f.policy ? ' by ' + f.policy : '') + (f.reason ? ' — ' + f.reason : '')
    + '. It never reached a provider.');
}

function takeRow(el, id, name) {
  const key = id || null;
  if (key && el._toolRows.has(key)) {
    const row = el._toolRows.get(key);
    el._toolRows.delete(key);
    return row;
  }
  // Unkeyed fallback: the oldest still-pending row for this tool name, matched
  // on the element's own data attribute rather than on its rendered text.
  for (const [k, row] of el._toolRows) {
    if (row.details.dataset.tool === name) {
      el._toolRows.delete(k);
      return row;
    }
  }
  return null;
}

/**
 * Rendered inline as a queue rather than a modal: the model issues parallel
 * tool calls, each with its own approval_id, and the backend resolves them
 * independently. A modal would serialise what the server does concurrently.
 */
export function approvalRequest(el, f) {
  const handle = approvalCard(f, (decision) => {
    handle.lock();
    void decide(el, f.approval_id, decision);
  });
  el._approvals.set(f.approval_id, handle);
  el._approvalsEl.append(handle.el);
  // Focus moves to the card because a turn is now blocked on this answer, and
  // the operator's attention should not have to be recruited by a colour.
  handle.focus();
  nudge(el);
}

/**
 * A call the gate cleared on its own. It goes into the transcript, not the
 * approval queue — nothing is pending and nothing needs focus.
 */
export function approvalAuto(el, f) {
  append(el, autoApprovedCard(f));
}

async function decide(el, approvalId, decision) {
  const res = await el._post('approve', { approval_id: approvalId, decision });
  // 409 means it was already settled — by the timeout, by another viewer, or
  // by abandonment. Say so rather than implying the click landed.
  if (res && res.status === 409) settle(el, approvalId, 'expired');
}

export function approvalResolved(el, f) {
  // Resolution can arrive from another tab or from the server's own timeout,
  // so this must clear the card regardless of what this tab did.
  settle(el, f.approval_id, f.outcome, f);
}

function settle(el, approvalId, outcome, frame) {
  const entry = el._approvals.get(approvalId);
  if (!entry) return;
  const record = entry.settle(frame);
  if (record) append(el, record);
  el._approvals.delete(approvalId);
  // The record carries the verdict, the name and the time on its own summary
  // line. A transcript line saying the same thing again is the noise, not the
  // record — so it is only written when there is no record to read.
  if (!record) {
    line(el, outcome === 'approved' ? 'output-dim' : 'output-warn', settleLine(outcome, frame));
  }
  // Nothing else is queued, so put the caret back where typing continues.
  if (!el._approvals.size && !el._input.disabled) el._input.focus();
}

/**
 * "approval approved by Ed — 14:02:11" when a person answered; the bare
 * outcome (plus a system tag for timeouts) when nobody did. The transcript
 * line is the glance version of the stamp on the card above it.
 */
function settleLine(outcome, frame) {
  if (frame && frame.approved_by) {
    const at = frame.decided_at ? ' — ' + new Date(frame.decided_at).toLocaleTimeString() : '';
    return 'approval ' + outcome + ' by ' + frame.approved_by + at;
  }
  if (frame && frame.actor === 'system') return 'approval ' + outcome + ' (system)';
  return 'approval ' + outcome;
}
