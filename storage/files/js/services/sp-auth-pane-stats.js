/**
 * The authoritative numbers behind <sp-auth-pane>: the stats fallback fetch,
 * and everything it paints — tiles, latency bars, model mix, and the credit
 * meter. The live path is the `stats` frame the terminal's SSE stream pushes;
 * the fetch here only runs when that push has gone quiet.
 */

import { ms, pct, compactTokens } from './sp-auth-pane-helpers.js';
import { applyGovChip, applyStages, renderFeed } from './sp-auth-pane-governance.js';

/** How often the fallback checks whether the push has gone quiet. */
export const FALLBACK_POLL_MS = 15000;

/** A push younger than this means the stream is doing its job — don't fetch.
 *  Longer than the fallback interval, so one late frame skips a whole tick. */
export const PUSH_FRESH_MS = 20000;

/**
 * A key can appear in more than one panel — the Overview repeats the
 * headline numbers other tabs own — so every instance is updated, not just
 * the first the selector happens to find.
 */
export function setStat(pane, key, value) {
  pane.querySelectorAll('[data-stat="' + key + '"]').forEach((el) => {
    if (el.textContent !== value) {
      el.textContent = value;
      // Re-triggering the animation needs the class to actually leave the
      // element first; a same-frame remove/add is coalesced away.
      el.classList.remove('is-changed');
      void el.offsetWidth;
      el.classList.add('is-changed');
    }
    // A block that actually happened is the number this pane exists to show.
    // The terminal header's meter already goes red on the same fact; the two
    // halves should not disagree about it.
    if (key === 'blocked') {
      el.parentElement.dataset.hot = value && value !== '0' ? '1' : '0';
    }
  });
}

export async function poll(pane) {
  if (!pane._conversation || !pane._token || !pane._feed) return;
  try {
    const url = '/api/public/pi/stats/' + encodeURIComponent(pane._conversation)
      + '?token=' + encodeURIComponent(pane._token);
    const res = await fetch(url, { credentials: 'same-origin', redirect: 'manual' });
    if (!res.ok) return;
    applyStats(pane, await res.json());
  } catch (_) {
    // A failed fetch is not worth surfacing: the pushed frames are still
    // moving the counters, and the fallback will try again.
  }
}

/**
 * Every field is guarded. The pane is served from `web/dist` and the API
 * from the binary, so a deploy can land one without the other; an older
 * server simply omits the new keys, and every new tile falls back to a dash
 * rather than printing `undefined` next to real numbers.
 */
export function applyStats(pane, s) {
  setStat(pane, 'model', s.model || '—');
  // The server sends this only when a route actually rewrote the model, so
  // the usual reading is the reassuring one: you got what you asked for.
  setStat(pane, 'requested', s.requested_model || 'as served');
  setStat(pane, 'provider', s.provider || '—');
  setStat(pane, 'route', s.route_match || 'default');
  setStat(pane, 'cache', pct(s.cache_hit_percent));

  setStat(pane, 'requests', String(s.requests || 0));
  setStat(pane, 'tools', String(Math.max(s.tool_calls || 0, pane._live.tools)));
  const denials = Math.max(s.denied || 0, pane._live.blocked);
  setStat(pane, 'blocked', String(denials));
  setStat(pane, 'errors', String(s.errors || 0));
  const reqs = s.requests || 0;
  setStat(pane, 'successRate', reqs
    ? Math.round(((reqs - (s.errors || 0)) / reqs) * 100) + '%'
    : '—');

  setStat(pane, 'latency', ms(s.latency_p50_ms));
  setStat(pane, 'latency95', ms(s.latency_p95_ms));
  setStat(pane, 'latencyLast', ms(s.latency_last_ms));
  applyLatencyBars(pane, s);

  setStat(pane, 'tokensIn', compactTokens(s.input_tokens));
  setStat(pane, 'tokensOut', compactTokens(s.output_tokens));
  setStat(pane, 'cacheRead', compactTokens(s.cache_read_tokens));
  setStat(pane, 'cacheWrite', compactTokens(s.cache_creation_tokens));

  setStat(pane, 'cost', s.cost_display || '$0');
  setStat(pane, 'costPer', s.cost_per_request_display || '$0');

  if (s.policy_stages && s.policy_stages.length) applyStages(pane, s.policy_stages);
  applyCredit(pane, s.credit);
  applyModelMix(pane, s.model_mix);
  applyGovChip(pane, (s.events || []).length, denials);
  renderFeed(pane, s.events || []);
}

