'use strict';

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
 */

const DEFAULT_ENDPOINT = '/api/public/pi';

/** Frames the widget renders as a tool row, keyed by tool_use_id. */
const TOOL_ICON = { pending: '▸', ok: '●', blocked: '✗' };

/** Backoff for manual stream reconnects. EventSource's own retry gives up. */
const RECONNECT_MIN_MS = 1000;
const RECONNECT_MAX_MS = 30000;

/** How close to the bottom still counts as "following the output". Roughly one
 *  line of slack, so a trackpad's inertial overscroll does not unpin the view. */
const PIN_SLACK_PX = 32;

/** Transcript cap, and how much is dropped when it is hit. Trimming in batches
 *  keeps the reflow cost off every line once a long session gets there. */
const MAX_LINES = 1200;
const TRIM_BATCH = 200;

/** Stats poll. Matches the interval the pane beside this one already uses. */
const STATS_MS = 3000;

/** Prompts kept for ↑/↓ recall. In memory only — a governed transcript is not
 *  something to leave in localStorage on a shared machine. */
const HISTORY_MAX = 50;

/** Composer ceiling, in rows. Past this the transcript matters more than seeing
 *  the whole draft. */
const INPUT_MAX_ROWS = 6;
const INPUT_ROW_PX = 22;

/** What an anonymous visitor sees. Mirrors the shape of a real exchange. */
const CANNED = [
  { cls: 'prompt', text: '>', tail: 'read src/auth.rs and tell me how sessions are minted' },
  { cls: 'output-dim', text: 'Reading the file…' },
  { cls: 'stages' },
  { cls: 'tool', name: 'read', arg: 'src/auth.rs', state: 'pending' },
  { cls: 'approval' },
  { cls: 'tool', name: 'read', arg: 'src/auth.rs', state: 'ok' },
  { cls: 'output', text: 'Sessions are minted by `SessionCreationService` and attested with an id the server issues, so spend and governance rows join on it.' },
];

/** The chain as the replay shows it. The card it feeds is labelled a replay, so
 *  standing in for a real frame here is not a claim about a real evaluation. */
const CANNED_STAGES = [
  { policy: 'scope_check', result: 'pass', detail: 'user scope is allowed for tool: read' },
  { policy: 'secret_scan', result: 'pass', detail: 'no credential pattern in the arguments' },
  { policy: 'tool_blocklist', result: 'pass', detail: 'read is not blocked' },
  { policy: 'rate_limit', result: 'pass', detail: '1 of 60 calls this minute' },
];

/** Step interval for the replay. Fast enough to finish before a visitor scrolls
 *  past, slow enough to read. */
const CANNED_STEP_MS = 340;

class SpPiTerminal extends HTMLElement {
  constructor() {
    super();
    this._conversationId = null;
    this._commands = [];
    this._source = null;
    this._lastSeq = 0;
    this._toolRows = new Map();
    this._approvals = new Map();
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
    this._cannedTimers = [];
    this._workEl = null;
    this._streamBuf = '';
    this._thinkBuf = '';
    this._thinkEl = null;
    this._raf = 0;
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
    if (this._statsTimer) clearInterval(this._statsTimer);
    this._cannedTimers.forEach(clearTimeout);
    this._approvals.forEach((a) => a.settle());
    this._approvals.clear();
  }

  // ── chrome ────────────────────────────────────────────────────────────────

