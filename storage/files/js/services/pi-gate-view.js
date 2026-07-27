'use strict';

/**
 * window.SpPiGate — the governance chain, rendered.
 *
 * This is the part of the terminal that no pty can produce. The stream carries a
 * typed `policy_stages` frame naming every policy that ran, in the order it ran,
 * with its real result — so the widget can show the pipeline resolving rather
 * than only reporting its verdict.
 *
 * The load-bearing detail is what happens on a deny: stages after the failure
 * are `skip`, and a skipped pip stays unlit. A visitor watching a blocked call
 * sees the chain stop. That is the whole demonstration, and it is why `skip` is
 * a distinct state here and in the Rust frame rather than being folded into a
 * boolean.
 *
 * Classic script attaching to a namespace, matching sp-pi-terminal.js's
 * no-import convention.
 */

/** Per-pip reveal delay. Long enough to read left-to-right, short enough not to
 *  delay an operator who is waiting to answer an approval card. */
const STAGGER_MS = 90;

const GLYPH = { pass: '✓', fail: '✗', skip: '·' };

/**
 * Human wording for the policies shipped in `policies/mod.rs`, plus the
 * caller-side confinement check.
 *
 * A lookup, not a rename: the pip is always labelled with the real policy id
 * from the frame, and this only supplies the sentence under it. A policy added
 * upstream therefore still renders — unlabelled prose is a cosmetic gap, whereas
 * a missing pip would be a lie about what ran.
 */
const EXPLAIN = {
  scope_check: 'the agent’s scope permits this tool',
  secret_scan: 'no credential pattern in the arguments',
  tool_blocklist: 'the tool is not blocked for this deployment',
  rate_limit: 'the conversation is inside its call budget',
  workspace_scope: 'every path stays inside the session workspace',
  human_approval: 'a person answered',
};

/** Whether to animate at all. Read per call, so an OS-level change mid-session
 *  is honoured without a reload. */
