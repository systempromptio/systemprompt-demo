# Skills and the Marketplace

A connected AI client is useful immediately because systemprompt.io delivers
**skills** and **MCP servers** to it through a **marketplace**.

## Skills

A skill is a packaged set of instructions for a particular task — flat YAML plus
a Markdown body under `services/skills/<id>/`. Skills that ship with this demo
teach a client how to use the resource hub and how to see governance in action:

- **`explain_systemprompt`** — introduces the product by driving `list_topics`
  then `get_topic`.
- **`explain_governance`** — walks the four-stage pipeline live: reads the
  `governance-pipeline` topic, then triggers a `secret_scan` deny and reads the
  audited decision.
- **`explore_systemprompt_docs`** — answers free-form questions over the hub via
  `search_docs`.
- **`demonstrate_governance`** — exercises the pipeline end to end and
  reconstructs the audit trail.
- **`use_dangerous_secret`** — a capability that exists in the catalog but is
  denied to the `user` role by policy, demonstrating access-control
  deny-overrides.

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

- `getting-started` — the three hub tools and your first call.
- `access-control` — how the marketplace grant and deny-overrides decide access.
