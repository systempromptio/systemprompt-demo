import { CAPACITY_MS, CAPACITY_QUEUE_MS } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { degrade } from './pi-terminal-canned.js';

/**
 * The header's slot meter, and the wait line behind it.
 *
 * One loop serves both: idle, it repaints the pips every CAPACITY_MS; queued
 * (the server answered a session request with `reason: "waitlisted"`), it
 * polls with `join=1` — which IS the waitlist heartbeat — every
 * CAPACITY_QUEUE_MS, and restarts the session the moment the server says a
 * slot is this visitor's to take.
 */
export function startCapacity(el) {
  const tick = async () => {
    el._capTimer = null;
    if (!el.isConnected) return;
    const queued = el._queued;
    let url = el._endpoint + '/capacity';
    if (queued && el._token) {
      url += '?join=1&token=' + encodeURIComponent(el._token);
    }
    const body = await getJson(url);
    if (body) {
      renderCapacity(el, body);
      if (queued) {
        updateQueueBlurb(el, body);
        if (body.admissible) {
          el._queued = false;
          // restart() re-runs the whole start sequence, which clears the
          // replay chrome and takes the freed slot.
          el.restart(undefined);
        }
      }
    }
    el._capTimer = setTimeout(tick, el._queued ? CAPACITY_QUEUE_MS : CAPACITY_MS);
  };
  tick();
}

/** Enter queue mode: replay below, "#N in line" above, heartbeat running. */
export function enterQueue(el, info) {
  el._queued = true;
  degrade(el, 'queued', info);
  // Reset the loop onto the faster queued cadence immediately.
  if (el._capTimer) clearTimeout(el._capTimer);
  startCapacity(el);
}

function renderCapacity(el, body) {
  if (!el._capEl) return;
  const max = body.max || 0;
  const used = Math.min(body.used || 0, max);
  if (!max) return;
  el._capEl.hidden = false;
  // Repainted in place: the pip count only changes if the operator re-sizes
  // the cap, so churn here is text and a data attribute, not layout.
  if (el._capPips.childElementCount !== max) {
    el._capPips.replaceChildren();
    for (let i = 0; i < max; i += 1) {
      const pip = document.createElement('i');
      pip.className = 'pi-cap-pip';
      el._capPips.append(pip);
    }
  }
  Array.from(el._capPips.children).forEach((pip, i) => {
    pip.dataset.filled = String(i < used);
  });
  el._capCount.textContent = used + '/' + max;
  const state = used >= max ? 'full' : (used / max >= 0.75 ? 'warn' : 'ok');
  el._capEl.dataset.state = state;
}

function updateQueueBlurb(el, body) {
  const pos = el.querySelector('[data-role="queue-pos"]');
  if (pos && typeof body.position === 'number') {
    pos.textContent = '#' + (body.position + 1);
  }
  el._status('in line');
}