  _build() {
    this.classList.add('pi-terminal');
    this.innerHTML = ''
      + '<div class="terminal active">'
      + '<div class="terminal-header">'
      + '<span class="pi-brand">'
      + '<img class="pi-brand-mark" src="/files/images/icon.svg" alt="" width="18" height="18">'
      + '<span class="pi-brand-name">systemprompt<span class="pi-brand-tld">.io</span></span>'
      + '</span>'
      + '<span class="pi-live" data-role="live"><i class="pi-live-dot" aria-hidden="true"></i>'
      + '<span class="pi-status" data-role="status"></span></span>'
      + '<span class="pi-jail-chip" data-role="jail" hidden></span>'
      + '<div class="pi-meters" data-role="meters" hidden>'
      + '<span class="pi-meter" data-role="m-tools" title="Tool calls the gate has judged">'
      + '<b>0</b><i>calls</i></span>'
      + '<span class="pi-meter" data-role="m-blocked" title="Calls policy or a person refused">'
      + '<b>0</b><i>blocked</i></span>'
      + '<span class="pi-meter" data-role="m-tokens" title="Tokens this conversation has spent">'
      + '<b>0</b><i>tokens</i></span>'
      + '<span class="pi-meter" data-role="m-cost" title="Metered spend for this conversation">'
      + '<b>$0.00</b><i>cost</i></span>'
      + '</div>'
      + '<a class="pi-trace-link" data-role="trace" href="/admin/demo/trace" hidden>audit trail →</a>'
      + '</div>'
      + '<div class="pi-body-wrap">'
      + '<div class="terminal-body" data-role="body" tabindex="0" role="log"'
      + ' aria-live="polite" aria-relevant="additions" aria-label="Agent transcript"></div>'
      + '<button type="button" class="pi-jump" data-role="jump" hidden></button>'
      + '</div>'
      + '<div class="pi-approvals" data-role="approvals"></div>'
      + '<div class="pi-palette" data-role="palette" role="listbox"'
      + ' aria-label="Skills" hidden></div>'
      + '<form class="pi-composer" data-role="composer">'
      // Drawn rather than typed: the ASCII `>` inherited the transcript's
      // metrics and could not be made to share a baseline with the input.
      + '<span class="prompt pi-prompt" aria-hidden="true">'
      + '<svg viewBox="0 0 12 12" focusable="false">'
      + '<path d="M3.75 2.5 7.25 6l-3.5 3.5" fill="none" stroke="currentColor"'
      + ' stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>'
      + '</svg></span>'
      + '<label class="sp-sr-only" for="pi-input-field">Ask the agent</label>'
      + '<textarea class="pi-input" id="pi-input-field" data-role="input" rows="1"'
      + ' autocomplete="off" spellcheck="false"'
      + ' placeholder="Ask the agent something, or type / for skills…"'
      + ' role="combobox" aria-expanded="false" aria-controls="pi-palette-list"'
      + ' aria-autocomplete="list" disabled></textarea>'
      + '<button type="submit" class="pi-btn pi-btn--send" data-role="send" disabled>'
      + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
      + '<path d="M3.5 2.25 9.25 6 3.5 9.75Z" fill="currentColor"/></svg>'
      + '<span class="pi-btn__label">Run</span>'
      + '</button>'
      + '<button type="button" class="pi-btn pi-btn--ghost pi-btn--stop" data-role="stop" hidden>'
      + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
      + '<rect x="3" y="3" width="6" height="6" rx="1" fill="currentColor"/></svg>'
      + '<span class="pi-btn__label">Stop</span>'
      + '</button>'
      + '</form>'
      + '<div class="pi-hint">'
      + '<span class="pi-hint__item"><kbd>↵</kbd>send</span>'
      + '<span class="pi-hint__item"><kbd>⇧↵</kbd>newline</span>'
      + '<span class="pi-hint__item"><kbd>↑↓</kbd>skills</span>'
      + '<span class="pi-hint__item"><kbd>↑</kbd>history</span>'
      + '<span class="pi-hint__item"><kbd>esc</kbd>stop</span>'
      + '</div>'
      + '<div class="pi-gate" data-role="gate" hidden></div>'
      + '</div>';

    this._body = this.querySelector('[data-role="body"]');
    this._approvalsEl = this.querySelector('[data-role="approvals"]');
    this._statusEl = this.querySelector('[data-role="status"]');
    this._liveEl = this.querySelector('[data-role="live"]');
    this._gateEl = this.querySelector('[data-role="gate"]');
    this._input = this.querySelector('[data-role="input"]');
    this._paletteEl = this.querySelector('[data-role="palette"]');
    this._sendBtn = this.querySelector('[data-role="send"]');
    this._stopBtn = this.querySelector('[data-role="stop"]');
    this._jumpBtn = this.querySelector('[data-role="jump"]');
    this._metersEl = this.querySelector('[data-role="meters"]');
    this._traceEl = this.querySelector('[data-role="trace"]');
    this._jailEl = this.querySelector('[data-role="jail"]');
    this._composer = this.querySelector('[data-role="composer"]');
    this._paletteEl.id = 'pi-palette-list';

    this._composer.addEventListener('submit', (e) => {
      e.preventDefault();
      this._send();
    });
    this._stopBtn.addEventListener('click', () => this._post('abort', {}));

    // The palette is discovery only. A leading `/` already works without it —
    // pi expands skill commands itself on the `prompt` utterance — so nothing
    // here parses or rewrites what the viewer typed.
    this._input.addEventListener('input', () => {
      this._autogrow();
      this._refreshPalette();
    });
    this._input.addEventListener('blur', () => {
      // Deferred: a click on a palette entry fires blur first, and hiding the
      // list synchronously would remove the element before the click lands.
      setTimeout(() => this._hidePalette(), 150);
    });
    this._input.addEventListener('keydown', (e) => this._onKey(e));

    // Autoscroll only while the visitor is actually at the bottom. Yanking the
    // view down mid-turn makes the transcript unreadable exactly when there is
    // something worth reading in it.
    this._body.addEventListener('scroll', () => {
      const gap = this._body.scrollHeight - this._body.scrollTop - this._body.clientHeight;
      this._pinned = gap < PIN_SLACK_PX;
      if (this._pinned) this._clearUnseen();
    });
    this._jumpBtn.addEventListener('click', () => {
      this._pinned = true;
      this._clearUnseen();
      this._body.scrollTop = this._body.scrollHeight;
    });
  }

  // ── slash commands ────────────────────────────────────────────────────────

