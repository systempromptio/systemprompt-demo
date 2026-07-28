import { RECONNECT_MIN_MS, RECONNECT_MAX_MS } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { onFrame } from './pi-terminal-frames.js';
import { meters } from './pi-terminal-meters.js';
import { degrade } from './pi-terminal-canned.js';

// The composer is enabled only by the stream's session_ready frame, so a stream
// that never attaches leaves a terminal nobody can type into. Past this many
// consecutive failures, say so instead of retrying behind a "reconnecting" label.
const MAX_FAILS = 4;

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
    el._streamFails = 0;
    el._status('live');
  };
  el._source.onerror = () => {
    if (el._closed) return;
    el._teardownStream();
    el._streamFails = (el._streamFails || 0) + 1;
    // A 404 means the session this stream names is gone; no number of
    // reconnects can bring it back, but a fresh POST /session can. Tried once,
    // so a server that keeps dropping the session cannot become a spawn loop.
    if (el._streamFails === 1 && !el._streamRecovered) {
      el._streamRecovered = true;
      void recover(el, url);
      return;
    }
    if (el._streamFails >= MAX_FAILS) {
      degrade(el, 'stream');
      return;
    }
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
 * Re-open the conversation when the stream says its session no longer exists.
 *
 * EventSource reports every failure identically, so the status has to be read
 * with a plain fetch. Anything other than a 404 is a transport problem a
 * reconnect can still solve, and falls back to the backoff.
 */
async function recover(el, url) {
  const conversationId = el._conversationId;
  let gone = false;
  try {
    const res = await fetch(url, { headers: { accept: 'text/event-stream' } });
    gone = res.status === 404;
    if (res.body) await res.body.cancel();
  } catch (_) {
    gone = false;
  }
  if (el._closed) return;
  if (gone) {
    const { restart } = await import('./pi-terminal-session.js');
    await restart(el, conversationId);
    return;
  }
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

