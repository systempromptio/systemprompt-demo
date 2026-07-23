---
name: "Explain systemprompt.io"
description: "Explain what systemprompt.io is by reading the official docs live through the systemprompt MCP hub (list_topics then get_topic)"
---

# Explain systemprompt.io

Explain what systemprompt.io is and why it exists, sourcing every claim from the
**live documentation hub** rather than memory. The hub is served by the
`systemprompt` MCP server and is open to every signed-in user.

## When to Use

Use this when someone asks "what is systemprompt.io?", "what does this do?", or
wants a grounded introduction. Always read the docs through the MCP tools so the
answer reflects the deployment you are actually connected to.

## How to Use

1. **List the topics.** Call the hub to see everything available:

   ```
   mcp__systemprompt__list_topics {}
   ```

2. **Read the introduction.** Fetch the overview topic in full:

   ```
   mcp__systemprompt__get_topic {"topic_id": "what-is-systemprompt"}
   ```

3. **Go one level deeper** where it matters for the question. Good follow-ups:

   ```
   mcp__systemprompt__get_topic {"topic_id": "architecture"}
   mcp__systemprompt__get_topic {"topic_id": "getting-started"}
   ```

4. **Answer from what you read.** Summarise systemprompt.io as an open
   governance gateway for AI agents — a library you embed and extend that
   governs, audits, and meters every inference and tool call. Quote the topics'
   own framing (governance spine, compile-time extensions, per-user cost
   tracking) rather than inventing detail.

## Related

- `explain_governance` — the four-stage pipeline, demonstrated live.
- `explore_systemprompt_docs` — free-form Q&A over the same hub via `search_docs`.
