import { approvalGrid, metaRow, settledRecord, toolTitle } from './pi-gate-parts.js';

/**
 * The approval card — the one row of the gate that asks a question.
 *
 * A card in a queue, never a modal. The model issues parallel tool calls, each
 * with its own approval_id, and the server resolves them independently — a modal
 * would serialise what the backend does concurrently, and would also hide the
 * transcript the operator needs in order to decide.
 *
 * Built on the chain rail rather than beside it: the card shows the policies
 * that already passed, so it cannot state a verdict the rail would contradict.
 * The operator is being asked to add a judgement on top of policy, not to
 * trust a bare prompt.
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
  const countdown = document.createElement('span');
  countdown.className = 'pi-countdown';
  const title = toolTitle(frame.tool_name, 'wants to run — policy cleared it, you decide');
  head.append(ring, title, countdown);

  const meta = metaRow(
    frame.policy_chain || [],
    frame.timeout_secs ? ['auto-denied if ignored'] : [],
  );

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

  card.append(head, approvalGrid(frame, 'already cleared'), meta, actions);

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
    /**
     * Resolution arrived. With an attributed frame the card becomes a
     * permanent record — buttons and countdown out, folded into a one-line
     * summary — and is returned so the caller can move it into the
     * transcript. Without one (expired, torn down) it just goes away.
     */
    settle(frame) {
      clearInterval(handle.timer);
      if (!frame || !frame.outcome) {
        card.remove();
        return null;
      }
      actions.remove();
      countdown.remove();
      ring.remove();
      card.classList.remove('is-settling');
      card.classList.add('is-settled');
      card.dataset.outcome = frame.outcome;
      card.removeAttribute('role');
      card.removeAttribute('aria-label');
      // The question is answered; the sub-line stops asking it.
      const sub = title.querySelector('.pi-approval-sub');
      if (sub) {
        sub.textContent = frame.outcome === 'approved'
          ? 'policy cleared it, then a person did'
          : 'policy cleared it, a person did not';
      }
      card.remove();
      return settledRecord(card, frame);
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
