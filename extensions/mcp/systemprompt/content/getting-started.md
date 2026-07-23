# Getting Started

This hub is served by the `systemprompt` MCP server, which every signed-in user
can reach. If you are reading this inside Claude Desktop, Cowork, or Claude
Code, you are already connected.

## The three hub tools

- **`list_topics`** — enumerate every documentation topic with a one-line
  summary. Start here.
- **`get_topic {topic_id}`** — read one topic in full (for example
  `get_topic {"topic_id": "governance-pipeline"}`).
- **`search_docs {query}`** — keyword search across all topics; returns the
  best-matching topics ranked, with short excerpts.

The same topics are also exposed as MCP **resources** under
`systemprompt://docs/<id>` (`text/markdown`), so a client that prefers to browse
resources rather than call tools can read them directly.

## A first governed call

1. Call `list_topics` to see what is available.
2. Call `get_topic` for `what-is-systemprompt` and `governance-pipeline`.
3. Call `search_docs` with a natural question such as
   `{"query": "how are secrets blocked"}`.

Each of those calls passes through the four-stage governance pipeline and lands
an audited `allow` row. To see the deny side, the `explain_governance` skill
walks you through triggering a `secret_scan` deny and reading the decision back.

## Connecting your own client

Point any Anthropic-SDK client at the gateway's `/v1/messages` endpoint with a
token minted for your identity. The gateway authenticates the token, applies
governance, meters the cost against your credit balance, and audits the call.
The Systemprompt Bridge does this setup for Claude Desktop and Cowork
automatically via device-link sign-in.

## Where to go next

- `what-is-systemprompt` — the one-paragraph pitch and the why.
- `skills-and-marketplace` — the skills that drive these tools for you.
