'use strict';

/**
 * The operator block of the platform pulse.
 *
 * Split out of `sp-auth-pane.js` because it is the largest thing that element
 * renders and the rarest — it appears for one tier of one audience, and folding
 * it into a file that already handles WebAuthn, session telemetry and a
 * governance feed would have made the common path harder to read for the sake
 * of the uncommon one.
 *
 * It renders whatever it is handed. The decision about whether an operator is
 * entitled to this block was made on the server, which is why there is no role
 * check anywhere in this file and no way for the browser to ask for it: if
 * `detail` is present in the payload, the server already decided.
 *
 * Every value here is escaped on the way in — `tool_name`, `agent_id`,
 * `display_name`, `page_url`, `source` and `country` are all attacker-influenced
 * (a tool name comes from a request body, a referrer from a header), and this
 * block is the one place they are rendered to a privileged viewer.
 */

/** Rows shown per table. Beyond this the pane scrolls rather than informs. */
const ROWS = 8;

/**
 * Render the admin detail into `host`.
 *
 * @param {HTMLElement} host  container, emptied first
 * @param {object} detail     the `detail` block from `/api/public/pi/pulse`
 */
export function renderAdminPulse(host, detail) {
  if (!host || !detail) return;
  const t = detail.traffic || {};
  const k = t.kpis || {};
  const rt = detail.realtime || {};
  const act = detail.activity || {};

  host.innerHTML = ''
    + '<h4 class="pulse-admin-h">Right now</h4>'
    + tiles([
      ['Sessions this hour', num(rt.sessions_this_hour)],
      ['Views this hour', num(rt.page_views_this_hour)],
      ['Visitors today', num(rt.unique_visitors_today)],
      ['Active accounts 24h', num(detail.active_users_24h)],
    ])

    // KPIs carry their own previous period, so every one of these can show a
    // direction. A number without a trend is a number an operator has to
    // remember last week's value of.
    + '<h4 class="pulse-admin-h">Traffic, 30 days</h4>'
    + tiles([
      ['Sessions', num(k.sessions_current), delta(k.sessions_current, k.sessions_previous)],
      ['Page views', num(k.page_views_current), delta(k.page_views_current, k.page_views_previous)],
      ['Unique visitors', num(k.unique_visitors_current),
        delta(k.unique_visitors_current, k.unique_visitors_previous)],
      ['Avg time', secs(k.avg_time_ms_current), delta(k.avg_time_ms_current, k.avg_time_ms_previous)],
      ['Avg scroll', pct(k.avg_scroll_current), delta(k.avg_scroll_current, k.avg_scroll_previous)],
    ])

    + spark('Sessions per day', (t.timeseries || []).map((b) => b.sessions))

    + twoCol(
      table('Sources', ['Source', 'Sessions'],
        (t.sources || []).slice(0, ROWS).map((s) => [s.source || 'direct', num(s.sessions)])),
      table('Countries', ['Country', 'Sessions'],
        (t.geo || []).slice(0, ROWS).map((g) => [g.country || 'unknown', num(g.sessions)])),
    )
    + twoCol(
      table('Devices', ['Device', 'Sessions'],
        (t.devices || []).slice(0, ROWS).map((d) => [d.device || 'unknown', num(d.sessions)])),
      table('Top pages', ['Page', 'Views'],
        (t.top_pages || []).slice(0, ROWS).map((p) => [p.page_url || '/', num(p.events)])),
    )

    + '<h4 class="pulse-admin-h">What they are running</h4>'
    + tiles([
      ['Events today', num(act.events_today)],
      ['Events this week', num(act.events_this_week)],
      ['Tool calls', num(act.mcp_tool_calls)],
      ['Tool errors', num(act.mcp_errors)],
      ['Logins', num(act.total_logins)],
    ])

    + twoCol(
      table('Tools, 7 days', ['Tool', 'Calls', 'Errors'],
        (detail.tools || []).slice(0, ROWS)
          .map((x) => [x.tool_name, num(x.calls), num(x.errors)])),
      table('Agents, 7 days', ['Agent', 'Calls', 'Errors'],
        (detail.agents || []).slice(0, ROWS)
          .map((x) => [x.agent_id, num(x.calls), num(x.errors)])),
    )
    + twoCol(
      table('Popular skills', ['Skill', 'Uses'],
        (detail.popular_skills || []).slice(0, ROWS)
          .map((s) => [s.tool_name, num(s.count)])),
      // Success rate rather than raw failures: a tool called twice and failing
      // once is a different problem from one called ten thousand times and
      // failing fifty, and the count alone cannot tell them apart.
      table('Tool reliability', ['Tool', 'Calls', 'Success'],
        (detail.tool_success || []).slice(0, ROWS)
          .map((s) => [s.tool_name, num(s.total), pct(s.success_pct)])),
    )

    + hours(detail.hourly_activity || [])

    + table('Busiest accounts', ['Account', 'Logins', 'Tool calls', 'Last active'],
      (detail.top_users || []).slice(0, ROWS).map((u) => [
        u.display_name || u.user_id, num(u.logins), num(u.mcp_calls), when(u.last_active),
      ]));
}

