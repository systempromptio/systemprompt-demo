# Explain Governance

Explain how systemprompt.io's governance works — and then **prove it live**.
First read the canonical description from the documentation hub, then trigger a
real deny and read the audited decision back out.

## When to Use

Use this when someone asks how governance, policy enforcement, or the "four-stage
pipeline" works, or wants to see that it is real rather than described. It pairs
the official docs (via the `systemprompt` MCP hub) with a live demonstration.

## How to Use

### 1. Read the canonical description

```
mcp__systemprompt__get_topic {"topic_id": "governance-pipeline"}
```

Summarise the four stages in order — `scope_check`, `secret_scan`,
`tool_blocklist`, `rate_limit` — and note that `secret_scan` and `rate_limit`
apply to every identity, while `scope_check` and `tool_blocklist` exempt admins.

### 2. Show an allowed call

A normal hub call passes all four stages and records an `allow`:

```
mcp__systemprompt__search_docs {"query": "how are secrets blocked"}
```

### 3. Trigger a real deny

Attempt a `search_docs` call whose query carries a plaintext credential. The
client's PreToolUse governance hook posts the query to the govern endpoint,
which denies it at the `secret_scan` stage before the tool runs — even for an
admin caller, because `secret_scan` scans the whole `tool_input`.

Do **not** paste a live credential into the chat (the gateway would re-scan it
every turn and block the session). Use the out-of-band script that carries the
real test key:

```bash
# runs the deny end to end against real test credentials, out of band
bash demo/governance/06-secret-breach.sh
```

The call shape it sends (credential lives in the script):

```bash
curl -s -X POST "http://localhost:8080/api/public/hooks/govern?plugin_id=systemprompt" \
  -H "Authorization: Bearer $(cat demo/.token)" -H "Content-Type: application/json" \
  -d '{"hook_event_name":"PreToolUse","tool_name":"mcp__systemprompt__search_docs","agent_id":"developer_agent","session_id":"explain-gov","cwd":"/var/www/html/systemprompt-demo","tool_input":{"query":"docs mentioning <AWS_ACCESS_KEY>"}}'
# -> {"permissionDecision":"deny", "reason": "...secret detected: AWS Access Key..."}
```

### 4. Read the audited decision

Every decision — the allow and the deny — is recorded. Show it:

```bash
systemprompt infra db query "SELECT decision, tool_name, policy, reason, created_at FROM governance_decisions ORDER BY created_at DESC LIMIT 5"
systemprompt infra logs trace list --limit 5
```

Point out that the deny row names `policy = secret_scan` and the reason, tying
the enforcement back to the description from step 1.

## Related

- `demonstrate_governance` — exercise all four stages, not just the secret scan.
- `use_dangerous_secret` — a capability denied at the access-control layer instead.