  /**
   * Load the skills this session can run. Failure is silent on purpose: the
   * palette is a convenience, and a terminal that refuses to start because a
   * dropdown could not be populated is a worse outcome than no dropdown.
   */
  async _loadCommands() {
    const url = this._endpoint + '/commands/' + encodeURIComponent(this._conversationId)
      + '?token=' + encodeURIComponent(this._token);
    try {
      const res = await fetch(url, { credentials: 'same-origin' });
      this._commands = res.ok ? await res.json() : [];
    } catch (_) {
      this._commands = [];
    }
  }

  _refreshPalette() {
    const value = this._input.value;
    if (!this._commands || !this._commands.length || value[0] !== '/') {
      this._hidePalette();
      return;
    }
    const needle = value.toLowerCase();
    const hits = this._commands.filter((c) => c.command.toLowerCase().startsWith(needle));
    if (!hits.length) {
      this._hidePalette();
      return;
    }

    this._paletteEl.textContent = '';
    hits.forEach((hit, n) => {
      const row = document.createElement('button');
      row.type = 'button';
      row.className = 'pi-palette-row';
      row.id = 'pi-cmd-' + n;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', 'false');
      row.tabIndex = -1;
      const name = document.createElement('span');
      name.className = 'pi-palette-cmd';
      name.textContent = hit.command;
      const desc = document.createElement('span');
      desc.className = 'pi-palette-desc';
      desc.textContent = hit.description;
      row.append(name, desc);
      row.dataset.command = hit.command;
      // Pointer and keyboard resolve to the same call, so the two can never
      // drift into doing subtly different things.
      row.addEventListener('click', () => this._acceptPalette(row));
      row.addEventListener('mousemove', () => this._selectPalette(n));
      this._paletteEl.append(row);
    });
    this._paletteEl.hidden = false;
    this._input.setAttribute('aria-expanded', 'true');
    // Preselect the first hit, so `/` then Enter is a complete keyboard flow.
    this._selectPalette(0);
  }

  /** Move the palette's selection. Wraps, so ↑ from the top reaches the end. */
  _selectPalette(n) {
    const rows = this._paletteRows();
    if (!rows.length) return;
    const at = (n + rows.length) % rows.length;
    rows.forEach((row, i) => {
      const on = i === at;
      row.classList.toggle('is-selected', on);
      row.setAttribute('aria-selected', on ? 'true' : 'false');
      if (on) {
        row.scrollIntoView({ block: 'nearest' });
        this._input.setAttribute('aria-activedescendant', row.id);
      }
    });
    this._paletteAt = at;
  }

  /** Take the highlighted command into the composer. */
  _acceptPalette(row) {
    const target = row || this._paletteRows()[this._paletteAt];
    if (!target) return;
    this._input.value = target.dataset.command + ' ';
    this._hidePalette();
    this._input.focus();
    this._autogrow();
  }

  _paletteRows() {
    return Array.from(this._paletteEl.querySelectorAll('.pi-palette-row'));
  }

  _hidePalette() {
    this._paletteEl.hidden = true;
    this._paletteEl.textContent = '';
    this._paletteAt = 0;
    this._input.setAttribute('aria-expanded', 'false');
    this._input.removeAttribute('aria-activedescendant');
  }

  _paletteOpen() {
    return !this._paletteEl.hidden;
  }

  // ── lifecycle ─────────────────────────────────────────────────────────────

  /**
   * Re-run the whole start sequence against whatever credential now exists.
   *
   * Public because signing in happens outside this element: the pane beside it
   * establishes the cookie without a navigation, so nothing reloads the page
   * and the terminal has to be told the visitor stopped being anonymous.
   */
  async restart() {
    this._teardownStream();
    if (this._reconnectTimer) clearTimeout(this._reconnectTimer);
    if (this._statsTimer) clearInterval(this._statsTimer);
    this._cannedTimers.forEach(clearTimeout);
    this._cannedTimers = [];
    this._approvals.forEach((a) => a.settle());
    this._approvals.clear();
    this._approvalsEl.innerHTML = '';
    this._body.innerHTML = '';
    this._toolRows.clear();
    this._hidePalette();
    this._conversationId = null;
    this._lastSeq = 0;
    this._reconnectMs = RECONNECT_MIN_MS;
    this._turnLive = false;
    this._closed = false;
    this._who = null;
    this._workEl = null;
    this._streamBuf = '';
    this._thinkBuf = '';
    this._thinkEl = null;
    this._railFor = null;
    this._lines = 0;
    this._pinned = true;
    this._clearUnseen();
    this._metersEl.hidden = true;
    this._traceEl.hidden = true;
    this._gateEl.hidden = true;
    this._gateEl.innerHTML = '';
    this.classList.remove('is-replay');
    await this._start();
  }

