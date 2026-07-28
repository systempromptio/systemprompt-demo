import { PIN_SLACK_PX } from './pi-constants.js';
import { getJson } from './pi-transport.js';
import { modelLabel, modelTitle } from './pi-format.js';
import { terminalChrome } from './pi-terminal-view.js';
import { autogrow, clearUnseen } from './pi-terminal-dom.js';
import { refreshPalette, hidePalette } from './pi-terminal-palette.js';
import { send, onKey } from './pi-terminal-input.js';
import { wireArtifacts } from './pi-terminal-artifacts.js';
import { startCapacity } from './pi-terminal-capacity.js';

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
  el._stopBtn = role('stop');
  el._jumpBtn = role('jump');
  el._metersEl = role('meters');
  el._traceEl = role('trace');
  el._jailEl = role('jail');
  el._capEl = role('cap');
  el._capPips = role('cap-pips');
  el._capCount = role('cap-count');
  el._modelEl = role('model');
  el._userEl = role('user');
  el._userNameEl = role('user-name');
  el._composer = role('composer');
  el._convChip = role('conv-chip');
  el._convPanel = role('conv-panel');
  el._artWrap = role('art-wrap');
  el._artChip = role('art-chip');
  el._artCount = role('art-count');
  el._artPanel = role('art-panel');

  wireConversations(el);
  wireArtifacts(el);
  // A change spawns a fresh child on the new model, resuming the same
  // conversation so the transcript carries over.
  el._modelEl.addEventListener('change', () => {
    el.restart(el._conversationId || undefined);
  });
  loadModels(el);
  startCapacity(el);
  el._paletteEl.id = 'pi-palette-list';

  el._composer.addEventListener('submit', (e) => {
    e.preventDefault();
    void send(el);
  });
  el._stopBtn.addEventListener('click', () => el._post('abort', {}));

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

function wireConversations(el) {
  // The picker element is created here rather than written in the page so
  // the dropdown works wherever the terminal is embedded. It talks back via
  // `for` and the document-level pi-* events, exactly as it did as a sibling.
  const convList = document.createElement('sp-conversation-list');
  if (el.id) convList.setAttribute('for', el.id);
  convList.setAttribute('endpoint', el._endpoint);
  el._convPanel.append(convList);

  el._convChip.addEventListener('click', () => {
    el._toggleConv(el._convPanel.hidden);
  });
  // Light-dismiss: a click outside the chip and panel, or Escape, closes it.
  el._onDocClick = (e) => {
    if (el._convPanel.hidden) return;
    if (!el._convChip.contains(e.target) && !el._convPanel.contains(e.target)) {
      el._toggleConv(false);
    }
  };
  el._onDocKey = (e) => {
    if (e.key === 'Escape' && !el._convPanel.hidden) el._toggleConv(false);
  };
  document.addEventListener('click', el._onDocClick);
  document.addEventListener('keydown', el._onDocKey);
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
