---
name: "Demonstrate Governance"
description: "Exercise the four-stage governance pipeline - scope check, secret scan, blocklist, rate limit - through the systemprompt MCP hub, then read back the audited decisions"
---

# Demonstrate Governance

Drive every stage of the governance pipeline end to end using the `systemprompt`
MCP documentation hub, then prove that each decision was audited. This is the
guided tour of the enforcement spine: the same four stages run on every tool
call in the workspace, including the hub's own `list_topics`, `get_topic`, and
`search_docs`.

## When to Use

Use this skill to show, in one sitting, that the four governance policies
actually fire and that every allow and deny lands an auditable row. The
`explain_governance` skill is the narrated, single-deny walkthrough; this skill
exercises all four stages and reconstructs the full audit trail behind them.

## The pipeline

Every tool call runs a synchronous four-stage check before it executes (config
in `services/governance/config.yaml`):

| Stage | Policy id | What it blocks |
|-------|-----------|----------------|
| Scope check | `scope_check` | A non-admin caller reaching for an admin-only tool prefix (`mcp__admin__*`) |
| Secret scan | `secret_scan` | Plaintext credentials in any tool input (35+ patterns), any scope |
| Blocklist | `tool_blocklist` | Destructive tool names (`delete`, `drop`, `destroy`) for user/non-admin scope |
| Rate limit | `rate_limit` | More than 300 calls per 60s for one identity |

Scope is derived from the **caller's live DB roles**, not the `agent_id` in the
payload. `admin`-role callers are exempt from `scope_check` and `tool_blocklist`
(they are the policies' admin escape hatch); `secret_scan` and `rate_limit`
apply to every identity. To prove a real `scope_check` / `tool_blocklist` deny
you therefore need a **user-scope** token, not the admin `demo/.token` (see the
deny recipe below).

Each decision is written to the `governance_decisions` table with the tool, the
agent, the policy, and the reason.

## How to Use

### 1. The allowed path

A normal, in-scope hub call passes all four stages. The `systemprompt` MCP hub
is open to every signed-in user, so any of its tools executes and records an
`allow` row:

```bash
systemprompt plugins mcp call systemprompt list_topics --args '{}'
systemprompt plugins mcp call systemprompt search_docs --args '{"query":"governance pipeline"}'
```

### 2. The secret-scan deny

Now put a plaintext credential inside a tool input. The `secret_scan` stage
denies it before execution — even for an admin caller — because it scans the
whole `tool_input` payload. The natural way to trigger it here is a
`search_docs` query that contains a fake AWS key: the client's PreToolUse
governance hook posts the query to the govern endpoint, which denies it.

The runnable recipe with real test credentials is
`demo/governance/06-secret-breach.sh` (out-of-band `curl`, so the secret never
enters this conversation — do **not** paste a live credential prefix into this
skill body or any chat turn, or the gateway secret scanner will re-scan it on
every turn and block the session). The shape of the call (live credential lives
in the script):

```bash
# secret_scan deny: a plaintext AWS key inside a search_docs query, denied for any scope
curl -s -X POST "http://localhost:8080/api/public/hooks/govern?plugin_id=systemprompt" \
  -H "Authorization: Bearer $(cat demo/.token)" -H "Content-Type: application/json" \
  -d '{"hook_event_name":"PreToolUse","tool_name":"mcp__systemprompt__search_docs","agent_id":"developer_agent","session_id":"demo-secret","cwd":"/var/www/html/systemprompt-demo","tool_input":{"query":"find docs mentioning <AWS_ACCESS_KEY>"}}'
# -> {"permissionDecision":"deny", "reason": "...secret detected: AWS Access Key..."}
```

### 3. The scope-check and blocklist denies

These two policies exempt admins, so use the **user-scope** token
(`demo/.token.user`, provisioned by `demo/00-preflight.sh`), not the
admin `demo/.token`.

```bash
# scope_check deny: a user-scope caller reaching for an admin-only tool prefix
curl -s -X POST "http://localhost:8080/api/public/hooks/govern?plugin_id=systemprompt" \
  -H "Authorization: Bearer $(cat demo/.token.user)" -H "Content-Type: application/json" \
  -d '{"hook_event_name":"PreToolUse","tool_name":"mcp__admin__reset_tenant","agent_id":"associate_agent","session_id":"demo-scope","cwd":"/var/www/html/systemprompt-demo"}'
# -> {"permissionDecision":"deny", ...}   (policy=scope_check, user scope, admin-only prefix)

# tool_blocklist deny: a destructive tool name blocked for user scope. Use a
# NON-admin-prefixed name (delete_records) so scope_check passes and the deny
# genuinely reads policy=tool_blocklist.
curl -s -X POST "http://localhost:8080/api/public/hooks/govern?plugin_id=systemprompt" \
  -H "Authorization: Bearer $(cat demo/.token.user)" -H "Content-Type: application/json" \
  -d '{"hook_event_name":"PreToolUse","tool_name":"delete_records","tool_input":{"table":"users"},"agent_id":"associate_agent","session_id":"demo-blocklist","cwd":"/var/www/html/systemprompt-demo"}'
# -> {"permissionDecision":"deny", "reason": "...blocked by list delete"}   (policy=tool_blocklist)
```

Sending the two scope/blocklist requests with the admin `demo/.token` returns
`allow` — admins are exempt from those two policies (`secret_scan` still denies
for any scope, as step 2 shows).

### 4. Read back the audited decisions

Every outcome is now in the spine. Query it directly, or use the trace CLI:

```bash
systemprompt infra db query "SELECT decision, tool_name, agent_id, agent_scope, policy, reason FROM governance_decisions ORDER BY created_at DESC LIMIT 10"
systemprompt infra logs trace list --limit 10
systemprompt infra logs trace list --status failed
```

### 5. Tie enforcement to spend per identity

```bash
systemprompt analytics costs breakdown --by agent
```

#### Two distinct rate limiters (do not conflate them)

There are **two** independent limiters; only the first is the governance stage
in the table above:

- **Governance `rate_limit` policy** — per-identity, 300 calls / 60s, configured
  in `services/governance/config.yaml`. This is the pipeline stage; its evidence
  lives in the audit table:

  ```bash
  systemprompt infra db query "SELECT decision, tool_name, reason, created_at FROM governance_decisions WHERE policy = 'rate_limit' ORDER BY created_at DESC LIMIT 10"
  ```

- **HTTP profile limiter** — a separate request limiter shown by
  `systemprompt admin config rate-limits show`. It guards the HTTP surface, is
  configured in the profile, and is **disabled in the local profile**. It is
  *not* the governance `rate_limit` policy and writes no `governance_decisions`
  rows.

### Typical workflow

1. Run the allowed hub calls (step 1) — confirm they execute and record `allow`.
2. Attempt the secret (step 2) — confirm it is denied for any scope.
3. Attempt the scope/blocklist denies with the user token (step 3).
4. `infra db query` / `infra logs trace list` (step 4) — see every row with its
   policy and reason.
5. `analytics costs breakdown --by agent` (step 5) — tie enforcement to spend.
