# The Governance Pipeline

Every tool call runs a synchronous **four-stage** check before it executes. The
stages run in order, and the first deny short-circuits the rest. Configuration
lives in `services/governance/config.yaml`.

| Order | Stage | Policy id | What it blocks |
|-------|-------|-----------|----------------|
| 1 | Scope check | `scope_check` | A non-admin caller reaching for an admin-only tool prefix |
| 2 | Secret scan | `secret_scan` | Plaintext credentials in any tool input (35+ patterns), for any scope |
| 3 | Blocklist | `tool_blocklist` | Destructive tool names (`delete`, `drop`, `destroy`) for user scope |
| 4 | Rate limit | `rate_limit` | More than the configured calls per window for one identity |

## How scope is decided

Scope is derived from the **caller's live database roles**, not from any
`agent_id` in the payload. An `admin`-role caller is exempt from `scope_check`
and `tool_blocklist` — those two policies have an admin escape hatch. But
`secret_scan` and `rate_limit` apply to **every** identity, including admins.
That is deliberate: a leaked credential is dangerous no matter who pasted it.

## Everything is audited

Each decision — allow or deny — is written to the `governance_decisions` table
with the tool name, the agent, the policy that fired, the caller's scope, and
the reason. There is no silent path. You can reconstruct the full story of any
call from the audit spine:

```
systemprompt infra logs trace list --status failed
systemprompt infra logs trace show <trace-id>
```

## Trying it live

Ask the resource hub to search for a topic and it passes all four stages and
records an `allow`. Ask it to search for a query that contains a plaintext
credential and the client's PreToolUse governance hook denies it at the
`secret_scan` stage before the tool ever runs — and that deny is audited too.
The `explain_governance` skill walks through this end to end.

## Where to go next

- `audit-trail` — how to read the decisions and costs back out.
- `access-control` — how roles and marketplace grants decide who can call what.
