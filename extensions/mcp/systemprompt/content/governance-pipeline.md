# The Governance Pipeline

Every tool call runs a synchronous **four-stage** check before it executes. The
stages run in order, and the first deny short-circuits the rest. Configuration
lives in `services/governance/config.yaml`.

| Order | Stage | Policy id | What it blocks |
|-------|-------|-----------|----------------|
| 1 | Scope check | `scope_check` | A non-admin caller reaching for an admin-only tool prefix |
| 2 | Secret scan | `secret_scan` | Plaintext credentials in any tool input (35+ patterns), for any scope |
| 3 | Blocklist | `tool_blocklist` | Tool names matching a blocked pattern (`delete`, `drop`, `destroy`, `fetch_remote`) for user scope |
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

From a client with no shell, the same rows come back through the hub's own
`governance_stats` tool, scoped to the calling identity.

## Trying it live

Ask the resource hub to search for a topic: it passes all four stages and
records an `allow`. Then two denies you can reproduce without a terminal:

- **`secret_scan`** — search for a query containing a credential-shaped string.
  The PreToolUse governance hook denies it before the tool runs, for any scope.
- **`tool_blocklist`** — call `fetch_remote_docs`. This deployment does not
  permit outbound egress, so `fetch_remote` is a blocked pattern and the call is
  refused before any connection is attempted.

Both denies are audited alongside the allow. The `demonstrate_governance` skill
walks through this end to end, and `demonstrate_tool_rejection` takes the second
one on its own.

## Where to go next

- `audit-trail` — how to read the decisions and costs back out.
- `access-control` — how roles and marketplace grants decide who can call what.
