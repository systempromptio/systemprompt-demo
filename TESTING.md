# End-to-End Testing Guide: The systemprompt.io Demo Funnel

This document is the exact script for testing the full anonymous-visitor flow:
land on the site → register → onboard → receive the $5-credit email → download
the Systemprompt Bridge → sign in → use the demos in Claude Desktop / Cowork.
Every step lists what you should see, so any deviation is a bug.

---

## 0. Prerequisites

```bash
just build      # compiles the workspace (auto-runs DB migrations)
just start      # starts the API (port 8080), agents, and the MCP server
just publish    # prerenders the public pages into web/dist
```

Optional — real email delivery. Without these secrets the welcome email is a
logged no-op and onboarding still succeeds:

```bash
# .systemprompt/profiles/local/secrets.json (or env vars SMTP_HOST, SMTP_PORT,
# SMTP_USERNAME, SMTP_PASSWORD, SMTP_FROM, SITE_URL)
"smtp_host": "...", "smtp_port": "587", "smtp_username": "...",
"smtp_password": "...", "smtp_from": "systemprompt.io <no-reply@systemprompt.io>",
"site_url": "http://localhost:8080"
```

Verify services: `systemprompt infra services status` → 3/3 running.

---

## 1. The splash homepage

Open **http://localhost:8080/**. You should see:

- A hero with two CTAs: **Get started** (→ `/register`) and **Sign in** (→ `/login`).
- One section (`#get-started`) with exactly five numbered steps:
  1. Create your account with a passkey
  2. Tell us about yourself (30-second form)
  3. Check your email — $5 of credit
  4. Download the Systemprompt Bridge (Mac / Windows badges)
  5. Sign in on the Bridge — Claude Desktop / Cowork configured automatically
- No video, no feature grids. The showreel now lives at **/resources/**
  (linked from the header) as a YouTube embed.

Clean root URLs `/login`, `/register`, `/onboarding`, `/setup` all 307-redirect
to their `/admin/...` counterparts.

## 2. Register (passkey)

1. Click **Get started** → a single centered card: name, email, one
   **Create passkey** button. There is no role selector; every self-registration
   is a plain `user`.
2. Your browser/OS prompts to create a passkey (Touch ID / Windows Hello / a
   security key). Approve it.
3. On success you are signed in and redirected to **/onboarding**.

## 3. Onboarding form

A short form: username + email (prefilled, read-only), full name (required),
company and "what do you want to try" (optional). Submitting:

- marks you onboarded (the form is the only writer of `users.full_name`),
- grants **$5.00 (5,000,000 microdollars)** once — the grant is idempotent
  (`credit_grants` has `UNIQUE (user_id, reason)`),
- fires the welcome email in the background,
- redirects to **/setup?welcome=1** with a banner: *"Check your email — we've
  added $5 of credit."*

Verify the grant:

```bash
psql "$(jq -r .database_url .systemprompt/profiles/local/secrets.json)" \
  -c "SELECT user_id, microdollars, reason FROM credit_grants ORDER BY created_at DESC LIMIT 1"
```

## 4. The email

Subject: **"Your $5 systemprompt credit is ready"**. Body (HTML + plain-text):
you've been given $5 of credit to try systemprompt with Claude Desktop, with
three steps — download the Bridge, sign in with your code from `{site_url}/setup`,
open Claude Desktop / Cowork.

If SMTP is not configured, nothing is sent and onboarding still succeeds; the
send degrades to a logged no-op inside the spawned email task.

## 5. The setup page (`/setup`)

The page shows, in order:

1. **Download badges** —
   - macOS (Apple Silicon): `https://github.com/systempromptio/systemprompt-demo/releases/latest/download/systemprompt-bridge-aarch64-apple-darwin`
   - Windows: `https://github.com/systempromptio/systemprompt-demo/releases/latest/download/systemprompt-bridge-x86_64-pc-windows-msvc.exe`
2. **Generate sign-in code** — POSTs to `/admin/devices/bridge-code` and
   displays `{code, expires_at}` (a one-time device-link code, short expiry —
   regenerate if it lapses).
3. **PAT fallback** — POSTs to `/admin/devices/pats`; the response shows the
   secret **once**, format `sp-live-<12hex>.<secret>`. Use this if you prefer
   configuring a client manually instead of the bridge sign-in.
