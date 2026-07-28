import { TOOL_ICON } from './pi-constants.js';
import { CANNED, CANNED_LOOP_MS, CANNED_STEP_MS } from './pi-replay.js';
import { markdown } from './pi-markdown.js';
import { motionOk } from './pi-gate-view.js';
import { approvalCard, blockedRow } from './pi-gate-cards.js';
import { append, line, echo } from './pi-terminal-dom.js';
import { policyStages } from './pi-terminal-rail.js';
import { toolRow } from './pi-terminal-gate.js';
import { cannedMeters } from './pi-terminal-stream.js';

/** Without a usable credential the terminal plays a scripted pass instead, so a
 *  public page can embed it unconditionally. */
export function degrade(el, reason) {
  el.classList.add('is-replay');
  el._status(reason === 'busy' ? 'session in use' : 'replay');
  el._input.disabled = true;
  el._sendBtn.disabled = true;

  cannedPlay(el);

  el._gateEl.hidden = false;
  const blurb = document.createElement('p');
  if (reason === 'busy') {
    // Not "you already have one": a second conversation from the same
    // account displaces the first, so the only 429 left is the server-wide
    // cap, which no action of this user's can clear.
    blurb.textContent = 'Every pi session on this server is in use. '
      + 'Sessions free up as they finish or go idle — reload in a minute.';
  } else if (el._who && el._who.email) {
    // Signed in but no token: the account exists and the terminal is
    // configured, so this is a server-side problem, not a sign-in prompt.
    const email = document.createElement('strong');
    email.textContent = el._who.email;
    blurb.append('Signed in as ', email, ', but no session could be started. '
      + 'The terminal may not be configured on this deployment.');
  } else {
    // Anonymous. The pane beside this terminal owns sign-in and registration —
    // one implementation of the ceremony, and it is the half of the screen the
    // visitor is already looking at. Here, say only what the replay is.
    const lead = document.createElement('strong');
    lead.textContent = 'This is a replay.';
    blurb.append(lead, ' Create an account or sign in to drive a real agent — '
      + 'every tool call it makes will stop here for your approval.');
  }
  el._gateEl.replaceChildren(blurb);
}

/**
 * Play the script, then play it again.
 *
 * Scheduled on a running clock rather than a fixed cadence: a paragraph and a
 * tool row do not deserve the same dwell, and the whole point of the replay is
 * that a visitor can read it. An anonymous visitor is the one being asked to
 * sign up, so they should see the chain resolve the way a real one does — the
 * pacing is the argument.
 *
 * Looping matters for the same reason: the visitor who arrives during act 3
 * would otherwise never be told what any of it is.
 */
function cannedPlay(el) {
  // Reduced motion gets the whole script at once, and no loop — a transcript
  // that rewrites itself on a timer is exactly what was asked to stop.
  if (!motionOk()) {
    CANNED.forEach((s) => cannedStep(el, s));
    return;
  }
  let at = 0;
  CANNED.forEach((s) => {
    el._cannedTimers.push(setTimeout(() => cannedStep(el, s), at));
    at += typeof s.ms === 'number' ? s.ms : CANNED_STEP_MS;
  });
  el._cannedTimers.push(setTimeout(() => {
    // A disconnected or since-credentialled element must not keep looping.
    if (!el.isConnected || !el.classList.contains('is-replay')) return;
    cannedReset(el);
    cannedPlay(el);
  }, at + CANNED_LOOP_MS));
}

/** Wipe what the last pass drew, leaving the chrome and the gate blurb. */
function cannedReset(el) {
  el._cannedTimers.forEach(clearTimeout);
  el._cannedTimers = [];
  el._cannedCards.forEach((c) => c.settle());
  el._cannedCards = [];
  el._approvals.forEach((a) => a.settle());
  el._approvals.clear();
  el._approvalsEl.replaceChildren();
  el._body.replaceChildren();
  el._toolRows.clear();
  el._cannedRow = null;
  el._railFor = null;
  el._railDecision = null;
  el._lines = 0;
  el._pinned = true;
  // Back to zero, so each pass counts up from nothing like a fresh session.
  cannedMeters(el, { calls: 0, blocked: 0, tokens: 0, cost: '$0.00' });
}

function cannedStep(el, step) {
  // Applied first, whichever branch the step takes. The reduced-motion path
  // dumps every step at once and correctly ends on the final totals.
  if (step.meters) cannedMeters(el, step.meters);
  if (step.cls === 'stages') {
    policyStages(el, { stages: step.stages });
    return;
  }
  if (step.cls === 'tool') {
    // Held so the matching tool-end can flip it. The live path keys rows by
    // tool_use_id; a scripted one has no ids and no concurrency, so the last
    // row drawn is unambiguously the one being ended.
    el._cannedRow = toolRow(el, step.name, step.arg, step.input || { path: step.arg });
    return;
  }
  if (step.cls === 'tool-end') {
    const row = el._cannedRow;
    el._cannedRow = null;
    if (!row) return;
    row.details.dataset.state = step.state;
    row.icon.textContent = TOOL_ICON[step.state] || TOOL_ICON.ok;
    row.state.textContent = step.state === 'ok' ? 'ran' : step.state;
    return;
  }
  if (step.cls === 'blocked') {
    const row = el._cannedRow;
    el._cannedRow = null;
    if (row) {
      row.details.dataset.state = 'blocked';
      row.icon.textContent = TOOL_ICON.blocked;
      row.state.textContent = 'blocked';
    }
    append(el, blockedRow(step.frame));
    return;
  }
  if (step.cls === 'note') {
    // Commentary, not agent output — the extra class styles it as an aside.
    line(el, 'output-dim pi-note', step.text);
    return;
  }
  if (step.cls === 'approval') {
    const handle = approvalCard({
      tool_name: step.tool,
      tool_input: step.input || { path: step.arg },
      policy_chain: step.stages.map((s) => s.policy),
      timeout_secs: 120,
    }, () => {});
    handle.el.classList.add('pi-approval-card--canned');
    // A replay's buttons must not look answerable: the card is evidence of
    // what the real thing does, not an offer to do it.
    handle.lock();
    handle.el.setAttribute('aria-label', 'Example approval card (replay)');
    // Held so its countdown interval dies with the loop rather than outliving
    // the card the next pass replaces.
    el._cannedCards.push(handle);
    el._approvalsEl.append(handle.el);
    return;
  }
  if (step.cls === 'prompt') {
    echo(el, step.tail);
    return;
  }
  if (step.cls === 'output') {
    const host = document.createElement('div');
    host.className = 'pi-prose';
    host.append(markdown(step.text));
    append(el, host);
    return;
  }
  line(el, step.cls, step.text);
}
