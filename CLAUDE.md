# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# systemprompt.io

**Use the CLI to discover commands.** `systemprompt --help` is your starting point.

---

## Branching & Release Flow

**All work lands on `next`. Never push to `main`.**

`next` is the repository's default branch, so a fresh clone starts there. `main`
is protected by a ruleset that requires a pull request and grants **no bypass to
anyone** — a direct `git push origin main` is refused for agents, sessions and
repository admins alike. Protection is pinned to `main` by name, so moving the
default branch does not move it.

```
next   ← default branch. Every agent, every session. Push freely.
       Bar: it builds and it runs. Nothing else.
  ↓ `just gate` when you are ready, then `just promote` to open the release PR
       This is where fmt, clippy, the source gates, tests, deny and audit run.
main   ← protected, release-only. Tagged. Never pushed to directly.
```

**The bar for a push to `next` is that the code works.** `just build` compiles
and the thing you changed actually runs. That is the whole gate — do not run
`cargo fmt --check`, clippy, the source-gate scripts, `cargo deny`, `cargo
audit`, or the test workspace before pushing here, and do not hold a working
change back because one of them is red. Every one of those runs in the
pre-release cycle below, where a red result is meant to be found and fixed.
Running them per-push costs minutes each and gates nothing.

**Nothing runs the pre-release cycle for you.** There is no scheduled job and
nothing gating a push to `next`. The gates run when a person decides to run
them:

1. `just gate [REF]` — dispatches every gate workflow against the ref
   (default: the tip of `next`) and waits.
2. `just promote [SHA]` — freezes that commit on the `promote` ref and **opens**
   the release pull request onto `main`. It does not merge; you do.
3. Tag `main` once merged. Tags are not covered by the ruleset.

The commit is frozen on `promote` rather than the PR being headed at `next`
because a PR headed at `next` merges whatever `next` points at *when you merge
it* — anything pushed meanwhile would ride along ungated. That happened once
for real.

## Quick Start

```bash
# First-time setup: writes .systemprompt/profiles/local/, starts Docker Postgres,
# runs publish_pipeline. With no key arg, the CLI prompts for which provider to
# use; the chosen provider becomes ai.default_provider (others disabled) and the
# gateway default. Passing keys is non-interactive — the first becomes default.
just setup-local                                                          # interactive provider pick
just setup-local <anthropic_key> [openai_key] [gemini_key] [http_port=8080] [pg_port=5432]

# Build (auto-uses live DB if reachable, else SQLX_OFFLINE=true)
just build            # debug
just build --release  # release

# Lint (workspace, -D warnings, same offline fallback as build)
just clippy

# Regenerate .sqlx/ offline query cache (needs live DB)
just prepare

# Start services
just start

# Discover CLI commands
systemprompt --help

# List skills
systemprompt core skills list
```

---

## CLI Structure

```
systemprompt <domain> <subcommand> [args]
```

| Domain | Purpose |
|--------|---------|
| `core` | Skills, content, files, contexts, plugins, hooks, artifacts |
| `infra` | Services, database, jobs, logs |
| `admin` | Users, agents, config, setup, session |
| `cloud` | Auth, deploy, sync, secrets, tenant, domain |
| `analytics` | Overview, conversations, agents, tools, requests, sessions, content, traffic, costs |
| `web` | Content-types, templates, assets, sitemap, validate |
| `plugins` | Extensions, MCP servers, capabilities |
| `build` | Build core workspace and MCP extensions |

**Use `systemprompt <domain> --help` to explore any domain.**

---

## CLI Discovery Workflow

When you need to perform a task, use the CLI help to find the right command:

```bash
# Top-level help
systemprompt --help

# Domain help
systemprompt core --help
systemprompt infra --help

# Subcommand help
systemprompt core skills --help
systemprompt core skills show --help
```

---

## Architecture (big picture)

