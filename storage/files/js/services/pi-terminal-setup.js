import { PIN_SLACK_PX } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { modelLabel, modelTitle } from './pi-format.js';
import { terminalChrome, mountWelcome } from './pi-terminal-view.js';
import { autogrow, clearUnseen } from './pi-terminal-dom.js';
import { refreshPalette, hidePalette } from './pi-terminal-palette.js';
import { send, onKey, wireWelcome } from './pi-terminal-input.js';
import { wireArtifacts } from './pi-terminal-artifacts.js';
import { wireExpand } from './pi-terminal-expand.js';
import { toggleApprovalMode } from './pi-terminal-gate.js';

/** Draw the chrome once, cache the roles, and wire every listener to it. */
export function build(el) {
  el.classList.add('pi-terminal');
  el.replaceChildren(terminalChrome());

  const role = (name) => el.querySelector('[data-role="' + name + '"]');
  el._body = role('body');
  el._approvalsEl = role('approvals');
  el._statusEl = role('status');
  el._liveEl = role('live');
  el._gateEl = role('gate');
  el._input = role('input');
  el._paletteEl = role('palette');
  el._sendBtn = role('send');
  el._sendLabel = role('send-label');
  el._stopBtn = role('stop');
  el._clearBtn = role('clear');
  el._jumpBtn = role('jump');
  el._metersEl = role('meters');
  el._traceEl = role('trace');
  el._jailEl = role('jail');
  el._modelEl = role('model');
  el._userEl = role('user');
  el._userNameEl = role('user-name');
  el._composer = role('composer');
  el._artWrap = role('art-wrap');
  el._artChip = role('art-chip');
  el._artCount = role('art-count');
  el._artPanel = role('art-panel');
  el._hintEl = role('hint');
  el._headerEl = role('header');
  el._replayBar = role('replay-bar');
  el._approvalModeBtn = role('approval-mode');
  el._approvalModeLabel = role('approval-mode-label');

  wireArtifacts(el);
  wireCta(el);
  wireExpand(el);
  wireWelcome(el);
  mountWelcome(el);
  // A change spawns a fresh child on the new model, resuming the same
  // conversation so the transcript carries over.
  el._modelEl.addEventListener('change', () => {
    el.restart(el._conversationId || undefined);
  });
  loadModels(el);
  el._paletteEl.id = 'pi-palette-list';

  // One primary button, two contracts. After the session ends there is nothing
  // to send, so the same control opens a fresh one in place — no page reload,
  // and no second button competing for the same corner.
  el._composer.addEventListener('submit', (e) => {
    e.preventDefault();
    if (el._sendBtn.dataset.mode === 'reconnect') {
      void el.newConversation();
    } else {
      void send(el);
    }
  });
  el._stopBtn.addEventListener('click', () => el._post('abort', {}));
  el._approvalModeBtn.addEventListener('click', () => void toggleApprovalMode(el));
  el._clearBtn.addEventListener('click', () => void el.newConversation());

  el._input.addEventListener('input', () => {
    autogrow(el);
    refreshPalette(el);
  });
  el._input.addEventListener('blur', () => {
    // Deferred: a click on a palette entry fires blur first, and hiding the
    // list synchronously would remove the element before the click lands.
    setTimeout(() => hidePalette(el), 150);
  });
  el._input.addEventListener('keydown', (e) => onKey(el, e));

  wireScroll(el);
}

/**
 * The replay CTAs, both of which hand off to the auth pane beside this
 * terminal — that pane owns the passkey ceremony, and there is no sign-in
 * page to link to. Every lookup is guarded because the element is embeddable
 * on pages that have no pane at all, where the CTAs simply do nothing.
 */
function wireCta(el) {
  const go = (tab) => {
    const pane = document.querySelector('sp-auth-pane');
    if (!pane) return;
    // The panes stack on narrow screens, so the pane is often off-screen when
    // the button is pressed; focusing without scrolling would move focus to
    // something the visitor cannot see.
    pane.scrollIntoView({
      block: 'nearest',
      behavior: matchMedia('(prefers-reduced-motion: reduce)').matches
        ? 'auto' : 'smooth',
    });
    pane.querySelector('[data-role="tab-' + tab + '"]')?.click();
    pane.querySelector('input[type="email"]')?.focus({ preventScroll: true });
    summon(pane);
  };
  el.querySelector('[data-role="cta-register"]')
    .addEventListener('click', () => go('register'));
  el.querySelector('[data-role="cta-signin"]')
    .addEventListener('click', () => go('signin'));
}

/**
 * Flag the pane as the thing that just answered the click.
 *
 * The button and the form it acts on are in different halves of the page, and
 * on a wide screen nothing moves when the CTA is pressed — the tab swap alone
 * is too quiet to read as a response. The class drives a one-shot ring in
 * `auth-pane-core.css`; it comes off on a timer rather than on animationend,
 * because under `prefers-reduced-motion` there is no animation to end and the
 * highlight would never clear.
 */
const SUMMON_MS = 750;

function summon(pane) {
  const shell = pane.querySelector('.pane');
  if (!shell) return;
  clearTimeout(pane._summonTimer);
  shell.classList.remove('is-summoned');
  // Force the removal to land before the class is set again, or the same
  // frame's add/remove pair is coalesced and no animation restarts.
  void shell.offsetWidth;
  shell.classList.add('is-summoned');
  pane._summonTimer = setTimeout(
    () => shell.classList.remove('is-summoned'), SUMMON_MS,
  );
}

// Autoscroll only while the visitor is actually at the bottom. Yanking the
// view down mid-turn makes the transcript unreadable exactly when there is
// something worth reading in it.
function wireScroll(el) {
  el._body.addEventListener('scroll', () => {
    const gap = el._body.scrollHeight - el._body.scrollTop - el._body.clientHeight;
    el._pinned = gap < PIN_SLACK_PX;
    if (el._pinned) clearUnseen(el);
  });
  el._jumpBtn.addEventListener('click', () => {
    el._pinned = true;
    clearUnseen(el);
    el._body.scrollTop = el._body.scrollHeight;
  });
}

/**
 * The model allow-list, fetched once. Silent on failure and hidden below
 * two entries: the picker is a convenience, never a precondition.
 */
async function loadModels(el) {
  const cat = await getJson(el._endpoint + '/models');
  // No catalogue, no picker — the server default still applies.
  if (!cat) return;
  const models = cat.models || [];
  if (models.length < 2 || !el._modelEl) return;

  // Grouped by provider, in catalogue order: the grouping is itself part of
  // the demo — one governed endpoint fronting several vendors.
  el._modelEl.replaceChildren();
  const groups = new Map();
  models.forEach((m) => {
    const provider = m.provider || 'gateway';
    if (!groups.has(provider)) {
      const g = document.createElement('optgroup');
      g.label = provider;
      groups.set(provider, g);
      el._modelEl.append(g);
    }
    const opt = document.createElement('option');
    opt.value = m.id;
    // The price card rides on the row: the header meters the bill, so the
    // picker states the rate. `$in/$out per MTok`, then the window — the
    // two numbers that actually differentiate models in the same family.
    opt.textContent = modelLabel(m);
    opt.title = modelTitle(m);
    opt.selected = m.id === cat.default;
    groups.get(provider).append(opt);
  });
  el._modelEl.hidden = false;
}
