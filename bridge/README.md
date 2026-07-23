# Systemprompt Bridge

The systemprompt.io build of the **systemprompt bridge** (credential helper +
plugin/MCP sync agent + local inference proxy). The desktop app for Windows and
macOS that connects Claude Cowork (Claude Desktop / Claude Code) to your
systemprompt gateway.

This crate is intentionally tiny. All behaviour lives in the shared core library
(`systemprompt-core/bin/bridge`); here we only supply a `Brand` value and the
brand assets. The crate is a **standalone workspace** (its own `[workspace]`),
not a member of the main server workspace, because it carries GUI dependencies
and ships on its own release cadence — exactly like core's bridge.

Building it requires `systemprompt-core` checked out as a sibling of this repo
(the core bridge library is `publish = false`, so it is a path dependency).

## Build & run

```bash
cd bridge
cargo build --release                 # host target
cargo run -- help                     # show branded help
cargo run -- gui                      # native settings UI (macOS/Windows only)
```

The GUI (winit + wry) compiles only on macOS/Windows; on Linux the crate builds
in headless/proxy mode.

Config, PAT, cache, and logs live under the `systemprompt` / `systemprompt-bridge`
paths (e.g. `~/.config/systemprompt/systemprompt-bridge.toml`), and all env
overrides use the `SP_BRIDGE_` prefix (`SP_BRIDGE_GATEWAY_URL`, `SP_BRIDGE_PAT`,
`SP_BRIDGE_CONFIG`, …).

Sign-in is plain gateway auth: the setup button opens the gateway's device-link
consent page (mounted at `/bridge-auth/device-link` in this repo) in the
browser; a PAT can be supplied instead via `SP_BRIDGE_PAT` or
`systemprompt-bridge login <pat>`.

Out of the box the gateway catalog offers the Claude hosts only (Claude Code,
Claude Desktop / Cowork). Note: core's signed manifest soft-defaults every known
host to enabled when a user has no saved preferences, so a separately installed
Codex CLI would still be synced — this distribution simply does not offer or
document it.

The bridge pre-trusts a `~/Systemprompt` workspace folder for Claude Cowork
(pushed as an `allowedWorkspaceFolders` entry by core's policy writer), so the
agent has a writable home without folder prompts.

## macOS .app bundle

```bash
cargo build --release --target aarch64-apple-darwin
scripts/make-mac-app.sh --target aarch64-apple-darwin   # → SystempromptBridge.app
```

## Icons

`assets/icon.svg` is the master systemprompt mark (white glyphs on the orange
`#f79938` rounded square, matching `storage/files/images/icon.svg`). The raster
icons consumed by the build are generated from it by `scripts/render-icons.py`
(cairosvg + Pillow):

```bash
python3 scripts/render-icons.py
```

This regenerates, idempotently:

- `assets/window-icon-1024.png` — GUI window icon + macOS `.icns` source.
- `assets/tray-icon.png` — 44×44 tray icon (mark on the orange rounded square,
  legible on both the dark macOS menu bar and a light Windows tray).
- `assets/app-icon.ico` — multi-resolution (16/32/48/256), embedded into the
  Windows `.exe` by `build.rs`. Rebuild (`cargo build --release`) after changing
  the icon so the new `.ico` is re-embedded.

Edit `assets/icon.svg` and rerun the script to change the mark. `assets/logo.svg`
is the full systemprompt.io wordmark, used by the GUI chrome.

`default_gateway_url` is `http://localhost:8080` so a dev build talks to a
`just start` server out of the box; point installs at a deployed gateway with
`systemprompt-bridge install --gateway <url>`.

## Recipe: a white-label bridge

The core/extension boundary makes a new white-label bridge a copy-and-swap job —
no forking of the bridge source:

1. Copy this `bridge/` crate to the new repo.
2. Replace everything in `assets/` with the client's marks + `theme.css`
   (override the `--sp-*` tokens; see `assets/theme.css`).
3. Edit the `Brand` const in `src/main.rs`: name, binary name, vendor, on-disk
   dir names, `env_prefix`, `default_gateway_url`, and chrome strings. Plugin
   ids are carried per-plugin in the gateway's signed manifest — there is no
   brand-level plugin-name field to set.
4. Update `build.rs` (Windows metadata), `macos/Info.plist` (bundle id + names),
   and `scripts/make-mac-app.sh` (bundle/app name).
5. Optionally register brand behaviour (e.g. a marketplace artifacts source)
   through core's `inventory` seams in a `src/registry.rs` compiled on
   macOS/Windows — see the astound bridge for the pattern; this build ships
   without one.
6. Wire up the release workflow (`.github/workflows/release-bridge.yml`).

Everything else — auth, sync, proxy, GUI, host integrations — is inherited from
core and stays in lockstep across all brands.
