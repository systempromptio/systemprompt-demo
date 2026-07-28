# Skills and the Marketplace

A connected AI client is useful immediately because systemprompt.io delivers
**skills** and **MCP servers** to it through a **marketplace**.

## Skills

A skill is a packaged set of instructions for a particular task — flat YAML plus
a Markdown body under `services/skills/<id>/`. Skills that ship with this demo
teach a client how to use the resource hub and how to see governance in action:

- **`explain_systemprompt`** — introduces the product by driving `list_topics`,
  `get_topic`, and `search_docs`, citing the topic id behind every claim.
- **`demonstrate_governance`** — explains all four stages and proves two of them
  live: a `secret_scan` deny on a credential-shaped query, and a
  `tool_blocklist` deny on an egress tool.
- **`demonstrate_tool_rejection`** — the refusal on its own: attempts
  `fetch_remote_docs`, is stopped before any connection, and reads the audited
  denial back.
- **`analyse_governance_stats`** — reads the caller's own spend, latency, and
  verdicts through `governance_stats` and explains what they show.

Each is deliberately runnable with hub tools alone, so the same body works in
Claude Desktop, in Cowork, and in the governed web terminal — none of which has
a shell.

## The marketplace

A marketplace (`services/marketplaces/<id>/config.yaml`) aggregates skills, MCP
servers, and agents by reference and controls who receives them via role-scoped
access. A grant on the marketplace cascades to its members, so a signed-in user
gets the whole bundle at once.

## The exported plugin

The marketplace is exported to a plugin bundle under
`storage/files/plugins/<id>/` — an `.mcp.json` pointing the client at the
gateway's MCP path plus copies of the member skills. This export is what makes a
freshly connected client immediately show the `systemprompt` MCP server and its
skills, with no manual configuration.

## Where to go next

- `getting-started` — the hub tools and your first call.
- `access-control` — how the marketplace grant and deny-overrides decide access.
