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
allow (**deny-overrides**). The demo uses this deliberately: the
`use_dangerous_secret` skill is catalogued by the marketplace but carries a
`deny` rule for the `user` role, so a user-role attempt to run it is refused by
policy before it can execute — independently of the runtime `secret_scan`
governance hook. That is the access-control demonstration: a dangerous
capability that exists in the catalog but is denied by policy.

## Scopes at the gateway and MCP layer

OAuth scopes gate the endpoints. The supported scopes are `user` and `admin`;
`admin` implies `user`, so a resource opened to `user` is reachable by both. The
resource-hub MCP server is scoped to `user`, so every signed-in identity can
read the docs; admin-only servers keep `admin`.

## Where to go next

- `governance-pipeline` — the content and rate checks that run after entitlement.
- `audit-trail` — seeing allow and deny decisions after the fact.
