//! Governance webhook: the four-stage decision pipeline invoked on every tool
//! call.
//!
//! Scope check, secret scan, blocklist, then rate limit. Every decision is
//! audited with a trace id whether it allows or denies.

mod authz;
mod engine;
mod handler;
pub mod inproc;
pub(crate) mod scope;
pub mod stages;
pub mod types;

pub use authz::govern_authz;
pub use handler::govern_tool_use;
pub use types::GovernanceDeps;
