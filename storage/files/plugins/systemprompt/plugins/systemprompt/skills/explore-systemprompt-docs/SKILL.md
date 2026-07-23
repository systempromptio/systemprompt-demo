---
name: "Explore systemprompt.io Docs"
description: "Answer free-form questions about systemprompt.io by searching the documentation hub with the MCP search_docs tool, then reading the best topics in full"
---

# Explore systemprompt.io Docs

Answer free-form questions about systemprompt.io by searching the **live
documentation hub** and reading the best-matching topics in full. The hub is
served by the `systemprompt` MCP server and is open to every signed-in user.

## When to Use

Use this for any specific question about systemprompt.io — "how is cost tracked?",
"what stops a leaked key?", "how do skills reach my client?" — where you want a
grounded answer rather than a guess. Search first, then read, then answer.

## How to Use

1. **Search with the user's own words.** Pass their question, or its key nouns,
   straight to `search_docs`:

   ```
   mcp__systemprompt__search_docs {"query": "how is cost tracked and enforced"}
   ```

   It returns the matching topics ranked, each with a short excerpt.

2. **Read the top hits in full** before answering — the excerpt is only a
   pointer:

   ```
   mcp__systemprompt__get_topic {"topic_id": "audit-trail"}
   ```

3. **If nothing matches**, broaden the query or fall back to browsing:

   ```
   mcp__systemprompt__list_topics {}
   ```

4. **Answer from the topics you read**, and cite the topic id(s) you used so the
   user can read further. Do not answer from memory when the hub disagrees —
   the hub reflects the deployment you are connected to.

## Related

- `explain_systemprompt` — a grounded introduction (list_topics then get_topic).
- `explain_governance` — the governance pipeline, demonstrated with a live deny.
