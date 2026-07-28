//! Governance webhook: the four-stage decision pipeline invoked on every tool
//! call.
//!
//! Scope check, secret scan, blocklist, then rate limit. Every decision is
//! audited with a trace id whether it allows or denies.

mod audit;
mod authz;
mod handler;
pub(crate) mod inproc;
pub(crate) mod policies;
pub(crate) mod policy;
pub(crate) mod scope;
pub(crate) mod secrets;
pub(crate) mod stages;
pub(crate) mod types;

pub(crate) use authz::govern_authz;
pub(crate) use handler::govern_tool_use;
pub(crate) use types::GovernanceDeps;
