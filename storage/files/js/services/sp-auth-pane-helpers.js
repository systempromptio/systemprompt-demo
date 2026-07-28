/** Pure formatters and small node builders for <sp-auth-pane>. */

/** Latencies are absent until a turn completes; say so rather than showing 0. */
export function ms(v) {
  return (v === null || v === undefined) ? '—' : v + 'ms';
}

export function pct(v) {
  return String(Number(v) || 0) + '%';
}

/**
 * Token counts get long fast; the pane has one line for them.
 *
 * Not [`compact`] from ./pi-format.js: this one coerces whatever the API sent
 * and capitalises the millions suffix, and the pane's tiles are sized for that.
 */
export function compactTokens(n) {
  const v = Number(n) || 0;
  if (v < 1000) return String(v);
  if (v < 1000000) return (v / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
  return (v / 1000000).toFixed(1).replace(/\.0$/, '') + 'M';
}

export function feedItem(e) {
  const li = document.createElement('li');
  li.className = 'pane-feed-item pane-feed-item--' + (e.outcome === 'deny' ? 'deny' : 'allow');
  const head = document.createElement('span');
  head.className = 'pane-feed-head';
  head.textContent = (e.kind || 'event') + ' · ' + (e.subject || '');
  const tail = document.createElement('span');
  tail.className = 'pane-feed-tail';
  tail.textContent = [e.policy, e.detail].filter(Boolean).join(' — ');
  li.append(head, tail);
  return li;
}

/** WebAuthn reports user choices as exceptions; say what actually happened. */
export function passkeyMessage(err, fallback) {
  if (err && err.name === 'NotAllowedError') return 'That was cancelled, or the passkey timed out.';
  if (err && err.name === 'NotSupportedError') return 'Passkeys are not supported on this device.';
  return (err && err.message) || fallback;
}
