import { CAPACITY_QUEUE_MS } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { degrade } from './pi-terminal-canned.js';

/**
 * The waitlist heartbeat, and nothing else.
 *
 * It only runs while this visitor is queued (the server answered a session
 * request with `reason: "waitlisted"`): polling `/capacity?join=1` IS the
 * heartbeat that holds the place in line, and it restarts the session the
 * moment the server says a slot is this visitor's to take. Idle occupancy is
 * not the header's business — nothing polls until a visitor is waiting.
 */
function startQueueHeartbeat(el) {
  const tick = async () => {
    el._capTimer = null;
    if (!el.isConnected || !el._queued) return;
    let url = el._endpoint + '/capacity';
    if (el._token) url += '?join=1&token=' + encodeURIComponent(el._token);
    const body = await getJson(url);
    if (body) {
      updateQueueBlurb(el, body);
      if (body.admissible) {
        el._queued = false;
        // restart() re-runs the whole start sequence, which clears the
        // replay chrome and takes the freed slot.
        el.restart(undefined);
        return;
      }
    }
    el._capTimer = setTimeout(tick, CAPACITY_QUEUE_MS);
  };
  tick();
}

/** Enter queue mode: replay below, "#N in line" above, heartbeat running. */
export function enterQueue(el, info) {
  el._queued = true;
  degrade(el, 'queued', info);
  if (el._capTimer) clearTimeout(el._capTimer);
  startQueueHeartbeat(el);
}

function updateQueueBlurb(el, body) {
  const pos = el.querySelector('[data-role="queue-pos"]');
  if (pos && typeof body.position === 'number') {
    pos.textContent = '#' + (body.position + 1);
  }
  el._status('in line');
}
