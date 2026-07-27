'use strict';

/**
 * <sp-pi-terminal endpoint="/api/public/pi" [token="..."]>
 *
 * A live view onto a server-side pi agent whose every tool call is gated in
 * Rust and then by a human. Light DOM on purpose, so the global --sp-* tokens
 * and [data-theme] apply the same way they do to the canned terminal this
 * widget is styled to match.
 *
 * Without a usable credential it renders a scripted replay instead, so a public
 * page can embed it unconditionally.
 */

const DEFAULT_ENDPOINT = '/api/public/pi';

/** Frames the widget renders as a tool row, keyed by tool_use_id. */
const TOOL_ICON = { pending: '▸', ok: '●', blocked: '✗' };

/** Backoff for manual stream reconnects. EventSource's own retry gives up. */
const RECONNECT_MIN_MS = 1000;
const RECONNECT_MAX_MS = 30000;

/** What an anonymous visitor sees. Mirrors the shape of a real exchange. */
const CANNED = [
  { cls: 'prompt', text: '>', tail: 'read src/auth.rs and tell me how sessions are minted' },
  { cls: 'output-dim', text: 'pi is reading the file…' },
  { cls: 'tool', name: 'read', arg: 'src/auth.rs', state: 'pending' },
  { cls: 'approval' },
  { cls: 'tool', name: 'read', arg: 'src/auth.rs', state: 'ok' },
  { cls: 'output', text: 'Sessions are minted by SessionCreationService and attested with an id the server issues, so spend and governance rows join on it.' },
];

class SpPiTerminal extends HTMLElement {
  constructor() {
    super();
    this._conversationId = null;
    this._source = null;
    this._lastSeq = 0;
    this._toolRows = new Map();
    this._approvals = new Map();
    this._reconnectMs = RECONNECT_MIN_MS;
    this._reconnectTimer = null;
    this._textLine = null;
    this._turnLive = false;
    this._closed = false;
  }

  connectedCallback() {
    if (this._built) return;
    this._built = true;
    this._endpoint = (this.getAttribute('endpoint') || DEFAULT_ENDPOINT).replace(/\/$/, '');
    this._build();
    this._start();
  }

  disconnectedCallback() {
    this._teardownStream();
    if (this._reconnectTimer) clearTimeout(this._reconnectTimer);
    this._approvals.forEach((a) => clearInterval(a.timer));
    this._approvals.clear();
  }

  // ── chrome ────────────────────────────────────────────────────────────────

  _build() {
    this.classList.add('pi-terminal');
    this.innerHTML = ''
      + '<div class="terminal active">'
      + '<div class="terminal-header">'
      + '<div class="terminal-dots"><span></span><span></span><span></span></div>'
      + '<span class="terminal-title">pi — governed</span>'
      + '<span class="pi-status" data-role="status"></span>'
      + '</div>'
      + '<div class="terminal-body" data-role="body"></div>'
      + '<div class="pi-approvals" data-role="approvals"></div>'
      + '<form class="pi-composer" data-role="composer">'
      + '<span class="prompt">&gt;</span>'
      + '<input class="pi-input" data-role="input" autocomplete="off" spellcheck="false"'
      + ' placeholder="Ask pi to read a file…" disabled>'
      + '<button type="submit" class="pi-btn" data-role="send" disabled>Run</button>'
      + '<button type="button" class="pi-btn pi-btn--ghost" data-role="stop" hidden>Stop</button>'
      + '</form>'
      + '<div class="pi-gate" data-role="gate" hidden></div>'
      + '</div>';

    this._body = this.querySelector('[data-role="body"]');
    this._approvalsEl = this.querySelector('[data-role="approvals"]');
    this._statusEl = this.querySelector('[data-role="status"]');
    this._gateEl = this.querySelector('[data-role="gate"]');
    this._input = this.querySelector('[data-role="input"]');
    this._sendBtn = this.querySelector('[data-role="send"]');
    this._stopBtn = this.querySelector('[data-role="stop"]');

    this.querySelector('[data-role="composer"]').addEventListener('submit', (e) => {
      e.preventDefault();
      this._send();
    });
    this._stopBtn.addEventListener('click', () => this._post('abort', {}));
  }

  // ── lifecycle ─────────────────────────────────────────────────────────────

  async _start() {
    this._status('connecting');
    const token = this.getAttribute('token') || await this._mintToken();
    if (!token) return this._degrade('anonymous');
    this._token = token;

    const res = await this._fetch(this._endpoint + '/session', { token });
    if (!res.ok) {
      // 429 is by far the likeliest and is not an error the visitor caused.
      return this._degrade(res.status === 429 ? 'busy' : 'anonymous');
    }
    const body = await res.json();
    this._conversationId = body.conversation_id;
    this._openStream();
  }

