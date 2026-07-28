/** The admin-only Platform tab of <sp-auth-pane>, as one self-contained unit. */

import { renderAdminPulse } from './sp-pulse-admin.js';
import { statHtml } from './sp-auth-pane-view.js';
import { ms, pct } from './sp-auth-pane-helpers.js';

/** The platform pulse is cached server-side for a minute; match it. */
const POLL_MS = 60000;

/**
 * Everything the Platform tab is — its elements, its timer, its token, its
 * lazily created operator block — lives inside this closure. The pane holds
 * the returned handle and nothing else: `setToken` when the terminal delivers
 * an embed token, `stop` on sign-out or disconnect. No pulse state sits on
 * the element, so no other module can half-own it.
 *
 * There is no role check here, and deliberately no way to ask for one: the
 * shape of the payload *is* the tier. The `detail` block arrives only for an
 * operator, and its presence is what reveals the tab. The first answer
 * without it retires the whole unit — for everyone but an admin the tab
 * never exists, and neither does the polling.
 */
export function createPulse(pane) {
  const tab = pane.querySelector('[data-tab="platform"]');
  const section = pane.querySelector('[data-role="pulse"]');
  const heading = pane.querySelector('[data-role="pulse-window"]');
  const stats = pane.querySelector('[data-role="pulse-stats"]');
  const models = pane.querySelector('[data-role="pulse-models"]');
  const allTime = pane.querySelector('[data-role="pulse-all-time"]');

  let token = null;
  let adminBlock = null;
  let timer = setInterval(refresh, POLL_MS);

  function stop() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  /**
   * The embed token is the credential the endpoint tiers on; before it
   * arrives a poll could only ever be answered as anonymous, so don't ask.
   * Its arrival triggers an immediate refresh rather than waiting out up to
   * a minute of the timer.
   */
  function setToken(t) {
    token = t;
    refresh();
  }

  async function refresh() {
    if (!timer || !token) return;
    try {
      const res = await fetch('/api/public/pi/pulse?token=' + encodeURIComponent(token), {
        credentials: 'same-origin', redirect: 'manual',
      });
      if (res.ok) render(await res.json());
    } catch (_) {
      // Context, not the visitor's own numbers — a miss costs nothing and
      // the tab simply stays as it was.
    }
  }

  function render(p) {
    if (!p) return;
    renderRibbon(p);
    if (!p.detail) {
      stop();
      return;
    }
    if (tab) tab.hidden = false;
    if (heading) heading.textContent = p.window ? 'last ' + (p.window_hours || 24) + 'h' : 'all time';
    renderWindow(p.window);
    renderAllTime(p.all_time || {});
    renderDetail(p.detail);
  }

  /**
   * The Overview's one-line proof of scale, for every tier. All-time totals
   * are always present; the 24h window joins the sentence when the tier (and
   * the aggregation floor) allows it. Numbers arrive pre-rounded server-side,
   * so nothing identifying is on this line for a member to see.
   */
  function renderRibbon(p) {
    const ribbon = pane.querySelector('[data-role="platform-ribbon"]');
    if (!ribbon || !p.all_time) return;
    const all = p.all_time;
    let text = 'Across this deployment: ' + all.requests + ' governed requests, '
      + all.tool_calls + ' tool calls, ' + all.secrets_caught + ' secrets caught.';
    if (p.window) {
      text += ' Last ' + (p.window_hours || 24) + 'h: ' + p.window.requests
        + ' requests from ' + p.window.people + ' people.';
    }
    ribbon.textContent = text;
    ribbon.hidden = false;
  }

  /**
   * Counts arrive pre-formatted as strings because the member tier rounds
   * them ("1.2k") and the admin tier does not ("1,247"). One render path,
   * two vocabularies, chosen server-side.
   */
  function renderWindow(w) {
    if (!stats) return;
    stats.hidden = !w;
    if (models) models.hidden = true;
    if (!w) return;
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
    if (models) {
      models.textContent = (mix ? 'Models: ' + mix + '.' : '') + worst;
      models.hidden = !mix && !worst;
    }
  }

  function renderAllTime(all) {
    if (!allTime) return;
    allTime.textContent = 'All time — '
      + all.sessions + ' sessions, ' + all.requests + ' requests, '
      + all.tool_calls + ' governed tool calls, '
      + all.secrets_caught + ' secrets caught.';
  }

  /**
   * The operator block, created on first use: it exists only after the
   * server has already said this caller is an admin, so there is nothing to
   * pre-render for anyone else.
   */
  function renderDetail(detail) {
    if (!adminBlock) {
      adminBlock = document.createElement('div');
      adminBlock.className = 'pulse-admin';
      section.append(adminBlock);
    }
    renderAdminPulse(adminBlock, detail);
  }

  refresh();
  return { setToken, stop };
}