  async _start() {
    this._status('connecting');
    const token = this.getAttribute('token') || await this._mintToken();
    if (!token) {
      this._who = await this._whoami();
      return this._degrade('anonymous');
    }
    this._token = token;

    const res = await this._fetch(this._endpoint + '/session', { token });
    if (!res.ok) {
      // 429 is by far the likeliest and is not an error the visitor caused.
      return this._degrade(res.status === 429 ? 'busy' : 'anonymous');
    }
    const body = await res.json();
    this._conversationId = body.conversation_id;
    // The stats pane polls per conversation and cannot mint its own token, so
    // the credential travels with the announcement. Same origin, same page.
    this._emit('pi-session', { conversation_id: this._conversationId, token: this._token });
    // Not awaited: the palette is discovery, and the stream is the thing the
    // viewer is waiting for.
    this._loadCommands();
    this._openStream();
    this._startStats();
    return undefined;
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

  /**
   * Who the session cookie belongs to, or null when anonymous.
   *
   * `/admin/auth/me` sits behind a middleware that 307s to the login page
   * rather than answering 401, so an anonymous visitor would otherwise get
   * 200 OK carrying HTML. `redirect: 'manual'` turns that into an opaque
   * response we can reject instead of trying to parse.
   */
  async _whoami() {
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

  // ── header meters ─────────────────────────────────────────────────────────

  /**
   * Poll the stats the pane already polls.
   *
   * Cost and denial counts belong in the terminal's own chrome: the claim this
   * page makes is that governance is metered, and a number that moves while you
   * watch is the cheapest possible proof. No new endpoint — this is the same
   * `GET stats/{id}` the pane beside it uses.
   */
  _startStats() {
    const poll = async () => {
      if (this._closed || !this._conversationId) return;
      try {
        const res = await fetch(this._endpoint + '/stats/'
          + encodeURIComponent(this._conversationId)
          + '?token=' + encodeURIComponent(this._token), { credentials: 'same-origin' });
        if (!res.ok) return;
        this._meters(await res.json());
      } catch (_) {
        // A failed poll is cosmetic. The transcript is the source of truth.
      }
    };
    void poll();
    this._statsTimer = setInterval(poll, STATS_MS);
  }

  _meters(s) {
    this._metersEl.hidden = false;
    this._traceEl.hidden = false;
    const set = (role, value) => {
      const el = this.querySelector('[data-role="' + role + '"] b');
      if (el) el.textContent = value;
    };
    set('m-tools', String(s.tool_calls || 0));
    set('m-blocked', String((s.tools_blocked || 0) + (s.prompts_blocked || 0)));
    set('m-tokens', compact((s.input_tokens || 0) + (s.output_tokens || 0)));
    set('m-cost', s.cost_display || '$0.00');
    const blocked = this.querySelector('[data-role="m-blocked"]');
    if (blocked) blocked.dataset.hot = (s.tools_blocked || s.prompts_blocked) ? '1' : '0';
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

    // Republished so a sibling pane can react to the same turn the terminal is
    // rendering, without opening a second EventSource against the one stream a
    // conversation has.
    this._emit('pi-frame', f);

    switch (f.type) {
      case 'session_ready': return this._enable();
      case 'turn_start': return this._turnStart();
      case 'text_delta': return this._delta(f.text, false);
      case 'thinking_delta': return this._delta(f.text, true);
      case 'policy_stages': return this._policyStages(f);
      case 'tool_start': return this._toolStart(f);
      case 'tool_end': return this._toolEnd(f);
      case 'tool_blocked': return this._toolBlocked(f);
      case 'prompt_blocked': return this._promptBlocked(f);
      case 'approval_request': return this._approvalRequest(f);
      case 'approval_resolved': return this._approvalResolved(f);
      case 'turn_end': return this._turnEnd();
      case 'stderr': return this._stderr(f.line);
      case 'error': return this._line('output-warn', f.message);
      case 'exit': return this._exit(f);
      default: return undefined;
    }
  }

  /**
   * The jail banner is a claim about confinement, and it was the first line of
   * the transcript — which made it the first thing to scroll out of sight. It
   * is promoted to the chrome, where it stays true for as long as it is true.
   */
  _stderr(line) {
    const jail = /^sp-pi-jail:\s*(.+)$/.exec(line || '');
    if (jail && this._jailEl) {
      const landlock = /\(([^)]*Landlock[^)]*)\)/i.exec(jail[1]);
      this._jailEl.textContent = landlock ? landlock[1] : 'sandboxed';
      this._jailEl.title = jail[1];
      this._jailEl.hidden = false;
      return undefined;
    }
    return this._line('output-dim', line);
  }

  _enable() {
    this._status('live');
    this._input.disabled = false;
    this._sendBtn.disabled = false;
  }

  _turnStart() {
    this._turnLive = true;
    this._flushStream();
    this._stopBtn.hidden = false;
    // The composer's live treatment — progress sliver, demoted Run — is CSS off
    // this one attribute, so the busy state can never disagree with _turnLive.
    this._composer.dataset.busy = 'true';
  }

  _turnEnd() {
    this._turnLive = false;
    this._flushStream();
    this._thinkBuf = '';
    this._thinkEl = null;
    this._orphanRail();
    this._stopBtn.hidden = true;
    delete this._composer.dataset.busy;
  }