  /**
   * Ask the server to mint a token for whoever owns the session cookie.
   * A 401 here is the ordinary anonymous case, not a failure.
   */
  async _mintToken() {
    try {
      const res = await fetch(this._endpoint + '/embed-token', {
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

  _openStream() {
    const url = this._endpoint + '/stream/' + encodeURIComponent(this._conversationId)
      + '?token=' + encodeURIComponent(this._token)
      + '&since=' + this._lastSeq;
    // EventSource cannot set headers, which is the whole reason the embed token
    // exists as a query-string credential rather than a bearer header.
    this._source = new EventSource(url);
    this._source.onmessage = (e) => this._onFrame(e.data);
    this._source.onopen = () => {
      this._reconnectMs = RECONNECT_MIN_MS;
      this._status('live');
    };
    this._source.onerror = () => {
      if (this._closed) return;
      this._teardownStream();
      this._status('reconnecting');
      this._reconnectTimer = setTimeout(() => this._openStream(), this._jitter());
      this._reconnectMs = Math.min(this._reconnectMs * 2, RECONNECT_MAX_MS);
    };

    // There is deliberately no visibilitychange handler. has_viewers() is a
    // receiver count, and a pending approval is abandoned — denied — after 15s
    // with nobody attached. Closing the stream on a hidden tab would silently
    // deny approvals the operator is about to answer.
  }

  _jitter() {
    return this._reconnectMs * (0.5 + Math.random() / 2);
  }

  _teardownStream() {
    if (this._source) {
      this._source.close();
      this._source = null;
    }
  }

  // ── frames ────────────────────────────────────────────────────────────────

  _onFrame(raw) {
    let f;
    try {
      f = JSON.parse(raw);
    } catch (_) {
      return;
    }

    // The stream subscribes before draining its replay buffer, so a frame
    // emitted in that window arrives twice. seq is monotonic; ignore the echo.
    if (typeof f.seq === 'number') {
      if (f.seq <= this._lastSeq) return;
      if (this._lastSeq && f.seq > this._lastSeq + 1) {
        this._line('output-dim', '── reconnected; earlier output may be missing ──');
      }
      this._lastSeq = f.seq;
    }

    switch (f.type) {
      case 'session_ready': return this._enable();
      case 'turn_start': return this._turnStart();
      case 'text_delta': return this._delta(f.text, false);
      case 'thinking_delta': return this._delta(f.text, true);
      case 'tool_start': return this._toolStart(f);
      case 'tool_end': return this._toolEnd(f);
      case 'tool_blocked': return this._toolBlocked(f);
      case 'prompt_blocked': return this._promptBlocked(f);
      case 'approval_request': return this._approvalRequest(f);
      case 'approval_resolved': return this._approvalResolved(f);
      case 'turn_end': return this._turnEnd();
      case 'stderr': return this._line('output-dim', f.line);
      case 'error': return this._line('output-warn', f.message);
      case 'exit': return this._exit(f);
      default: return undefined;
    }
  }

  _enable() {
    this._status('live');
    this._input.disabled = false;
    this._sendBtn.disabled = false;
  }

  _turnStart() {
    this._turnLive = true;
    this._textLine = null;
    this._stopBtn.hidden = false;
    this._cursor(true);
  }

  _turnEnd() {
    this._turnLive = false;
    this._textLine = null;
    this._stopBtn.hidden = true;
    this._cursor(false);
  }

  _delta(text, thinking) {
    if (!text) return;
    if (!this._textLine || this._textLineThinking !== thinking) {
      this._textLine = this._line(thinking ? 'output-dim' : 'output', '');
      this._textLineThinking = thinking;
    }
    this._textLine.textContent += text;
    this._scroll();
  }

  _toolStart(f) {
    this._textLine = null;
    const row = this._line('pi-tool-row', '');
    row.textContent = TOOL_ICON.pending + ' ' + f.tool_name + ' ' + summarise(f.tool_input);
    // tool_use_id is nullable; fall back to a per-row key so two concurrent
    // calls of the same tool cannot collide.
    this._toolRows.set(f.tool_use_id || 'anon:' + this._lastSeq, row);
  }

  _toolEnd(f) {
    const row = this._takeRow(f.tool_use_id, f.tool_name);
    if (!row) return;
    if (row.dataset.blocked === '1') return;
    row.textContent = (f.ok ? TOOL_ICON.ok : TOOL_ICON.blocked) + ' ' + f.tool_name
      + (f.ok ? ' ✓' : ' ✗');
  }

  _toolBlocked(f) {
    const row = this._takeRow(f.tool_use_id, f.tool_name)
      || this._line('pi-tool-row', '');
    row.classList.add('pi-tool-row--blocked');
    row.dataset.blocked = '1';
    row.textContent = TOOL_ICON.blocked + ' ' + f.tool_name + ' blocked'
      + (f.policy ? ' by ' + f.policy : '') + (f.reason ? ' — ' + f.reason : '');
  }

  _promptBlocked(f) {
    this._line('output-warn', 'Prompt blocked'
      + (f.policy ? ' by ' + f.policy : '') + (f.reason ? ' — ' + f.reason : '')
      + '. It never reached a provider.');
  }

  _takeRow(id, name) {
    const key = id || null;
    if (key && this._toolRows.has(key)) {
      const row = this._toolRows.get(key);
      this._toolRows.delete(key);
      return row;
    }
    // Unkeyed fallback: the oldest still-pending row for this tool name.
    for (const [k, row] of this._toolRows) {
      if (row.textContent.indexOf(' ' + name + ' ') === 1) {
        this._toolRows.delete(k);
        return row;
      }
    }
    return null;
  }

  _exit(f) {
    this._closed = true;
    this._teardownStream();
    this._status('ended');
    this._input.disabled = true;
    this._sendBtn.disabled = true;
    this._stopBtn.hidden = true;
    this._cursor(false);
    this._line('output-dim', 'Session ended'
      + (typeof f.code === 'number' ? ' (exit ' + f.code + ')' : '') + '.');
  }

  // ── approvals ─────────────────────────────────────────────────────────────

  /**
   * Rendered inline as a queue rather than a modal: the model issues parallel
   * tool calls, each with its own approval_id, and the backend resolves them
   * independently. A modal would serialise what the server does concurrently.
   */
  _approvalRequest(f) {
    const card = document.createElement('div');
    card.className = 'pi-approval-card';
    card.innerHTML = ''
      + '<div class="pi-approval-head">'
      + '<strong data-role="tool"></strong>'
      + '<span class="pi-countdown" data-role="countdown"></span>'
      + '</div>'
      + '<pre class="pi-approval-input" data-role="input"></pre>'
      + '<div class="pi-policy-chain" data-role="chain"></div>'
      + '<div class="pi-approval-actions">'
      + '<button type="button" class="pi-btn" data-role="allow">Approve</button>'
      + '<button type="button" class="pi-btn pi-btn--danger" data-role="deny">Deny</button>'
      + '</div>';

    card.querySelector('[data-role="tool"]').textContent = f.tool_name;
    card.querySelector('[data-role="input"]').textContent = pretty(f.tool_input);

    const chain = f.policy_chain || [];
    card.querySelector('[data-role="chain"]').textContent = chain.length
      ? 'cleared: ' + chain.join(' → ')
      : '';

    const countdownEl = card.querySelector('[data-role="countdown"]');
    let left = f.timeout_secs || 0;
    const tick = () => {
      countdownEl.textContent = left > 0 ? left + 's' : 'expired';
      if (left <= 0) clearInterval(entry.timer);
      left -= 1;
    };
    const entry = { card, timer: setInterval(tick, 1000) };
    tick();

    card.querySelector('[data-role="allow"]').addEventListener('click', () => this._decide(f.approval_id, 'allow'));
    card.querySelector('[data-role="deny"]').addEventListener('click', () => this._decide(f.approval_id, 'deny'));

    this._approvals.set(f.approval_id, entry);
    this._approvalsEl.append(card);
    this._scroll();
  }

  async _decide(approvalId, decision) {
    const entry = this._approvals.get(approvalId);
    if (entry) entry.card.querySelectorAll('button').forEach((b) => { b.disabled = true; });
    const res = await this._post('approve', { approval_id: approvalId, decision });
    // 409 means it was already settled — by the timeout, by another viewer, or
    // by abandonment. Say so rather than implying the click landed.
    if (res && res.status === 409) this._settle(approvalId, 'expired');
  }

  _approvalResolved(f) {
    // Resolution can arrive from another tab or from the server's own timeout,
    // so this must clear the card regardless of what this tab did.
    this._settle(f.approval_id, f.outcome);
  }

  _settle(approvalId, outcome) {
    const entry = this._approvals.get(approvalId);
    if (!entry) return;
    clearInterval(entry.timer);
    entry.card.remove();
    this._approvals.delete(approvalId);
    this._line('output-dim', 'approval ' + outcome);
  }

  // ── sending ───────────────────────────────────────────────────────────────

  async _send() {
    const message = this._input.value.trim();
    if (!message) return;
    this._input.value = '';
    this._echo(message);
    // Mid-turn input redirects the running turn instead of queueing a new one.
    await this._post(this._turnLive ? 'steer' : 'prompt', { message });
  }

  _post(path, extra) {
    const payload = Object.assign(
      { token: this._token, conversation_id: this._conversationId },
      extra,
    );
    return this._fetch(this._endpoint + '/' + path, payload).catch(() => null);
  }

  _fetch(url, payload) {
    return fetch(url, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload),
    });
  }

  // ── degraded mode ─────────────────────────────────────────────────────────

  _degrade(reason) {
    this._status(reason === 'busy' ? 'session in use' : 'demo');
    this._input.disabled = true;
    this._sendBtn.disabled = true;
    CANNED.forEach((step) => this._cannedStep(step));
    this._gateEl.hidden = false;
    this._gateEl.innerHTML = reason === 'busy'
      ? '<p>A session is already running for your account — one at a time. '
        + 'Close the other tab and reload to take it back.</p>'
      : '<p><strong>This is a replay.</strong> Register or sign in to drive a real '
        + 'pi agent, with every tool call gated in front of you.</p>'
        + '<p class="pi-gate-actions">'
        // /register and /login are the site-level redirects the rest of the
        // homepage uses; the login bounce returns here with the cookie set.
        + '<a class="pi-btn" href="/register">Create an account</a> '
        + '<a class="pi-btn pi-btn--ghost" href="/admin/login?redirect=%2F%23pi-terminal">Sign in</a>'
        + '</p>';
  }

  _cannedStep(step) {
    if (step.cls === 'tool') {
      const icon = step.state === 'ok' ? TOOL_ICON.ok : TOOL_ICON.pending;
      const tail = step.state === 'ok' ? ' ✓' : '';
      this._line('pi-tool-row', icon + ' ' + step.name + ' ' + step.arg + tail);
      return;
    }
    if (step.cls === 'approval') {
      const card = document.createElement('div');
      card.className = 'pi-approval-card pi-approval-card--canned';
      card.innerHTML = '<div class="pi-approval-head"><strong>read</strong>'
        + '<span class="pi-countdown">120s</span></div>'
        + '<div class="pi-policy-chain">cleared: scope_check → secret_scan '
        + '→ tool_blocklist → rate_limit</div>'
        + '<div class="pi-approval-actions">'
        + '<span class="pi-btn pi-btn--disabled">Approve</span>'
        + '<span class="pi-btn pi-btn--ghost pi-btn--disabled">Deny</span></div>';
      this._approvalsEl.append(card);
      return;
    }
    const el = this._line(step.cls, step.text);
    if (step.tail) {
      const cmd = document.createElement('span');
      cmd.className = 'command';
      cmd.textContent = ' ' + step.tail;
      el.append(cmd);
    }
  }

  // ── dom helpers ───────────────────────────────────────────────────────────

  _echo(message) {
    const line = document.createElement('div');
    line.className = 'terminal-line';
    const p = document.createElement('span');
    p.className = 'prompt';
    p.textContent = '>';
    const c = document.createElement('span');
    c.className = 'command';
    c.textContent = ' ' + message;
    line.append(p, c);
    this._body.append(line);
    this._scroll();
  }

  _line(cls, text) {
    const line = document.createElement('div');
    line.className = 'terminal-line';
    const span = document.createElement('span');
    span.className = cls;
    span.textContent = text;
    line.append(span);
    this._body.append(line);
    this._scroll();
    return span;
  }

  _cursor(on) {
    if (on && !this._cursorEl) {
      this._cursorEl = document.createElement('span');
      this._cursorEl.className = 'cursor';
      this._cursorEl.textContent = '▋';
      this._body.append(this._cursorEl);
    } else if (!on && this._cursorEl) {
      this._cursorEl.remove();
      this._cursorEl = null;
    }
  }

  _status(text) {
    this._statusEl.textContent = text;
    this._statusEl.dataset.state = text;
  }

  _scroll() {
    this._body.scrollTop = this._body.scrollHeight;
  }
}

/** One-line form of a tool's arguments, for the collapsed row. */
function summarise(input) {
  if (!input || typeof input !== 'object') return '';
  const v = input.path || input.file_path || input.pattern || input.command;
  if (typeof v === 'string') return v;
  const keys = Object.keys(input);
  return keys.length ? keys.join(', ') : '';
}

function pretty(input) {
  try {
    return JSON.stringify(input, null, 2);
  } catch (_) {
    return String(input);
  }
}

if (!customElements.get('sp-pi-terminal')) {
  customElements.define('sp-pi-terminal', SpPiTerminal);
}
