# What is systemprompt.io?

systemprompt.io is an **open governance gateway** for AI agents. It sits between
your AI clients (Claude Desktop, Cowork, Claude Code, or any Anthropic-SDK
client) and the model providers, and it governs, audits, and meters every
inference call and every tool call that passes through it.

It is a **library, not a framework**: you embed and extend code you own. There
is no dynamic plugin loader — all customization is compile-time via the Rust
`inventory` crate, so what runs in production is exactly what you compiled and
reviewed.

## What it gives you

- **A single governed endpoint.** Point any Anthropic-SDK client at
  `/v1/messages` and every request is authenticated, scope-checked, secret-
  scanned, rate-limited, metered, and audited before it reaches a provider.
- **A governance spine.** Every inference call and every MCP tool call lands a
  row in the same audit tables, with a `trace_id` linking identity → agent →
  tool → result → cost. Nothing runs unaudited.
- **MCP servers, skills, and agents** delivered to your AI clients through a
  marketplace, so a connected client is configured and useful the moment it
  signs in.
- **Per-user cost tracking and hard credit enforcement** at the gateway, so
  spend is attributed to real identities and can be capped.

## Who it is for

Teams that want to standardize how their engineers and agents use AI: one place
to see what was asked, what it cost, who asked it, and which policy allowed or
denied it — without giving up self-hosting or control of their data.

## Where to go next

- `governance-pipeline` — the four-stage check that runs on every tool call.
- `architecture` — how the library is assembled and extended.
- `getting-started` — connect a client and make your first governed call.
