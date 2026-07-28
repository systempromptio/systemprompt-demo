/** Small pure formatters shared by the pi components. */

/** One-line form of a tool's arguments, for the collapsed row. */
export function summarise(input) {
  if (!input || typeof input !== 'object') return '';
  const v = input.path || input.file_path || input.pattern || input.command;
  if (typeof v === 'string') return v;
  const keys = Object.keys(input);
  return keys.length ? keys.join(', ') : '';
}

export function pretty(input) {
  try {
    return JSON.stringify(input, null, 2);
  } catch (_) {
    return String(input);
  }
}

/** Rough token count for the thinking summary. Four characters per token is the
 *  usual English approximation, and this is a label, not an invoice. */
export function approxTokens(s) {
  return Math.max(1, Math.round(s.length / 4));
}

/** Fence count, to tell a closed code block from one still being written. */
export function countFences(s) {
  const m = s.match(/^\s*```/gm);
  return m ? m.length : 0;
}

/** 1200 -> 1.2k. Keeps the header meters from reflowing as a session runs. */
export function compact(n) {
  if (n < 1000) return String(n);
  if (n < 1000000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
  return (n / 1000000).toFixed(1).replace(/\.0$/, '') + 'm';
}

/** `$5/$25` — the two rates a picker row has room for. Sub-dollar rates keep
 *  their cents (`$0.35`), dollar rates drop them: the point is comparison
 *  across rows, not an invoice. */
export function price(n) {
  if (typeof n !== 'number' || !(n > 0)) return null;
  return '$' + (n < 1 ? n.toFixed(2).replace(/0$/, '') : String(Math.round(n)));
}

export function modelLabel(m) {
  const parts = [m.id];
  const inp = price(m.input_per_million);
  const out = price(m.output_per_million);
  if (inp && out) parts.push(inp + '/' + out);
  if (m.context_window) parts.push(compact(m.context_window) + ' ctx');
  return parts.join('  ·  ');
}

export function modelTitle(m) {
  const bits = [];
  const inp = price(m.input_per_million);
  const out = price(m.output_per_million);
  if (inp && out) bits.push('input ' + inp + ' / output ' + out + ' per million tokens');
  if (m.context_window) bits.push('context window ' + compact(m.context_window));
  if (m.max_output_tokens) bits.push('max output ' + compact(m.max_output_tokens));
  return bits.join(' · ');
}
