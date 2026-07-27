# Access Control

Access control decides **who may reach which capability**. It is distinct from
the governance pipeline: governance inspects the *content and rate* of a call
(secrets, blocklisted names, throughput), while access control decides whether
an identity is even entitled to the entity (a skill, an MCP server, an agent, a
gateway route) in the first place.

## Roles and the bootstrap file

Role-scoped rules live in `services/access-control/roles.yaml`. This file is the
**bootstrap source of truth**: on startup the publish pipeline reads it and
upserts each rule into the `access_control_rules` table. Dashboard edits write
to the database only — there is no write-back to the file — so to make a rule
permanent across deployments you edit the file and redeploy.

Each rule names an `entity_type` (`marketplace`, `skill`, `mcp_server`, `agent`,
`gateway_route`, ...), targets it by `entity_id` or a glob `entity_match`, and
grants or denies a list of `roles`.

## Marketplace cascade and deny-overrides

Rules cascade. A grant on a **marketplace** flows down to every member skill,
agent, and MCP server that lacks its own rule. This is why most shipped skills
need no per-skill rule — they inherit the marketplace grant.

Denies win. An explicit `access: deny` on an entity overrides any inherited
allow (**deny-overrides**), so a capability can be catalogued by the marketplace
and still be unreachable for a given role.

This demo currently ships no per-skill deny, and the reason is worth stating: an
authz deny removes the skill from the catalog the caller sees, which in a chat
window is indistinguishable from the skill never having existed. Refusal is more
legible one layer down, where the call is issued and then visibly stopped — see
`governance-pipeline` and the `demonstrate_tool_rejection` skill. The two layers
are complementary, not alternatives: authz decides what you may hold, the policy
chain decides what you may do with it.

## Scopes at the gateway and MCP layer

OAuth scopes gate the endpoints. The supported scopes are `user` and `admin`;
`admin` implies `user`, so a resource opened to `user` is reachable by both. The
resource-hub MCP server is scoped to `user`, so every signed-in identity can
read the docs; admin-only servers keep `admin`.

## Where to go next

- `governance-pipeline` — the content and rate checks that run after entitlement.
- `audit-trail` — seeing allow and deny decisions after the fact.
