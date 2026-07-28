/** The platform-wide pulse section of <sp-auth-pane>. */

import { renderAdminPulse } from './sp-pulse-admin.js';
import { statHtml } from './sp-auth-pane-view.js';
import { ms, pct } from './sp-auth-pane-helpers.js';

/** The platform pulse is cached server-side for a minute; match it. */
export const PULSE_POLL_MS = 60000;

/**
 * The token is sent when there is one and omitted when there is not.
 *
 * An anonymous visitor has no embed token — `/embed-token` reads the session
 * cookie — and the endpoint treats its absence as the anonymous tier rather
 * than as a failure. So this runs on the sign-in view too, where it fills in
 * the lifetime totals under the form.
 */
export async function pollPulse(pane) {
  if (!pane._pulse) return;
  const url = pane._token
    ? '/api/public/pi/pulse?token=' + encodeURIComponent(pane._token)
    : '/api/public/pi/pulse';
  try {
    const res = await fetch(url, { credentials: 'same-origin', redirect: 'manual' });
    if (!res.ok) return;
    applyPulse(pane, await res.json());
  } catch (_) {
    // Context, not the visitor's own numbers — a miss costs nothing and the
    // section simply stays as it was.
  }
}

/**
 * Show the deployment counted across everyone, at whatever depth the server
 * decided this caller is owed.
 *
 * There is no tier check here, and deliberately no way to ask for a richer
 * one: the shape of the payload *is* the tier. A window arrives only if the
 * caller is signed in and the window holds enough people to aggregate without
 * identifying them, and `detail` arrives only for an operator. Suppression
 * used to be decided in this file, which meant the sparse numbers were sent
 * and then hidden — a privacy control enforced by the party it protects
 * against. Now they never leave the server.
 *
 * Counts arrive pre-formatted as strings because the member tier rounds them
 * ("1.2k") and the admin tier does not ("1,247"). One render path, two
 * vocabularies, chosen server-side.
 */
export function applyPulse(pane, p) {
  if (!pane._pulse || !p) return;
  const w = p.window;
  const all = p.all_time || {};

  const stats = pane.querySelector('[data-role="pulse-stats"]');
  const models = pane.querySelector('[data-role="pulse-models"]');
  const heading = pane.querySelector('[data-role="pulse-window"]');
  if (heading) heading.textContent = w ? 'last ' + (p.window_hours || 24) + 'h' : 'all time';

  if (w && stats) {
    stats.hidden = false;
    stats.innerHTML = ''
      + statHtml('pPeople', 'People', w.people)
      + statHtml('pSessions', 'Sessions', w.sessions)
      + statHtml('pRequests', 'Requests', w.requests)
      + statHtml('pTools', 'Tool calls', w.tool_calls)
      + statHtml('pAllow', 'Allowed', w.allow_rate_percent === null
        || w.allow_rate_percent === undefined ? '—' : pct(w.allow_rate_percent))
      + statHtml('pLatency', 'Latency p50', ms(w.latency_p50_ms));

    const mix = (w.model_mix || []).slice(0, 3)
      .map((m) => m.model + ' ' + pct(m.percent)).join(' · ');
    const blocked = (w.blocked_tools || [])[0];
    const worst = blocked
      ? ' Most refused: ' + blocked.tool_name + ' ×' + blocked.denials + '.'
      : '';
    models.textContent = (mix ? 'Models: ' + mix + '.' : '') + worst;
    models.hidden = !mix && !worst;
  } else if (stats) {
    stats.hidden = true;
    if (models) models.hidden = true;
  }

  pane.querySelector('[data-role="pulse-all-time"]').textContent = 'All time — '
    + all.sessions + ' sessions, ' + all.requests + ' requests, '
    + all.tool_calls + ' governed tool calls, '
    + all.secrets_caught + ' secrets caught.';

  applyAdminPulse(pane, p.detail);
  pane._pulse.hidden = false;
}

/**
 * The operator block, when the server sent one.
 *
 * Created on first use rather than rendered empty and filled: for two of the
 * three tiers it never arrives, and an empty container that only ever gets
 * hidden is markup every visitor pays for so one does not have to.
 */
export function applyAdminPulse(pane, detail) {
  if (!detail) {
    if (pane._pulseAdmin) pane._pulseAdmin.hidden = true;
    return;
  }
  if (!pane._pulseAdmin) {
    pane._pulseAdmin = document.createElement('div');
    pane._pulseAdmin.className = 'pulse-admin';
    pane._pulse.append(pane._pulseAdmin);
  }
  pane._pulseAdmin.hidden = false;
  renderAdminPulse(pane._pulseAdmin, detail);
}
