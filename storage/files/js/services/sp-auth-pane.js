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
import { createPulse } from './sp-auth-pane-pulse.js';
import {
  poll, setStat, applyStats, FALLBACK_POLL_MS, PUSH_FRESH_MS,
} from './sp-auth-pane-stats.js';
import { applyStages, pushFeed, syncFeedPreview, IDLE_STAGES } from './sp-auth-pane-governance.js';

class SpAuthPane extends HTMLElement {
  constructor() {
    super();
    this._who = null;
    this._conversation = null;
    this._token = null;
    this._pollTimer = null;
    this._pulse = null;
    this._lastFrameAt = 0;
    this._lastStatsPushAt = 0;
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
    if (this._pulse) this._pulse.stop();
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
    // Fires on both paths into the profile view: a fresh sign-in and a session
    // recognised on load. `sp-auth:authenticated` only covers the first, and
    // the page hides the site header until an identity exists either way.
    this.dispatchEvent(new CustomEvent('sp-auth:identified', {
      detail: this._who, bubbles: true,
    }));
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
    wireTabs(this);
    const viewGov = this.querySelector('[data-role="view-gov"]');
    if (viewGov) viewGov.addEventListener('click', () => selectTab(this, 'governance'));
    syncFeedPreview(this);
    // Render the four stages at zero straight away. Waiting for the first poll
    // would mean the pipeline appears to come into existence once something
    // trips it, which is the opposite of the claim.
    applyStages(this, IDLE_STAGES);
    this.querySelector('[data-role="signout"]').addEventListener('click', () => this._signOut());

    if (this._conversation) this._startPolling();
    // The Platform tab is a self-contained unit: it owns its own timer,
    // elements, and reveal decision. A render replaces the DOM it points at,
    // so the old unit is retired and a fresh one built over the new tree.
    if (this._pulse) this._pulse.stop();
    this._pulse = createPulse(this);
    if (this._token) this._pulse.setToken(this._token);
  }

  // ── session ───────────────────────────────────────────────────────────────

  _onSession(detail) {
    this._conversation = detail.conversation_id;
    this._token = detail.token;
    this._lastFrameAt = Date.now();
    // A new conversation is a new set of numbers; carrying the last one's
    // counters over would show the visitor tool calls they never made.
    this._live = { tools: 0, blocked: 0, approvals: 0, turns: 0 };
    this._lastStatsPushAt = 0;
    if (this._who) this._startPolling();
    if (this._pulse) this._pulse.setToken(this._token);
  }

  /**
   * Frames are the fast path — they move the counters in the same beat as the
   * terminal renders them. The `stats` frame behind them is what makes the
   * numbers true: tokens, cost, and latency only exist once the request row
   * lands, and the stream pushes that snapshot once each turn settles.
   */
  _onFrame(f) {
    this._lastFrameAt = Date.now();
    if (!this._feed) return;
    if (f.type === 'stats') {
      this._lastStatsPushAt = Date.now();
      applyStats(this, f.stats || {});
    } else if (f.type === 'tool_start') {
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
    } else if (f.type === 'turn_end' && !this._lastStatsPushAt) {
      // A server that pushes stats will follow this turn with its own
      // snapshot; one that never has is an older binary, so read the numbers
      // the turn just landed ourselves.
      poll(this);
    }
  }

  // ── timers ────────────────────────────────────────────────────────────────

  /**
   * Not the data path — the safety net. The stream pushes a stats frame on
   * connect and after every settled turn; this timer only fetches when no
   * push has arrived recently, which covers an older server binary and the
   * window where the terminal's EventSource is down.
   */
  _startPolling() {
    this._stopPolling();
    poll(this);
    this._pollTimer = setInterval(() => {
      if (Date.now() - this._lastStatsPushAt > PUSH_FRESH_MS) poll(this);
    }, FALLBACK_POLL_MS);
  }

  _stopPolling() {
    if (this._pollTimer) clearInterval(this._pollTimer);
    this._pollTimer = null;
  }

  async _signOut() {
    try {
      await fetch('/api/public/auth/session', { method: 'DELETE', credentials: 'same-origin' });
    } catch (_) {
      // Even if the call failed, drop the local view — the cookie is either
      // gone or about to be rejected, and leaving a stale profile up is worse.
    }
    this._stopPolling();
    if (this._pulse) this._pulse.stop();
    this._pulse = null;
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
