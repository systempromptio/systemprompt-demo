//! MCP server crate for the systemprompt.io template.
//!
//! Implements the `systemprompt` MCP server that ships with the demo plugin: a
//! read-only documentation hub over the systemprompt.io reference topics.
//! Tools are defined in [`tools`] and exposed through [`SystempromptServer`];
//! topic content lives in [`topics`]; errors normalise on
//! [`error::SystempromptToolError`]. The `main` binary is a thin `tokio::main`
//! shell that builds a [`SystempromptServer`] and serves it over HTTP.

pub mod error;
pub mod server;
pub mod tools;
pub mod topics;

pub use server::SystempromptServer;
