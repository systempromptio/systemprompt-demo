'use strict';

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
 */

import {
  makeRequest,
  preparePublicKeyCredentialCreationOptions,
  preparePublicKeyCredentialRequestOptions,
} from '/js/services/webauthn-utils.js';
import {
  buildAuthCredentialPayload, buildCreationCredentialPayload,
  establishSessionInline, WEBAUTHN_BASE,
} from '/js/services/webauthn-passkey-helpers.js';

/** How often the authoritative numbers are re-read while a session is live. */
const POLL_MS = 3000;

/** Backs off to this once a conversation has gone quiet, to stop hammering. */
const IDLE_POLL_MS = 15000;

/** Nothing has happened for this long → treat the session as idle. */
const IDLE_AFTER_MS = 60000;

const TEAM_SIZES = ['1–10', '11–50', '51–200', '201–1000', '1000+'];

class SpAuthPane extends HTMLElement {
  constructor() {
    super();
    this._who = null;
    this._conversation = null;
    this._token = null;
    this._pollTimer = null;
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

    this._who = await this._whoami();
    this._render();
  }

  disconnectedCallback() {
    this._stopPolling();
  }

  // ── identity ──────────────────────────────────────────────────────────────

  /**
   * `/admin/auth/me` 307s to the login page for an anonymous visitor rather
   * than answering 401, so an unguarded fetch would return 200 OK carrying
   * HTML. `redirect: 'manual'` makes that an opaque response we can reject.
   */
  async _whoami() {
    try {
      const res = await fetch('/admin/auth/me', {
        credentials: 'same-origin',
        redirect: 'manual',
      });
      if (!res.ok) return null;
      if ((res.headers.get('content-type') || '').indexOf('application/json') === -1) return null;
      return await res.json();
    } catch (_) {
      return null;
    }
  }

  /**
   * Announce the new identity so the terminal restarts against it, then swap
   * this pane over to the telemetry view.
   */
  async _onAuthenticated() {
    this._who = await this._whoami();
    this._render();
    this.dispatchEvent(new CustomEvent('sp-auth:authenticated', {
      detail: this._who, bubbles: true,
    }));
  }

  // ── render ────────────────────────────────────────────────────────────────

  _render() {
    if (this._who && this._who.user_id) this._renderProfile();
    else this._renderAuth();
  }

  _renderAuth() {
    this.innerHTML = ''
      + '<div class="pane">'
      + '<header class="pane-head">'
      + '<h2 class="pane-title">Drive it yourself</h2>'
      + '<p class="pane-sub">The terminal on the left is a replay until you sign in. '
      + 'With an account it runs a real agent whose every tool call stops for your '
      + 'approval — and everything it does lands here.</p>'
      + '</header>'
      + '<div class="pane-tabs" role="tablist">'
      + '<button type="button" class="pane-tab is-active" data-role="tab-signin" role="tab">Sign in</button>'
      + '<button type="button" class="pane-tab" data-role="tab-register" role="tab">Create account</button>'
      + '</div>'
      + '<div class="pane-alert" data-role="alert" hidden></div>'
      + '<div class="pane-busy" data-role="busy" hidden><span class="pane-spinner"></span>'
      + '<span data-role="busy-text"></span></div>'
      + this._signinFormHtml()
      + this._registerFormHtml()
      + '</div>';

    this._alert = this.querySelector('[data-role="alert"]');
    this._busy = this.querySelector('[data-role="busy"]');
    this._busyText = this.querySelector('[data-role="busy-text"]');
    this._signin = this.querySelector('[data-role="signin"]');
    this._register = this.querySelector('[data-role="register"]');

    const tabSignin = this.querySelector('[data-role="tab-signin"]');
    const tabRegister = this.querySelector('[data-role="tab-register"]');
    const show = (which) => {
      const registering = which === 'register';
      this._signin.hidden = registering;
      this._register.hidden = !registering;
      tabSignin.classList.toggle('is-active', !registering);
      tabRegister.classList.toggle('is-active', registering);
      this._clearAlert();
    };
    this._showTab = show;
    tabSignin.addEventListener('click', () => show('signin'));
    tabRegister.addEventListener('click', () => show('register'));

    this._signin.addEventListener('submit', (e) => {
      e.preventDefault();
      this._doSignIn();
    });
    this._register.addEventListener('submit', (e) => {
      e.preventDefault();
      this._doRegister();
    });
    const step1 = this.querySelector('[data-role="step-1"]');
    const step2 = this.querySelector('[data-role="step-2"]');
    this.querySelector('[data-role="next"]').addEventListener('click', () => {
      // `required` only fires on submit, and step one has no submit button, so
      // the check has to be explicit or step two accepts a blank email.
      const email = this.querySelector('#ap-reg-email');
      const name = this.querySelector('#ap-reg-name');
      if (!email.checkValidity() || !name.value.trim()) {
        this._showAlert('Enter your work email and name to continue.', 'error');
        return;
      }
      this._clearAlert();
      step1.hidden = true;
      step2.hidden = false;
    });
    this.querySelector('[data-role="back"]').addEventListener('click', () => {
      this._clearAlert();
      step2.hidden = true;
      step1.hidden = false;
    });

    // No magic-link fallback offered here: `request_magic_link` mints a token
    // and logs it, but this deployment wires no email sender, so the button
    // would be a promise nothing keeps. `/admin/add-passkey` still honours a
    // `return` param for when one is wired.

    if (!window.PublicKeyCredential) {
      this._showAlert('This browser does not support passkeys. Use a recent Chrome, '
        + 'Firefox, Safari, or Edge.', 'error');
      this.querySelectorAll('button[type="submit"]').forEach((b) => { b.disabled = true; });
    }
  }

