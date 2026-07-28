/**
 * The anonymous half of <sp-auth-pane>: the form's wiring, and the two passkey
 * ceremonies behind it. Every function takes the pane element as its first
 * argument — these were methods, and they still read as if they were.
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
import { authHtml } from './sp-auth-pane-forms.js';
import { passkeyMessage } from './sp-auth-pane-helpers.js';

export function renderAuth(pane) {
  pane.innerHTML = authHtml();

  pane._alert = pane.querySelector('[data-role="alert"]');
  pane._busy = pane.querySelector('[data-role="busy"]');
  pane._busyText = pane.querySelector('[data-role="busy-text"]');
  pane._signin = pane.querySelector('[data-role="signin"]');
  pane._register = pane.querySelector('[data-role="register"]');

  const tabSignin = pane.querySelector('[data-role="tab-signin"]');
  const tabRegister = pane.querySelector('[data-role="tab-register"]');
  const show = (which) => {
    const registering = which === 'register';
    pane._signin.hidden = registering;
    pane._register.hidden = !registering;
    tabSignin.classList.toggle('is-active', !registering);
    tabRegister.classList.toggle('is-active', registering);
    pane._clearAlert();
  };
  pane._showTab = show;
  tabSignin.addEventListener('click', () => show('signin'));
  tabRegister.addEventListener('click', () => show('register'));

  pane._signin.addEventListener('submit', (e) => {
    e.preventDefault();
    doSignIn(pane);
  });
  pane._register.addEventListener('submit', (e) => {
    e.preventDefault();
    doRegister(pane);
  });
  const step1 = pane.querySelector('[data-role="step-1"]');
  const step2 = pane.querySelector('[data-role="step-2"]');
  pane.querySelector('[data-role="next"]').addEventListener('click', () => {
    // `required` only fires on submit, and step one has no submit button, so
    // the check has to be explicit or step two accepts a blank email.
    const email = pane.querySelector('#ap-reg-email');
    const name = pane.querySelector('#ap-reg-name');
    if (!email.checkValidity() || !name.value.trim()) {
      pane._showAlert('Enter your work email and name to continue.', 'error');
      return;
    }
    pane._clearAlert();
    step1.hidden = true;
    step2.hidden = false;
  });
  pane.querySelector('[data-role="back"]').addEventListener('click', () => {
    pane._clearAlert();
    step2.hidden = true;
    step1.hidden = false;
  });

  // No magic-link fallback offered here: `request_magic_link` mints a token
  // and logs it, but this deployment wires no email sender, so the button
  // would be a promise nothing keeps. `/admin/add-passkey` still honours a
  // `return` param for when one is wired.

  if (!window.PublicKeyCredential) {
    pane._showAlert('This browser does not support passkeys. Use a recent Chrome, '
      + 'Firefox, Safari, or Edge.', 'error');
    pane.querySelectorAll('button[type="submit"]').forEach((b) => { b.disabled = true; });
  }

  pane._pulse = pane.querySelector('[data-role="pulse"]');
  pane._pulseAdmin = null;
  pane._startPulsePolling();
}

export async function doSignIn(pane) {
  const email = pane.querySelector('#ap-signin-email').value.trim();
  if (!email) return;
  pane._clearAlert();
  try {
    pane._setBusy('Waiting for your passkey…');
    const start = await makeRequest(
      WEBAUTHN_BASE + '/auth/start?email=' + encodeURIComponent(email), 'POST',
    );
    const options = preparePublicKeyCredentialRequestOptions(start.data.publicKey);
    const credential = await navigator.credentials.get({ publicKey: options });
    if (!credential) throw new Error('Sign-in was cancelled.');
    pane._setBusy('Verifying…');
    const finish = await makeRequest(WEBAUTHN_BASE + '/auth/finish', 'POST', {
      challenge_id: start.data.challenge_id,
      credential: buildAuthCredentialPayload(credential),
    });
    await establishSessionInline(
      finish.data.user_id, finish.data.auth_token, (m) => pane._setBusy(m),
    );
    await pane._onAuthenticated();
  } catch (err) {
    pane._setBusy(null);
    pane._showAlert(passkeyMessage(err, 'Sign-in failed. Please try again.'), 'error');
  }
}

export async function doRegister(pane) {
  const value = (id) => (pane.querySelector(id).value || '').trim();
  const payload = {
    name: value('#ap-reg-name'),
    email: value('#ap-reg-email'),
    company: value('#ap-reg-company'),
    role: value('#ap-reg-role'),
    team_size: value('#ap-reg-team'),
    why_assessing: value('#ap-reg-why'),
  };
  if (Object.values(payload).some((v) => !v)) {
    pane._showAlert('Please complete every field.', 'error');
    return;
  }
  pane._clearAlert();
  try {
    pane._setBusy('Submitting your request…');
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
      pane._setBusy(null);
      pane._showTab('signin');
      pane.querySelector('#ap-signin-email').value = payload.email;
      pane._showAlert('That email already has an account — sign in with your passkey.', 'info');
      return;
    }

    pane._setBusy('Creating your passkey…');
    const start = await makeRequest(
      WEBAUTHN_BASE + '/link/start?token=' + encodeURIComponent(data.token), 'GET',
    );
    const options = preparePublicKeyCredentialCreationOptions(
      start.data.challenge ? start.data.challenge.publicKey : start.data.publicKey,
    );
    const credential = await navigator.credentials.create({ publicKey: options });
    if (!credential) throw new Error('Passkey creation was cancelled.');
    pane._setBusy('Finishing registration…');
    await makeRequest(WEBAUTHN_BASE + '/link/finish', 'POST', {
      challenge_id: start.data.challenge_id || start.challengeId,
      token: data.token,
      credential: buildCreationCredentialPayload(credential),
    });

    // Straight into a session: the passkey that was just created is the one
    // the sign-in ceremony is about to ask for.
    pane._setBusy('Signing you in…');
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
      finish.data.user_id, finish.data.auth_token, (m) => pane._setBusy(m),
    );
    await pane._onAuthenticated();
  } catch (err) {
    pane._setBusy(null);
    pane._showAlert(passkeyMessage(err, 'Registration failed. Please try again.'), 'error');
  }
}
