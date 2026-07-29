//! Data access for the governance spine, one module per domain.
//!
//! Callers path-qualify; this module re-exports nothing, so the module path is
//! the only name a symbol has and collisions between domains cannot arise.

pub mod activity;
pub mod analytics;
pub mod bridge;
pub mod dashboard;
pub mod governance;
pub mod retention;
pub mod scope;
pub mod share_token;
pub mod usage_events;
pub mod user_access;
