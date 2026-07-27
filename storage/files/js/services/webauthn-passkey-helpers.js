'use strict';

import { generateRandomString, generateCodeChallenge } from '/js/services/webauthn-utils.js';

const CLIENT_ID = 'marketplace-admin';
const WEBAUTHN_BASE = '/api/v1/core/oauth/webauthn';
const LOGIN_PATH = '/admin/login';
const DEFAULT_REDIRECT = '/admin/access/users';

export { WEBAUTHN_BASE };

export const buildAuthCredentialPayload = (credential) => ({
  id: credential.id,
  rawId: Array.from(new Uint8Array(credential.rawId)),
  response: {
    authenticatorData: Array.from(new Uint8Array(credential.response.authenticatorData)),
    clientDataJSON: Array.from(new Uint8Array(credential.response.clientDataJSON)),
    signature: Array.from(new Uint8Array(credential.response.signature)),
    userHandle: credential.response.userHandle
      ? Array.from(new Uint8Array(credential.response.userHandle))
      : null,
  },
  type: credential.type,
});

export const buildCreationCredentialPayload = (credential) => ({
  id: credential.id,
  rawId: Array.from(new Uint8Array(credential.rawId)),
  response: {
    attestationObject: Array.from(new Uint8Array(credential.response.attestationObject)),
    clientDataJSON: Array.from(new Uint8Array(credential.response.clientDataJSON)),
  },
  type: credential.type,
});

/**
 * Finish the OAuth leg without navigating, so the ceremony can run inside a
 * pane on a page that stays put.
 *
 * `/complete` answers with JSON rather than a 302 when the request does not
 * ask for HTML, which is what makes this possible at all — the redirect is
 * the only reason the flow ever left the page. `redirect_uri` still has to be
 * sent, and has to be byte-identical in both calls, because `/token` compares
 * it against the value stored with the code. It is never navigated to, so
 * LOGIN_PATH stays correct even from `/`.
 */
export async function establishSessionInline(userId, authToken, onProgress) {
  if (!userId || typeof authToken !== 'string' || authToken.length === 0) {
    throw new Error('Login session invalid — please reload this page and try again.');
  }
  const progress = onProgress || (() => {});
  const codeVerifier = generateRandomString(64);
  const codeChallenge = await generateCodeChallenge(codeVerifier);
  const redirectUri = window.location.origin + LOGIN_PATH;

  progress('Authorising...');
  const completeUrl = WEBAUTHN_BASE + '/complete?' + new URLSearchParams({
    user_id: userId, auth_token: authToken,
    response_type: 'code', client_id: CLIENT_ID,
    redirect_uri: redirectUri, scope: 'user', state: generateRandomString(32),
    code_challenge: codeChallenge, code_challenge_method: 'S256',
  }).toString();
  const completeRes = await fetch(completeUrl, {
    headers: { Accept: 'application/json' },
    credentials: 'same-origin',
    redirect: 'manual',
  });
  const completeData = await completeRes.json().catch(() => ({}));
  const code = completeData.authorization_code || completeData.code;
  if (!completeRes.ok || !code) {
    throw new Error(completeData.error_description || completeData.error || 'Authorisation failed.');
  }

  progress('Exchanging token...');
  const tokenRes = await fetch('/api/v1/core/oauth/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    credentials: 'same-origin',
    body: new URLSearchParams({
      grant_type: 'authorization_code', code,
      redirect_uri: redirectUri, code_verifier: codeVerifier, client_id: CLIENT_ID,
    }),
  });
  const tokenData = await tokenRes.json().catch(() => ({}));
  if (!tokenRes.ok || !tokenData.access_token) {
    throw new Error(tokenData.error_description || tokenData.error || 'Token exchange failed.');
  }

  progress('Starting your session...');
  const sessionRes = await fetch('/api/public/auth/session', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify({
      access_token: tokenData.access_token,
      expires_in: tokenData.expires_in || 3600,
      refresh_token: tokenData.refresh_token,
    }),
  });
  if (!sessionRes.ok) {
    const data = await sessionRes.json().catch(() => ({}));
    throw new Error(data.message || data.error || 'Could not start your session.');
  }
}

export async function initPkceAndRedirect(userId, authToken, showLoading, redirect) {
  if (!userId || typeof authToken !== 'string' || authToken.length === 0) {
    throw new Error('Login session invalid — please reload this page and try again.');
  }
  const codeVerifier = generateRandomString(64);
  const codeChallenge = await generateCodeChallenge(codeVerifier);
  const csrfState = generateRandomString(32);
  localStorage.setItem('pkce_code_verifier', codeVerifier);
  localStorage.setItem('pkce_csrf_state', csrfState);
  localStorage.setItem('login_redirect', redirect || DEFAULT_REDIRECT);
  showLoading('Redirecting...');
  window.location.href = WEBAUTHN_BASE + '/complete?' + new URLSearchParams({
    user_id: userId, auth_token: authToken,
    response_type: 'code', client_id: CLIENT_ID,
    redirect_uri: window.location.origin + LOGIN_PATH, scope: 'user', state: csrfState,
    code_challenge: codeChallenge, code_challenge_method: 'S256',
  }).toString();
}
