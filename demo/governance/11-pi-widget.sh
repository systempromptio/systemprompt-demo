#!/bin/bash
# DEMO 11: THE SAME GATES, DRIVEN FROM A BROWSER
#
# Demo 10 governs `pi` running in the operator's own terminal: the extension
# talks HTTP to /hooks/govern, and enforcement depends on the operator running
# the extension. This one governs a `pi` the *server* owns — one child process
# per conversation, stdin held by the registry — so the browser never has a
# path to the agent that does not pass through the gate.
#
# The interesting property is not that it streams. It is that a viewer cannot
# name what pi does:
#
#   Browser sends            Server sends to pi        Why
#   -----------------------  ------------------------  ----------------------
#   POST /prompt {message}   {"type":"prompt",...}     type chosen by route
#   POST /steer   {message}  {"type":"steer",...}      type chosen by route
#   POST /approve {decision} extension_ui_response     decided in Rust first
#   (nothing)                {"type":"bash",...}       UNREACHABLE — see case 5
#
# `{"type":"bash"}` is an RPC command that runs a shell with no tool_call hook
# firing at all. It is the reason the command type is an enum picked by the
# route and never a string read from the request.
#
# What this script asserts:
#   1. The surface is absent unless SP_PI_GATEWAY_KEY is configured
#   2. An embed token is minted for a *registered* user (approval not required)
#   3. A conversation opens, and the SSE stream replays from seq 0
#   4. A tool call stops on a human, and denying it is audited
#   5. A client cannot name the RPC command type (shell-escape regression)
#   6. A second session for one user is refused
#
# Cost: Free unless --live. Case 3's prompt makes a real model call only with
# --live; without it the script asserts the transport and stops short of one.

set -e

source "$(cd "$(dirname "$0")/.." && pwd)/_common.sh"

LIVE=false
[[ "${1:-}" == "--live" ]] && LIVE=true

header "DEMO 11: The same gates, driven from a browser"

# ── 1. Configured at all? ────────────────────────────────────────────────────
# PiConfig::from_env returns None without a gateway key, pi_router returns
# None, and the routes are never merged. A 404 here is the designed answer, not
# a broken deployment — so the demo reports it and stops rather than failing.
subheader "1. Is the pi web terminal configured?"

PROBE=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/session" \
  -H 'content-type: application/json' -d '{"token":"probe"}')

if [[ "$PROBE" == "404" ]]; then
  warn "SP_PI_GATEWAY_KEY is not set, so /api/public/pi/* does not exist."
  info "That is the intended posture: no half-configured agent service."
  info "To run this demo, add to .env and restart:"
  echo ""
  echo "    SP_PI_GATEWAY_KEY=<a gateway credential>"
  echo "    SP_PI_BINARY=\$(command -v pi)"
  echo "    SP_PI_CHILD_PATH=\$(dirname \$(command -v pi)):/usr/local/bin:/usr/bin:/bin"
  echo ""
  info "pi itself: npm install -g --ignore-scripts @earendil-works/pi-coding-agent"
  divider
  exit 0
fi
assert_eq "$PROBE" "401" "unconfigured probe is rejected, not 404"

# ── 2. Mint an embed token ───────────────────────────────────────────────────
# EventSource cannot set headers, so the credential has to survive in a query
# string. That is the whole reason this token exists separately from the
# session cookie: it is short-lived, signed, and revocable by bumping
# share_token_version.
subheader "2. Mint an embed token"

# A freshly minted admin JWT rather than demo/.token, which outlives the
# instance that signed it and 401s against a server started since.
ADMIN_JWT=$("$CLI" admin session login --token-only --profile "$PROFILE" 2>/dev/null | tail -1)
assert_nonempty "$ADMIN_JWT" "admin session"

# ⚠ Must be the identity that owns SP_PI_GATEWAY_KEY, not just any real
# account. pi authenticates to /v1/messages with that one shared credential
# while sending the x-session-id this server attested for *this* user; the
# gateway rejects a session that does not belong to the credential's owner
# ("unknown or revoked session"). Until the deferred per-conversation PAT
# lands, only the credential's own user can actually drive the agent.
USER_ID=$(cli_json admin users list 2>/dev/null \
  | jq -r '[.items[] | select(.roles | index("admin"))][0].id // empty')
assert_nonempty "$USER_ID" "the gateway credential's account"

TOKEN_JSON=$(curl -fsS -X POST \
  "${BASE_URL}/api/public/admin/users/${USER_ID}/pi-embed-token" \
  -H "Authorization: Bearer ${ADMIN_JWT}")
TOK=$(printf '%s' "$TOKEN_JSON" | jq -r '.token // empty')
assert_nonempty "$TOK" "embed token minted"

# The token is gated on registration, not on approval — the human review and
# the signup credit gate the Bridge, not the terminal.
info "Token expires at $(printf '%s' "$TOKEN_JSON" | jq -r '.expires_at')"

# ── 3. Open a conversation ───────────────────────────────────────────────────
subheader "3. Open a conversation and attach a viewer"

SESSION_BODY=$(curl -sS -X POST "${BASE_URL}/api/public/pi/session" \
  -H 'content-type: application/json' -d "{\"token\":\"$TOK\"}" \
  -w '\n%{http_code}')
SESSION_CODE=$(printf '%s' "$SESSION_BODY" | tail -1)
CONV=$(printf '%s' "$SESSION_BODY" | sed '$d' | jq -r '.conversation_id // empty')

