import { compact } from './pi-format.js';

/**
 * The header meter strip. Its own module because both the stream (initial
 * paint) and the frame dispatcher (pushed `stats` frames) draw it — importing
 * it from either of those would make the other a circular import.
 */
export function meters(el, s) {
  el._metersEl.hidden = false;
  el._traceEl.hidden = false;
  if (el._conversationId) {
    el._traceEl.href = '/trace/' + encodeURIComponent(el._conversationId);
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
