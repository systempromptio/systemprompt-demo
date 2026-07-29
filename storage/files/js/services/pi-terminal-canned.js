import { TOOL_ICON } from './pi-constants.js';
import { CANNED, CANNED_LOOP_MS, CANNED_STEP_MS } from './pi-replay.js';
import { markdown } from './pi-markdown.js';
import { motionOk } from './pi-gate-view.js';
import { approvalCard } from './pi-gate-cards.js';
import { blockedRow } from './pi-gate-records.js';
import { endGateRun, gateRecord } from './pi-gate-runs.js';
import { append, line, echo } from './pi-terminal-dom.js';
import { policyStages } from './pi-terminal-rail.js';
import { setReplayChrome } from './pi-terminal-view.js';
import { toolRow } from './pi-terminal-gate.js';
import { cannedMeters } from './pi-terminal-meters.js';

/** Without a usable credential the terminal plays a scripted pass instead, so a
 *  public page can embed it unconditionally. */
export function degrade(el, reason, info) {
  el.classList.add('is-replay');
  el.classList.remove('is-session');
  el._status(reason === 'busy' ? 'session in use'
    : (reason === 'queued' ? 'in line'
      : (reason === 'stream' ? 'disconnected' : 'replay')));
  el._input.disabled = true;
  el._sendBtn.disabled = true;
  setReplayChrome(el, true);

  cannedPlay(el);

  el._gateEl.hidden = false;
  const blurb = document.createElement('p');
  if (reason === 'queued') {
    // The replay plays below so the wait is not a blank screen; the number
    // here is kept current by the capacity heartbeat, and the reconnect is
    // automatic — the one thing the visitor must not do is leave.
    const pos = document.createElement('strong');
    pos.dataset.role = 'queue-pos';
    pos.textContent = '#' + ((info && typeof info.position === 'number')
      ? info.position + 1 : 1);
    blurb.append('Every live slot on this server is taken. You are ', pos,
      ' in line — this terminal will connect automatically when a slot frees. '
      + 'Meanwhile, a replay:');
  } else if (reason === 'stream') {
    // The composer is dead either way; what it must not do is stay dead behind
    // a "reconnecting" label that implies the wait is doing something.
    blurb.append('Lost the connection to this session and could not get it '
      + 'back. ');
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'pi-btn';
    btn.textContent = 'Reconnect';
    btn.addEventListener('click', () => { void el.restart(null); });
    blurb.append(btn, ' to start a fresh one. Meanwhile, a replay:');
  } else if (reason === 'busy') {
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
    // The replay bar above already carries the ask and the buttons, so this
    // says only what the transcript is — two statements of the same CTA read
    // as nagging, and dilute both.
    const lead = document.createElement('strong');
    lead.textContent = 'This is a replay.';
    blurb.append(lead, ' A recording of a real governed session, played back.');
  }
  el._gateEl.replaceChildren(blurb);
}

/**
 * Play the script once, then stop and offer to play it again.
 *
 * Scheduled on a running clock rather than a fixed cadence: a paragraph and a
 * tool row do not deserve the same dwell, and the whole point of the replay is
 * that a visitor can read it. An anonymous visitor is the one being asked to
 * sign up, so they should see the chain resolve the way a real one does — the
 * pacing is the argument.
 *
 * It ends on a button rather than looping back to act 1: the finished
 * transcript is the thing worth reading — scrolling back through the refusal
 * and the approval record is how a visitor engages with it — and a script that
 * wipes itself on a timer takes that away.
 */
function cannedPlay(el) {
  // Reduced motion gets the whole script at once, already ended.
  if (!motionOk()) {
    CANNED.forEach((s) => cannedStep(el, s));
    cannedEnd(el);
    return;
  }
  let at = 0;
  CANNED.forEach((s) => {
    el._cannedTimers.push(setTimeout(() => cannedStep(el, s), at));
    at += typeof s.ms === 'number' ? s.ms : CANNED_STEP_MS;
  });
  el._cannedTimers.push(setTimeout(() => cannedEnd(el), at + CANNED_LOOP_MS));
}

/** Close the pass with the control that starts the next one. */
function cannedEnd(el) {
  // A disconnected or since-credentialled element has no replay to restart.
  if (!el.isConnected || !el.classList.contains('is-replay')) return;
  const foot = document.createElement('div');
  foot.className = 'pi-replay-end';
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'pi-btn pi-replay-restart';
  btn.textContent = 'Play again';
  btn.addEventListener('click', () => {
    cannedReset(el);
    cannedPlay(el);
  });
  foot.append(btn);
  append(el, foot);
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
  el._gateRun = null;
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
    gateRecord(el, 'blocked', blockedRow(step.frame), step.name || step.tool || 'tool');
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
    if (step.resolve) cannedResolve(el, handle, step);
    return;
  }
  if (step.cls === 'prompt') {
    echo(el, step.tail);
    return;
  }
  if (step.cls === 'output') {
    // Prose divides one run of tool calls from the next, the same as the live
    // path's renderProse.
    endGateRun(el);
    const host = document.createElement('div');
    host.className = 'pi-prose';
    host.append(markdown(step.text));
    append(el, host);
    return;
  }
  line(el, step.cls, step.text);
}

/**
 * The replay's card resolves the way a live one does: after a beat, the same
 * settle path folds it into a one-line record stamped "Ed approved at …" and
 * moves it into the transcript. The timestamp is playback time, so every pass
 * reads fresh.
 * Reduced motion skips the beat and shows the resolved state at once.
 */
function cannedResolve(el, handle, step) {
  const finish = () => {
    const record = handle.settle({
      outcome: step.resolve.action,
      approved_by: step.resolve.by,
      decided_at: new Date().toISOString(),
      actor: 'user',
      tool_name: step.tool,
    });
    // No transcript echo: the folded record already carries the verdict, the
    // name and the time on its summary line.
    if (record) {
      gateRecord(el, step.resolve.action === 'approved' ? 'ok' : 'blocked',
        record, step.tool);
    }
  };
  if (!motionOk()) {
    finish();
    return;
  }
  el._cannedTimers.push(setTimeout(finish, step.resolve.afterMs || 3000));
}
