# The Audit Trail

Every inference call (`/v1/messages`) and every MCP tool call lands a row in the
same governance spine. There is one CLI surface for both — no separate "gateway
logs" versus "tool logs".

## The two views

- **`infra logs request list`** — one row per `/v1/messages` hit: user, model,
  token counts, cost, latency, status. This is the gateway path any
  Anthropic-SDK client uses.

  ```
  systemprompt infra logs request list --limit 20
  systemprompt infra logs request list --since 1h --provider anthropic
  ```

- **`infra logs trace list`** — MCP tool-call traces
  (PreToolUse → decision → spawn → result):

  ```
  systemprompt infra logs trace list --limit 20
  systemprompt infra logs trace list --status failed
  systemprompt infra logs trace show <trace-id>
  ```

Both are backed by the same 18-column tables sharing `user_id`, `tenant_id`,
`session_id`, and `trace_id`, so a full audit reconstructs the chain from
identity to cost:

```
systemprompt infra logs audit <request-id> --full
```

## Governance decisions

Every policy decision — allow or deny, with the policy that fired and its reason
— is recorded and queryable. This is how you prove that a `secret_scan` deny
actually happened and why.

## Operational versus dashboard

`infra logs` is for **triaging a live issue** — recent rows, quick aggregates.
`analytics` is for **reporting** — dashboard metrics over a time range with model
filtering, cache-hit rate, and CSV export:

```
systemprompt analytics costs
systemprompt analytics requests
systemprompt analytics agents
systemprompt analytics tools
```

Both read the same `ai_requests` table underneath. For live tailing while you
reproduce something, use `infra logs view --follow --since 30s`.

## Where to go next

- `governance-pipeline` — what generates these decisions.
- `getting-started` — make a call, then find it here.
