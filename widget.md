# pi web terminal — remaining work

Handoff for the `<sp-pi-terminal>` widget. The Rust half landed in
`feat/pi-web-terminal` (commit `07e8193`); the browser half does not exist yet, so the endpoints
below are currently exercisable only by curl.

Design rationale and the measured pi behaviour behind it live in the plan at
`~/.claude/plans/can-the-gui-for-flickering-hanrahan.md`. Read its **Verified protocol** section
before changing anything about the gate — several of the constraints are non-obvious and were
established empirically against pi 0.82.0.

---

## What already works

`extensions/web/admin/src/handlers/pi/` — one `pi --mode rpc` child per conversation.

| File | Role |
|---|---|
| `mod.rs` | Routes, embed-token auth, SSE stream, RPC command allowlist |
| `config.rs` | `PiConfig::from_env`; the whole surface is absent unless configured |
| `spawn.rs` | Workspace tree, `env_clear`, per-session `HOME` + `models.json` (0600) |
| `session.rs` | Held stdin, broadcast + replay ring, pending-approval map |
| `registry.rs` | Session table, per-user/global caps, idle + lifetime reaper |
| `pump.rs` | Owns stdout; routes each `extension_ui_request` to the gate |
| `gate.rs` | Policy chain, then the human round-trip. Policy first, always |
| `rpc.rs` | Wire types pinned against a captured transcript |
| `events.rs` | pi frames → the widget's stable event vocabulary |
| `token.rs` | HMAC embed token with a signed expiry |
| `shim/governance-shim.ts` | The in-pi enforcement point. Decides nothing |

Plus `webhook/governance/inproc.rs` (the in-process seam into the existing four-stage chain, same
audit row shape as the CLI path) and `util/hmac.rs` (factored out of `share.rs`, RFC 4231 vector).

28 unit tests pass; `cargo clippy --workspace` is clean.

---

## 1. Verify what was not re-run

The last two gates were skipped at the end of the session. The lint fixes made after the last green
test run were mechanical (`const fn`, extracted routers, drop ordering), so this is expected to pass,
but it has not been seen:

```bash
./scripts/check-repository-naming.sh
SQLX_OFFLINE=true cargo test -p systemprompt-web-admin --lib
just clippy
```

---

## 2. The widget itself — the main remaining work

Two new files, both dependency-free vanilla JS/CSS per the project standard:

- `storage/files/js/services/sp-pi-terminal.js`
- `storage/files/css/components/pi-terminal.css`

Register both, then publish:

- add an `svc_js!` entry in `extensions/web/site/src/assets/js_services.rs`
- add a css entry in `extensions/web/site/src/assets/css.rs`
- `just publish` (bundles, then copies to `web/dist/`)

**Hard constraints, not preferences:**

- **No ES module imports.** The admin bundle path (`extensions/web/jobs/src/bundle_admin_js.rs`) is
  plain Rust file concatenation, not a bundler. Use an IIFE + `class extends HTMLElement`.
- **No shadow DOM.** Matches `sp-toast.js` / `sp-confirm-dialog.js`, and it lets the global `--sp-*`
  tokens and dark mode apply for free.
- Guard registration with `if (!customElements.get('sp-pi-terminal'))`.
- **`EventSource` cannot set headers** — the token must ride in the query string. This is why the
  embed token exists at all; see §5.

This will be the **first `EventSource` consumer in the codebase**. Budget for retry/backoff, an
`onerror` path, and pausing on `visibilitychange` — none of it is free.

### Element shape

```html
<sp-pi-terminal endpoint="/api/public/pi" token="…"></sp-pi-terminal>
```

Visual target: reuse the class vocabulary and look of `storage/files/js/terminal-demo.js` and
`CliRemoteAnimationPartialRenderer` so the live widget is continuous with the existing fake terminal.
Tool rows read `● read src/auth.rs 12ms`, `● edit ✓`, `▸ bash cargo check …`, with streaming text and
a blinking `▋`.

### Events to render

Each frame is JSON with a monotonic `seq` (used as the SSE event id, so `Last-Event-ID` resumes):

`session_ready`, `turn_start`, `text_delta`, `thinking_delta`, `tool_start`, `tool_end`,
`tool_blocked`, `prompt_blocked`, `approval_request`, `approval_resolved`, `turn_end`, `stderr`,
`error`, `exit`.

