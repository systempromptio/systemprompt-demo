//! Data access for the admin surface, one module per domain.
//!
//! Callers path-qualify (`repositories::config::gateway::create_route`), so
//! the module path is the only name a symbol has and collisions between
//! domains cannot arise. The audit-side modules are re-exported from
//! [`systemprompt_web_governance`] under the same paths.

pub mod config;
pub mod departments;
pub mod jobs;
pub mod marketplace;
pub mod mcp;
pub mod pi;
pub mod secrets;
pub mod site_markdown;
pub mod users;

pub use systemprompt_web_governance::repositories::{analytics, bridge, dashboard, governance};
