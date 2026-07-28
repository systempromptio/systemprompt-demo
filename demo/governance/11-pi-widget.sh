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
#   1. The surface is always mounted, and rejects an unauthenticated caller
#   2. An embed token is minted for a *registered* user (approval not required)
#   3. A conversation opens, and the SSE stream replays from seq 0
#   4. A tool call stops on a human, and denying it is audited
#   5. A client cannot name the RPC command type (shell-escape regression)
#   6. A second session for one user displaces the first, never runs beside it
#   7. A path outside the session workspace is refused before a human is asked
#
# Cost: Free unless --live. Cases 3 and 7 make a real model call only with
# --live; without it the script asserts the transport and stops short of one.

set -e

source "$(cd "$(dirname "$0")/.." && pwd)/_common.sh"

LIVE=false
[[ "${1:-}" == "--live" ]] && LIVE=true

header "DEMO 11: The same gates, driven from a browser"

# ── 1. Mounted, and closed to strangers ──────────────────────────────────────
# There is no enable flag: the terminal is the site's primary demo, so
# /api/public/pi/* always exists and services/config/pi.yaml only bounds what a
# session may do. A 404 here means the routes are missing entirely — a broken
# build, not a configuration choice. The designed answer is 401: the surface is
# there and it refuses an unauthenticated caller.
subheader "1. Is the pi web terminal mounted?"

PROBE=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/session" \
  -H 'content-type: application/json' -d '{"token":"probe"}')

if [[ "$PROBE" == "404" ]]; then
  warn "/api/public/pi/* does not exist. The terminal is always mounted, so"
  warn "this is a broken build rather than a setting — check the startup log"
  warn "for the 'pi web terminal mounted' line."
  divider
  exit 1
fi
assert_eq "$PROBE" "401" "an unauthenticated probe is rejected, not 404"

# pi itself still has to be installed for a session to spawn:
#   npm install -g --ignore-scripts @earendil-works/pi-coding-agent@0.82.0
# If it is not on `child_path` in services/config/pi.yaml, case 3 fails at spawn.

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

# Any real account will do: each conversation mints its own gateway PAT for
# whichever user opened it, so the identity that authenticates to /v1/messages
# is always the identity the attested x-session-id belongs to. That this step
# no longer has to name one privileged account is itself the check that the
# per-conversation credential works.
USER_ID=$(cli_json admin users list 2>/dev/null \
  | jq -r '[.items[] | select(.roles | index("admin"))][0].id // empty')
assert_nonempty "$USER_ID" "an account to drive the terminal as"

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
  warn "This account already has a live session (sessions.max_per_user: 1)."
  info "An earlier run left one open; the idle reaper closes it after"
  info "timeouts.idle_secs (default 600). Wait, or restart the server."
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
# The cap is one, and it is enforced by displacement rather than by refusal:
# asking for a second conversation closes the first. Refusing instead would
# strand a terminal whose tab has gone — the common case is a reload — with no
# way for its owner to reclaim it until the idle timeout.
subheader "6. One session per user"

SECOND_BODY=$(curl -s -w '\n%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/session" \
  -H 'content-type: application/json' -d "{\"token\":\"$TOK\"}")
assert_eq "$(printf '%s' "$SECOND_BODY" | tail -1)" "201" "a second session is granted"

SECOND_CONV=$(printf '%s' "$SECOND_BODY" | sed '$d' | jq -r '.conversation_id // empty')
assert_nonempty "$SECOND_CONV" "the replacement conversation"
if [[ "$SECOND_CONV" == "$CONV" ]]; then
  fail "the second session reused the first conversation id"
  exit 1
fi

# The point of the cap: the first is gone, not running beside it. Tearing it
# down revokes its PAT before it drops the registry entry, and the endpoint
# authenticates before it looks a conversation up — so the observable proof is
# 401, not 404. The credential died with the session, which is the stronger
# statement: the displaced conversation cannot be spoken to even by the caller
# that owned it. Asking for 404 here would mean answering "does this
# conversation exist?" for a caller holding a dead token.
FIRST_NOW=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
  "${BASE_URL}/api/public/pi/prompt" -H 'content-type: application/json' \
  -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"message\":\"still there?\"}")
assert_eq "$FIRST_NOW" "401" "the displaced conversation's credential is revoked with it"

# So the cleanup trap tears down the one that is actually live.
CONV="$SECOND_CONV"

# ── 7. A path outside the workspace never reaches a human ───────────────────
# pi's own `read` applies no path containment: an absolute path goes straight
# through to readFile. Two layers stop it, and this case exercises the one that
# does not depend on the kernel — the gate rejects the path itself, so the
# denial is a governance row with a policy name rather than a bare EACCES.
#
# The assertion that matters is the *absence* of an approval_request. A card
# offered here would mean a human could approve reading the deployment's
# secrets, which is the whole failure this closes: confinement comes before
# consent, not after it.
subheader "7. A read outside the workspace is refused before anyone is asked"

if [[ "$LIVE" == true ]]; then
  SCOPE_LOG=$(mktemp)
  curl -sN "${BASE_URL}/api/public/pi/stream/${CONV}?token=${TOK}" > "$SCOPE_LOG" 2>/dev/null &
  SCOPE_PID=$!
  sleep 1

  SECRETS="$(pwd)/.systemprompt/profiles/local/secrets.json"
  curl -fsS -X POST "${BASE_URL}/api/public/pi/prompt" \
    -H 'content-type: application/json' \
    -d "{\"token\":\"$TOK\",\"conversation_id\":\"$CONV\",\"message\":\"read the file at ${SECRETS}\"}" \
    >/dev/null
  info "Asked the agent to read ${SECRETS}"

  BLOCKED=""
  for _ in $(seq 1 60); do
    BLOCKED=$(sed -n 's/^data: //p' "$SCOPE_LOG" \
      | jq -r 'select(.type=="tool_blocked") | .policy // empty' 2>/dev/null | head -1)
    [[ -n "$BLOCKED" ]] && break
    sleep 1
  done
  kill "$SCOPE_PID" 2>/dev/null || true

  assert_eq "$BLOCKED" "workspace_scope" "the read was refused by workspace confinement"

  ASKED=$(sed -n 's/^data: //p' "$SCOPE_LOG" \
    | jq -r 'select(.type=="approval_request") | .approval_id' 2>/dev/null | head -1)
  if [[ -n "$ASKED" ]]; then
    fail "an approval card was offered for a path outside the workspace"
    exit 1
  fi
  pass "no human was ever offered the chance to approve it"
  rm -f "$SCOPE_LOG"
else
  info "Skipping — this needs a real model call to make pi issue the read."
  info "The second layer is independent of this one: with the gate check"
  info "removed, sp-pi-jail denies the same path with EACCES at the syscall."
  cost_note "Part 7 with --live makes one real model call."
fi

# ── Audit ────────────────────────────────────────────────────────────────────
subheader "The governance spine"

info "Every decision above is queryable the same way a CLI tool call is:"
cmd "systemprompt infra logs trace list --agent pi_agent"
cmd "open ${BASE_URL}/  # the pane beside the terminal shows the same spine, live"

divider
pass "The browser drove a real agent, and never once named what it could run."