  /**
   * Assistant prose, buffered and never shown raw.
   *
   * Markdown cannot be parsed token by token — a fence is not a fence until its
   * closing line lands, and a table is not a table until its separator row does.
   * The old behaviour streamed the raw buffer into the transcript and only
   * rendered it at the end of the turn, which meant the viewer read literal
   * `## heading` and `**bold**` for the entire time an answer took to arrive.
   *
   * So nothing is written until it is renderable. Deltas accumulate, the working
   * indicator carries the fact that something is happening, and every *complete*
   * top-level block is revealed as prose the moment it closes.
   */
  _delta(text, thinking) {
    if (!text) return;
    if (thinking) {
      this._thinkBuf += text;
      if (!this._thinkEl) this._thinkEl = this._thinkBlock();
      this._thinkEl.body.textContent = this._thinkBuf;
      this._thinkEl.count.textContent = approxTokens(this._thinkBuf) + ' tokens';
      this._nudge();
      return;
    }
    this._streamBuf += text;
    this._working(true);
    // Coalesced onto a frame: a blank-line scan per delta is wasted work when
    // twenty of them land between two paints.
    if (!this._raf) {
      this._raf = requestAnimationFrame(() => {
        this._raf = 0;
        this._revealComplete();
        this._workingSync();
        this._nudge();
      });
    }
  }

  /**
   * Reveal every block that has finished, keep the rest buffered.
   *
   * Waiting for the whole turn would leave a long answer as nothing but a
   * spinner for as long as it takes to write, which trades one bad experience
   * for another. A blank line is markdown's own block separator, so everything
   * before the last one is complete by definition and can be rendered now.
   *
   * The exception is an open code fence: an odd number of ``` means the reader
   * is mid-block and its blank lines separate nothing, so the whole buffer waits.
   */
  _revealComplete() {
    if (!window.SpPiRender) return;
    const cut = this._streamBuf.lastIndexOf('\n\n');
    if (cut === -1) return;
    const head = this._streamBuf.slice(0, cut);
    if (!head.trim()) return;
    if (countFences(head) % 2 !== 0) return;
    this._renderProse(head);
    this._streamBuf = this._streamBuf.slice(cut + 2);
  }

  /** Render one finished unit of markdown into the transcript. */
  _renderProse(md) {
    this._orphanRail();
    const host = document.createElement('div');
    host.className = 'pi-prose pi-reveal';
    host.append(window.SpPiRender.markdown(md));
    this._append(host);
  }

  /** End of a block: render whatever is left, however it ends. */
  _flushStream() {
    if (this._raf) {
      cancelAnimationFrame(this._raf);
      this._raf = 0;
    }
    if (this._streamBuf.trim()) {
      if (window.SpPiRender) {
        this._renderProse(this._streamBuf);
      } else {
        // No renderer: plain text beats nothing, and beats markup.
        const line = document.createElement('div');
        line.className = 'terminal-line pi-stream';
        line.textContent = this._streamBuf;
        this._append(line);
      }
    }
    this._streamBuf = '';
    this._working(false);
  }

  /** Chain-of-thought, collapsed. Interesting, but not the answer. */
  _thinkBlock() {
    const details = document.createElement('details');
    details.className = 'pi-think';
    const summary = document.createElement('summary');
    const label = document.createElement('span');
    label.textContent = 'thinking';
    const count = document.createElement('span');
    count.className = 'pi-think-count';
    summary.append(label, count);
    const body = document.createElement('div');
    body.className = 'pi-think-body';
    details.append(summary, body);
    this._append(details);
    return { details, body, count };
  }

  // ── the gate ──────────────────────────────────────────────────────────────

  /**
   * The chain that judged the call that follows.
   *
   * Rendered for every governed call, allow or deny. A gate only visible when it
   * blocks something reads as an error path; a gate visible on every call reads
   * as what it is.
   *
   * Held rather than drawn immediately. A rail on its own line says a chain ran
   * but not what it judged, and two of them in a row — one for the prompt, one
   * for the tool it triggered — read as the same thing rendered twice. So the
   * stages wait here for the row they belong to and are drawn inside it.
   */
  _policyStages(f) {
    if (!window.SpPiGate) return;
    this._railFor = f.stages || [];
  }

  /** Draw a held chain inline, or on its own line if nothing claimed it. */
  _railEl(stages, compact) {
    const rail = window.SpPiGate.chainRail(stages, { compact });
    if (compact) return rail;
    const wrap = document.createElement('div');
    wrap.className = 'pi-rail-line';
    if (stages.some((st) => st.result === 'fail')) wrap.classList.add('is-denied');
    wrap.append(rail);
    return wrap;
  }

  /**
   * Nothing claimed the chain, so give it its own line.
   *
   * A held rail must never be silently dropped: the rail's whole claim to being
   * evidence is that it is a complete record of what ran.
   */
  _orphanRail() {
    if (!this._railFor || !window.SpPiGate) return;
    this._append(this._railEl(this._railFor, false));
    this._railFor = null;
  }

