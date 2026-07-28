import { RECONNECT_MIN_MS, RECONNECT_MAX_MS } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { onFrame } from './pi-terminal-frames.js';
import { meters } from './pi-terminal-meters.js';

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
 * Paint the meters once, before the stream's first stats frame arrives.
 *
 * Cost and denial counts belong in the terminal's own chrome: the claim this
 * page makes is that governance is metered, and a number that moves while you
 * watch is the cheapest possible proof. The stream pushes a `stats` frame
 * after every settled turn and on connect, so there is no timer here — this
 * single fetch covers the gap before the EventSource opens, and doubles as
 * the fallback against an older server that does not push yet.
 */
export function startStats(el) {
  const paint = async () => {
    if (el._closed || !el._conversationId) return;
    // A failed fetch is cosmetic. The transcript is the source of truth.
    const stats = await getJson(el._endpoint + '/stats/'
      + encodeURIComponent(el._conversationId)
      + '?token=' + encodeURIComponent(el._token));
    if (stats) meters(el, stats);
  };
  void paint();
}

