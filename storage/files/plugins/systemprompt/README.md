# systemprompt.io — open gateway

A governed documentation hub and a set of explainer skills for Claude Desktop
and Cowork, delivered through the systemprompt.io open gateway. Every tool call
passes the four-stage governance pipeline and is audited.

## What's Inside

- **1 MCP server**: `systemprompt` — a documentation hub open to every signed-in
  user, exposing three read-only tools: `list_topics`, `get_topic`, and
  `search_docs`. Topics are also available as resources under
  `systemprompt://docs/<id>`.
- **5 skills**:
  - `explain_systemprompt` — introduces the product (list_topics → get_topic).
  - `explain_governance` — walks the governance pipeline and triggers a live deny.
  - `explore_systemprompt_docs` — free-form Q&A over the hub via search_docs.
  - `demonstrate_governance` — exercises all four governance stages and the audit.
  - `use_dangerous_secret` — a capability denied by access-control policy.
- **HTTP hooks**: a PreToolUse governance hook that evaluates every tool call,
  plus tracking hooks for all events.

## Install

```bash
claude plugin marketplace add https://github.com/systempromptio/systemprompt.git
```

## Setup

After installing, add your plugin token to Claude Code settings:

```bash
claude settings set env.SYSTEMPROMPT_PLUGIN_TOKEN "your-token-here"
```

Get your token by signing up at [systemprompt.io](https://systemprompt.io), or
let the Systemprompt Bridge configure Claude Desktop / Cowork for you via
device-link sign-in.

## Try It

### 1. Read the docs (allowed)

Ask Claude to explain what systemprompt.io is. It calls `list_topics` then
`get_topic` — governed, allowed, and audited.

### 2. Watch governance deny a secret (blocked)

Run the `explain_governance` skill. It reads the governance pipeline topic, then
attempts a `search_docs` query carrying a plaintext credential. The PreToolUse
governance hook denies it at the `secret_scan` stage, and you can read the
audited decision back with `systemprompt infra logs trace list`.

### 3. Access-control deny

Ask Claude to use the `use_dangerous_secret` skill. It is catalogued but denied
to the `user` role by policy (deny-overrides), so it never runs.

## How It Works

### HTTP Hooks

All hooks use `type: "http"` — Claude Code POSTs the event payload directly to
the platform endpoint. No shell scripts required.

- **Governance** (`PreToolUse`): synchronous hook calling `/api/public/hooks/govern`.
  Returns `allow` or `deny` with a reason.
- **Tracking** (all other events): async hooks calling `/api/public/hooks/track`.

### MCP Server

The `systemprompt` MCP server authenticates via OAuth (scope: `user`, so every
signed-in identity can reach it). Claude Code handles the OAuth flow
automatically when you first use one of its tools.

### Governance Rules

The governance endpoint evaluates four stages in order:

1. **Scope check** — enforces admin-only tool prefixes for non-admin scopes.
2. **Secret detection** — scans tool inputs for API keys, tokens, passwords.
3. **Tool blocklist** — blocks destructive operations (delete, drop, destroy)
   for non-admin scopes.
4. **Rate limiting** — caps tool calls per identity per window.
