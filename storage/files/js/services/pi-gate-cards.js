import { pretty } from './pi-format.js';
import { EXPLAIN, chainRail, motionOk } from './pi-gate-view.js';

/**
 * The two rows a governance decision can produce: a call waiting on a person,
 * and a call that never ran.
 *
 * Both are built on the chain rail rather than beside it — an approval card
 * shows the policies that already passed, and a blocked row names the one that
 * failed, so neither can state a verdict the rail would contradict.
 */

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
export function approvalCard(frame, onDecide) {
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

  // Arguments as a key/value ledger when the input is flat — `TO board@acme.com`
  // reads at a glance where a JSON blob has to be parsed by eye. Anything
  // nested falls back to the pretty-printed form, which stays exact.
  const args = kvArgs(frame.tool_input);

  const chain = (frame.policy_chain || []);
  const cleared = document.createElement('div');
  cleared.className = 'pi-approval-cleared';
  const clearedLabel = document.createElement('span');
  clearedLabel.className = 'pi-approval-cleared-label';
  clearedLabel.textContent = 'already cleared';
  cleared.append(clearedLabel);
  // The chain that passed, shown before the question rather than after it. The
  // operator is being asked to add a judgement on top of policy, not to trust a
  // bare prompt — so what policy established has to be on the card. Compact:
  // the dots carry the verdicts and the names arrive on hover or focus.
  cleared.append(chainRail(
    chain.map((p) => ({ policy: p, result: 'pass', detail: EXPLAIN[p] || '' })),
    { compact: true },
  ));

  // Args on the left, the cleared chain on the right — the two facts the
  // decision rests on, side by side instead of stacked into a banner.
  const grid = document.createElement('div');
  grid.className = 'pi-approval-grid';
  grid.append(args, cleared);

  // Small true facts, stated as chips: how much of the chain passed, and what
  // ignoring the card does. Both come from the frame, not from copy.
  const meta = document.createElement('div');
  meta.className = 'pi-detail-row';
  if (chain.length) {
    meta.append(detailChip(chain.length + '/' + chain.length + ' policies passed'));
  }
  if (frame.timeout_secs) meta.append(detailChip('auto-denied if ignored'));

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

  card.append(head, grid, meta, actions);

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

/**
 * A call the gate cleared without asking anyone — approve_all is off, so policy
 * alone decided. A compact, non-interactive record: the same facts the human
 * card shows (tool, args, cleared chain), minus the question.
 */
export function autoApprovedCard(frame) {
  const card = document.createElement('div');
  card.className = 'pi-approval-card pi-approval-card--auto';

  const head = document.createElement('div');
  head.className = 'pi-approval-head';
  const mark = document.createElement('span');
  mark.className = 'pi-auto-mark';
  mark.textContent = '✓';
  mark.setAttribute('aria-hidden', 'true');
  const title = document.createElement('div');
  title.className = 'pi-approval-title';
  const tool = document.createElement('strong');
  tool.textContent = frame.tool_name;
  const sub = document.createElement('span');
  sub.className = 'pi-approval-sub';
  sub.textContent = 'ran — policy cleared it, no human asked';
  title.append(tool, sub);
  head.append(mark, title);

  const args = kvArgs(frame.tool_input);

  const chain = (frame.policy_chain || []);
  const cleared = document.createElement('div');
  cleared.className = 'pi-approval-cleared';
  const clearedLabel = document.createElement('span');
  clearedLabel.className = 'pi-approval-cleared-label';
  clearedLabel.textContent = 'cleared';
  cleared.append(clearedLabel);
  cleared.append(chainRail(
    chain.map((p) => ({ policy: p, result: 'pass', detail: EXPLAIN[p] || '' })),
    { compact: true },
  ));

  const grid = document.createElement('div');
  grid.className = 'pi-approval-grid';
  grid.append(args, cleared);

  const meta = document.createElement('div');
  meta.className = 'pi-detail-row';
  if (chain.length) {
    meta.append(detailChip(chain.length + '/' + chain.length + ' policies passed'));
  }
  meta.append(detailChip('auto-approved'));

  card.append(head, grid, meta);
  return card;
}

/** A blocked call, given the weight it deserves. */
export function blockedRow(frame) {
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

  // Detail chips, right-aligned in the head row. Facts about the evaluation —
  // pattern counts, timing — belong in chrome, not padded into the reason prose.
  if (frame.meta) {
    const detail = document.createElement('span');
    detail.className = 'pi-detail-row pi-blocked-meta';
    Object.values(frame.meta).forEach((v) => {
      if (v) detail.append(detailChip(String(v)));
    });
    head.append(detail);
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

/** One small true fact, as a pill. */
function detailChip(text) {
  const chip = document.createElement('span');
  chip.className = 'pi-detail-chip';
  chip.textContent = text;
  return chip;
}

/**
 * A tool's arguments as a key/value ledger, when they are flat enough to be
 * one. Nested or non-object input falls back to the exact pretty-printed JSON —
 * a card asking for a judgement must never summarise away what it is asking
 * about.
 */
function kvArgs(input) {
  // A tool with no parameters is a fact worth stating, not a `{}` to decode.
  const empty = input == null || (typeof input === 'object' && !Array.isArray(input)
    && !Object.keys(input).length);
  if (empty) {
    const none = document.createElement('p');
    none.className = 'pi-approval-noargs';
    none.textContent = 'no arguments';
    return none;
  }
  const flat = typeof input === 'object' && !Array.isArray(input)
    && Object.values(input).every((v) => ['string', 'number', 'boolean'].includes(typeof v));
  if (!flat) {
    const pre = document.createElement('pre');
    pre.className = 'pi-approval-input';
    pre.textContent = pretty(input);
    return pre;
  }
  const dl = document.createElement('dl');
  dl.className = 'pi-approval-kv';
  Object.entries(input).forEach(([k, v]) => {
    const dt = document.createElement('dt');
    dt.textContent = k;
    const dd = document.createElement('dd');
    dd.textContent = String(v);
    dl.append(dt, dd);
  });
  return dl;
}