/**
 * The three latencies as one comparative picture: each bar is scaled against
 * the slowest of them, so the spread between p50 and p95 is visible rather
 * than three equally sized tiles saying numbers.
 */
export function applyLatencyBars(pane, s) {
  const vals = {
    latency: s.latency_p50_ms,
    latency95: s.latency_p95_ms,
    latencyLast: s.latency_last_ms,
  };
  const max = Math.max(...Object.values(vals).map((v) => Number(v) || 0), 1);
  Object.entries(vals).forEach(([k, v]) => {
    const el = pane.querySelector('[data-bar="' + k + '"]');
    if (el) el.style.width = Math.round(((Number(v) || 0) / max) * 100) + '%';
  });
}

/** Which models actually served this conversation, as labelled share bars. */
export function applyModelMix(pane, mix) {
  const section = pane.querySelector('[data-role="mix-section"]');
  const list = pane.querySelector('[data-role="mix"]');
  if (!section || !list) return;
  if (!mix || !mix.length) {
    section.hidden = true;
    return;
  }
  section.hidden = false;
  list.innerHTML = '';
  mix.forEach((m) => {
    const row = document.createElement('div');
    row.className = 'pane-mix-row';
    const label = document.createElement('span');
    label.className = 'pane-mix-label';
    label.textContent = m.model;
    const bar = document.createElement('div');
    bar.className = 'pane-bar';
    bar.setAttribute('role', 'img');
    bar.setAttribute('aria-label', m.model + ' ' + pct(m.percent) + ' of requests');
    const fill = document.createElement('span');
    fill.style.width = Math.max(0, Math.min(100, Number(m.percent) || 0)) + '%';
    bar.append(fill);
    const share = document.createElement('span');
    share.className = 'pane-mix-pct';
    share.textContent = pct(m.percent);
    row.append(label, bar, share);
    list.append(row);
  });
}

/**
 * Show what is left of the grant.
 *
 * Stays hidden when nothing has been granted rather than rendering "$0 of
 * $0": an account still awaiting approval has no grant yet, and an empty
 * meter would read as "you have spent it all" — the opposite of the truth,
 * and the discouraging half of the two possible misreadings.
 */
export function applyCredit(pane, credit) {
  if (!pane._credit) return;
  if (!credit || !credit.granted_microdollars) {
    pane._credit.hidden = true;
    return;
  }
  pane._credit.hidden = false;
  pane.querySelector('[data-role="credit-left"]').textContent = credit.remaining_display;
  pane.querySelector('[data-role="credit-of"]').textContent = 'of ' + credit.granted_display;

  const remaining = Math.max(0, Math.min(100, credit.remaining_percent));
  const bar = pane.querySelector('[data-role="credit-bar"]');
  pane.querySelector('[data-role="credit-fill"]').style.width = remaining + '%';
  bar.setAttribute('aria-label', credit.remaining_display + ' of '
    + credit.granted_display + ' remaining');
  bar.dataset.state = credit.exhausted ? 'empty' : (remaining <= 15 ? 'low' : 'ok');

  const note = pane.querySelector('[data-role="credit-note"]');
  if (credit.exhausted) {
    // The terminal is about to start refusing turns. Saying so here, next to
    // the number that explains it, beats letting the agent go quiet first.
    note.textContent = 'Your credit is spent — the gateway will refuse the next request.';
  } else {
    note.textContent = 'Spent ' + credit.spent_display + ' so far. Every request is '
      + 'metered against this balance before it reaches a provider.';
  }
}
