import { RECONNECT_MIN_MS, REPLAY_PAGES } from './pi-constants.js';
import { mintToken, whoami, getJson, postJson, conversations } from './pi-transport.js';
import { line, clearUnseen } from './pi-terminal-dom.js';
import { hidePalette, loadCommands } from './pi-terminal-palette.js';
import { flushStream } from './pi-terminal-prose.js';
import { onFrame } from './pi-terminal-frames.js';
import { openStream, startStats } from './pi-terminal-stream.js';
import { degrade } from './pi-terminal-canned.js';
import { enterQueue } from './pi-terminal-capacity.js';
import { closeArtifact } from './pi-artifact-overlay.js';
import { resetArtifacts } from './pi-terminal-artifacts.js';
import { mountWelcome, setReplayChrome, setSendMode } from './pi-terminal-view.js';
import { setApprovalMode } from './pi-terminal-gate.js';

/**
 * Re-run the whole start sequence against whatever credential now exists.
 *
 * Called from outside this element as well: signing in happens in the pane
 * beside it, which establishes the cookie without a navigation, so nothing
 * reloads the page and the terminal has to be told the visitor stopped being
 * anonymous.
 */
export async function restart(el, resume) {
  el._queued = false;
  el._teardownStream();
  if (el._reconnectTimer) clearTimeout(el._reconnectTimer);
  el._cannedTimers.forEach(clearTimeout);
  el._cannedTimers = [];
  el._cannedCards.forEach((c) => c.settle());
  el._cannedCards = [];
  el._cannedRow = null;
  el._approvals.forEach((a) => a.settle());
  el._approvals.clear();
  el._approvalsEl.replaceChildren();
  el._body.replaceChildren();
  // An emptied transcript is an empty transcript: the same opening chips a
  // first-time visitor gets. `append` clears it again on the first line.
  mountWelcome(el);
  el._toolRows.clear();
  el._gateRun = null;
  closeArtifact(el);
  resetArtifacts(el);
  hidePalette(el);
  el._conversationId = null;
  el._lastSeq = 0;
  el._reconnectMs = RECONNECT_MIN_MS;
  el._turnLive = false;
  el._closed = false;
  el._who = null;
  el._workEl = null;
  el._streamBuf = '';
  el._thinkBuf = '';
  el._thinkEl = null;
  el._railFor = null;
  el._railDecision = null;
  el._lines = 0;
  el._pinned = true;
  clearUnseen(el);
  el._metersEl.hidden = true;
  el._traceEl.hidden = true;
  el._userEl.hidden = true;
  el._approvalModeBtn.hidden = true;
  el._gateEl.hidden = true;
  el._gateEl.replaceChildren();
  el.classList.remove('is-replay');
  el.classList.remove('is-session');
  setReplayChrome(el, false);
  await start(el, resume);
}

/**
 * Start a conversation, continuing stored history unless told otherwise.
 *
 * `resume` is the conversation to reopen: `undefined` continues the most
 * recent one, and `null` explicitly starts a new one. The distinction is the
 * whole feature — a reload is `undefined` and gets its transcript back.
 */
export async function start(el, resume) {
  el._status('connecting');
  // Back to a disabled Send for the duration: leaving Reconnect live would
  // invite a second click that opens a second session.
  setSendMode(el, 'send', false);
  // whoami first: an anonymous visitor never POSTs embed-token, so a public
  // page load logs no 401.
  el._who = await whoami();
  const token = el.getAttribute('token')
    || (el._who ? await mintToken(el._endpoint) : null);
  if (!token) {
    return degrade(el, 'anonymous');
  }
  el._token = token;

  const wanted = resume === undefined ? await latestConversation(el) : resume;
  const create = wanted ? { token, resume: wanted } : { token };
  // The picker's value rides along when one is showing; absent, the server
  // default applies.
  if (el._modelEl && !el._modelEl.hidden && el._modelEl.value) {
    create.model = el._modelEl.value;
  }
  const res = await postJson(el._endpoint + '/session', create);
  if (!res.ok) {
    // 429 is by far the likeliest and is not an error the visitor caused. A
    // waitlisted body means the server put us in line — enter queue mode and
    // let the capacity heartbeat reconnect us when a slot frees.
    if (res.status === 429) {
      const info = await res.json().catch(() => null);
      if (info && info.reason === 'waitlisted') return enterQueue(el, info);
      return degrade(el, 'busy');
    }
    return degrade(el, 'anonymous');
  }
  const body = await res.json();
  el._conversationId = body.conversation_id;
  // The server decides which mode a session opens in; the chip only ever
  // shows what the gate is actually doing.
  setApprovalMode(el, Boolean(body.manual_approval));
  el._approvalModeBtn.hidden = false;
  // Replay before the stream attaches, so the live frames land after the
  // history rather than interleaved with it.
  if (body.resumed) await replay(el);
  // The stats pane polls per conversation and cannot mint its own token, so
  // the credential travels with the announcement. Same origin, same page.
  el._emit('pi-session', { conversation_id: el._conversationId, token: el._token });
  // Not awaited: the palette is discovery, and the stream is the thing the
  // viewer is waiting for.
  loadCommands(el);
  openStream(el);
  startStats(el);
  return undefined;
}

/**
 * The conversation to reopen by default: the one touched most recently.
 *
 * A failure here is not fatal — the caller falls through to a new
 * conversation, which is what a visitor with no history gets anyway.
 */
async function latestConversation(el) {
  const list = await conversations(el._endpoint, el._token);
  return list.length ? list[0].id : null;
}

/**
 * Draw the stored transcript into the body before the live stream starts.
 *
 * Stored frames are the same shape the stream sends, so they go through the
 * one dispatcher rather than through a second set of renderers that could
 * drift from it. `_replaying` is what keeps the viewer's own messages
 * visible here while staying suppressed live, where `echo` already drew
 * them the moment they were typed.
 */
async function replay(el) {
  let after = 0;
  el._replaying = true;
  try {
    // Paged, because a long conversation is more frames than one response
    // should carry. `more` is the server saying it stopped at its own cap.
    for (let page = 0; page < REPLAY_PAGES; page += 1) {
      const url = el._endpoint + '/conversations/'
        + encodeURIComponent(el._conversationId) + '/history'
        + '?token=' + encodeURIComponent(el._token)
        + '&after_seq=' + after;
      const body = await getJson(url);
      if (!body) break;
      (body.events || []).forEach((f) => onFrame(el, JSON.stringify(f)));
      after = body.last_seq || after;
      if (!body.more) break;
    }
  } catch (_) {
    line(el, 'output-dim', '── earlier messages could not be loaded ──');
  }
  el._replaying = false;
  if (el._lastSeq) {
    flushStream(el);
    line(el, 'output-dim', '── restored; continuing this conversation ──');
  }
}
