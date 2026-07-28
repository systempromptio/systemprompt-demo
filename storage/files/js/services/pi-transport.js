/**
 * Network access for the pi components: every fetch the terminal and the
 * conversation list make goes through here, with one policy for credentials
 * (same-origin cookies) and one policy for failure (null / empty, never a
 * throw — the transcript, not an exception, tells the viewer what happened).
 */

/**
 * Ask the server to mint a token for whoever owns the session cookie.
 * Only called for a signed-in visitor; anonymous visitors never POST here.
 */
export async function mintToken(endpoint) {
  try {
    const res = await fetch(endpoint + '/embed-token', {
      method: 'POST',
      credentials: 'same-origin',
      // The route 404s entirely when the terminal is unconfigured, and an
      // /admin redirect would arrive as HTML. Either way: no token.
      redirect: 'manual',
    });
    if (!res.ok) return null;
    const body = await res.json();
    return body.token || null;
  } catch (_) {
    return null;
  }
}

/**
 * Who the session cookie belongs to, or null when anonymous.
 *
 * `/admin/auth/me` sits behind a middleware that 307s to the login page
 * rather than answering 401, so an anonymous visitor would otherwise get
 * 200 OK carrying HTML. `redirect: 'manual'` turns that into an opaque
 * response we can reject instead of trying to parse.
 */
export async function whoami() {
  try {
    const res = await fetch('/admin/auth/me', {
      credentials: 'same-origin',
      redirect: 'manual',
    });
    if (!res.ok) return null;
    const type = res.headers.get('content-type') || '';
    if (type.indexOf('application/json') === -1) return null;
    return await res.json();
  } catch (_) {
    return null;
  }
}

/** GET a JSON resource with the session cookie; null on any failure. */
export async function getJson(url) {
  try {
    const res = await fetch(url, { credentials: 'same-origin' });
    if (!res.ok) return null;
    return await res.json();
  } catch (_) {
    return null;
  }
}

/** Send a JSON payload with the session cookie. Returns the raw Response. */
export function sendJson(method, url, payload) {
  return fetch(url, {
    method,
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(payload),
  });
}

export function postJson(url, payload) {
  return sendJson('POST', url, payload);
}

/** This viewer's conversations, newest first. Never throws. */
export async function conversations(endpoint, token) {
  if (!token) return [];
  const body = await getJson(endpoint + '/conversations?token=' + encodeURIComponent(token));
  return Array.isArray(body) ? body : [];
}
