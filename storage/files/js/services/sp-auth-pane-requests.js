/**
 * The Traffic tab's drilldowns and the Overview's request-pulse strip: every
 * request in the pane's current scope — the whole account by default, or just
 * the focused conversation — as a latency chart, a success split, and an
 * expandable per-request list with audit links.
 *
 * Data comes from the stats sub-resources, refreshed whenever the headline
 * stats land (push or poll) and throttled so a busy turn costs one fetch.
 */

import { ms } from './sp-auth-pane-helpers.js';
import { columnChart, splitBar } from './sp-pane-charts.js';

const REFRESH_MIN_MS = 4000;

const LIST_ROWS = 12;

export function refreshRequests(pane) {
  if (!pane._conversation || !pane._token) return;
  const now = Date.now();
  if (pane._reqFetchAt && now - pane._reqFetchAt < REFRESH_MIN_MS) return;
  pane._reqFetchAt = now;
  const base = '/api/public/pi/stats/' + encodeURIComponent(pane._conversation);
  const token = 'token=' + encodeURIComponent(pane._token)
    + '&scope=' + encodeURIComponent(pane._scope || 'all');
  fetchJson(base + '/requests?' + token).then((body) => {
    if (body) renderRequests(pane, body.requests || []);
  });
  fetchJson(base + '/timeseries?' + token).then((body) => {
    if (body) renderTimeseries(pane, body);
  });
}

async function fetchJson(url) {
  try {
    const res = await fetch(url, { credentials: 'same-origin', redirect: 'manual' });
    return res.ok ? await res.json() : null;
  } catch (_) {
    // The headline stats are still being pushed; the drilldown just waits
    // for the next refresh.
    return null;
  }
}

function renderRequests(pane, rows) {
  renderStrip(pane, rows);
  renderSplit(pane, rows);
  renderList(pane, rows);
}

/** Last requests as latency-height columns, denials-red on failure. */
function renderStrip(pane, rows) {
  const host = pane.querySelector('[data-role="req-strip"]');
  if (!host) return;
  host.hidden = !rows.length;
  if (!rows.length) return;
  host.innerHTML = '';
  const recent = rows.slice(0, 30).reverse();
  host.append(columnChart(
    recent.map((r) => ({
      value: Number(r.latency_ms) || 1,
      hot: r.status === 'failed' || r.status === 'rejected',
      title: (r.model || r.status) + ' · ' + ms(r.latency_ms),
    })),
    'Recent requests by latency, newest right',
  ));
}

function renderSplit(pane, rows) {
  const host = pane.querySelector('[data-role="ok-bar"]');
  if (!host) return;
  host.hidden = !rows.length;
  if (!rows.length) return;
  const bad = rows.filter((r) => r.status === 'failed').length;
  host.innerHTML = '';
  host.append(splitBar(rows.length - bad, bad, 'succeeded', 'failed'));
}

function renderList(pane, rows) {
  const host = pane.querySelector('[data-role="req-list"]');
  if (!host) return;
  host.innerHTML = '';
  if (!rows.length) {
    const li = document.createElement('li');
    li.className = 'pane-feed-empty';
    li.textContent = 'No requests yet — send a prompt in the terminal and each '
      + 'one lands here with its latency, cost, and audit trail.';
    host.append(li);
    return;
  }
  rows.slice(0, LIST_ROWS).forEach((r) => host.append(requestRow(pane, r)));
}

function requestRow(pane, r) {
  const li = document.createElement('li');
  const failed = r.status === 'failed';
  li.className = 'pane-req' + (failed ? ' pane-req--failed' : '');

  const head = document.createElement(failed && r.error_message ? 'summary' : 'div');
  head.className = 'pane-req-head';
  const at = document.createElement('span');
  at.className = 'pane-req-at';
  at.textContent = new Date(r.at).toLocaleTimeString();
  const model = document.createElement('span');
  model.className = 'pane-req-model';
  model.textContent = r.model || r.requested_model || r.status;
  const lat = document.createElement('span');
  lat.className = 'pane-req-lat';
  lat.textContent = ms(r.latency_ms) + ' · ' + (r.cost_display || '$0')
    + (r.cache_hit ? ' · cached' : '');
  const status = document.createElement('span');
  status.className = 'pane-req-status';
  status.dataset.state = r.status;
  status.textContent = r.status;
  head.append(at, model, lat, status);

  if (failed && r.error_message) {
    // A native <details>: the error is one click away with no state to lose
    // on the next repaint.
    const details = document.createElement('details');
    details.className = 'pane-req-details';
    const msg = document.createElement('p');
    msg.className = 'pane-req-error';
    msg.textContent = r.error_message;
    details.append(head, msg);
    li.append(details);
  } else {
    li.append(head);
  }

  if (pane._conversation) {
    const audit = document.createElement('a');
    audit.className = 'pane-link pane-link--sm';
    audit.href = '/trace/' + encodeURIComponent(pane._conversation)
      + (r.id ? '#call-' + encodeURIComponent(r.id) : '');
    audit.target = '_blank';
    audit.rel = 'noopener';
    audit.textContent = 'audit →';
    li.append(audit);
  }
  return li;
}

/** Latency over the conversation's life, one column per time bucket. */
function renderTimeseries(pane, body) {
  const section = pane.querySelector('[data-role="lat-chart-section"]');
  const host = pane.querySelector('[data-role="lat-chart"]');
  if (!section || !host) return;
  const buckets = body.buckets || [];
  section.hidden = buckets.length < 2;
  if (buckets.length < 2) return;
  host.innerHTML = '';
  host.append(columnChart(
    buckets.map((b) => ({
      value: Number(b.latency_p95_ms) || 0,
      hot: (b.errors || 0) > 0,
      title: new Date(b.at).toLocaleTimeString() + ' · p95 ' + ms(b.latency_p95_ms)
        + ' · ' + b.requests + ' req' + (b.errors ? ' · ' + b.errors + ' failed' : ''),
    })),
    'p95 latency per ' + Math.round((body.bucket_secs || 60) / 60) + ' minute bucket',
  ));
}
