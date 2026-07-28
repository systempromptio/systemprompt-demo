import { RECONNECT_MIN_MS, RECONNECT_MAX_MS } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { onFrame } from './pi-terminal-frames.js';
import { meters } from './pi-terminal-meters.js';
import { degrade } from './pi-terminal-canned.js';

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
    void onStreamError(el, url);
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
 * Decide whether a dropped stream is worth reconnecting to.
 *
 * EventSource reports every failure identically, so the status has to be read
 * with a plain fetch. A transport failure — a redeploy, a sleeping laptop —
 * still deserves the indefinite backoff this has always had. A 404 does not:
 * the session the URL names is gone, and only a fresh POST /session can
 * produce another. Retrying that forever is what leaves a terminal disabled
 * behind a "reconnecting" label nobody can act on.
 */
async function onStreamError(el, url) {
  const conversationId = el._conversationId;
  if (!(await sessionGone(url)) || el._closed) {
    backoff(el);
    return;
  }
  // Re-opened once. If the replacement session is missing too, the server is
  // refusing to hold one, and a second attempt would only be a spawn loop.
  if (el._streamRecovered) {
    degrade(el, 'stream');
    return;
  }
  el._streamRecovered = true;
  const { restart } = await import('./pi-terminal-session.js');
  await restart(el, conversationId);
}

async function sessionGone(url) {
  try {
    const res = await fetch(url, { headers: { accept: 'text/event-stream' } });
    if (res.body) await res.body.cancel();
    return res.status === 404;
  } catch (_) {
    return false;
  }
}

function backoff(el) {
  if (el._closed) return;
  el._status('reconnecting');
  el._reconnectTimer = setTimeout(() => openStream(el), jitter(el));
  el._reconnectMs = Math.min(el._reconnectMs * 2, RECONNECT_MAX_MS);
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