- `src/main.rs` is a thin entry point that delegates to the published `systemprompt` core crates (sibling checkout at `../systemprompt-core`, patched in via `[patch.crates-io]` for cross-repo work). All customization is **compile-time** via the [`inventory`](https://docs.rs/inventory) crate — there is no dynamic plugin loader.
- Rust code lives in `extensions/`: `extensions/mcp/*` for MCP server extensions, `extensions/web` for page data and template rendering. Each MCP extension has its own crate with `Cargo.toml` + `.sqlx/` offline cache.
- Configuration is YAML under `services/`, loaded through `services/config/config.yaml`'s explicit `includes:` list. Unknown keys error loudly (`#[serde(deny_unknown_fields)]`).
- Governance runs as a four-stage synchronous pipeline on every tool call: **scope check → secret scan (35+ patterns) → blocklist → rate limit**. Every decision is audited to Postgres with a trace_id linking identity → agent → tool → result → cost.
- Per-clone Docker Postgres: `just db-up / db-down / db-logs [tenant=local]`. Project name is derived from a hash of the repo path, so multiple clones on one host get isolated containers and volumes. There is no destructive reset recipe — recover migration checksum drift in place with `just repair-migrations`.
- Deploy flow: `just deploy` — one command. It depends on `build-all` (release binary + MCP servers + web assets), so there is no separate build step to remember. The `publish_pipeline` job also runs automatically at server startup.
- `bridge/` is the **Systemprompt Bridge** desktop app (Windows/macOS) — a standalone Cargo workspace, NOT a member of the root workspace (`exclude = ["tests", "bridge"]`). It path-depends on `../systemprompt-core/bin/bridge` (core's bridge is `publish = false`, so a sibling core checkout is required to build it). Build with `just bridge-build` or `cd bridge && cargo build --release`. It supplies only a `Brand` const + assets; all behaviour lives in core. Device-link sign-in hits this repo's `/bridge-auth` mount. Released via `.github/workflows/release-bridge.yml` on `bridge-v*` tags. Out of the box the catalog offers the Claude hosts only (Claude Code / Claude Desktop / Cowork); Codex is not shipped.

---

## Debugging & Troubleshooting

```bash
# Quick error check
systemprompt infra logs view --level error --since 1h

# Debug AI request failures
systemprompt infra logs request list --limit 10
systemprompt infra logs audit <request-id> --full

# Debug MCP tool failures
systemprompt plugins mcp logs <server-name>

# Debug agent issues
systemprompt infra logs trace list --agent <agent-name> --status failed
```

**Key debugging workflow:**
1. `infra logs view --level error` — Find the error
2. `infra logs request list` — Find failed AI requests
3. `infra logs audit <id> --full` — Get full conversation context
4. `plugins mcp logs <server>` or `logs/mcp-*.log` — Get MCP tool errors

---

## Viewing Governance

Every inference call (`/v1/messages`) and every MCP tool call lands a row in the governance spine. Same CLI surface for both — no separate "gateway logs" vs "tool logs":

```bash
# Every AI request — user, model, token counts, cost, latency, status
systemprompt infra logs request list --limit 20
systemprompt infra logs request list --since 1h --provider anthropic   # request list filters: --since / --model / --provider (no --status)
systemprompt infra logs trace list --status failed          # only failed runs — --status lives on trace list, not request list

# Full audit for one request — identity, policy evals, prompt, response, cost
systemprompt infra logs audit <request-id> --full

# Tool-call traces (PreToolUse → decision → spawn → result)
systemprompt infra logs trace list --limit 20
systemprompt infra logs trace list --agent <name> --status failed
systemprompt infra logs trace show <trace-id>

# Cost + usage rollups (hits the same audit table)
systemprompt analytics costs
systemprompt analytics requests
systemprompt analytics agents
systemprompt analytics tools
```

`logs request list` shows one row per `/v1/messages` hit — the gateway path Cowork / any Anthropic-SDK client uses. `logs trace list` shows MCP tool calls. Both are backed by the same 18-column `ai_requests` / trace tables with `user_id`, `tenant_id`, `session_id`, `trace_id` — so `audit <id> --full` reconstructs the chain from identity to cost.

**`infra logs` vs `analytics` — operational vs dashboard.** The `infra logs request {list,stats}` commands are quick operational views (recent rows, by-provider / by-model aggregate). Their `analytics requests {list,stats}` counterparts are dashboard metrics over a time range with model filtering, cache-hit rate, and CSV export. Same `ai_requests` table underneath — reach for `infra logs` when triaging a live issue, `analytics` when reporting. The `--help` on each cross-references the other.

For live tailing while reproducing an issue: `infra logs view --follow --since 30s`.

---

## Services Configuration

All runtime configuration lives as flat YAML files under `services/`. The root `services/config/config.yaml` is a thin aggregator with an explicit `includes:` list — every resource file must be listed.

```
services/
  config/config.yaml        Root aggregator (includes all resource files)
  agents/<id>.yaml          Flat agent definitions
  mcp/<name>.yaml           Flat MCP server definitions
  skills/<id>.yaml          Flat skill definitions
  skills/<id>.md            Skill instruction bodies (referenced via !include)
  plugins/<name>.yaml       Flat plugin binding descriptors
  ai/config.yaml            AI provider config
  scheduler/config.yaml     Job scheduler
  web/config.yaml           Web frontend config (full WebConfig)
  content/config.yaml       Content source config
```

Unknown YAML keys cause loud errors at load time (`#[serde(deny_unknown_fields)]`). Nested `includes:` resolve recursively. Plugin YAMLs are binding descriptors that reference top-level agents, skills, mcp servers, and content sources by id — never inline copies.

---

## Critical Rules

0. **Load `development:rust-coding-standards` before writing Rust** — mandatory for every agent and subagent, before creating or editing any `.rs` file. Invoke it with the Skill tool first; don't write Rust from memory of the conventions. Spawned subagents that touch Rust must be told to load it too. Its style rules
   apply here in full; its "Validation Workflow — before committing any code"
   checklist does **not**. On `next` this repo's bar is the one in
   [Branching & Release Flow](#branching--release-flow): it builds and it runs.
   Those gates belong to `just gate`.
1. **`next` builds against the sibling core checkout** — `[patch.crates-io]` in `Cargo.toml` is **live on `next`**, routing every `systemprompt-*` crate to `../systemprompt-core` (itself on its own `next`). Both repos move together: a core change is picked up by a rebuild here, with no crates.io release in between. `Dockerfile`'s `CORE_REV` pins the commit CI and the image build use — bump it, run `just prepare`, and commit the refreshed `.sqlx` in the same change. Re-comment the patch block only for a release that must build from crates.io alone.
2. **Rust code -> `extensions/`** — All `.rs` files live here.
3. **Config only -> `services/`** — YAML/Markdown only. No Rust code.
4. **CSS files -> `storage/files/css/`** — NEVER put CSS in `extensions/*/assets/css/`.
5. **Brand name is `systemprompt.io`** — Use "systemprompt.io" for display and URLs.
6. **It's a library, not a framework** — Embedded code you own and extend. NEVER call it a "framework".
7. **Demo scripts must work on macOS and Linux** — BSD vs GNU differ on `grep -oP`, `head -n -1`, `sha256sum`, `sed -i`, and binary downloads (pick `hey_darwin_amd64` vs `hey_linux_amd64`). `demo/_common.sh` provides `install_hey()` for the last case; prefer `grep -oE` + `sed -n 's/.../\1/p'` over `grep -oP … \K …`.
8. **No Co-Authored-By in commits** — `coauthorAttribution: false` is set in `.claude/settings.json`. Never add `Co-Authored-By:` trailers to commit messages.

---

## Database

**Postgres-only, by design.** Repositories take `PgPool` and use compile-time-checked
`sqlx::query!` macros against the committed `.sqlx/` offline caches; `DbPool`
(`Arc<Database>`, from core) is the handle extensions receive at their wiring seams.
Do not abstract repositories over the backend — portability would cost the
compile-time query verification and the offline-cache workflow that gate this repo.

---

## GeoIP (country analytics) — opt-in, off by default

`analytics traffic geo` and the country column on `user_sessions` come from a
MaxMind GeoLite2-City database resolved at boot from `paths.geoip_database`.
It is **off in a fresh clone and stays off**: `setup-local` doesn't fetch it,
the Dockerfile doesn't download it, and the generated profiles leave
`geoip_database: null`. GeoLite2 needs the operator's own MaxMind account, so
nothing here obtains one on their behalf. No database = a startup notice and a
NULL country, never a failure.

To enable it, as this deployment does:

```bash
export MAXMIND_LICENSE_KEY=...   # https://www.maxmind.com/en/geolite2/signup
just geoip                       # or: just geoip --mirror (CC BY-SA redistribution)
# then set paths.geoip_database in the profile and restart
```

The ~60 MB `.mmdb` is gitignored. `cloud deploy` generates a Dockerfile that
does `COPY storage /app/storage`, so `storage/geoip/GeoLite2-City.mmdb` in the
working copy ships with `just deploy` — that plus `paths.geoip_database` in
`.systemprompt/profiles/production/` (gitignored too) is the whole production
wiring, with nothing landing in the public template.

Country attribution needs the *client* IP, so it only works behind a proxy when
`security.trusted_proxies` covers that proxy — the production profile lists the
Cloudflare and Fly ranges; local trusts nothing, so local sessions record
`127.0.0.1` with no country.

---

## Repository Naming Convention

Every function under `extensions/web/admin/src/repositories/` is named for what
it returns, so a call site reads the same as its signature:

| Returns | Prefix | Example |
|---------|--------|---------|
| `Vec<T>` — zero or more rows | `list_` | `list_top_users` |
| `Option<T>` — a row that may be absent | `find_` | `find_session_header` |
| `T` — exactly one value, or an error | `get_` | `get_request_stats` |
| a page plus its total, `(Vec<T>, i64)` | `list_` | `list_requests_paged` |

Mutations keep the verb that describes them: `insert_`, `update_`, `delete_`,
`set_`, `count_`.

`scripts/check-repository-naming.sh` enforces this: it rejects `fetch_`
outright, and checks every other prefix against the function's actual return
type, so the table above cannot quietly stop being true.

`fetch_` is banned because it is not a synonym for the three above —
it was doing all three jobs at once, which is how the convention drifted: a
reader could not tell from `fetch_summary` whether an absent row was `None` or
an error, and had to open the file to find out.

---

## CSS Files (IMPORTANT)

**All CSS files go in `storage/files/css/`** and must be registered in `extensions/web/src/extension.rs`.

```
storage/files/css/          <- CSS SOURCE (put files here)
extensions/web/src/extension.rs  <- REGISTER here in required_assets()
web/dist/css/               <- OUTPUT (generated, never edit)
```

**To add CSS:**
1. Create file in `storage/files/css/`
2. Register in `extension.rs` `required_assets()`
3. `just publish` to compile templates, bundle CSS/JS, and copy all assets to `web/dist/`

---

## Publishing Assets

After changing templates, CSS, JS, or static files, run:

```bash
just publish
```

This runs (in order): `bundle_admin_css` -> `bundle_admin_js` -> `copy_extension_assets` -> `content_prerender`. Order matters — bundles must be built before `copy_extension_assets` copies them to `web/dist/`. Admin pages are SSR'd at runtime from `.hbs` templates in `storage/files/admin/templates/`, not precompiled.

**Exception: the public-site partials are compiled into the binary.** `services/web/templates/partials/{head-assets,header,footer,scripts}.html` are `include_str!`-embedded by `extensions/web/site/src/partials.rs`. Editing them requires a rebuild (`just build`) and a server restart before `just publish` — running publish alone keeps serving the markup baked into the old binary.

---

## Plugins

Plugins are flat YAML files under `services/plugins/<name>.yaml` that aggregate agents, skills, mcp servers, and content sources by reference:

```yaml
plugins:
  systemprompt:
    id: systemprompt
    name: "systemprompt.io"
    version: "2.0.0"
    enabled: true
    agents:
      include: []
    skills:
      include:
        - explain_systemprompt
        - demonstrate_tool_rejection
    mcp_servers: []
    content_sources: []
```

Every id listed must resolve to a real top-level resource in `services/`. `ServicesConfig::validate()` enforces this at load time.
