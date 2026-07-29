# Explain systemprompt.io

Give a full, confident explanation of what systemprompt.io is and why it is
worth having. This skill carries its own grounded context — condensed from the
deployment's documentation hub, with the topic id behind each section — so the
explanation is complete without a single tool call. Lead with the value; reach
for the hub only when the visitor wants to go deeper.

## When to Use

Use this when someone asks "what is systemprompt.io?", "what does this do?",
"why would I want this?", or wants an introduction. The deliverable is a clear
explanation of the system and its value, not a demonstration of tool use.

## Grounded context

Everything below is condensed from this deployment's own documentation topics.
Cite the bracketed topic id when a claim needs a source; each is readable in
full via `mcp__systemprompt__get_topic {"topic_id": "<id>"}`.

### The one-paragraph pitch `[what-is-systemprompt]`

systemprompt.io is an **open governance gateway** for AI agents. It sits
between AI clients — Claude Desktop, Cowork, Claude Code, any Anthropic-SDK
client — and the model providers, and it governs, audits, and meters every
inference call and every tool call that passes through. It is a **library, not
a framework**: code you embed, own, and extend. There is no dynamic plugin
loader — all customization is compile-time via the Rust `inventory` crate, so
what runs in production is exactly what was compiled and reviewed.

### The problem it solves `[what-is-systemprompt]`

Teams adopting AI agents lose three things at once: visibility (what was
asked?), attribution (who asked it, and what did it cost?), and control (what
stopped the call that should not have run?). systemprompt.io answers all three
in one place — one gateway where you can see what was asked, what it cost, who
asked it, and which policy allowed or denied it — without giving up
self-hosting or control of your data.

### The four pillars

1. **A single governed endpoint** `[what-is-systemprompt]` — point any
   Anthropic-SDK client at `/v1/messages`; every request is authenticated,
   scope-checked, secret-scanned, rate-limited, metered, and audited before it
   reaches a provider.
2. **The governance pipeline** `[governance-pipeline]` — four synchronous
   stages on every tool call, first deny short-circuits: scope check (non-admin
   callers cannot reach admin-only tools) → secret scan (35+ credential
   patterns in any tool input, no admin exemption) → blocklist (banned tool
   patterns such as `fetch_remote`) → rate limit (per-identity throughput).
   Scope comes from the caller's live database roles, never from what an agent
   claims about itself.
3. **The audit spine** `[audit-trail]` — inference calls and tool calls land in
   the same tables, sharing `user_id`, `session_id`, and `trace_id`, so one
   query reconstructs the chain from identity → agent → tool → result → cost.
   Every allow and every deny is recorded with the deciding policy and reason;
   there is no silent path.
4. **Delivery through a marketplace** `[skills-and-marketplace]` — skills, MCP
   servers, and agents reach a connected client as a bundle the moment it signs
   in, with role-scoped grants that cascade and explicit denies that override
   `[access-control]`. A fresh client is configured and useful with zero manual
   setup.

### Why the design is trustworthy `[architecture]`

Compile-time assembly means the attack surface is the reviewed binary, not a
plugin directory. Flat YAML configuration errors loudly on unknown keys, so
drift surfaces instead of hiding. Postgres is the system of record for
identity, audit, cost, and governance decisions. Cost is recorded per request
against a real identity at call time — which is what makes AI spend chargeable
and cappable, not estimated.

### The proof available in this very terminal

The conversation the visitor is having *is* the product running: the meters
above the terminal count calls judged and refused live, and the audit-trail
link opens the rows this session just wrote.

## How to answer

1. **Explain from the context above, in your own words.** Shape it to the
   visitor: an engineer gets the compile-time and audit-spine story; a lead
   gets attribution, spend caps, and policy control. Lead with the problem and
   the value, not the feature list.
2. **Cite topic ids** for load-bearing claims so the answer stays checkable.
3. **Go to the hub only on demand.** If the visitor asks for detail beyond
   this summary, or challenges a claim, read the relevant topic live and quote
   it:

   ```
   mcp__systemprompt__get_topic {"topic_id": "governance-pipeline"}
   mcp__systemprompt__search_docs {"query": "<the visitor's own words>"}
   ```

4. **Never fill gaps from memory.** If neither this context nor the hub covers
   it, say so plainly — on a governance product, a checkable "I don't know"
   beats a confident guess.
5. **Close with one offered next step**, matched to their interest:
   `demonstrate_governance` to watch enforcement fire, `governance_dashboard`
   to see the spine visualised, or `audit_this_session` for their own receipt.

## Related

- `demonstrate_governance` — the pipeline above, with two stages proven live.
- `governance_dashboard` — the audit spine as a live dashboard.
- `audit_this_session` — this session's own itemised bill and decision log.
- `browse_site_content` — what the site publishes right now, read live.
