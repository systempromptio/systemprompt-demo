<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://systemprompt.io/files/images/logo.svg">
  <source media="(prefers-color-scheme: light)" srcset="https://systemprompt.io/files/images/logo-dark.svg">
  <img src="https://systemprompt.io/files/images/logo-dark.svg" alt="systemprompt.io" width="380">
</picture>

# Private, managed Claude Cowork. Five minutes from sign-up.

**Sign up, get $1 of credit, download the Bridge, and Claude Cowork and Claude Desktop are configured for you automatically.** Every request runs through a gateway you can see into: every prompt, every tool call, every cent, one audit trail. Most AI access is a black box someone else operates. This one shows you every row.

[![Built on systemprompt-core](https://img.shields.io/badge/built%20on-systemprompt--core-2b6cb0?style=flat-square)](https://github.com/systempromptio/systemprompt-core)
[![Template · MIT](https://img.shields.io/badge/template-MIT-16a34a?style=flat-square)](LICENSE)
[![Core · BSL--1.1](https://img.shields.io/badge/core-BSL--1.1-2b6cb0?style=flat-square)](https://github.com/systempromptio/systemprompt-core/blob/main/LICENSE)
[![Rust 1.75+](https://img.shields.io/badge/rust-1.75+-f97316?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![PostgreSQL 18+](https://img.shields.io/badge/postgres-18+-336791?style=flat-square&logo=postgresql&logoColor=white)](https://www.postgresql.org/)

[**systemprompt.io**](https://systemprompt.io) · [**Documentation**](https://systemprompt.io/documentation/) · [**Guides**](https://systemprompt.io/guides) · [**Discord**](https://discord.gg/wkAbSuPWpr)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="demo/recording/svg/output/dark/cap-secrets.svg">
  <source media="(prefers-color-scheme: light)" srcset="demo/recording/svg/output/light/cap-secrets.svg">
  <img src="demo/recording/svg/output/dark/cap-secrets.svg" alt="An AI agent attempts to exfiltrate a GitHub PAT through a tool call. The secret-detection layer denies the call before the tool process spawns. One row is written to the audit table." width="820">
</picture>

<sub>Not a diagram. A live capture: an agent tries to pass a GitHub PAT through a tool argument. Denied in under 5 ms, before the tool process spawns. One audit row. The model never saw the key.</sub>

</div>

---

This repository is the source of **demo.systemprompt.io**, the hosted demo of [systemprompt.io](https://systemprompt.io): a governed `/v1/messages` gateway you sign up for, plus the **Systemprompt Bridge** desktop app that connects Claude Cowork, Claude Desktop, and Claude Code to it. Built on [systemprompt-core](https://github.com/systempromptio/systemprompt-core), published on crates.io as [`systemprompt`](https://crates.io/crates/systemprompt).

## From landing page to governed Claude in five steps

The hosted demo lives at **[demo.systemprompt.io](https://demo.systemprompt.io)** (launching soon). This is the exact flow. No sales call, no credit card, no API key of your own: the $1 credit covers your usage.

1. **Create your account with a passkey.** Touch ID, Windows Hello, or a security key. No password to leak.
2. **Tell us about yourself.** A 30-second form.
3. **Check your email.** $1 of credit is waiting on your account.
4. **Download the Systemprompt Bridge** for [macOS (Apple Silicon)](https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-aarch64-apple-darwin-app.zip) or [Windows](https://github.com/systempromptio/systemprompt-demo/releases/download/bridge-v0.18.4/systemprompt-bridge-x86_64-pc-windows-msvc.exe).
5. **Sign in with a one-time code** from your setup page. The Bridge writes the MCP configuration for Claude Cowork, Claude Desktop, and Claude Code. You are done.

From that point, everything Claude does runs through your gateway. Prefer configuring a client by hand? The setup page also issues a personal access token, shown once.

## What you get inside Claude

Enable the Bridge and every signed-in user gets the **systemprompt** MCP server: a documentation hub with four read-only tools (`list_topics`, `get_topic`, `search_docs`, `governance_stats`) over seven topics — plus `fetch_remote_docs`, which policy refuses on purpose — and four marketplace skills that show the platform explaining and enforcing itself.

Try these prompts in Claude Cowork or Claude Desktop:

- **"What is systemprompt?"** The agent lists the topics and reads the answer from the hub.
- **"How does governance work here?"** It pulls the governance-pipeline topic, then demonstrates a live policy denial.
- **Run the `demonstrate_tool_rejection` skill.** It reaches for an egress tool the blocklist refuses, and the call is stopped before any connection is made. That refusal is the product working.
- **Run the `analyse_governance_stats` skill.** It reads back what the session just cost and how every call was judged.

## Why "private" is not a slogan here

Every inference request and every MCP tool call passes a synchronous four-stage pipeline before anything executes: scope check, secret scan (35+ credential patterns), blocklist, rate limit. Allow or deny, the decision lands in an audit row in PostgreSQL, linked from identity to agent to tool to cost.

- **Your keys cannot enter the model's context.** Credentials are decrypted from an encrypted store and injected into the tool subprocess environment only. The process that owns the LLM context never writes the value, and the secret scan denies any tool call that tries to smuggle one through arguments. The recording above is that denial happening.
- **Your usage is a query, not a mystery.** `systemprompt infra logs request list` shows every gateway request with model, tokens, cost, and latency. `systemprompt infra logs audit <id> --full` reconstructs one request end to end.
- **Your $1 is enforced at the gateway.** Cost is metered per request in microdollars. When the credit is gone, the gateway returns a clean 403 instead of a surprise bill.

<details>
<summary><strong>The pipeline, in one screen</strong></summary>

<br>

```
  Claude (Cowork / Desktop / Code)
      │
      ▼
  Governance pipeline  (in-process, synchronous, <5 ms p99)
      │
      ├─ 1. Identity & scope check
      ├─ 2. Secret detection      (35+ patterns: API keys, PATs, PEM, AWS)
      ├─ 3. Blocklist             (destructive operation categories)
      └─ 4. Rate limiting         (per session, role multipliers)
      │
      ▼
  ALLOW or DENY  →  audit row, always
      │
      ▼ (ALLOW)
  spawn tool process   credentials injected here, never in the LLM context
```

The gateway speaks the Anthropic wire format at `POST /v1/messages`, so any Anthropic-SDK client works unmodified. Model routing and provider configuration: [docs/gateway-routes.md](docs/gateway-routes.md).

</details>

## Or host the whole funnel yourself

This repository is the source of demo.systemprompt.io. You can run the entire funnel, from splash page to credit exhaustion, on your own machine. One difference from the hosted demo: locally there is no funded gateway behind you, so `setup-local` asks for your own AI provider key and inference is billed to it. The $1 credit mechanics still work, they just meter spend against your key.

```bash
git clone https://github.com/systempromptio/systemprompt-demo
cd systemprompt-demo
just setup-local      # prompts for a provider key, starts Docker Postgres
just build            # compiles the workspace, runs migrations
just start            # gateway + agents + MCP server on :8080
just publish          # prerenders the public pages
```

Open http://localhost:8080 and walk the five steps above against your own binary. The scripted governance and analytics demos live in [`demo/`](demo/README.md), and the Bridge source is in [`bridge/`](bridge/README.md).

`setup-local` grants no credit, so the gateway refuses inference until a grant exists — that is the exhaustion path, reached from the other side:

```sql
INSERT INTO credit_grants (id, user_id, microdollars, reason)
VALUES (gen_random_uuid(), '<user-id>', 1000000, 'local_dev');
```

You will need Docker, Rust 1.75+, [`just`](https://just.systems/), and at least one AI provider key. `systemprompt --help` covers the rest.

---

## Upgrading core

Two ways to depend on `systemprompt-core`, chosen by the `[patch.crates-io]`
blocks in `Cargo.toml` and `tests/Cargo.toml`:

```bash
# Published release from crates.io — patch blocks commented out (the default).
just core-bump X.Y.Z

# Local sibling checkout, for a core change that is not released yet —
# patch blocks uncommented in BOTH manifests, pins set to the core version.
just build
```

Either way the core version pin in both manifests must match the version you
are building against: with a stale pin the patch is dropped **silently** and
you keep building the published crates while believing you are testing local
core. Core ships its own migrations, so run the new binary once against your
database after a bump.

---

## License

**This template** is [MIT](LICENSE). Fork it, modify it, use it however you like.

**[systemprompt-core](https://github.com/systempromptio/systemprompt-core)** is [BSL-1.1](https://github.com/systempromptio/systemprompt-core/blob/main/LICENSE): free for evaluation, testing, and non-production use. Production use requires a commercial license. Each version converts to Apache 2.0 four years after publication. Licensing enquiries: [ed@systemprompt.io](mailto:ed@systemprompt.io).

---

<div align="center">

[![systemprompt.io](https://img.shields.io/badge/systemprompt.io-2b6cb0?style=for-the-badge)](https://systemprompt.io) &nbsp; [![Core](https://img.shields.io/badge/systemprompt--core-2b6cb0?style=for-the-badge)](https://github.com/systempromptio/systemprompt-core) &nbsp; [![Documentation](https://img.shields.io/badge/documentation-16a34a?style=for-the-badge)](https://systemprompt.io/documentation/) &nbsp; [![Guides](https://img.shields.io/badge/guides-f97316?style=for-the-badge)](https://systemprompt.io/guides) &nbsp; [![Discord](https://img.shields.io/badge/discord-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/wkAbSuPWpr)

<sub>Sign up. Spend the $1. Read your own audit trail. Then decide who should operate your AI layer.</sub>

</div>
