import { whoami } from './pi-transport.js';

/**
 * <sp-auth-pane> — the right half of the homepage.
 *
 * One element with two lives. Anonymous, it is the sign-in and registration
 * form. Signed in, it is the visitor's own governance readout for the agent
 * running in the terminal beside it: what policy allowed, what it blocked, what
 * the model cost, how long it took.
 *
 * It never navigates. The passkey ceremony completes in place
 * (`establishSessionInline`), which is the only reason the terminal can go from
 * a scripted replay to a live session without the page reloading underneath it.
 *
 * Light DOM, like the other two `sp-` components, so the global --sp-* tokens
 * and [data-theme] reach it.
 *
 * This file is the controller: lifecycle, identity, timers, and the element
 * state the sibling modules read off it.
 */

import { renderAuth } from './sp-auth-pane-auth.js';
import { profileHtml } from './sp-auth-pane-forms.js';
import { wireTabs, selectTab } from './sp-auth-pane-tabs.js';
import { pollPulse, PULSE_POLL_MS } from './sp-auth-pane-pulse.js';
import { poll, setStat, POLL_MS } from './sp-auth-pane-stats.js';
import { applyStages, pushFeed, syncFeedPreview, IDLE_STAGES } from './sp-auth-pane-governance.js';

class SpAuthPane extends HTMLElement {
  constructor() {
    super();
    this._who = null;
    this._conversation = null;
    this._token = null;
    this._pollTimer = null;
    this._pulseTimer = null;
    this._lastFrameAt = 0;
    this._live = { tools: 0, blocked: 0, approvals: 0, turns: 0 };
  }

  async connectedCallback() {
    if (this._built) return;
    this._built = true;
    this.classList.add('auth-pane');

    // The terminal announces its conversation as soon as it has one. Listening
    // on the document rather than on a sibling reference keeps the two elements
    // independent of where the page chooses to put them.
    document.addEventListener('pi-session', (e) => this._onSession(e.detail));
    document.addEventListener('pi-frame', (e) => this._onFrame(e.detail));

    this._who = await whoami();
    this._render();
  }

  disconnectedCallback() {
    this._stopPolling();
  }

  // ── identity ──────────────────────────────────────────────────────────────

  /**
   * Announce the new identity so the terminal restarts against it, then swap
   * this pane over to the telemetry view.
   */
  async _onAuthenticated() {
    this._who = await whoami();
    this._render();
    this.dispatchEvent(new CustomEvent('sp-auth:authenticated', {
      detail: this._who, bubbles: true,
    }));
  }

  // ── render ────────────────────────────────────────────────────────────────

  _render() {
    if (this._who && this._who.user_id) this._renderProfile();
    else renderAuth(this);
  }

  _renderProfile() {
    const who = this._who || {};
    const pending = who.is_approved === false;
    this.innerHTML = profileHtml(pending);

    const name = who.username || (who.email || '').split('@')[0] || 'You';
    this.querySelector('[data-role="name"]').textContent = name;
    this.querySelector('[data-role="email"]').textContent = who.email || '';
    this.querySelector('[data-role="avatar"]').textContent = name.slice(0, 1).toUpperCase();
    const badge = this.querySelector('[data-role="badge"]');
    badge.textContent = pending ? 'Under review' : 'Approved';
    badge.classList.add(pending ? 'pane-badge--pending' : 'pane-badge--ok');

    this._feed = this.querySelector('[data-role="feed"]');
    this._feedCount = this.querySelector('[data-role="feed-count"]');
    this._feedPreview = this.querySelector('[data-role="feed-preview"]');
    this._credit = this.querySelector('[data-role="credit"]');
    this._stages = this.querySelector('[data-role="stages"]');
    this._stageSub = this.querySelector('[data-role="stage-sub"]');
    this._stageMini = this.querySelector('[data-role="stage-mini"]');
    this._govChip = this.querySelector('[data-role="gov-chip"]');
    this._pulse = this.querySelector('[data-role="pulse"]');
    wireTabs(this);
    const viewGov = this.querySelector('[data-role="view-gov"]');
    if (viewGov) viewGov.addEventListener('click', () => selectTab(this, 'governance'));
    syncFeedPreview(this);
    // Render the four stages at zero straight away. Waiting for the first poll
    // would mean the pipeline appears to come into existence once something
    // trips it, which is the opposite of the claim.
    applyStages(this, IDLE_STAGES);
    this.querySelector('[data-role="signout"]').addEventListener('click', () => this._signOut());

    // The admin block is rebuilt from scratch on each render, so drop the stale
    // reference rather than pointing at a node that is no longer in the tree.
    this._pulseAdmin = null;
    if (this._conversation) this._startPolling();
    this._startPulsePolling();
  }

