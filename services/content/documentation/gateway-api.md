---
title: "Gateway API (/v1/messages)"
description: "The governed inference gateway: the /v1/messages contract, the attested x-session-id header and how to mint one, model allow-listing, and the three profile API URLs."
author: "systemprompt.io"
slug: "gateway-api"
keywords: "gateway, /v1/messages, x-session-id, inference, model allow-list, api url, governance"
kind: "guide"
public: true
tags: ["gateway", "api", "governance"]
published_at: "2026-05-19"
updated_at: "2026-07-24"
after_reading_this:
  - "Call the governed inference gateway at POST /v1/messages"
  - "Supply the required x-session-id header so a request is not rejected with HTTP 400"
  - "Mint an attested session for an API-key caller at POST /api/public/gateway/sessions"
  - "Understand how the gateway model allow-list rejects un-listed models with HTTP 403"
  - "Pick the right profile api_*_url for in-container vs host callers"
related_docs:
  - title: "Authentication"
    url: "/documentation/authentication"
  - title: "Access Control"
    url: "/documentation/access-control"
---

# Gateway API

**TL;DR:** the gateway exposes `POST /v1/messages` — an Anthropic-Messages-compatible
endpoint that authenticates, authorises, governs, and audits every inference call before
proxying it upstream. Every request **must** carry an `x-session-id` header and an
`Authorization` (or `x-api-key`) credential. The model named in the body must be
permitted by the gateway policy allow-list, or the gateway returns `403` before any
upstream call is made.

## Request contract

`POST /v1/messages`

Required headers:

| Header | Purpose | Missing → |
|--------|---------|-----------|
| `Authorization: Bearer <jwt>` *or* `x-api-key: <key>` | Caller identity | `401 Unauthorized` |
| `x-session-id: <session id>` | Binds the call to a session for audit and conversation continuity | `400 Bad Request` ("missing required x-session-id header") |
| `x-gateway-conversation-id: <id>` *(optional)* | Pins the conversation; otherwise it is derived from the message body | — |

Body: the Anthropic Messages shape — `model`, `max_tokens`, `messages[]`.

## Where the session id comes from

The `x-session-id` header is **mandatory**, and it must name a session the server
issued to the calling identity. A value the server does not recognise — or one
that belongs to another user — is rejected with `401`:

```
unknown or revoked session; mint one at POST /api/public/gateway/sessions
```

That is deliberate. `ai_requests.session_id` is evidence in the audit trail, so
the gateway will not record an id the caller invented. There are two ways to get
a real one:

**JWT callers** already have one. The `session_id` claim inside the token was
minted alongside the token itself, and the gateway requires
`x-session-id` to equal that claim. Mismatch → `401 X-Session-ID does not match
authenticated session`. The desktop bridge does this for you: it owns both the
JWT and the matching header and refreshes them together, which is why local
clients point at the bridge proxy rather than the gateway directly.

**API-key (PAT) callers** mint one first:

```bash
SESSION_ID=$(curl -sS -X POST "$API_URL/api/public/gateway/sessions" \
  -H "x-api-key: $SYSTEMPROMPT_API_KEY" \
  | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')

curl -sS -X POST "$API_URL/v1/messages" \
  -H "x-api-key: $SYSTEMPROMPT_API_KEY" \
  -H "x-session-id: $SESSION_ID" \
  -H "content-type: application/json" \
  -d '{"model":"claude-sonnet-5","max_tokens":64,"messages":[{"role":"user","content":"hello"}]}'
```

`POST /api/public/gateway/sessions` authenticates the PAT, writes a
`user_sessions` row owned by that key's user (recording the calling IP and user
agent), and returns `{"session_id": "sess_…"}`. Reuse it for the life of the
agent's work; mint a new one when it expires. The endpoint accepts API keys
only — a JWT caller is told to use its own claim instead.

## Model allow-listing

The profile's provider registry (`profile.providers`, e.g. in
`.systemprompt/profiles/local/profile.yaml`) declares every provider and the
models it serves; those model ids are the deployment's allow-list. A request
whose `model` is neither declared in the registry nor matched by a gateway route
(or absorbed by `gateway.default_provider`) is denied with `403` before any
upstream call — this is the egress control an air-gapped deployment relies on.

Both the dispatch gate (`GatewayConfig::is_model_exposed`) and the `/profile`
model list derive from the registry, so adding a model means editing the
`providers` block.

Gateway-route RBAC additionally keys on the route `id`. If the caller's role or
department is not assigned to the route (and the route is not `default_included`), the
gateway returns `403` with a message that names the route id, the model, and the
remedy.

## Profile API URLs

A profile declares three server URLs; pick the right one for the caller:

| Field | Use it for |
|-------|------------|
| `api_internal_url` | Service-to-service calls inside the deployment network |
| `api_server_url` | The address the server binds / in-container CLI calls |
| `api_external_url` | The public, host-facing address; used for OAuth/WebAuthn URL generation and consumed by `session login` |

When the host-published port differs from the in-container port (as in the air-gap
stack), an in-container caller must use the in-container address and host callers must
use the published one — a single value cannot satisfy both. The air-gap profile pins
all three to the in-container `:8080` and automated host callers pass the published
port explicitly.
