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
import { renderAdminPulse } from './sp-pulse-admin.js';

/** How often the authoritative numbers are re-read while a session is live. */
const POLL_MS = 3000;

/** Backs off to this once a conversation has gone quiet, to stop hammering. */
const IDLE_POLL_MS = 15000;

/** Nothing has happened for this long → treat the session as idle. */
const IDLE_AFTER_MS = 60000;

/** The platform pulse is cached server-side for a minute; match it. */
const PULSE_POLL_MS = 60000;

/** The pipeline, before any poll has told us what it did. */
const IDLE_STAGES = [
  { id: 'secret_scan', label: 'Secret scan', passed: 0, failed: 0, active: false },
  { id: 'scope_check', label: 'Scope check', passed: 0, failed: 0, active: false },
  { id: 'tool_blocklist', label: 'Tool blocklist', passed: 0, failed: 0, active: false },
  { id: 'rate_limit', label: 'Rate limit', passed: 0, failed: 0, active: false },
];

const TEAM_SIZES = ['1–10', '11–50', '51–200', '201–1000', '1000+'];

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
    else this._renderAuth();
  }

  _renderAuth() {
    this.innerHTML = ''
      + '<div class="pane">'
      // The offer sits above the form rather than inside the copy under it.
      // It is the reason to complete the form, and a visitor who reads only one
      // element on this half of the page should read this one.
      + '<div class="pane-offer">'
      + '<strong class="pane-offer-amount">$5 of free AI</strong>'
      + '<span class="pane-offer-line">on us, to learn what systemprompt.io does</span>'
      + '<span class="pane-offer-fine">No card. Passkey only. Spend it in the terminal '
      + 'on the left and watch every cent land in your own audit trail.</span>'
      + '</div>'
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
      // Lifetime totals only, and only once they arrive. A visitor who has not
      // signed in is shown that the deployment has governed real traffic — the
      // one claim worth making before they have any numbers of their own — and
      // nothing narrow enough to be about a person.
      + '<section class="pane-section pane-section--pulse" data-role="pulse" hidden>'
      + '<h3 class="pane-h3">Across the platform '
      + '<span class="pane-h3-sub" data-role="pulse-window"></span></h3>'
      + '<p class="pane-pulse-note" data-role="pulse-all-time"></p>'
      + '</section>'
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

    this._pulse = this.querySelector('[data-role="pulse"]');
    this._pulseAdmin = null;
    this._startPulsePolling();
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
      + '<p class="pane-note">The terminal, your $5 credit, and the Bridge are '
      + 'yours the moment you register.</p>'
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
      // The credit meter sits above the tabs, not inside one: it is the one
      // number that must stay visible whatever the visitor is looking at.
      + '<section class="pane-section pane-section--credit" data-role="credit" hidden>'
      + '<h3 class="pane-h3">Your credit <span class="pane-h3-sub" data-role="credit-of"></span></h3>'
      + '<div class="pane-credit">'
      + '<div class="pane-credit-figure">'
      + '<strong class="pane-credit-left" data-role="credit-left">$0</strong>'
      + '<span class="pane-credit-cap">left</span>'
      + '</div>'
      // A meter rather than a bare number: the shape of the bar is what makes
      // "you have barely touched it" legible at a glance, which is the whole
      // point of showing a free grant back to the person who was given it.
      + '<div class="pane-credit-bar" data-role="credit-bar" role="img"'
      + ' aria-label="credit remaining"><span data-role="credit-fill"></span></div>'
      + '<p class="pane-credit-note" data-role="credit-note"></p>'
      + '</div>'
      + '</section>'
      // Four tabs, each one question: what is happening, how much and how
      // fast, what it consumed, and what policy did about it. Every panel is
      // rendered once and stays in the DOM; switching only toggles `hidden`,
      // so live updates keep landing in panels the visitor is not looking at.
      + tabsHtml()
      + panelHtml('overview', overviewHtml(), false)
      + panelHtml('traffic', trafficHtml(), true)
      + panelHtml('usage', usageHtml(), true)
      + panelHtml('governance', governanceHtml(), true)
      // Hidden until the pulse arrives. A visitor's own numbers prove we record
      // them; this proves the machinery is not a diorama built for one person.
      + '<section class="pane-section pane-section--pulse" data-role="pulse" hidden>'
      + '<h3 class="pane-h3">Across the platform '
      + '<span class="pane-h3-sub" data-role="pulse-window"></span></h3>'
      + '<dl class="pane-stats pane-stats--pulse" data-role="pulse-stats"></dl>'
      + '<p class="pane-pulse-note" data-role="pulse-models"></p>'
      + '<p class="pane-pulse-note" data-role="pulse-all-time"></p>'
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
    this._feedPreview = this.querySelector('[data-role="feed-preview"]');
    this._credit = this.querySelector('[data-role="credit"]');
    this._stages = this.querySelector('[data-role="stages"]');
    this._stageSub = this.querySelector('[data-role="stage-sub"]');
    this._stageMini = this.querySelector('[data-role="stage-mini"]');
    this._govChip = this.querySelector('[data-role="gov-chip"]');
    this._pulse = this.querySelector('[data-role="pulse"]');
    this._wireTabs();
    const viewGov = this.querySelector('[data-role="view-gov"]');
    if (viewGov) viewGov.addEventListener('click', () => this._selectTab('governance'));
    this._syncFeedPreview();
    // Render the four stages at zero straight away. Waiting for the first poll
    // would mean the pipeline appears to come into existence once something
    // trips it, which is the opposite of the claim.
    this._applyStages(IDLE_STAGES);
    this.querySelector('[data-role="signout"]').addEventListener('click', () => this._signOut());

    // The admin block is rebuilt from scratch on each render, so drop the stale
    // reference rather than pointing at a node that is no longer in the tree.
    this._pulseAdmin = null;
    if (this._conversation) this._startPolling();
    this._startPulsePolling();
  }

  /**
   * A key can appear in more than one panel — the Overview repeats the
   * headline numbers other tabs own — so every instance is updated, not just
   * the first the selector happens to find.
   */
  _stat(key, value) {
    this.querySelectorAll('[data-stat="' + key + '"]').forEach((el) => {
      if (el.textContent !== value) {
        el.textContent = value;
        // Re-triggering the animation needs the class to actually leave the
        // element first; a same-frame remove/add is coalesced away.
        el.classList.remove('is-changed');
        void el.offsetWidth;
        el.classList.add('is-changed');
      }
      // A block that actually happened is the number this pane exists to show.
      // The terminal header's meter already goes red on the same fact; the two
      // halves should not disagree about it.
      if (key === 'blocked') {
        el.parentElement.dataset.hot = value && value !== '0' ? '1' : '0';
      }
    });
  }

  // ── tabs ──────────────────────────────────────────────────────────────────

  _wireTabs() {
    this._tabs = Array.from(this.querySelectorAll('.pane-tabs--stats .pane-tab'));
    this._panels = Array.from(this.querySelectorAll('.pane-panel'));
    if (!this._tabs.length) return;
    this._tabs.forEach((tab) => {
      tab.addEventListener('click', () => this._selectTab(tab.dataset.tab));
    });
    this.querySelector('.pane-tabs--stats').addEventListener('keydown', (e) => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
      e.preventDefault();
      const current = this._tabs.findIndex((t) => t.dataset.tab === this._activeTab);
      let next = current;
      if (e.key === 'ArrowLeft') next = (current - 1 + this._tabs.length) % this._tabs.length;
      else if (e.key === 'ArrowRight') next = (current + 1) % this._tabs.length;
      else if (e.key === 'Home') next = 0;
      else next = this._tabs.length - 1;
      this._selectTab(this._tabs[next].dataset.tab);
      this._tabs[next].focus();
    });
    // Survives a re-render (sign-out/in, re-auth): the last chosen tab is
    // restored rather than snapping back to Overview.
    this._selectTab(this._activeTab || 'overview');
  }

  _selectTab(id) {
    this._activeTab = id;
    this._tabs.forEach((tab) => {
      const active = tab.dataset.tab === id;
      tab.classList.toggle('is-active', active);
      tab.setAttribute('aria-selected', active ? 'true' : 'false');
      tab.tabIndex = active ? 0 : -1;
    });
    this._panels.forEach((p) => { p.hidden = p.id !== 'ap-panel-' + id; });
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
    // Clears only the session timer. Calling the full `_stopPolling` here would
    // take the pulse down with it and never bring it back — the pulse is
    // started once per render, not once per conversation.
    this._stopSessionPolling();
    this._poll();
    this._pollTimer = setInterval(() => this._poll(), POLL_MS);
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
    this._pollPulse();
    this._pulseTimer = setInterval(() => this._pollPulse(), PULSE_POLL_MS);
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

  /**
   * The token is sent when there is one and omitted when there is not.
   *
   * An anonymous visitor has no embed token — `/embed-token` reads the session
   * cookie — and the endpoint treats its absence as the anonymous tier rather
   * than as a failure. So this runs on the sign-in view too, where it fills in
   * the lifetime totals under the form.
   */
  async _pollPulse() {
    if (!this._pulse) return;
    const url = this._token
      ? '/api/public/pi/pulse?token=' + encodeURIComponent(this._token)
      : '/api/public/pi/pulse';
    try {
      const res = await fetch(url, { credentials: 'same-origin', redirect: 'manual' });
      if (!res.ok) return;
      this._applyPulse(await res.json());
    } catch (_) {
      // Context, not the visitor's own numbers — a miss costs nothing and the
      // section simply stays as it was.
    }
  }

  /**
   * Show the deployment counted across everyone, at whatever depth the server
   * decided this caller is owed.
   *
   * There is no tier check here, and deliberately no way to ask for a richer
   * one: the shape of the payload *is* the tier. A window arrives only if the
   * caller is signed in and the window holds enough people to aggregate without
   * identifying them, and `detail` arrives only for an operator. Suppression
   * used to be decided in this file, which meant the sparse numbers were sent
   * and then hidden — a privacy control enforced by the party it protects
   * against. Now they never leave the server.
   *
   * Counts arrive pre-formatted as strings because the member tier rounds them
   * ("1.2k") and the admin tier does not ("1,247"). One render path, two
   * vocabularies, chosen server-side.
   */
  _applyPulse(p) {
    if (!this._pulse || !p) return;
    const w = p.window;
    const all = p.all_time || {};

    const stats = this.querySelector('[data-role="pulse-stats"]');
    const models = this.querySelector('[data-role="pulse-models"]');
    const heading = this.querySelector('[data-role="pulse-window"]');
    if (heading) heading.textContent = w ? 'last ' + (p.window_hours || 24) + 'h' : 'all time';

    if (w && stats) {
      stats.hidden = false;
      stats.innerHTML = ''
        + statHtml('pPeople', 'People', w.people)
        + statHtml('pSessions', 'Sessions', w.sessions)
        + statHtml('pRequests', 'Requests', w.requests)
        + statHtml('pTools', 'Tool calls', w.tool_calls)
        + statHtml('pAllow', 'Allowed', w.allow_rate_percent === null
          || w.allow_rate_percent === undefined ? '—' : pct(w.allow_rate_percent))
        + statHtml('pLatency', 'Latency p50', ms(w.latency_p50_ms));

      const mix = (w.model_mix || []).slice(0, 3)
        .map((m) => m.model + ' ' + pct(m.percent)).join(' · ');
      const blocked = (w.blocked_tools || [])[0];
      const worst = blocked
        ? ' Most refused: ' + blocked.tool_name + ' ×' + blocked.denials + '.'
        : '';
      models.textContent = (mix ? 'Models: ' + mix + '.' : '') + worst;
      models.hidden = !mix && !worst;
    } else if (stats) {
      stats.hidden = true;
      if (models) models.hidden = true;
    }

    this.querySelector('[data-role="pulse-all-time"]').textContent = 'All time — '
      + all.sessions + ' sessions, ' + all.requests + ' requests, '
      + all.tool_calls + ' governed tool calls, '
      + all.secrets_caught + ' secrets caught.';

    this._applyAdminPulse(p.detail);
    this._pulse.hidden = false;
  }

  /**
   * The operator block, when the server sent one.
   *
   * Created on first use rather than rendered empty and filled: for two of the
   * three tiers it never arrives, and an empty container that only ever gets
   * hidden is markup every visitor pays for so one does not have to.
   */
  _applyAdminPulse(detail) {
    if (!detail) {
      if (this._pulseAdmin) this._pulseAdmin.hidden = true;
      return;
    }
    if (!this._pulseAdmin) {
      this._pulseAdmin = document.createElement('div');
      this._pulseAdmin.className = 'pulse-admin';
      this._pulse.append(this._pulseAdmin);
    }
    this._pulseAdmin.hidden = false;
    renderAdminPulse(this._pulseAdmin, detail);
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

  /**
   * Every field is guarded. The pane is served from `web/dist` and the API
   * from the binary, so a deploy can land one without the other; an older
   * server simply omits the new keys, and every new tile falls back to a dash
   * rather than printing `undefined` next to real numbers.
   */
  _applyStats(s) {
    this._stat('model', s.model || '—');
    // The server sends this only when a route actually rewrote the model, so
    // the usual reading is the reassuring one: you got what you asked for.
    this._stat('requested', s.requested_model || 'as served');
    this._stat('provider', s.provider || '—');
    this._stat('route', s.route_match || 'default');
    this._stat('cache', pct(s.cache_hit_percent));

    this._stat('requests', String(s.requests || 0));
    this._stat('tools', String(Math.max(s.tool_calls || 0, this._live.tools)));
    const denials = Math.max(s.denied || 0, this._live.blocked);
    this._stat('blocked', String(denials));
    this._stat('errors', String(s.errors || 0));
    const reqs = s.requests || 0;
    this._stat('successRate', reqs
      ? Math.round(((reqs - (s.errors || 0)) / reqs) * 100) + '%'
      : '—');

    this._stat('latency', ms(s.latency_p50_ms));
    this._stat('latency95', ms(s.latency_p95_ms));
    this._stat('latencyLast', ms(s.latency_last_ms));
    this._applyLatencyBars(s);

    this._stat('tokensIn', compact(s.input_tokens));
    this._stat('tokensOut', compact(s.output_tokens));
    this._stat('cacheRead', compact(s.cache_read_tokens));
    this._stat('cacheWrite', compact(s.cache_creation_tokens));

    this._stat('cost', s.cost_display || '$0');
    this._stat('costPer', s.cost_per_request_display || '$0');

    if (s.policy_stages && s.policy_stages.length) this._applyStages(s.policy_stages);
    this._applyCredit(s.credit);
    this._applyModelMix(s.model_mix);
    this._applyGovChip((s.events || []).length, denials);
    this._renderFeed(s.events || []);
  }

  /**
   * The three latencies as one comparative picture: each bar is scaled against
   * the slowest of them, so the spread between p50 and p95 is visible rather
   * than three equally sized tiles saying numbers.
   */
  _applyLatencyBars(s) {
    const vals = {
      latency: s.latency_p50_ms,
      latency95: s.latency_p95_ms,
      latencyLast: s.latency_last_ms,
    };
    const max = Math.max(...Object.values(vals).map((v) => Number(v) || 0), 1);
    Object.entries(vals).forEach(([k, v]) => {
      const el = this.querySelector('[data-bar="' + k + '"]');
      if (el) el.style.width = Math.round(((Number(v) || 0) / max) * 100) + '%';
    });
  }

  /** Which models actually served this conversation, as labelled share bars. */
  _applyModelMix(mix) {
    const section = this.querySelector('[data-role="mix-section"]');
    const list = this.querySelector('[data-role="mix"]');
    if (!section || !list) return;
    if (!mix || !mix.length) {
      section.hidden = true;
      return;
    }
    section.hidden = false;
    list.innerHTML = '';
    mix.forEach((m) => {
      const row = document.createElement('div');
      row.className = 'pane-mix-row';
      const label = document.createElement('span');
      label.className = 'pane-mix-label';
      label.textContent = m.model;
      const bar = document.createElement('div');
      bar.className = 'pane-bar';
      bar.setAttribute('role', 'img');
      bar.setAttribute('aria-label', m.model + ' ' + pct(m.percent) + ' of requests');
      const fill = document.createElement('span');
      fill.style.width = Math.max(0, Math.min(100, Number(m.percent) || 0)) + '%';
      bar.append(fill);
      const share = document.createElement('span');
      share.className = 'pane-mix-pct';
      share.textContent = pct(m.percent);
      row.append(label, bar, share);
      list.append(row);
    });
  }

  /**
   * The Governance tab's count chip lives in the always-visible tablist, so a
   * denial registers even while that panel is hidden.
   */
  _applyGovChip(count, denials) {
    if (!this._govChip) return;
    this._govChip.hidden = !count;
    this._govChip.textContent = String(count);
    this._govChip.dataset.alert = denials > 0 ? '1' : '0';
    const tab = this.querySelector('#ap-tab-governance');
    if (tab) {
      tab.setAttribute('aria-label', 'Governance, ' + count + ' events'
        + (denials ? ', ' + denials + ' blocked' : ''));
    }
  }

  /**
   * The four checks, in the order they run.
   *
   * A stage that has never evaluated anything is dimmed rather than hidden:
   * "four checks run on every call, none has tripped" is the claim, and a list
   * that grows from one row to four as things happen argues the opposite.
   */
  _applyStages(stages) {
    if (!this._stages) return;
    const blocked = stages.reduce((n, st) => n + (st.failed || 0), 0);
    this._stageSub.textContent = blocked
      ? blocked + (blocked === 1 ? ' block' : ' blocks')
      : stages.length + ' checks per call';

    this._stages.innerHTML = '';
    stages.forEach((st) => {
      const li = document.createElement('li');
      li.className = 'pane-stage';
      li.dataset.hot = st.failed > 0 ? '1' : '0';
      li.dataset.active = st.active ? '1' : '0';

      const name = document.createElement('span');
      name.className = 'pane-stage-name';
      name.textContent = st.label || st.id;

      const tally = document.createElement('span');
      tally.className = 'pane-stage-tally';
      tally.textContent = st.failed > 0
        ? st.passed + ' passed · ' + st.failed + ' blocked'
        : (st.active ? st.passed + ' passed' : 'idle');

      li.append(name, tally);
      this._stages.append(li);
    });
    this._applyStageMini(stages);
  }

  /**
   * The Overview's one-line echo of the pipeline: four named pips, red where
   * a stage has blocked something. The full tallies live on the Governance
   * tab; this exists so the pipeline is present on the default view at all.
   */
  _applyStageMini(stages) {
    if (!this._stageMini) return;
    this._stageMini.innerHTML = '';
    stages.forEach((st) => {
      const pip = document.createElement('span');
      pip.className = 'pane-stage-pip';
      pip.dataset.hot = st.failed > 0 ? '1' : '0';
      pip.dataset.active = st.active ? '1' : '0';
      pip.textContent = st.label || st.id;
      if (st.failed > 0) pip.title = st.failed + ' blocked';
      this._stageMini.append(pip);
    });
  }

  /**
   * Show what is left of the grant.
   *
   * Stays hidden when nothing has been granted rather than rendering "$0 of
   * $0": an account still awaiting approval has no grant yet, and an empty
   * meter would read as "you have spent it all" — the opposite of the truth,
   * and the discouraging half of the two possible misreadings.
   */
  _applyCredit(credit) {
    if (!this._credit) return;
    if (!credit || !credit.granted_microdollars) {
      this._credit.hidden = true;
      return;
    }
    this._credit.hidden = false;
    this.querySelector('[data-role="credit-left"]').textContent = credit.remaining_display;
    this.querySelector('[data-role="credit-of"]').textContent = 'of ' + credit.granted_display;

    const pct = Math.max(0, Math.min(100, credit.remaining_percent));
    const bar = this.querySelector('[data-role="credit-bar"]');
    this.querySelector('[data-role="credit-fill"]').style.width = pct + '%';
    bar.setAttribute('aria-label', credit.remaining_display + ' of '
      + credit.granted_display + ' remaining');
    bar.dataset.state = credit.exhausted ? 'empty' : (pct <= 15 ? 'low' : 'ok');

    const note = this.querySelector('[data-role="credit-note"]');
    if (credit.exhausted) {
      // The terminal is about to start refusing turns. Saying so here, next to
      // the number that explains it, beats letting the agent go quiet first.
      note.textContent = 'Your credit is spent — the gateway will refuse the next request.';
    } else {
      note.textContent = 'Spent ' + credit.spent_display + ' so far. Every request is '
        + 'metered against this balance before it reaches a provider.';
    }
  }

  _renderFeed(events) {
    this._feedCount.textContent = events.length ? events.length + ' recorded' : '';
    if (!events.length) return;
    this._feed.innerHTML = '';
    // Newest first: the pane is short, and the thing that just happened is the
    // thing being watched for.
    events.slice(-40).reverse().forEach((e) => this._feed.append(feedItem(e)));
    this._syncFeedPreview();
  }

  _pushFeed(e) {
    const empty = this._feed.querySelector('.pane-feed-empty');
    if (empty) empty.remove();
    this._feed.prepend(feedItem(e));
    this._syncFeedPreview();
  }

  /** The Overview shows the three newest decisions; the full list is a tab away. */
  _syncFeedPreview() {
    if (!this._feedPreview || !this._feed) return;
    this._feedPreview.innerHTML = '';
    const items = Array.from(this._feed.children)
      .filter((li) => !li.classList.contains('pane-feed-empty'))
      .slice(0, 3);
    if (!items.length) {
      const li = document.createElement('li');
      li.className = 'pane-feed-empty';
      li.textContent = 'Ask the agent to read a file — every decision lands here live.';
      this._feedPreview.append(li);
      return;
    }
    items.forEach((li) => this._feedPreview.append(li.cloneNode(true)));
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

// ── tabbed telemetry layout ─────────────────────────────────────────────────

const TABS = [
  { id: 'overview', label: 'Overview' },
  { id: 'traffic', label: 'Traffic' },
  { id: 'usage', label: 'Usage' },
  { id: 'governance', label: 'Governance' },
];

function tabsHtml() {
  return '<div class="pane-tabs pane-tabs--stats" role="tablist" aria-label="Session telemetry">'
    + TABS.map((t, i) => {
      const active = i === 0;
      return '<button type="button" class="pane-tab' + (active ? ' is-active' : '')
        + '" role="tab" id="ap-tab-' + t.id + '" aria-controls="ap-panel-' + t.id
        + '" aria-selected="' + active + '" tabindex="' + (active ? '0' : '-1')
        + '" data-tab="' + t.id + '">' + t.label
        + (t.id === 'governance'
          ? '<span class="pane-tab-chip" data-role="gov-chip" hidden></span>'
          : '')
        + '</button>';
    }).join('')
    + '</div>';
}

function panelHtml(id, inner, hidden) {
  return '<section class="pane-panel" role="tabpanel" id="ap-panel-' + id
    + '" aria-labelledby="ap-tab-' + id + '" tabindex="0"' + (hidden ? ' hidden' : '') + '>'
    + inner + '</section>';
}

/**
 * The default view answers "what is going on" in one screen: the headline
 * numbers large, the model in one line, the pipeline as pips, and the last
 * few decisions. Everything on it is repeated in full on another tab.
 */
function overviewHtml() {
  return '<dl class="pane-stats pane-stats--hero">'
    + statHtml('requests', 'Requests', '0')
    + statHtml('tools', 'Tool calls', '0')
    + statHtml('blocked', 'Blocked', '0')
    + statHtml('cost', 'Session cost', '$0')
    + '</dl>'
    + '<p class="pane-model-line"><span data-stat="model">—</span>'
    + '<span class="pane-model-sep">·</span><span data-stat="provider">—</span>'
    + '<span class="pane-model-sep">·</span><span data-stat="route">—</span></p>'
    + '<div class="pane-stage-mini" data-role="stage-mini"'
    + ' aria-label="policy pipeline"></div>'
    + '<section class="pane-section pane-section--feed">'
    + '<h3 class="pane-h3">Latest decisions '
    + '<button type="button" class="pane-link pane-link--sm" data-role="view-gov">'
    + 'view all</button></h3>'
    + '<ol class="pane-feed pane-feed--preview" data-role="feed-preview"></ol>'
    + '</section>';
}

function trafficHtml() {
  return sectionHtml('Traffic', 'traffic', [
    statHtml('requests', 'Requests', '0'),
    statHtml('tools', 'Tool calls', '0'),
    statHtml('blocked', 'Blocked', '0'),
    statHtml('errors', 'Errors', '0'),
    statHtml('successRate', 'Success', '—'),
  ])
  + '<section class="pane-section">'
  + '<h3 class="pane-h3">Latency</h3>'
  + '<div class="pane-lat">'
  + latencyRowHtml('latency', 'p50')
  + latencyRowHtml('latency95', 'p95')
  + latencyRowHtml('latencyLast', 'Last turn')
  + '</div>'
  + '</section>';
}

function latencyRowHtml(key, label) {
  return '<div class="pane-lat-row"><span class="pane-lat-label">' + label + '</span>'
    + '<div class="pane-bar"><span data-bar="' + key + '"></span></div>'
    + '<span class="pane-lat-val" data-stat="' + key + '">—</span></div>';
}

function usageHtml() {
  return sectionHtml('Tokens', 'tokens', [
    statHtml('tokensIn', 'Input', '0'),
    statHtml('tokensOut', 'Output', '0'),
    statHtml('cacheRead', 'Cache read', '0'),
    statHtml('cacheWrite', 'Cache written', '0'),
  ])
  + sectionHtml('Cost', 'cost-section', [
    statHtml('cost', 'This session', '$0'),
    statHtml('costPer', 'Per request', '$0'),
  ])
  + sectionHtml('Model &amp; route', 'route', [
    statHtml('model', 'Model', '—'),
    statHtml('requested', 'Requested', 'as served'),
    statHtml('provider', 'Provider', '—'),
    statHtml('route', 'Gateway route', '—'),
    statHtml('cache', 'Cache hits', '0%'),
  ])
  + '<section class="pane-section" data-role="mix-section" hidden>'
  + '<h3 class="pane-h3">Model mix</h3>'
  + '<div class="pane-mix" data-role="mix"></div>'
  + '</section>';
}

function governanceHtml() {
  // The stage list, not a tile grid: these are four sequential checks and
  // the order they run in is part of what is being shown.
  return '<section class="pane-section">'
    + '<h3 class="pane-h3">Policy pipeline '
    + '<span class="pane-h3-sub" data-role="stage-sub"></span></h3>'
    + '<ol class="pane-stages" data-role="stages"></ol>'
    + '</section>'
    + '<section class="pane-section pane-section--feed">'
    + '<h3 class="pane-h3">Governance <span class="pane-h3-sub" data-role="feed-count"></span></h3>'
    + '<ol class="pane-feed" data-role="feed">'
    + '<li class="pane-feed-empty">Ask the agent to read a file. Every decision it '
    + 'triggers is recorded here.</li>'
    + '</ol>'
    + '</section>';
}

/** One labelled group of tiles. */
function sectionHtml(title, role, tiles) {
  return '<section class="pane-section">'
    + '<h3 class="pane-h3">' + title + '</h3>'
    + '<dl class="pane-stats" data-role="' + role + '">' + tiles.join('') + '</dl>'
    + '</section>';
}

/** Latencies are absent until a turn completes; say so rather than showing 0. */
function ms(v) {
  return (v === null || v === undefined) ? '—' : v + 'ms';
}

function pct(v) {
  return String(Number(v) || 0) + '%';
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

customElements.define('sp-auth-pane', SpAuthPane);