function motionOk() {
  return !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * The chain rail: one pip per stage, in evaluation order.
 *
 * `stages` is the frame's array verbatim — this function never invents, reorders,
 * or filters an entry, because the rail's only claim to being evidence is that it
 * is a direct rendering of what the evaluation reported.
 */
function chainRail(stages, opts) {
  const animate = (!opts || opts.animate !== false) && motionOk();
  // Compact is the inline form, drawn inside the row the chain judged. The pips
  // shrink to their dots and the policy names are revealed on hover or focus —
  // attached to its subject, the rail no longer has to name it in full.
  const compact = !!(opts && opts.compact);
  const rail = document.createElement('div');
  rail.className = compact ? 'pi-rail pi-rail--compact' : 'pi-rail';
  rail.setAttribute('role', 'list');
  rail.setAttribute('aria-label', 'Governance chain');

  stages.forEach((stage, n) => {
    const pip = document.createElement('span');
    pip.className = 'pi-pip';
    pip.dataset.result = stage.result;
    pip.setAttribute('role', 'listitem');

    const dot = document.createElement('span');
    dot.className = 'pi-pip-dot';
    dot.textContent = GLYPH[stage.result] || GLYPH.skip;
    dot.setAttribute('aria-hidden', 'true');

    const name = document.createElement('span');
    name.className = 'pi-pip-name';
    name.textContent = stage.policy;

    pip.append(dot, name);

    // The screen-reader text says the outcome in words; the glyph and the colour
    // are both hidden from it, so nothing depends on either being perceived.
    const sr = document.createElement('span');
    sr.className = 'sp-sr-only';
    const verdict = stage.result === 'pass' ? 'passed'
      : stage.result === 'fail' ? 'failed' : 'not run';
    sr.textContent = ' ' + stage.policy + ' ' + verdict
      + (stage.detail ? ': ' + stage.detail : '') + '. ';
    pip.append(sr);

    // The detail is the policy's own wording, straight from the audit spine, so
    // the tooltip and the trace row cannot disagree about why.
    const why = stage.detail || EXPLAIN[stage.policy] || '';
    if (why) pip.title = stage.policy + ' — ' + why;

    if (animate) {
      pip.classList.add('is-pending');
      setTimeout(() => pip.classList.remove('is-pending'), n * STAGGER_MS);
    }
    rail.append(pip);
  });

  return rail;
}

/**
 * The approval card.
 *
 * A card in a queue, never a modal. The model issues parallel tool calls, each
 * with its own approval_id, and the server resolves them independently — a modal
 * would serialise what the backend does concurrently, and would also hide the
 * transcript the operator needs in order to decide.
 *
 * `onDecide(decision)` is called with 'allow' or 'deny'. Returns a handle with
 * `el` and `settle()`, because resolution can arrive from another tab or from
 * the server's own timeout, not only from these buttons.
 */
function approvalCard(frame, onDecide) {
  const card = document.createElement('div');
  card.className = 'pi-approval-card';
  // alertdialog, not dialog: it interrupts, it is time-limited, and the default
  // outcome if it is ignored is a denial the operator did not choose.
  card.setAttribute('role', 'alertdialog');
  card.setAttribute('aria-label', 'Approve or deny ' + frame.tool_name);

  const head = document.createElement('div');
  head.className = 'pi-approval-head';

  const ring = document.createElement('div');
  ring.className = 'pi-ring';
  const glyph = document.createElement('span');
  glyph.className = 'pi-ring-glyph';
  glyph.textContent = '⏻';
  glyph.setAttribute('aria-hidden', 'true');
  ring.append(glyph);

  const title = document.createElement('div');
  title.className = 'pi-approval-title';
  const tool = document.createElement('strong');
  tool.textContent = frame.tool_name;
  const sub = document.createElement('span');
  sub.className = 'pi-approval-sub';
  sub.textContent = 'wants to run — policy cleared it, you decide';
  title.append(tool, sub);

  const countdown = document.createElement('span');
  countdown.className = 'pi-countdown';
  head.append(ring, title, countdown);

  const args = document.createElement('pre');
  args.className = 'pi-approval-input';
  args.textContent = pretty(frame.tool_input);

  const cleared = document.createElement('div');
  cleared.className = 'pi-approval-cleared';
  const clearedLabel = document.createElement('span');
  clearedLabel.className = 'pi-approval-cleared-label';
  clearedLabel.textContent = 'already cleared';
  cleared.append(clearedLabel);
  // The chain that passed, shown before the question rather than after it. The
  // operator is being asked to add a judgement on top of policy, not to trust a
  // bare prompt — so what policy established has to be on the card.
  cleared.append(chainRail(
    (frame.policy_chain || []).map((p) => ({ policy: p, result: 'pass', detail: EXPLAIN[p] || '' })),
  ));

  const actions = document.createElement('div');
  actions.className = 'pi-approval-actions';
  const deny = document.createElement('button');
  deny.type = 'button';
  deny.className = 'pi-btn pi-btn--deny';
  deny.textContent = 'Deny';
  const allow = document.createElement('button');
  allow.type = 'button';
  allow.className = 'pi-btn pi-btn--allow';
  allow.textContent = 'Approve';
  // Deny first in the DOM, so it is also first in tab order. Three of the four
  // ways an approval can end are denials; the UI should not lean on allow.
  actions.append(deny, allow);

  card.append(head, args, cleared, actions);

  const total = frame.timeout_secs || 0;
  let left = total;
  const tick = () => {
    countdown.textContent = left > 0 ? left + 's' : 'expired';
    countdown.dataset.urgency = left <= 10 ? 'critical' : left <= 30 ? 'warn' : 'calm';
    if (total > 0) {
      const frac = Math.max(0, Math.min(1, left / total));
      ring.style.setProperty('--pi-ring-fill', String(frac));
    }
    // Announced at two thresholds only. A polite live region that updated every
    // second would talk over the operator for the whole window.
    if (left === 30 || left === 10) {
      countdown.setAttribute('role', 'status');
      countdown.setAttribute('aria-label', left + ' seconds left to decide');
    }
    if (left <= 0) clearInterval(handle.timer);
    left -= 1;
  };

  const handle = {
    el: card,
    timer: setInterval(tick, 1000),
    settle() {
      clearInterval(handle.timer);
      card.remove();
    },
    /** Freeze the card while the POST is in flight, so it cannot be answered
     *  twice from one click. */
    lock() {
      allow.disabled = true;
      deny.disabled = true;
      card.classList.add('is-settling');
    },
    focus() {
      // Focus lands on Deny, the conservative option, so an operator answering
      // by keyboard cannot approve a call by reflexively hitting space.
      deny.focus();
    },
  };
  tick();

  allow.addEventListener('click', () => onDecide('allow'));
  deny.addEventListener('click', () => onDecide('deny'));

  return handle;
}

/** A blocked call, given the weight it deserves. */
function blockedRow(frame) {
  const box = document.createElement('div');
  box.className = 'pi-blocked';
  if (motionOk()) box.classList.add('is-arriving');

  const head = document.createElement('div');
  head.className = 'pi-blocked-head';
  const mark = document.createElement('span');
  mark.className = 'pi-blocked-mark';
  mark.textContent = '✗';
  mark.setAttribute('aria-hidden', 'true');
  const what = document.createElement('strong');
  what.textContent = frame.tool_name + ' blocked';
  head.append(mark, what);
  if (frame.policy) {
    const chip = document.createElement('span');
    chip.className = 'pi-policy-chip';
    chip.textContent = frame.policy;
    head.append(chip);
  }

  box.append(head);

  if (frame.reason) {
    const why = document.createElement('p');
    why.className = 'pi-blocked-reason';
    why.textContent = frame.reason;
    box.append(why);
  }

  // Worth stating plainly: the reason above is for the operator. pi's confirm
  // hook answers a bare boolean, so the model is told no and never learns why —
  // which is what stops it from negotiating around the rule.
  const note = document.createElement('p');
  note.className = 'pi-blocked-note';
  note.textContent = 'The agent was told no, and not why. This reason exists for you.';
  box.append(note);

  return box;
}

function pretty(input) {
  try {
    return JSON.stringify(input, null, 2);
  } catch (_) {
    return String(input);
  }
}

window.SpPiGate = { chainRail, approvalCard, blockedRow, motionOk, EXPLAIN };
