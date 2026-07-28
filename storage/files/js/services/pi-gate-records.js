import { motionOk } from './pi-gate-view.js';
import {
  approvalGrid, attributionStamp, detailChip, metaRow, toolTitle,
} from './pi-gate-parts.js';

/**
 * The two rows a decision leaves behind once no person is involved: a call
 * policy cleared on its own, and a call that never ran. Both are records, not
 * questions — the interactive card lives in ./pi-gate-cards.js.
 */

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
  head.append(mark, toolTitle(frame.tool_name, 'ran — policy cleared it, no human asked'));

  card.append(
    head,
    approvalGrid(frame, 'cleared'),
    metaRow(frame.policy_chain || [], ['auto-approved']),
    attributionStamp({ name: 'policy', actor: 'system', action: 'cleared this call — no human asked' }),
  );
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
