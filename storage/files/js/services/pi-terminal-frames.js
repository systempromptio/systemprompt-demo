import { EXIT_REASONS } from './pi-constants.js';
import { append, line, echo } from './pi-terminal-dom.js';
import { delta, flushStream } from './pi-terminal-prose.js';
import { orphanRail, policyStages } from './pi-terminal-rail.js';
import {
  toolStart, toolEnd, toolBlocked, promptBlocked, approvalRequest, approvalResolved,
  approvalAuto,
} from './pi-terminal-gate.js';
import { toolArtifact } from './pi-terminal-artifacts.js';
import { meters } from './pi-terminal-meters.js';
import { remember, send } from './pi-terminal-input.js';
import { setSendMode } from './pi-terminal-view.js';

/** The one dispatcher every frame goes through, live or replayed. */
export function onFrame(el, raw) {
  let f;
  try {
    f = JSON.parse(raw);
  } catch (_) {
    return;
  }

  // The stream subscribes before draining its replay buffer, so a frame
  // emitted in that window arrives twice. seq is monotonic; ignore the echo.
  if (typeof f.seq === 'number') {
    if (f.seq <= el._lastSeq) return;
    if (el._lastSeq && f.seq > el._lastSeq + 1) {
      line(el, 'output-dim', '── reconnected; earlier output may be missing ──');
    }
    el._lastSeq = f.seq;
  }

  // Republished so a sibling pane can react to the same turn the terminal is
  // rendering, without opening a second EventSource against the one stream a
  // conversation has.
  el._emit('pi-frame', f);

  switch (f.type) {
    case 'session_ready': return enable(el);
    // Live, `echo` already drew this the moment it was typed — rendering the
    // server's copy too would double every prompt. On replay it is the only
    // record of the viewer's half of the conversation — and it also refills
    // ↑-recall, which lives in memory and would otherwise start every page
    // load empty even though the transcript above shows what was asked.
    case 'user_message':
      if (!el._replaying) return undefined;
      remember(el, f.text);
      return echo(el, f.text);
    case 'turn_start': return turnStart(el);
    case 'text_delta': return delta(el, f.text, false);
    case 'thinking_delta': return delta(el, f.text, true);
    case 'policy_stages': return policyStages(el, f);
    case 'tool_start': return toolStart(el, f);
    case 'tool_end': return toolEnd(el, f);
    case 'tool_artifact': return toolArtifact(el, f);
    case 'tool_blocked': return toolBlocked(el, f);
    case 'prompt_blocked': return promptBlocked(el, f);
    case 'approval_request': return approvalRequest(el, f);
    case 'approval_auto': return approvalAuto(el, f);
    case 'approval_resolved': return approvalResolved(el, f);
    case 'turn_end': return turnEnd(el);
    // Pushed after every settled turn and once on connect — this frame is why
    // the terminal no longer polls GET stats/{id}. No seq: it is ephemeral,
    // so it bypasses the dedupe above by design.
    case 'stats': return meters(el, f.stats || {});
    case 'stderr': return stderr(el, f.line);
    case 'error': return error(el, f);
    case 'exit': return exit(el, f);
    default: return undefined;
  }
}

/**
 * The jail banner is a claim about confinement, and it was the first line of
 * the transcript — which made it the first thing to scroll out of sight. It
 * is promoted to the chrome, where it stays true for as long as it is true.
 */
function stderr(el, text) {
  const jail = /^sp-pi-jail:\s*(.+)$/.exec(text || '');
  if (jail && el._jailEl) {
    const landlock = /\(([^)]*Landlock[^)]*)\)/i.exec(jail[1]);
    el._jailEl.textContent = landlock ? landlock[1] : 'sandboxed';
    el._jailEl.title = jail[1];
    el._jailEl.hidden = false;
    return undefined;
  }
  return line(el, 'output-dim', text);
}

/**
 * The server sends at most one frame per distinct error (deduped at emit and
 * on history read), so this renders unconditionally. Credit exhaustion gets
 * a card with the way out; every other error stays a plain warning line.
 */
function error(el, f) {
  if (f.code !== 'credit_exhausted') {
    return line(el, 'output-warn', f.message);
  }
  const card = document.createElement('div');
  card.className = 'pi-error-card';
  const icon = document.createElement('span');
  icon.className = 'pi-error-card__icon';
  icon.textContent = '◌';
  const text = document.createElement('span');
  text.className = 'pi-error-card__text';
  text.textContent = f.message;
  const link = document.createElement('a');
  link.className = 'pi-error-card__action';
  link.href = '/admin';
  link.textContent = 'Add credit';
  card.append(icon, text, link);
  append(el, card);
  return card;
}

function enable(el) {
  el._status('live');
  el.classList.add('is-session');
  el._input.disabled = false;
  setSendMode(el, 'send', true);
  // A welcome chip clicked before the session was live parked its prompt in
  // the composer; the moment the session can carry it, it goes.
  if (el._input.dataset.pending) {
    delete el._input.dataset.pending;
    void send(el);
  }
  // The session is established, so the header can now say — truthfully —
  // whose identity every call is signed to.
  if (el._who && el._who.email) {
    el._userNameEl.textContent = el._who.email;
    el._userEl.title = 'Signed in as ' + el._who.email
      + ' — every call this session makes is signed to this identity';
    el._userEl.hidden = false;
  }
}

function turnStart(el) {
  el._turnLive = true;
  flushStream(el);
  el._stopBtn.hidden = false;
  // The composer's live treatment — progress sliver, demoted Run — is CSS off
  // this one attribute, so the busy state can never disagree with _turnLive.
  el._composer.dataset.busy = 'true';
}

function turnEnd(el) {
  el._turnLive = false;
  flushStream(el);
  el._thinkBuf = '';
  el._thinkEl = null;
  orphanRail(el);
  el._stopBtn.hidden = true;
  delete el._composer.dataset.busy;
}

function exit(el, f) {
  el._closed = true;
  el._teardownStream();
  flushStream(el);
  el._status('ended');
  el.classList.remove('is-session');
  el._input.disabled = true;
  el._stopBtn.hidden = true;
  delete el._composer.dataset.busy;
  // Sessions end on their own — four ways — so this is the normal case, not a
  // fault. The button says how to carry on from it.
  setSendMode(el, 'reconnect', true);
  const why = EXIT_REASONS[f.reason];
  line(el, 'output-dim', 'Session ended'
    + (why ? ' — ' + why : '')
    + (typeof f.code === 'number' ? ' (exit ' + f.code + ')' : '')
    + '. Reconnect to start a new one.');
}