  _signinFormHtml() {
    return ''
      + '<form class="pane-form" data-role="signin">'
      + '<label class="pane-label" for="ap-signin-email">Email</label>'
      + '<input class="pane-field" id="ap-signin-email" type="email" autocomplete="email"'
      + ' placeholder="you@company.com" required>'
      + '<button type="submit" class="pane-btn pane-btn--primary">Continue with passkey</button>'
      + '<p class="pane-note">No password — your device authenticates you.</p>'
      + '</form>';
  }

  /**
   * Two steps rather than one long column: the profile fields are what a human
   * reads when approving the account, so they are not optional, but asking for
   * six of them before anything has happened reads as a wall.
   */
  _registerFormHtml() {
    const sizes = TEAM_SIZES.map((s) => '<option>' + s + '</option>').join('');
    return ''
      + '<form class="pane-form" data-role="register" hidden>'
      + '<fieldset class="pane-step" data-role="step-1">'
      + '<label class="pane-label" for="ap-reg-email">Work email</label>'
      + '<input class="pane-field" id="ap-reg-email" type="email" autocomplete="email"'
      + ' placeholder="you@company.com" required>'
      + '<label class="pane-label" for="ap-reg-name">Your name</label>'
      + '<input class="pane-field" id="ap-reg-name" type="text" autocomplete="name" required>'
      + '<button type="button" class="pane-btn pane-btn--primary" data-role="next">Continue</button>'
      + '</fieldset>'
      + '<fieldset class="pane-step" data-role="step-2" hidden>'
      + '<label class="pane-label" for="ap-reg-company">Company</label>'
      + '<input class="pane-field" id="ap-reg-company" type="text" autocomplete="organization" required>'
      + '<label class="pane-label" for="ap-reg-role">Role</label>'
      + '<input class="pane-field" id="ap-reg-role" type="text" autocomplete="organization-title" required>'
      + '<label class="pane-label" for="ap-reg-team">Engineers using AI tools</label>'
      + '<select class="pane-field" id="ap-reg-team" required>' + sizes + '</select>'
      + '<label class="pane-label" for="ap-reg-why">What are you evaluating?</label>'
      + '<textarea class="pane-field" id="ap-reg-why" rows="3" required'
      + ' placeholder="Governing Claude Code across the team"></textarea>'
      + '<div class="pane-actions">'
      + '<button type="button" class="pane-btn pane-btn--ghost" data-role="back">Back</button>'
      + '<button type="submit" class="pane-btn pane-btn--primary">Create account</button>'
      + '</div>'
      + '<p class="pane-note">The terminal works the moment you register. '
      + 'The $5 credit and the Bridge wait on a short review.</p>'
      + '</fieldset>'
      + '</form>';
  }