// ── pieces ──────────────────────────────────────────────────────────────────

function tiles(rows) {
  return '<dl class="pulse-admin-tiles">'
    + rows.map(([label, value, d]) => ''
      + '<div class="pulse-admin-tile">'
      + '<dt>' + esc(label) + '</dt>'
      + '<dd>' + esc(value) + (d || '') + '</dd>'
      + '</div>').join('')
    + '</dl>';
}

function table(title, head, rows) {
  if (!rows.length) {
    return '<section class="pulse-admin-block"><h4 class="pulse-admin-h">' + esc(title) + '</h4>'
      + '<p class="pulse-admin-empty">Nothing recorded yet.</p></section>';
  }
  return '<section class="pulse-admin-block">'
    + '<h4 class="pulse-admin-h">' + esc(title) + '</h4>'
    + '<div class="pulse-admin-scroll"><table class="pulse-admin-table">'
    + '<thead><tr>' + head.map((h) => '<th>' + esc(h) + '</th>').join('') + '</tr></thead>'
    + '<tbody>' + rows.map((r) => '<tr>'
      + r.map((c, i) => '<td' + (i ? ' class="num"' : '') + '>' + esc(c) + '</td>').join('')
      + '</tr>').join('') + '</tbody>'
    + '</table></div></section>';
}

function twoCol(a, b) {
  return '<div class="pulse-admin-cols">' + a + b + '</div>';
}

/**
 * A bar per bucket, scaled to the largest.
 *
 * Bars rather than a line: the series is daily counts over a month, which is
 * discrete data, and a line between two days implies values at 3am that were
 * never measured.
 */
function spark(title, values) {
  if (!values.length) return '';
  const max = Math.max.apply(null, values.concat([1]));
  return '<section class="pulse-admin-block">'
    + '<h4 class="pulse-admin-h">' + esc(title) + '</h4>'
    + '<div class="pulse-admin-spark" role="img" aria-label="'
    + esc(title + ', peak ' + max) + '">'
    + values.map((v) => '<span style="height:'
      + Math.max(2, Math.round((Number(v) || 0) / max * 100)) + '%"></span>').join('')
    + '</div></section>';
}

/** Activity by hour of day, as a 24-slot histogram. */
function hours(rows) {
  if (!rows.length) return '';
  const byHour = new Array(24).fill(0);
  rows.forEach((r) => {
    const h = Number(r.hour);
    if (h >= 0 && h < 24) byHour[h] = Number(r.count) || 0;
  });
  return spark('Activity by hour (UTC)', byHour);
}

// ── formatting ──────────────────────────────────────────────────────────────

function num(n) {
  return (Number(n) || 0).toLocaleString('en-US');
}

function pct(v) {
  return Math.round(Number(v) || 0) + '%';
}

function secs(msValue) {
  const s = Math.round((Number(msValue) || 0) / 1000);
  if (s < 60) return s + 's';
  return Math.floor(s / 60) + 'm ' + (s % 60) + 's';
}

/**
 * A trend arrow, or nothing when there is no previous period to compare with.
 *
 * A first-ever period has no direction, and rendering "+100%" for it would
 * invent a trend out of the absence of history.
 */
function delta(now, prev) {
  const a = Number(now) || 0;
  const b = Number(prev) || 0;
  if (!b) return '';
  const change = Math.round((a - b) / b * 100);
  if (change === 0) return '<i class="pulse-admin-flat">flat</i>';
  const up = change > 0;
  return '<i class="pulse-admin-' + (up ? 'up' : 'down') + '">'
    + (up ? '▲' : '▼') + Math.abs(change) + '%</i>';
}

function when(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '—';
  const mins = Math.round((Date.now() - d.getTime()) / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return mins + 'm ago';
  if (mins < 1440) return Math.floor(mins / 60) + 'h ago';
  return Math.floor(mins / 1440) + 'd ago';
}

/**
 * Escape for interpolation into markup.
 *
 * Every caller above routes through this. The block is assembled as an HTML
 * string for consistency with the rest of the pane, which means the escaping
 * has to be unconditional rather than applied where a value "looks" untrusted.
 */
function esc(v) {
  return String(v === null || v === undefined ? '' : v)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