  _toolStart(f) {
    // Claim the held chain before flushing: _renderProse orphans anything still
    // unclaimed, and this call is exactly the thing that was going to claim it.
    const held = this._railFor;
    this._railFor = null;
    this._flushStream();
    this._railFor = held;
    const row = this._toolRow(f.tool_name, summarise(f.tool_input), f.tool_input);
    // tool_use_id is nullable; fall back to a per-row key so two concurrent
    // calls of the same tool cannot collide.
    this._toolRows.set(f.tool_use_id || 'anon:' + this._lastSeq, row);
  }

  /**
   * One tool call, expandable.
   *
   * The summary is the glance version; the arguments are one click away. Keying
   * is on dataset.tool rather than on the rendered text, so relabelling a row
   * cannot break the lookup that pairs it with its tool_end.
   */
  _toolRow(name, arg, input) {
    const details = document.createElement('details');
    details.className = 'pi-tool';
    details.dataset.tool = name;
    details.dataset.state = 'pending';

    const summary = document.createElement('summary');
    const icon = document.createElement('span');
    icon.className = 'pi-tool-icon';
    icon.textContent = TOOL_ICON.pending;
    icon.setAttribute('aria-hidden', 'true');
    const label = document.createElement('span');
    label.className = 'pi-tool-name';
    label.textContent = name;
    const argEl = document.createElement('span');
    argEl.className = 'pi-tool-arg';
    argEl.textContent = arg || '';
    const state = document.createElement('span');
    state.className = 'pi-tool-state';
    state.textContent = 'awaiting the gate';
    summary.append(icon, label, argEl, state);
    if (this._railFor && window.SpPiGate) {
      if (this._railFor.some((st) => st.result === 'fail')) {
        details.classList.add('is-denied');
      }
      summary.append(this._railEl(this._railFor, true));
      this._railFor = null;
    }

    const body = document.createElement('pre');
    body.className = 'pi-tool-body';
    body.textContent = pretty(input);
    details.append(summary, body);

    this._append(details);
    return { details, icon, state };
  }

  _toolEnd(f) {
    const row = this._takeRow(f.tool_use_id, f.tool_name);
    if (!row) return;
    if (row.details.dataset.state === 'blocked') return;
    row.details.dataset.state = f.ok ? 'ok' : 'failed';
    row.icon.textContent = f.ok ? TOOL_ICON.ok : TOOL_ICON.blocked;
    row.state.textContent = f.ok ? 'ran' : 'failed';
  }

  _toolBlocked(f) {
    const row = this._takeRow(f.tool_use_id, f.tool_name);
    if (row) {
      row.details.dataset.state = 'blocked';
      row.icon.textContent = TOOL_ICON.blocked;
      row.state.textContent = 'blocked';
    }
    if (window.SpPiGate) this._append(window.SpPiGate.blockedRow(f));
    else {
      this._line('output-warn', f.tool_name + ' blocked'
        + (f.policy ? ' by ' + f.policy : '') + (f.reason ? ' — ' + f.reason : ''));
    }
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
    // Unkeyed fallback: the oldest still-pending row for this tool name, matched
    // on the element's own data attribute rather than on its rendered text.
    for (const [k, row] of this._toolRows) {
      if (row.details.dataset.tool === name) {
        this._toolRows.delete(k);
        return row;
      }
    }
    return null;
  }

  _exit(f) {
    this._closed = true;
    this._teardownStream();
    if (this._statsTimer) clearInterval(this._statsTimer);
    this._flushStream();
    this._status('ended');
    this._input.disabled = true;
    this._sendBtn.disabled = true;
    this._stopBtn.hidden = true;
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
    if (!window.SpPiGate) return;
    const handle = window.SpPiGate.approvalCard(f, (decision) => {
      handle.lock();
      void this._decide(f.approval_id, decision);
    });
    this._approvals.set(f.approval_id, handle);
    this._approvalsEl.append(handle.el);
    // Focus moves to the card because a turn is now blocked on this answer, and
    // the operator's attention should not have to be recruited by a colour.
    handle.focus();
    this._nudge();
  }

  async _decide(approvalId, decision) {
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
    entry.settle();
    this._approvals.delete(approvalId);
    this._line(outcome === 'approved' ? 'output-dim' : 'output-warn', 'approval ' + outcome);
    // Nothing else is queued, so put the caret back where typing continues.
    if (!this._approvals.size && !this._input.disabled) this._input.focus();
  }

  // ── sending ───────────────────────────────────────────────────────────────

  async _send() {
    const message = this._input.value.trim();
    if (!message) return;
    this._input.value = '';
    this._autogrow();
    this._hidePalette();
    this._remember(message);
    this._echo(message);
    // Mid-turn input redirects the running turn instead of queueing a new one.
    await this._post(this._turnLive ? 'steer' : 'prompt', { message });
  }

