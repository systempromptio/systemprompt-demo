---
title: "Systemprompt Bridge"
description: "Install the Systemprompt Bridge on Windows or macOS and connect Claude Cowork to your self-hosted gateway with device-link sign-in or a PAT."
author: "systemprompt.io"
slug: "bridge"
keywords: "bridge, Claude Cowork, Claude Desktop, Claude Code, install, Windows, macOS, device link, PAT, credential helper"
kind: "guide"
public: true
tags: ["bridge", "installation", "claude-cowork"]
published_at: "2026-07-23"
updated_at: "2026-07-23"
after_reading_this:
  - "Install the Systemprompt Bridge desktop app on Windows or macOS"
  - "Sign in with the browser device-link flow or a personal access token"
  - "Understand the pre-trusted ~/Systemprompt workspace folder for Claude Cowork"
  - "Verify that governed requests are landing in the gateway audit trail"
related_docs:
  - title: "Authentication"
    url: "/documentation/authentication"
  - title: "Gateway API"
    url: "/documentation/gateway-api"
---

# Systemprompt Bridge

The Systemprompt Bridge is the desktop companion for this gateway. It is a
single small binary (with a native settings app on Windows and macOS) that does
three jobs:

- **Credential helper** — exchanges your stored credential for a short-lived
  gateway JWT, so Claude Cowork routes every inference request through the
  governed gateway instead of hitting the provider directly.
- **Plugin and MCP sync** — pulls the gateway's signed plugin manifest and
  managed MCP allowlist into Claude's `org-plugins/` mount and keeps it fresh
  on a schedule.
- **Workspace setup** — creates a `~/Systemprompt` folder and pre-trusts it as
  a Claude Cowork workspace, so the agent has a writable home without folder
  prompts.

Out of the box the gateway offers Claude Cowork (Claude Desktop / Claude Code)
as the host integration.

## 1. Install

Download the release for your platform from the repository's `bridge-v*`
releases, or build from source (requires a sibling checkout of
`systemprompt-core`):

```bash
cd bridge
cargo build --release
```

On macOS, wrap the binary into an app bundle:

```bash
scripts/make-mac-app.sh --target aarch64-apple-darwin   # → SystempromptBridge.app
```

On Windows, run `systemprompt-bridge.exe` — first launch opens the settings
app in the system tray.

## 2. Sign in

Point the bridge at your gateway and register the sync schedule:

```bash
systemprompt-bridge install --gateway https://your-gateway.example.com
```

Then either:

- **Device link (recommended)** — click **Sign in** in the bridge app (or run
  `systemprompt-bridge login`). Your browser opens the gateway's consent page
  at `/bridge-auth/device-link`; approve it and the device is linked
  automatically.
- **Personal access token** — issue a PAT on the gateway
  (`systemprompt admin users pat issue <user-id> --name bridge-laptop`) and run
  `systemprompt-bridge login <sp-live-...>`.

Configuration lives at `~/.config/systemprompt/systemprompt-bridge.toml`
(Linux/macOS) or `%APPDATA%\systemprompt\systemprompt-bridge.toml` (Windows).
Dev overrides: `SP_BRIDGE_GATEWAY_URL`, `SP_BRIDGE_PAT`.

## 3. Verify

```bash
systemprompt-bridge status    # config paths and what is set up
systemprompt-bridge whoami    # authenticated identity from the gateway
systemprompt-bridge doctor    # end-to-end diagnosis
```

After your first Claude Cowork prompt, every request lands a row in the
gateway's audit spine with identity, tokens, cost, and latency:

```bash
systemprompt infra logs request list --limit 10
systemprompt infra logs audit <request-id> --full
```
