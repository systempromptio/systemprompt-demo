/**
 * The Activity tab: what this account has been doing across every
 * conversation — all-time tallies, a row per conversation with its own
 * spend and denials, and the most-used tools.
 *
 * One fetch of `/api/public/pi/me/summary`, refreshed with the headline
 * stats but throttled harder: cross-conversation rollups move slowly.
 */

import { shareBar } from './sp-pane-charts.js';

const REFRESH_MIN_MS = 10000;

export function refreshActivity(pane) {
  if (!pane._token) return;
  const now = Date.now();
  if (pane._actFetchAt && now - pane._actFetchAt < REFRESH_MIN_MS) return;
  pane._actFetchAt = now;
  fetchSummary(pane);
}

async function fetchSummary(pane) {
  try {
    const res = await fetch('/api/public/pi/me/summary?token='
      + encodeURIComponent(pane._token), { credentials: 'same-origin', redirect: 'manual' });
    if (res.ok) renderSummary(pane, await res.json());
  } catch (_) {
    // Rollups are context, not the live numbers — the next refresh retries.
  }
}

function renderSummary(pane, s) {
  renderTotals(pane, s.totals || {});
  renderConversations(pane, s.conversations || []);
  renderTools(pane, s.top_tools || []);
}

function renderTotals(pane, t) {
  const host = pane.querySelector('[data-role="act-totals"]');
  if (!host) return;
  host.innerHTML = '';
  [
    ['Conversations', String(t.conversations || 0)],
    ['Requests', String(t.requests || 0)],
    ['Tool calls', String(t.tool_calls || 0)],
    ['Blocked', String(t.denied || 0)],
    ['Total spend', t.cost_display || '$0'],
  ].forEach(([label, value]) => {
    const tile = document.createElement('div');
    tile.className = 'pane-stat';
    const dt = document.createElement('dt');
    dt.textContent = label;
    const dd = document.createElement('dd');
    dd.textContent = value;
    if (label === 'Blocked') tile.dataset.hot = value !== '0' ? '1' : '0';
    tile.append(dt, dd);
    host.append(tile);
  });
}

function renderConversations(pane, rows) {
  const host = pane.querySelector('[data-role="act-list"]');
  if (!host) return;
  host.innerHTML = '';
  if (!rows.length) {
    const li = document.createElement('li');
    li.className = 'pane-feed-empty';
    li.textContent = 'No conversations yet — start one in the terminal and its '
      + 'full audit trail appears here.';
    host.append(li);
    return;
  }
  const maxCost = Math.max(1, ...rows.map((r) => Number(r.cost_microdollars) || 0));
  rows.forEach((r) => host.append(conversationRow(pane, r, maxCost)));
}

function conversationRow(pane, r, maxCost) {
  const li = document.createElement('li');
  li.className = 'pane-conv' + (r.id === pane._conversation ? ' pane-conv--live' : '');

  const title = document.createElement('a');
  title.className = 'pane-conv-title';
  title.href = '/trace/' + encodeURIComponent(r.id);
  title.textContent = r.title || 'Untitled conversation';

  const tally = document.createElement('span');
  tally.className = 'pane-conv-tally';
  const parts = [r.requests + ' req'];
  if (r.tool_calls) parts.push(r.tool_calls + ' tools');
  if (r.denied) parts.push(r.denied + ' blocked');
  if (r.errors) parts.push(r.errors + ' failed');
  tally.textContent = parts.join(' · ');

  const cost = document.createElement('span');
  cost.className = 'pane-conv-cost';
  cost.textContent = r.cost_display || '$0';

  const when = document.createElement('span');
  when.className = 'pane-conv-when';
  when.textContent = new Date(r.updated_at).toLocaleDateString();

  li.append(title, tally,
    shareBar(r.cost_microdollars, maxCost,
      (r.title || 'conversation') + ' cost ' + (r.cost_display || '$0')),
    cost, when);
  return li;
}

function renderTools(pane, tools) {
  const section = pane.querySelector('[data-role="act-tools-section"]');
  const host = pane.querySelector('[data-role="act-tools"]');
  if (!section || !host) return;
  section.hidden = !tools.length;
  if (!tools.length) return;
  host.innerHTML = '';
  const max = Math.max(1, ...tools.map((t) => Number(t.calls) || 0));
  tools.forEach((t) => {
    const row = document.createElement('div');
    row.className = 'pane-mix-row';
    const label = document.createElement('span');
    label.className = 'pane-mix-label';
    label.textContent = t.tool;
    const share = document.createElement('span');
    share.className = 'pane-mix-pct';
    share.textContent = String(t.calls);
    row.append(label, shareBar(t.calls, max, t.tool + ' called ' + t.calls + ' times'), share);
    host.append(row);
  });
}