⚠️ **`tool_start` is emitted by the gate, not by pi's `tool_execution_start`.** That pi frame fires
*before* the gate resolves and also fires for blocked calls, so it is deliberately dropped in
`events.rs`. Do not reintroduce it as a "running" signal or denied calls will flash as executing.

### Approval UI

`approval_request` carries `approval_id`, `tool_name`, `tool_input`, `policy_chain`, `timeout_secs`.

- Render **inline in the stack as a queue, not a modal** — the model can issue parallel tool calls,
  and each gets its own `approval_id`. A modal would serialise what the backend handles concurrently.
- Show `[Approve] [Deny]`, a countdown from `timeout_secs`, the collapsed `tool_input`, and the
  policy chain that already cleared it — the operator should see what passed, not be asked to trust a
  bare prompt.
- Clear the card on `approval_resolved`, whose `outcome` is `approved` | `denied` | `timeout` |
  `abandoned`. Resolution can come from another viewer or from the timeout, not just this tab.
- `POST /approve` answers **409** when the approval is already settled. Show "expired" rather than
  pretending the click landed.

### Degradation

With no token or an invalid one, render the canned `terminal-demo.js` replay plus a "Sign in to run
this live" footer, so a public page can drop the element in unconditionally.

---

## 3. Endpoints

All under `/api/public/pi`, public by design: the site auth gate 302-redirects unauthenticated hits
on protected prefixes, and an `EventSource` reports a redirect-to-HTML as an opaque error rather than
a 401. The embed token is the only credential.

| Method | Path | Body / query |
|---|---|---|
| POST | `/session` | `{token}` → `201 {conversation_id}`; `429` at a cap |
| GET | `/stream/{conversation_id}` | `?token=…&since=…`, honours `Last-Event-ID` |
| POST | `/prompt` | `{token, conversation_id, message}` |
| POST | `/steer` | same — redirect the running turn |
| POST | `/follow-up` | same — queue for after the turn |
| POST | `/abort` | `{token, conversation_id}` |
| POST | `/approve` | `{token, conversation_id, approval_id, decision}` |

Issue a token (admin session required):
`POST /api/public/admin/users/{user_id}/pi-embed-token` → `{token, expires_at}`.

🔴 **Never add a passthrough that lets a client name the RPC command type.** `{"type":"bash"}`
executes a shell command with **no `tool_call` hook firing at all** — verified. The command type is
chosen by the route (`Utterance` in `mod.rs`), never read from a request. Relaying raw RPC would hand
every viewer a shell.

---

## 4. Configuration

The widget is absent unless `SP_PI_GATEWAY_KEY` is set — no half-configured agent service.

| Var | Default | Notes |
|---|---|---|
| `SP_PI_GATEWAY_KEY` | — | **Required.** Gateway credential for `/v1/messages` |
| `SP_PI_BINARY` | `pi` | |
| `SP_PI_WORKSPACE_ROOT` | `/tmp/systemprompt-pi-sessions` | Put on a size-capped tmpfs |
| `SP_PI_BASE_URL` | `http://127.0.0.1:8080` | |
| `SP_PI_PROVIDER` / `SP_PI_MODEL` | `systemprompt` / `claude-sonnet-4-6` | |
| `SP_PI_TOOLS` | `read` | **See §6 before widening** |
| `SP_PI_APPROVE_ALL` | `1` | Ask about every tool call |
| `SP_PI_APPROVAL_TIMEOUT_SECS` | `120` | Ours alone; pi has no ceiling to duck under |
| `SP_PI_IDLE_SECS` / `SP_PI_MAX_LIFETIME_SECS` | `600` / `3600` | |
| `SP_PI_MAX_PER_USER` / `SP_PI_MAX_TOTAL` | `1` / `8` | |

pi must be on `PATH` (or `SP_PI_BINARY`): `npm install -g --ignore-scripts @earendil-works/pi-coding-agent`.

---

## 5. Still to port from `systemprompt-template`

- `examples/pi/**` — the **CLI** path: `models.json`, `setup.sh`, `new-user.sh`, `routes.sh`,
  `trace.sh`, themes, three `.md` docs. Pure bash plus one `.ts`, hitting only endpoints this repo
  already serves. Keep `examples/pi/extensions/governance.ts` **separate** from the widget's shim:
  the CLI extension talks HTTP to `/hooks/govern`, the shim talks over pi's own RPC channel. Two
  paths, deliberately.
