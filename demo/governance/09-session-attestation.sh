#!/bin/bash
# DEMO 9: SESSION ATTESTATION — THE GATEWAY ONLY ACCEPTS SESSIONS IT ISSUED
#
# The audit spine claims that ai_requests.session_id is evidence. That is only
# true if the caller cannot pick the id. This script proves it:
#
#   1. Issue a PAT via POST /api/v1/admin/api-keys (admin session JWT)
#   2. Mint a session:  POST /api/public/gateway/sessions  (PAT auth)
#   3. Call /v1/messages with a FABRICATED session id      -> asserted 401
#   4. Call /v1/messages with the ATTESTED session id      -> past auth
#   5. Show the session row and its source in user_sessions
#
# Step 4 deliberately asks for a model the profile does not expose, so the call
# is rejected at the allow-list (403) instead of spending upstream tokens. That
# is the point of the assertion: a 403 can only be reached *after* the session
# was attested, so it separates "auth accepted my session" from "the model gate
# said no". Set RUN_INFERENCE=1 to send a real (billed) inference instead.
#
# Cost: Free by default (no upstream call).

set -e

source "$(cd "$(dirname "$0")/.." && pwd)/_common.sh"

echo ""
echo "=========================================="
echo "  DEMO 9: SESSION ATTESTATION"
echo "  x-session-id must name a server-issued session"
echo "=========================================="
echo ""

# Why: demo/.token is the plugin/hook credential (aud=hook|plugin). Issuing an
# API key is an admin API route, so it needs an admin session JWT — mint one the
# same way every other admin-route demo does.
ADMIN_TOKEN=$("$CLI" admin session login --token-only --profile "$PROFILE" 2>/dev/null | tail -1)
if [[ -z "$ADMIN_TOKEN" ]]; then
  echo "  ✗ FAIL — could not mint an admin session token." >&2
  exit 1
fi

# ── 1. Issue a PAT ─────────────────────────────
echo "------------------------------------------"
echo "  1. Issue a personal access token"
echo "  POST $BASE_URL/api/v1/admin/api-keys"
echo "------------------------------------------"

PAT=$(curl -sS -X POST "$BASE_URL/api/v1/admin/api-keys" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  -d '{"name":"demo-session-attestation"}' | jq -r '.secret // empty')

if [[ -z "$PAT" ]]; then
  echo "  ✗ FAIL — could not issue an API key; is the server running and the token admin?" >&2
  exit 1
fi
echo "  Issued PAT: ${PAT:0:16}…"
echo ""

# ── 2. Mint an attested session ────────────────
echo "------------------------------------------"
echo "  2. Mint a session for that PAT"
echo "  POST $BASE_URL/api/public/gateway/sessions"
echo "------------------------------------------"

SESSION_ID=$(curl -sS -X POST "$BASE_URL/api/public/gateway/sessions" \
  -H "x-api-key: $PAT" | jq -r '.session_id // empty')

if [[ -z "$SESSION_ID" ]]; then
  echo "  ✗ FAIL — no session_id returned. Is gateway.enabled true in the profile?" >&2
  exit 1
fi
echo "  Minted session: $SESSION_ID"
case "$SESSION_ID" in
  sess_*) echo -e "  ${GREEN}✓ PASS${R} — server-issued id (sess_ prefix)" ;;
  *) echo -e "  ${RED}✗ FAIL${R} — unexpected session id shape: $SESSION_ID" >&2; exit 1 ;;
esac
echo ""

# Same request body throughout; only the session header changes.
_messages_status() {
  local session="$1" model="$2"
  curl -sS -o /tmp/sp-attest-body.$$ -w '%{http_code}' \
    -X POST "$BASE_URL/v1/messages" \
    -H "x-api-key: $PAT" \
    -H "x-session-id: $session" \
    -H "content-type: application/json" \
    -d "{\"model\":\"$model\",\"max_tokens\":16,\"messages\":[{\"role\":\"user\",\"content\":\"hello\"}]}"
}

# ── 3. Fabricated session -> 401 ───────────────
echo "------------------------------------------"
echo "  3. Call /v1/messages with a fabricated session id"
echo "------------------------------------------"

FAKE_SESSION="sess_$(uuidgen 2>/dev/null || echo "00000000-0000-0000-0000-000000000000")"
STATUS=$(_messages_status "$FAKE_SESSION" "claude-sonnet-5")
echo "  x-session-id: $FAKE_SESSION"
echo "  HTTP $STATUS — $(head -c 200 /tmp/sp-attest-body.$$)"
echo ""
assert_eq "$STATUS" "401" "fabricated session rejected"
echo ""

# ── 4. Attested session -> past auth ───────────
echo "------------------------------------------"
echo "  4. Call /v1/messages with the attested session id"
echo "------------------------------------------"

if [[ "${RUN_INFERENCE:-0}" == "1" ]]; then
  MODEL="${DEMO_MODEL:-claude-sonnet-5}"
  EXPECT=200
  echo "  RUN_INFERENCE=1 — sending a real, billed request to $MODEL"
else
  MODEL="not-a-real-model-for-this-profile"
  EXPECT=403
  echo "  Asking for an un-exposed model so no upstream tokens are spent."
fi

STATUS=$(_messages_status "$SESSION_ID" "$MODEL")
echo "  x-session-id: $SESSION_ID"
echo "  HTTP $STATUS — $(head -c 200 /tmp/sp-attest-body.$$)"
echo ""
assert_eq "$STATUS" "$EXPECT" "attested session accepted by auth"
if [[ "$EXPECT" == "403" ]]; then
  echo "  (403 is the model allow-list, reached only after attestation passed.)"
fi
rm -f /tmp/sp-attest-body.$$
echo ""

# ── 5. The row behind the id ───────────────────
echo "------------------------------------------"
echo "  5. The session row the gateway attested against"
echo "------------------------------------------"
cmd "systemprompt infra db query \"SELECT session_id, user_id, session_source, expires_at FROM user_sessions WHERE session_id = '$SESSION_ID'\""
cli_json infra db query \
  "SELECT session_id, user_id, session_source, expires_at FROM user_sessions WHERE session_id = '$SESSION_ID'" \
  | jq -r '.items[]? | "  \(.session_id)  source=\(.session_source)  expires=\(.expires_at)"'
echo ""

N=$(db_count "SELECT COUNT(*) FROM user_sessions WHERE session_id = '$SESSION_ID' AND session_source = 'api'")
assert_min "$N" 1 "user_sessions row minted with source=api"

echo ""
echo "=========================================="
echo "  Session attestation verified"
echo ""
echo "  A PAT caller cannot invent a session id: it mints one, and"
echo "  every /v1/messages row then joins to a real user_sessions row"
echo "  owned by that key's user."
echo "=========================================="
echo ""
