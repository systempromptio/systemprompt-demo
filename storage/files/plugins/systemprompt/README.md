# systemprompt.io — open gateway

A governed documentation hub and a set of explainer skills for Claude Desktop
and Cowork, delivered through the systemprompt.io open gateway. Every tool call
passes the four-stage governance pipeline and is audited.

## What's Inside

- **1 MCP server**: `systemprompt` — a documentation hub open to every signed-in
  user, exposing four read-only tools (`list_topics`, `get_topic`,
  `search_docs`, `governance_stats`) and one that policy refuses on purpose
  (`fetch_remote_docs`). Topics are also available as resources under
  `systemprompt://docs/<id>`.
- **4 skills**:
  - `explain_systemprompt` — introduces the product (list_topics → get_topic).
  - `demonstrate_governance` — the four-stage pipeline, two stages proven live.
  - `demonstrate_tool_rejection` — a tool call refused by policy, and the audit row.
  - `analyse_governance_stats` — reads back spend, latency, and verdicts.
- **HTTP hooks**: a PreToolUse governance hook that evaluates every tool call,
  plus tracking hooks for all events.

The skills are generated from `services/skills/` by
`scripts/generate-plugin-bundle.sh` — edit them there, not here.

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

Run the `demonstrate_governance` skill. It reads the pipeline topic, makes an
allowed call, then attempts a `search_docs` query carrying a credential-shaped
string. The PreToolUse governance hook denies it at the `secret_scan` stage, and
you can read the audited decision back with `systemprompt infra logs trace list`.

### 3. Watch a tool be refused outright

Run `demonstrate_tool_rejection`. It attempts `fetch_remote_docs`, an egress
tool this deployment does not permit, and `tool_blocklist` refuses it before any
connection is made. The denial is recorded next to the allows.

### 4. Read the spine back

Run `analyse_governance_stats`. Everything above — spend, latency, every verdict
with its policy and reason — comes back through `governance_stats`, scoped to
the calling identity.

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
3. **Tool blocklist** — blocks tool names matching a blocked pattern (delete,
   drop, destroy, fetch_remote) for non-admin scopes.
4. **Rate limiting** — caps tool calls per identity per window.