  // ── ceremonies ────────────────────────────────────────────────────────────

  async _doSignIn() {
    const email = this.querySelector('#ap-signin-email').value.trim();
    if (!email) return;
    this._clearAlert();
    try {
      this._setBusy('Waiting for your passkey…');
      const start = await makeRequest(
        WEBAUTHN_BASE + '/auth/start?email=' + encodeURIComponent(email), 'POST',
      );
      const options = preparePublicKeyCredentialRequestOptions(start.data.publicKey);
      const credential = await navigator.credentials.get({ publicKey: options });
      if (!credential) throw new Error('Sign-in was cancelled.');
      this._setBusy('Verifying…');
      const finish = await makeRequest(WEBAUTHN_BASE + '/auth/finish', 'POST', {
        challenge_id: start.data.challenge_id,
        credential: buildAuthCredentialPayload(credential),
      });
      await establishSessionInline(
        finish.data.user_id, finish.data.auth_token, (m) => this._setBusy(m),
      );
      await this._onAuthenticated();
    } catch (err) {
      this._setBusy(null);
      this._showAlert(passkeyMessage(err, 'Sign-in failed. Please try again.'), 'error');
    }
  }

  async _doRegister() {
    const value = (id) => (this.querySelector(id).value || '').trim();
    const payload = {
      name: value('#ap-reg-name'),
      email: value('#ap-reg-email'),
      company: value('#ap-reg-company'),
      role: value('#ap-reg-role'),
      team_size: value('#ap-reg-team'),
      why_assessing: value('#ap-reg-why'),
    };
    if (Object.values(payload).some((v) => !v)) {
      this._showAlert('Please complete every field.', 'error');
      return;
    }
    this._clearAlert();
    try {
      this._setBusy('Submitting your request…');
      const res = await fetch('/admin/api/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(data.error || data.message || 'Registration failed.');

      // The email already owns an account, so there is no passkey to create.
      // Send them to the sign-in tab with it filled in rather than erroring.
      if (data.already_registered) {
        this._setBusy(null);
        this._showTab('signin');
        this.querySelector('#ap-signin-email').value = payload.email;
        this._showAlert('That email already has an account — sign in with your passkey.', 'info');
        return;
      }

      this._setBusy('Creating your passkey…');
      const start = await makeRequest(
        WEBAUTHN_BASE + '/link/start?token=' + encodeURIComponent(data.token), 'GET',
      );
      const options = preparePublicKeyCredentialCreationOptions(
        start.data.challenge ? start.data.challenge.publicKey : start.data.publicKey,
      );
      const credential = await navigator.credentials.create({ publicKey: options });
      if (!credential) throw new Error('Passkey creation was cancelled.');
      this._setBusy('Finishing registration…');
      await makeRequest(WEBAUTHN_BASE + '/link/finish', 'POST', {
        challenge_id: start.data.challenge_id || start.challengeId,
        token: data.token,
        credential: buildCreationCredentialPayload(credential),
      });

      // Straight into a session: the passkey that was just created is the one
      // the sign-in ceremony is about to ask for.
      this._setBusy('Signing you in…');
      const authStart = await makeRequest(
        WEBAUTHN_BASE + '/auth/start?email=' + encodeURIComponent(payload.email), 'POST',
      );
      const authOptions = preparePublicKeyCredentialRequestOptions(authStart.data.publicKey);
      const authCredential = await navigator.credentials.get({ publicKey: authOptions });
      if (!authCredential) throw new Error('Sign-in was cancelled.');
      const finish = await makeRequest(WEBAUTHN_BASE + '/auth/finish', 'POST', {
        challenge_id: authStart.data.challenge_id,
        credential: buildAuthCredentialPayload(authCredential),
      });
      await establishSessionInline(
        finish.data.user_id, finish.data.auth_token, (m) => this._setBusy(m),
      );
      await this._onAuthenticated();
    } catch (err) {
      this._setBusy(null);
      this._showAlert(passkeyMessage(err, 'Registration failed. Please try again.'), 'error');
    }
  }

  // ── profile + telemetry ───────────────────────────────────────────────────

  _renderProfile() {
    const who = this._who || {};
    const pending = who.is_approved === false;
    this.innerHTML = ''
      + '<div class="pane">'
      + '<header class="pane-head pane-head--profile">'
      + '<div class="pane-id">'
      + '<span class="pane-avatar" data-role="avatar"></span>'
      + '<div><strong class="pane-name" data-role="name"></strong>'
      + '<span class="pane-email" data-role="email"></span></div>'
      + '</div>'
      + '<span class="pane-badge" data-role="badge"></span>'
      + '</header>'
      + (pending
        ? '<p class="pane-note pane-note--pending">Your account is under review. '
          + 'The terminal is yours now; the $5 credit and the Bridge unlock once a '
          + 'human approves it.</p>'
        : '')
      + '<section class="pane-section">'
      + '<h3 class="pane-h3">This session</h3>'
      + '<dl class="pane-stats" data-role="stats">'
      + statHtml('model', 'Model', '—')
      + statHtml('tools', 'Tool calls', '0')
      + statHtml('blocked', 'Blocked', '0')
      + statHtml('latency', 'Latency p50', '—')
      + statHtml('tokens', 'Tokens', '0')
      + statHtml('cost', 'Cost', '$0')
      + '</dl>'
      + '</section>'
      + '<section class="pane-section pane-section--feed">'
      + '<h3 class="pane-h3">Governance <span class="pane-h3-sub" data-role="feed-count"></span></h3>'
      + '<ol class="pane-feed" data-role="feed">'
      + '<li class="pane-feed-empty">Ask the agent to read a file. Every decision it '
      + 'triggers is recorded here.</li>'
      + '</ol>'
      + '</section>'
      + '<footer class="pane-foot">'
      + '<button type="button" class="pane-link" data-role="signout">Sign out</button>'
      + '</footer>'
      + '</div>';

    const name = who.username || (who.email || '').split('@')[0] || 'You';
    this.querySelector('[data-role="name"]').textContent = name;
    this.querySelector('[data-role="email"]').textContent = who.email || '';
    this.querySelector('[data-role="avatar"]').textContent = name.slice(0, 1).toUpperCase();
    const badge = this.querySelector('[data-role="badge"]');
    badge.textContent = pending ? 'Under review' : 'Approved';
    badge.classList.add(pending ? 'pane-badge--pending' : 'pane-badge--ok');

    this._feed = this.querySelector('[data-role="feed"]');
    this._feedCount = this.querySelector('[data-role="feed-count"]');
    this.querySelector('[data-role="signout"]').addEventListener('click', () => this._signOut());

    if (this._conversation) this._startPolling();
  }

  _stat(key, value) {
    const el = this.querySelector('[data-stat="' + key + '"]');
    if (el && el.textContent !== value) {
      el.textContent = value;
      // Re-triggering the animation needs the class to actually leave the
      // element first; a same-frame remove/add is coalesced away.
      el.classList.remove('is-changed');
      void el.offsetWidth;
      el.classList.add('is-changed');
    }
  }

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
      this._stat('tools', String(this._live.tools));
    } else if (f.type === 'tool_blocked' || f.type === 'prompt_blocked') {
      this._live.blocked += 1;
      this._stat('blocked', String(this._live.blocked));
      this._pushFeed({
        kind: f.type === 'tool_blocked' ? 'tool' : 'prompt',
        subject: f.tool_name || 'user_prompt',
        outcome: 'deny',
        policy: f.policy || '',
        detail: f.reason || '',
      });
    } else if (f.type === 'turn_end') {
      // A turn just settled, so the request row exists — read it now rather
      // than waiting out the poll interval.
      this._poll();
    }
  }

  _startPolling() {
    this._stopPolling();
    this._poll();
    this._pollTimer = setInterval(() => this._poll(), POLL_MS);
  }

  _stopPolling() {
    if (this._pollTimer) clearInterval(this._pollTimer);
    this._pollTimer = null;
  }

  async _poll() {
    if (!this._conversation || !this._token || !this._feed) return;
    // A conversation nobody is driving still costs a request every few seconds.
    // Once it has gone quiet, check it a quarter as often.
    const idle = Date.now() - this._lastFrameAt > IDLE_AFTER_MS;
    if (idle && this._pollTimer && this._pollMs !== IDLE_POLL_MS) {
      this._pollMs = IDLE_POLL_MS;
      clearInterval(this._pollTimer);
      this._pollTimer = setInterval(() => this._poll(), IDLE_POLL_MS);
    }
    try {
      const url = '/api/public/pi/stats/' + encodeURIComponent(this._conversation)
        + '?token=' + encodeURIComponent(this._token);
      const res = await fetch(url, { credentials: 'same-origin', redirect: 'manual' });
      if (!res.ok) return;
      this._applyStats(await res.json());
    } catch (_) {
      // A failed poll is not worth surfacing: the next one is three seconds
      // away, and the frame-driven counters are still moving.
    }
  }

  _applyStats(s) {
    this._stat('model', s.model || '—');
    this._stat('tools', String(Math.max(s.tool_calls, this._live.tools)));
    this._stat('blocked', String(Math.max(s.denied, this._live.blocked)));
    this._stat('latency', s.latency_p50_ms === null || s.latency_p50_ms === undefined
      ? '—' : s.latency_p50_ms + 'ms');
    this._stat('tokens', compact(s.input_tokens) + ' in / ' + compact(s.output_tokens) + ' out');
    this._stat('cost', s.cost_display || '$0');
    this._renderFeed(s.events || []);
  }

  _renderFeed(events) {
    this._feedCount.textContent = events.length ? events.length + ' recorded' : '';
    if (!events.length) return;
    this._feed.innerHTML = '';
    // Newest first: the pane is short, and the thing that just happened is the
    // thing being watched for.
    events.slice(-40).reverse().forEach((e) => this._feed.append(feedItem(e)));
  }

  _pushFeed(e) {
    const empty = this._feed.querySelector('.pane-feed-empty');
    if (empty) empty.remove();
    this._feed.prepend(feedItem(e));
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

function statHtml(key, label, initial) {
  return '<div class="pane-stat"><dt>' + label + '</dt>'
    + '<dd data-stat="' + key + '">' + initial + '</dd></div>';
}

function feedItem(e) {
  const li = document.createElement('li');
  li.className = 'pane-feed-item pane-feed-item--' + (e.outcome === 'deny' ? 'deny' : 'allow');
  const head = document.createElement('span');
  head.className = 'pane-feed-head';
  head.textContent = (e.kind || 'event') + ' · ' + (e.subject || '');
  const tail = document.createElement('span');
  tail.className = 'pane-feed-tail';
  tail.textContent = [e.policy, e.detail].filter(Boolean).join(' — ');
  li.append(head, tail);
  return li;
}

/** Token counts get long fast; the pane has one line for them. */
function compact(n) {
  const v = Number(n) || 0;
  if (v < 1000) return String(v);
  if (v < 1000000) return (v / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
  return (v / 1000000).toFixed(1).replace(/\.0$/, '') + 'M';
}

/** WebAuthn reports user choices as exceptions; say what actually happened. */
function passkeyMessage(err, fallback) {
  if (err && err.name === 'NotAllowedError') return 'That was cancelled, or the passkey timed out.';
  if (err && err.name === 'NotSupportedError') return 'Passkeys are not supported on this device.';
  return (err && err.message) || fallback;
}

if (!customElements.get('sp-auth-pane')) {
  customElements.define('sp-auth-pane', SpAuthPane);
}