- `demo/governance/09-pi-agent.sh`, plus a new `10-pi-widget.sh`.
- **The audit view** — `handlers/ssr/ssr_demo_trace/{mod,view}.rs`,
  `storage/files/admin/templates/demo-trace.hbs`, the `governance::demo_trace` queries, the sidebar
  entry, `.trace-*` CSS. No schema change needed; it is how you *see* that widget decisions land
  alongside CLI ones. Rename the repository fns on port to satisfy
  `scripts/check-repository-naming.sh` (`list_`/`find_`/`get_` by return type).

Note the template's own pi credentials point at `:8099` and will not validate against this repo's
database — mint fresh ones.

---

## 6. Sandboxing — read before widening the tool set

pi ships **no sandbox**; its tools run with the server process's permissions. `SP_PI_TOOLS=read` is
enforced by pi itself, which is a stronger guarantee than a policy evaluated after the fact.

Setting `SP_PI_TOOLS=bash,write,edit` on the current single-container deployment turns this into a
remote code execution service. The governance chain is not a sandbox: `bash` is not blocklisted by
default, and even a locked-down blocklist loses to `read` of `/proc/self/environ`. Human approval is
not a substitute either — it gates *intent*, not blast radius.

Enabling `bash` requires: one throwaway container per session, read-only rootfs with a writable
workspace mount, dropped capabilities, seccomp, and egress restricted to the gateway origin. Already
in place regardless: `env_clear`, per-session `HOME`/cwd outside the repo, 0600 credential file,
per-user cap, hard lifetime.

Not yet done and worth adding: `RLIMIT_NPROC`/`AS`/`FSIZE` via `pre_exec`, a dedicated low-privilege
uid, and a tmpfs workspace root.

---

## 7. Verification

```bash
just build && just publish && just start
# token
AT=$(./target/debug/systemprompt admin session login --token-only --profile local)
curl -sS -X POST localhost:8080/api/public/admin/users/<id>/pi-embed-token -H "Authorization: Bearer $AT"
# session, stream, prompt
CONV=$(curl -sS -X POST localhost:8080/api/public/pi/session -H 'content-type: application/json' \
  -d "{\"token\":\"$TOK\"}" | sed -n 's/.*"conversation_id":"\([^"]*\)".*/\1/p')
curl -N "localhost:8080/api/public/pi/stream/$CONV?token=$TOK" &
curl -sS -X POST localhost:8080/api/public/pi/prompt -H 'content-type: application/json' \
  -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"message\":\"read README.md\"}"
# expect approval_request on the stream, then deny it
curl -sS -X POST localhost:8080/api/public/pi/approve -H 'content-type: application/json' \
  -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"approval_id\":\"<id>\",\"decision\":\"deny\"}"
```

```sql
select session_id, tool_name, agent_id, decision, policy, reason
from governance_decisions where agent_id = 'pi_agent' order by created_at desc limit 10;
```

Expect a `human_approval` / deny row whose `session_id` joins `ai_requests`.

Negative tests — each must **deny**:

1. Kill the pi child mid-approval → nothing ran.
2. Proxy never answers → tool never executes; `approval timeout` audit row.
3. Close the browser mid-approval → `abandoned` after the 15s grace, not a full 120s wait.
4. Approve with another user's token → 401/404, tool stays blocked.
5. Two parallel tool calls → distinct `approval_id`s; approve one, deny the other, independent.
6. Prompt containing a fake `sp-live-…` secret → `secret_scan` denies at the prompt gate, **no**
   `ai_requests` row (it never reached a provider).
7. Two sessions as one user → second returns 429.
8. Idle 11 min → reaped, workspace directory gone, SSE closed.
9. Client sends `{"type":"bash",…}` shaped input → cannot reach pi as a command. Regression test for
   the shell-escape finding.

---

## 8. Later

- Per-conversation gateway PAT instead of one shared credential (attribution is already per-session
  via the attested `x-session-id`; the credential is the part still shared).
- Container per session, then `bash`.
- `fork` / `compact` / `set_model` from the widget.
- Transcript persistence, so a reconnect beyond the 200-frame replay ring can still catch up.
- Multi-viewer collaboration — the broadcast channel already supports it; only the UI is missing.
