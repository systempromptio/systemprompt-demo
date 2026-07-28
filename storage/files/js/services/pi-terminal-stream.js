import { RECONNECT_MIN_MS, RECONNECT_MAX_MS, STATS_MS } from './pi-constants.js';
import { compact } from './pi-format.js';
import { getJson } from './pi-transport.js';
import { onFrame } from './pi-terminal-frames.js';

export function openStream(el) {
  const url = el._endpoint + '/stream/' + encodeURIComponent(el._conversationId)
    + '?token=' + encodeURIComponent(el._token)
    + '&since=' + el._lastSeq;
  // EventSource cannot set headers, which is the whole reason the embed token
  // exists as a query-string credential rather than a bearer header.
  el._source = new EventSource(url);
  el._source.onmessage = (e) => onFrame(el, e.data);
  el._source.onopen = () => {
    el._reconnectMs = RECONNECT_MIN_MS;
    el._status('live');
  };
  el._source.onerror = () => {
    if (el._closed) return;
    el._teardownStream();
    el._status('reconnecting');
    el._reconnectTimer = setTimeout(() => openStream(el), jitter(el));
    el._reconnectMs = Math.min(el._reconnectMs * 2, RECONNECT_MAX_MS);
  };

  // There is deliberately no visibilitychange handler. has_viewers() is a
  // receiver count, and a pending approval is abandoned — denied — after 15s
  // with nobody attached. Closing the stream on a hidden tab would silently
  // deny approvals the operator is about to answer.
}

function jitter(el) {
  return el._reconnectMs * (0.5 + Math.random() / 2);
}

/**
 * Poll the stats the pane already polls.
 *
 * Cost and denial counts belong in the terminal's own chrome: the claim this
 * page makes is that governance is metered, and a number that moves while you
 * watch is the cheapest possible proof. No new endpoint — this is the same
 * `GET stats/{id}` the pane beside it uses.
 */
export function startStats(el) {
  const poll = async () => {
    if (el._closed || !el._conversationId) return;
    // A failed poll is cosmetic. The transcript is the source of truth.
    const stats = await getJson(el._endpoint + '/stats/'
      + encodeURIComponent(el._conversationId)
      + '?token=' + encodeURIComponent(el._token));
    if (stats) meters(el, stats);
  };
  void poll();
  el._statsTimer = setInterval(poll, STATS_MS);
}

export function meters(el, s) {
  el._metersEl.hidden = false;
  el._traceEl.hidden = false;
  if (el._conversationId) {
    el._traceEl.href = '/admin/demo/trace?session='
      + encodeURIComponent(el._conversationId);
  }
  const set = (role, value) => {
    const node = el.querySelector('[data-role="' + role + '"] b');
    if (node) node.textContent = value;
  };
  set('m-tools', String(s.tool_calls || 0));
  set('m-blocked', String((s.tools_blocked || 0) + (s.prompts_blocked || 0)));
  set('m-tokens', compact((s.input_tokens || 0) + (s.output_tokens || 0)));
  set('m-cost', s.cost_display || '$0.00');
  const blocked = el.querySelector('[data-role="m-blocked"]');
  if (blocked) blocked.dataset.hot = (s.tools_blocked || s.prompts_blocked) ? '1' : '0';
}

/**
 * The replay's meters. The strip must count the replay's own calls — a chrome
 * that says "0 calls, $0.00" over a transcript showing four tool calls and a
 * block is the pane contradicting itself. The trace link stays hidden: no
 * real audit trail exists behind a scripted pass, and linking one would be
 * the dishonest move.
 */
export function cannedMeters(el, m) {
  meters(el, {
    tool_calls: m.calls,
    tools_blocked: m.blocked,
    input_tokens: m.tokens,
    output_tokens: 0,
    cost_display: m.cost,
  });
  el._traceEl.hidden = true;
}
