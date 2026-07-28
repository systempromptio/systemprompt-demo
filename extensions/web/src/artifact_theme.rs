//! The systemprompt.io skin for server-rendered MCP artifacts.
//!
//! A tool result opens as a sandboxed frame over the terminal, rendered by
//! core's artifact renderers rather than by anything in this repo. Core styles
//! those renderers entirely through its `--mcpui-*` design tokens, so branding
//! them is a matter of registering a theme that re-declares the tokens — the
//! renderers themselves stay untouched and keep working for any other
//! deployment.
//!
//! The token values live in `storage/files/css/artifacts/artifact-theme.css`
//! because CSS belongs there; this module only embeds them. The registration
//! itself sits in `extension`, beside `register_extension!` — an `inventory`
//! submission is only linked if something in its object file is reached, and
//! `WebExtension` is the one item in this crate the binary is guaranteed to
//! touch.

use systemprompt::mcp::services::ui_renderer::ArtifactTheme;

const TOKENS: &str = include_str!("../../../storage/files/css/artifacts/artifact-theme.css");

pub(crate) const fn systemprompt_theme() -> ArtifactTheme {
    ArtifactTheme::new(TOKENS)
}
