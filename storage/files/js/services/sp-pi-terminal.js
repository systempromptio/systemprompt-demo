import { PI_API_BASE, RECONNECT_MIN_MS } from './pi-constants.js';
import { postJson } from './pi-transport.js';
import { build } from './pi-terminal-setup.js';
import { start, restart } from './pi-terminal-session.js';

/**
 * <sp-pi-terminal endpoint="/api/public/pi" [token="..."]>
 *
 * A live view onto a server-side pi agent whose every tool call is gated in
 * Rust and then by a human. Light DOM on purpose, so the global --sp-* tokens
 * and [data-theme] apply.
 *
 * Not a terminal emulator, and not trying to be. The stream carries a typed
 * vocabulary — text deltas, tool calls, and the governance chain that judged
 * them — with no cursor addressing and no ANSI, so there is nothing to emulate.
 * What this renders instead is the gate: which policies ran, in what order, what
 * each decided, and where the chain stopped. See pi-gate-view.js.
 *
 * Without a usable credential it plays a scripted replay instead, so a public
 * page can embed it unconditionally.
 *
 * The element owns the state and the lifecycle; every renderer lives in a
 * pi-terminal-*.js module and takes the element as its first argument.
 */
class SpPiTerminal extends HTMLElement {
  constructor() {
    super();
    this._conversationId = null;
    this._commands = [];
    this._source = null;
    this._lastSeq = 0;
    this._toolRows = new Map();
    this._approvals = new Map();
    this._artifacts = new Map();
    this._reconnectMs = RECONNECT_MIN_MS;
    this._reconnectTimer = null;
    this._turnLive = false;
    this._closed = false;
    this._pinned = true;
    this._unseen = 0;
    this._lines = 0;
    this._history = [];
    this._historyAt = -1;
    this._statsTimer = null;
    this._capTimer = null;
    this._queued = false;
    this._cannedTimers = [];
    this._cannedCards = [];
    this._cannedRow = null;
    this._workEl = null;
    this._streamBuf = '';
    this._thinkBuf = '';
    this._thinkEl = null;
    this._raf = 0;
    this._replaying = false;
  }

  connectedCallback() {
    if (this._built) return;
    this._built = true;
    this._endpoint = (this.getAttribute('endpoint') || PI_API_BASE).replace(/\/$/, '');
    build(this);
    start(this);
  }

  disconnectedCallback() {
    if (this._onDocClick) document.removeEventListener('click', this._onDocClick);
    if (this._onDocKey) document.removeEventListener('keydown', this._onDocKey);
    this._teardownStream();
    if (this._reconnectTimer) clearTimeout(this._reconnectTimer);
    if (this._statsTimer) clearInterval(this._statsTimer);
    if (this._capTimer) clearTimeout(this._capTimer);
    this._cannedTimers.forEach(clearTimeout);
    this._cannedCards.forEach((c) => c.settle());
    this._approvals.forEach((a) => a.settle());
    this._approvals.clear();
  }

  /**
   * Re-run the whole start sequence against whatever credential now exists.
   *
   * Public because signing in happens outside this element: the pane beside it
   * establishes the cookie without a navigation, so nothing reloads the page
   * and the terminal has to be told the visitor stopped being anonymous.
   */
  restart(resume) {
    return restart(this, resume);
  }

  /** Abandon the current conversation and open an empty one. */
  async newConversation() {
    this._toggleConv(false);
    await restart(this, null);
  }

  /** Reopen a stored conversation of this viewer's. */
  async openConversation(conversationId) {
    this._toggleConv(false);
    await restart(this, conversationId);
  }

  _toggleConv(open) {
    if (!this._convPanel) return;
    this._convPanel.hidden = !open;
    this._convChip.setAttribute('aria-expanded', String(open));
  }

  _teardownStream() {
    if (this._source) {
      this._source.close();
      this._source = null;
    }
  }

  _post(path, extra) {
    const payload = Object.assign(
      { token: this._token, conversation_id: this._conversationId },
      extra,
    );
    return postJson(this._endpoint + '/' + path, payload).catch(() => null);
  }

  _emit(name, detail) {
    this.dispatchEvent(new CustomEvent(name, { detail, bubbles: true }));
  }

  _status(text) {
    this._statusEl.textContent = text;
    this._statusEl.dataset.state = text;
    this._liveEl.dataset.state = text;
  }
}

customElements.define('sp-pi-terminal', SpPiTerminal);