  // ── session ───────────────────────────────────────────────────────────────

  _onSession(detail) {
    this._conversation = detail.conversation_id;
    this._token = detail.token;
    this._lastFrameAt = Date.now();
    // A new conversation is a new set of numbers; carrying the last one's
    // counters over would show the visitor tool calls they never made.
    this._live = { tools: 0, blocked: 0, approvals: 0, turns: 0 };
    this._pollMs = POLL_MS;
    if (this._who) this._startPolling();
  }

  /**
   * Frames are the fast path — they move the counters in the same beat as the
   * terminal renders them. The poll behind it is what makes the numbers true:
   * tokens, cost, and latency only exist once the request row lands.
   */
  _onFrame(f) {
    this._lastFrameAt = Date.now();
    if (!this._feed) return;
    if (f.type === 'tool_start') {
      this._live.tools += 1;
      setStat(this, 'tools', String(this._live.tools));
    } else if (f.type === 'tool_blocked' || f.type === 'prompt_blocked') {
      this._live.blocked += 1;
      setStat(this, 'blocked', String(this._live.blocked));
      pushFeed(this, {
        kind: f.type === 'tool_blocked' ? 'tool' : 'prompt',
        subject: f.tool_name || 'user_prompt',
        outcome: 'deny',
        policy: f.policy || '',
        detail: f.reason || '',
      });
    } else if (f.type === 'turn_end') {
      // A turn just settled, so the request row exists — read it now rather
      // than waiting out the poll interval.
      poll(this);
    }
  }

  // ── timers ────────────────────────────────────────────────────────────────

  _startPolling() {
    // Clears only the session timer. Calling the full `_stopPolling` here would
    // take the pulse down with it and never bring it back — the pulse is
    // started once per render, not once per conversation.
    this._stopSessionPolling();
    poll(this);
    this._pollTimer = setInterval(() => poll(this), POLL_MS);
  }

  _stopSessionPolling() {
    if (this._pollTimer) clearInterval(this._pollTimer);
    this._pollTimer = null;
  }

  /**
   * The platform pulse runs on its own timer, an order of magnitude slower than
   * the session poll: the aggregate is cached server-side for a minute, so
   * asking every three seconds would be twenty requests for one answer.
   *
   * Separate from [`_startPolling`] because the two have different
   * preconditions. Session telemetry needs a conversation; the pulse needs
   * nothing at all, and is the only thing on this pane an anonymous visitor
   * ever sees.
   */
  _startPulsePolling() {
    this._stopPulsePolling();
    pollPulse(this);
    this._pulseTimer = setInterval(() => pollPulse(this), PULSE_POLL_MS);
  }

  _stopPulsePolling() {
    if (this._pulseTimer) clearInterval(this._pulseTimer);
    this._pulseTimer = null;
  }

  /** Everything, for disconnect. */
  _stopPolling() {
    this._stopSessionPolling();
    this._stopPulsePolling();
  }

  async _signOut() {
    try {
      await fetch('/api/public/auth/session', { method: 'DELETE', credentials: 'same-origin' });
    } catch (_) {
      // Even if the call failed, drop the local view — the cookie is either
      // gone or about to be rejected, and leaving a stale profile up is worse.
    }
    this._stopPolling();
    this._who = null;
    this._conversation = null;
    this._token = null;
    this._live = { tools: 0, blocked: 0, approvals: 0, turns: 0 };
    this._render();
    this.dispatchEvent(new CustomEvent('sp-auth:signed-out', { bubbles: true }));
  }

  // ── small ui helpers ──────────────────────────────────────────────────────

  _setBusy(message) {
    if (!this._busy) return;
    this._busy.hidden = !message;
    if (message) this._busyText.textContent = message;
    this.querySelectorAll('.pane-form button').forEach((b) => { b.disabled = !!message; });
  }

  _showAlert(message, tone) {
    if (!this._alert) return;
    this._alert.textContent = message;
    this._alert.dataset.tone = tone || 'info';
    this._alert.hidden = false;
  }

  _clearAlert() {
    if (this._alert) this._alert.hidden = true;
  }
}

customElements.define('sp-auth-pane', SpAuthPane);
