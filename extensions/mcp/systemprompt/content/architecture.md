# Architecture

systemprompt.io is assembled at **compile time**. There is no dynamic plugin
loader; extensions register themselves through the Rust `inventory` crate, so
the running system is exactly what you built and reviewed.

## The big picture

- **Thin entry point.** `src/main.rs` delegates to the published `systemprompt`
  core crates. For cross-repo work a sibling `../systemprompt-core` checkout is
  patched in via `[patch.crates-io]`, but the default is a normal crates.io
  dependency. Core is a **library you consume**, not a framework you run inside.
- **Extensions live in `extensions/`.** Each MCP server is its own crate under
  `extensions/mcp/*`; the web surface (page data and template rendering) lives
  in `extensions/web`. Each extension carries its own `Cargo.toml` and, where it
  runs SQL, its own `.sqlx/` offline query cache.
- **Configuration is flat YAML under `services/`.** The root
  `services/config/config.yaml` is a thin aggregator with an explicit
  `includes:` list. Unknown keys error loudly at load time
  (`#[serde(deny_unknown_fields)]`), so configuration drift surfaces
  immediately rather than being silently ignored.

## Data and isolation

- **Postgres is the system of record** for identity, audit, cost, and
  governance decisions. Every clone gets an isolated Docker Postgres — the
  container and volume names are derived from a hash of the repo path, so many
  checkouts share one host without colliding.
- **The governance spine** is a small set of tables (`ai_requests`, the trace
  tables, `governance_decisions`) sharing `user_id`, `tenant_id`, `session_id`,
  and `trace_id`. Because both inference and tool calls write here, one audit
  query reconstructs the chain from identity to cost.

## Deploy flow

Build the release binary, MCP servers, and web assets, then deploy. A
`publish_pipeline` job also runs automatically at server startup, so bootstrap
configuration (roles, marketplace grants) is reconciled on every boot.

## Where to go next

- `governance-pipeline` — what runs on every tool call.
- `skills-and-marketplace` — how capabilities reach connected clients.