  /**
   * Keyboard handling for the composer.
   *
   * Enter sends because this is a chat surface and that is what a chat surface
   * does; shift-enter is the escape hatch for a multi-line prompt. Escape closes
   * the palette if it is open and otherwise stops a running turn — the narrower
   * meaning wins, so escape never does something drastic while a dropdown is
   * covering the thing the viewer was looking at.
   */
  _onKey(e) {
    if (e.key === 'Escape') {
      if (this._paletteOpen()) {
        e.preventDefault();
        this._hidePalette();
      } else if (this._turnLive) {
        e.preventDefault();
        void this._post('abort', {});
      }
      return;
    }
    // The palette owns the arrows and the accept keys while it is open, and
    // recalling history under an open list would fight what the viewer is
    // reading. This block used to be a bare `return` with a comment saying the
    // palette owned these keys — nothing did, so the list was mouse-only.
    if (this._paletteOpen()) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        this._selectPalette(this._paletteAt + 1);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        this._selectPalette(this._paletteAt - 1);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        this._acceptPalette();
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void this._send();
      return;
    }
    if (this._paletteOpen()) return;
    // History only when the caret cannot usefully move, so ↑ still navigates a
    // prompt the visitor is part-way through writing.
    if (e.key === 'ArrowUp' && this._caretAtStart()) {
      if (this._historyAt + 1 < this._history.length) {
        this._historyAt += 1;
        this._recall();
        e.preventDefault();
      }
      return;
    }
    if (e.key === 'ArrowDown' && this._caretAtEnd()) {
      if (this._historyAt > 0) {
        this._historyAt -= 1;
        this._recall();
        e.preventDefault();
      } else if (this._historyAt === 0) {
        this._historyAt = -1;
        this._input.value = '';
        this._autogrow();
        e.preventDefault();
      }
    }
  }

  _caretAtStart() {
    return this._input.selectionStart === 0 && this._input.selectionEnd === 0;
  }

  _caretAtEnd() {
    const n = this._input.value.length;
    return this._input.selectionStart === n && this._input.selectionEnd === n;
  }

  _recall() {
    this._input.value = this._history[this._historyAt] || '';
    this._autogrow();
    const n = this._input.value.length;
    this._input.setSelectionRange(n, n);
  }

  _remember(message) {
    this._history.unshift(message);
    if (this._history.length > HISTORY_MAX) this._history.pop();
    this._historyAt = -1;
  }

  /** Grow with the prompt, up to a ceiling. */
  _autogrow() {
    this._input.style.height = 'auto';
    this._input.style.height
      = Math.min(this._input.scrollHeight, INPUT_MAX_ROWS * INPUT_ROW_PX) + 'px';
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
    this.classList.add('is-replay');
    this._status(reason === 'busy' ? 'session in use' : 'replay');
    this._input.disabled = true;
    this._sendBtn.disabled = true;

    // Played on a timeline rather than dumped. An anonymous visitor is the one
    // being asked to sign up, so they should see the chain resolve the way a
    // real one does — the pacing is the argument.
    const step = window.SpPiGate && window.SpPiGate.motionOk() ? CANNED_STEP_MS : 0;
    CANNED.forEach((s, n) => {
      if (!step) {
        this._cannedStep(s);
        return;
      }
      this._cannedTimers.push(setTimeout(() => this._cannedStep(s), n * step));
    });

    this._gateEl.hidden = false;
    if (reason === 'busy') {
      // Not "you already have one": a second conversation from the same
      // account displaces the first, so the only 429 left is the server-wide
      // cap, which no action of this user's can clear.
      this._gateEl.innerHTML = '<p>Every pi session on this server is in use. '
        + 'Sessions free up as they finish or go idle — reload in a minute.</p>';
      return;
    }
    // Signed in but no token: the account exists and the terminal is
    // configured, so this is a server-side problem, not a sign-in prompt.
    if (this._who && this._who.email) {
      this._gateEl.innerHTML = '<p>Signed in as <strong></strong>, but no session '
        + 'could be started. The terminal may not be configured on this deployment.</p>';
      this._gateEl.querySelector('strong').textContent = this._who.email;
      return;
    }
    // Anonymous. The pane beside this terminal owns sign-in and registration —
    // one implementation of the ceremony, and it is the half of the screen the
    // visitor is already looking at. Here, say only what the replay is.
    this._gateEl.innerHTML = '<p><strong>This is a replay.</strong> Create an account '
      + 'or sign in to drive a real agent — every tool call it makes will stop here '
      + 'for your approval.</p>';
  }

  _cannedStep(step) {
    if (step.cls === 'stages') {
      this._policyStages({ stages: CANNED_STAGES });
      return;
    }
    if (step.cls === 'tool') {
      const row = this._toolRow(step.name, step.arg, { path: step.arg });
      if (step.state === 'ok') {
        row.details.dataset.state = 'ok';
        row.icon.textContent = TOOL_ICON.ok;
        row.state.textContent = 'ran';
      }
      return;
    }
    if (step.cls === 'approval') {
      if (!window.SpPiGate) return;
      const handle = window.SpPiGate.approvalCard({
        tool_name: 'read',
        tool_input: { path: 'src/auth.rs' },
        policy_chain: CANNED_STAGES.map((s) => s.policy),
        timeout_secs: 120,
      }, () => {});
      handle.el.classList.add('pi-approval-card--canned');
      // A replay's buttons must not look answerable: the card is evidence of
      // what the real thing does, not an offer to do it.
      handle.lock();
      handle.el.setAttribute('aria-label', 'Example approval card (replay)');
      this._approvalsEl.append(handle.el);
      return;
    }
    if (step.cls === 'prompt') {
      this._echo(step.tail);
      return;
    }
    if (step.cls === 'output' && window.SpPiRender) {
      const host = document.createElement('div');
      host.className = 'pi-prose';
      host.append(window.SpPiRender.markdown(step.text));
      this._append(host);
      return;
    }
    this._line(step.cls, step.text);
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
    this._append(line);
  }

  _line(cls, text) {
    const line = document.createElement('div');
    line.className = 'terminal-line';
    const span = document.createElement('span');
    span.className = cls;
    span.textContent = text;
    line.append(span);
    this._append(line);
    return span;
  }

  /** Append, trim, and scroll — the one place the transcript grows. */
  _append(node) {
    this._body.append(node);
    this._lines += 1;
    this._trim();
    // Counted here and not in _nudge: a streaming turn calls that once per
    // frame, and "↓ 400 new" for one paragraph would be worse than no badge.
    if (!this._pinned) this._unseen += 1;
    this._nudge();
  }

  /**
   * Cap the transcript.
   *
   * A long session would otherwise hold every line for the tab's lifetime. The
   * marker is not decoration: the server's own replay buffer is capped too, and
   * both places say so rather than letting a gap look like silence.
   */
  _trim() {
    if (this._lines <= MAX_LINES) return;
    const marked = this._body.querySelector('.pi-trimmed');
    for (let i = 0; i < TRIM_BATCH; i += 1) {
      // Drop from the head, keeping the marker itself at the top.
      const victim = marked ? marked.nextSibling : this._body.firstChild;
      if (!victim) break;
      victim.remove();
      this._lines -= 1;
    }
    if (!marked) {
      const mark = document.createElement('div');
      mark.className = 'terminal-line pi-trimmed';
      mark.textContent = '── earlier output trimmed ──';
      this._body.prepend(mark);
    }
  }

  /** Follow the output, or offer to. */
  _nudge() {
    if (this._pinned) {
      this._body.scrollTop = this._body.scrollHeight;
      return;
    }
    if (!this._unseen) return;
    this._jumpBtn.hidden = false;
    this._jumpBtn.textContent = '↓ ' + this._unseen + ' new';
  }

  _clearUnseen() {
    this._unseen = 0;
    this._jumpBtn.hidden = true;
  }

  /**
   * The working indicator.
   *
   * This replaced a blinking caret that was appended to the transcript body. A
   * caret in the transcript claims the transcript is an input — it is not, and
   * the one that is sat several hundred pixels below it, so the page gave no
   * honest answer to "where do I type?". The composer owns the caret now.
   *
   * What belongs here instead is the fact that something is running and how
   * much of it has arrived, which is exactly what a viewer waiting on a block
   * wants to know while the raw markdown is deliberately withheld.
   */
  _working(on) {
    if (on && !this._workEl) {
      const row = document.createElement('div');
      row.className = 'pi-working';
      row.setAttribute('aria-hidden', 'true');
      const dot = document.createElement('i');
      dot.className = 'pi-working-dot';
      const label = document.createElement('span');
      label.className = 'pi-working-label';
      label.textContent = 'working';
      const count = document.createElement('span');
      count.className = 'pi-working-count';
      row.append(dot, label, count);
      this._body.append(row);
      this._workEl = { row, count };
    } else if (!on && this._workEl) {
      this._workEl.row.remove();
      this._workEl = null;
    }
  }

  /**
   * Keep the indicator last and its count current.
   *
   * Revealed blocks are appended to the body, so without this the indicator
   * would be stranded above the prose it is supposedly still producing.
   */
  _workingSync() {
    if (!this._workEl) return;
    this._workEl.count.textContent = this._streamBuf
      ? approxTokens(this._streamBuf) + ' tokens' : '';
    this._body.append(this._workEl.row);
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

/** Rough token count for the thinking summary. Four characters per token is the
 *  usual English approximation, and this is a label, not an invoice. */
function approxTokens(s) {
  return Math.max(1, Math.round(s.length / 4));
}

/** Fence count, to tell a closed code block from one still being written. */
function countFences(s) {
  const m = s.match(/^\s*```/gm);
  return m ? m.length : 0;
}

/** 1200 -> 1.2k. Keeps the header meters from reflowing as a session runs. */
function compact(n) {
  if (n < 1000) return String(n);
  if (n < 1000000) return (n / 1000).toFixed(1).replace(/\.0$/, '') + 'k';
  return (n / 1000000).toFixed(1).replace(/\.0$/, '') + 'm';
}

if (!customElements.get('sp-pi-terminal')) {
  customElements.define('sp-pi-terminal', SpPiTerminal);
}