4. Three-step device-link instructions.

## 6. The Bridge app

Download and open the binary for your OS (macOS: you may need to allow it under
System Settings → Privacy & Security on first launch). The Bridge shows a
sign-in screen: paste the code from `/setup` (device-link). The bridge exchanges
it at `/auth/bridge/session-pat` for a durable PAT and then keeps a session
alive silently. Its host catalog offers **Claude Code, Claude Desktop, and
Cowork**; enabling a host writes that host's MCP configuration so the
`systemprompt` server is available immediately.

## 7. What you see in Claude Desktop / Cowork

One MCP server — **systemprompt** (the documentation hub, port 5010, proxied at
`/api/v1/mcp/systemprompt/mcp`) — enabled for **all** signed-in users, with
three tools and seven `systemprompt://docs/<id>` markdown resources:

| Tool | Input | Does |
|---|---|---|
| `list_topics` | `{}` | Lists the 7 documentation topics |
| `get_topic` | `{"topic_id": "..."}` | Full markdown for one topic |
| `search_docs` | `{"query": "..."}` | Keyword-ranked matches with excerpts |

Five skills ship in the marketplace (v3.0.0):
`explain_systemprompt`, `explain_governance`, `explore_systemprompt_docs`,
`demonstrate_governance`, `use_dangerous_secret`. Each explainer skill drives
the hub tools (`mcp__systemprompt__list_topics/get_topic/search_docs`).

Demo prompts to run in the client:

- "What is systemprompt?" → agent should call `list_topics` then
  `get_topic what-is-systemprompt`.
- "How does governance work here?" → `get_topic governance-pipeline`, then a
  live deny demonstration per the `demonstrate_governance` skill.
- Run the `use_dangerous_secret` skill → denied by the access-control rule in
  `services/access-control/roles.yaml` (the deny-overrides demo).

CLI smoke test of the same surface:

```bash
systemprompt plugins mcp call systemprompt list_topics --args '{}'
systemprompt plugins mcp call systemprompt get_topic --args '{"topic_id":"governance-pipeline"}'
systemprompt plugins mcp call systemprompt search_docs --args '{"query":"rate limit"}'
systemprompt infra logs trace list --limit 5     # every call above is audited
```

## 8. Credit enforcement

Every `/v1/messages` request through the gateway records its cost in
`ai_requests.cost_microdollars`; balance = grants − costs, enforced by a
gateway guard (30 s in-process cache, fails open on DB error).

```bash
# Watch usage
systemprompt analytics costs
systemprompt infra logs request list --limit 5

# Force exhaustion for a user, then make a request — expect HTTP 429:
#   {"type":"error","error":{"type":"api_error",
#    "message":"Credit exhausted. Your $5 systemprompt credit has been used up."}}
psql "$DB" -c "INSERT INTO credit_grants (user_id, microdollars, reason)
               VALUES ('<user_id>', -6000000, 'burn-test')"
# ... make a gateway request with that user's PAT → 429 within 30 s ...
psql "$DB" -c "DELETE FROM credit_grants WHERE reason='burn-test'"
# after ≤30 s (cache TTL) requests succeed again
```

Note: gateway requests require a valid `x-session-id` belonging to a real
`user_sessions` row (clients get one automatically; hand-rolled curl tests must
reuse a real session id or the audit row insert fails on the FK).

## 9. Known caveats

- **Core patch active**: root `Cargo.toml` has `[patch.crates-io]` pointing at
  `../systemprompt-core` (branch `gateway-request-guard`) for the
  `GatewayRequestGuard` inventory hook. Before landing, publish + bump core and
  re-comment the patch block (Critical Rule 1 in CLAUDE.md).
- **`plugins generate` reads `services/plugins/`**, which this repo doesn't
  have — the exported plugin under `storage/files/plugins/` is hand-maintained.
  Keep `.claude-plugin/marketplace.json`, `plugin.json`, and the exported
  `skills/` copies in sync with `services/skills/` when skills change.
- The exported top-level `.mcp.json` points at port 5000 (deployed layout);
  the local profile serves 8080. The bridge configures hosts against the real
  gateway URL, so this only matters for hand-written client configs.
- WebAuthn requires a secure context: `localhost` is fine; a LAN IP is not.