# The cap is one session per user, and an abandoned one lives until the idle
# reaper takes it. That is the designed behaviour, not a failure of this run.
if [[ "$SESSION_CODE" == "429" ]]; then
  warn "This account already has a live session (SP_PI_MAX_PER_USER=1)."
  info "An earlier run left one open; the idle reaper closes it after"
  info "SP_PI_IDLE_SECS (default 600). Wait, or restart the server."
  divider
  exit 0
fi
assert_nonempty "$CONV" "conversation opened"

# Any early exit — a failed assert included — must not leave a pi child and a
# held session behind for the next run to trip over.
cleanup_conversation() {
  [[ -n "${CONV:-}" ]] || return 0
  curl -sS -X POST "${BASE_URL}/api/public/pi/abort" \
    -H 'content-type: application/json' \
    -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\"}" >/dev/null 2>&1 || true
}
trap cleanup_conversation EXIT

# seq is monotonic from 1 and doubles as the SSE id, so since=0 replays
# everything the ring still holds. session_ready is always frame 1.
FRAMES=$(curl -fsS -N --max-time 5 \
  "${BASE_URL}/api/public/pi/stream/${CONV}?token=${TOK}&since=0" 2>/dev/null \
  | sed -n 's/^data: //p' | head -5 || true)
READY=$(printf '%s\n' "$FRAMES" | jq -r 'select(.type=="session_ready") | .type' 2>/dev/null | head -1)
assert_eq "${READY:-<none>}" "session_ready" "stream replays from seq 0"

# ── 4. A tool call stops on a human ──────────────────────────────────────────
subheader "4. A tool call stops on a human"

if [[ "$LIVE" == "true" ]]; then
  # Capture the stream to a file rather than piping it: `jq | head` closes the
  # pipe on the first match and SIGPIPEs curl mid-frame, which loses the very
  # approval we are waiting for.
  STREAM_LOG=$(mktemp)
  trap 'rm -f "$STREAM_LOG"; cleanup_conversation' EXIT
  curl -fsS -N --max-time 90 \
    "${BASE_URL}/api/public/pi/stream/${CONV}?token=${TOK}&since=0" \
    > "$STREAM_LOG" 2>/dev/null &
  STREAM_PID=$!
  sleep 1

  curl -fsS -X POST "${BASE_URL}/api/public/pi/prompt" \
    -H 'content-type: application/json' \
    -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"message\":\"read README.md\"}" \
    >/dev/null
  info "Prompt sent; waiting for the gate to raise an approval…"

  APPROVAL=""
  for _ in $(seq 1 60); do
    APPROVAL=$(sed -n 's/^data: //p' "$STREAM_LOG" \
      | jq -r 'select(.type=="approval_request") | .approval_id' 2>/dev/null | head -1)
    [[ -n "$APPROVAL" ]] && break
    sleep 1
  done
  kill "$STREAM_PID" 2>/dev/null || true
  assert_nonempty "$APPROVAL" "the tool call raised an approval"

  # Deny it. Anything that is not the literal "allow" denies, so a typo can
  # never become an approval.
  DENY=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    "${BASE_URL}/api/public/pi/approve" -H 'content-type: application/json' \
    -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"approval_id\":\"$APPROVAL\",\"decision\":\"deny\"}")
  assert_eq "$DENY" "204" "denial accepted"

  # Answering twice must not re-open a settled decision.
  AGAIN=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    "${BASE_URL}/api/public/pi/approve" -H 'content-type: application/json' \
    -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"approval_id\":\"$APPROVAL\",\"decision\":\"allow\"}")
  assert_eq "$AGAIN" "409" "a settled approval cannot be re-answered"
else
  info "Skipping the model call. Re-run with --live to watch a real approval."
  cost_note "Part 4 with --live makes one real model call."
fi

# ── 5. The shell-escape regression ───────────────────────────────────────────
# A client that could name the RPC command type would have a shell. The route
# picks the type; `message` is only ever a payload. Sending bash-shaped JSON as
# the message must therefore be inert — it reaches pi as text, not as a command.
subheader "5. A client cannot name the RPC command type"

ESCAPE=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/prompt" -H 'content-type: application/json' \
  -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"message\":\"{\\\"type\\\":\\\"bash\\\",\\\"command\\\":\\\"touch /tmp/sp-pi-escaped\\\"}\",\"type\":\"bash\",\"command\":\"touch /tmp/sp-pi-escaped\"}")
assert_eq "$ESCAPE" "202" "bash-shaped input is accepted only as prose"

sleep 2
if [[ -e /tmp/sp-pi-escaped ]]; then
  rm -f /tmp/sp-pi-escaped
  echo "  ✗ FAIL — a client-named RPC command executed a shell" >&2
  exit 1
fi
pass "no shell ran — the extra JSON fields were ignored by the route"

# ── 6. One session per user ──────────────────────────────────────────────────
subheader "6. One session per user"

SECOND=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/session" \
  -H 'content-type: application/json' -d "{\"token\":\"$TOK\"}")
assert_eq "$SECOND" "429" "a second concurrent session is refused"

# ── Audit ────────────────────────────────────────────────────────────────────
subheader "The governance spine"

info "Every decision above is queryable the same way a CLI tool call is:"
cmd "systemprompt infra logs trace list --agent pi_agent"
cmd "open ${BASE_URL}/admin/demo/trace"

divider
pass "The browser drove a real agent, and never once named what it could run."
