//! MCP server crate for the systemprompt.io template.
//!
//! Implements the `systemprompt` MCP server that ships with the demo plugin: a
//! read-only documentation hub over the systemprompt.io reference topics.
//! Tools are defined in [`tools`] and exposed through [`SystempromptServer`];
//! topic content lives in [`topics`]; the audit queries behind
//! `governance_stats` live in `repositories`; errors normalise on
//! [`error::SystempromptToolError`]. The `main` binary is a thin `tokio::main`
//! shell that builds a [`SystempromptServer`] and serves it over HTTP.

pub mod error;
pub(crate) mod repositories;
pub mod server;
pub mod tools;
pub mod topics;

pub use server::SystempromptServer;

/// Crate-private items that out-of-crate unit tests drive directly.
pub mod test_support {
    pub use crate::server::tool::site_pages::{site_page_url, truncate_for_model};
}
