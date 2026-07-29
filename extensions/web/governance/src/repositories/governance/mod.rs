//! Persistence for the governance record — what actually happened.
//!
//! Every tool-call decision, the policies that produced it, and the rollups the
//! audit pages read are served from here. Rows are append-only history: nothing
//! in this module changes what is allowed, only what was decided.
//!
//! The configured side of that pairing — gateway routes, agent definitions,
//! the access-control YAML — lives in `super::config`.

pub mod demo_trace;
pub mod stages;

#[derive(Debug, Clone, Copy, Default)]
pub struct GovernanceCounts {
    pub total: i64,
    pub allowed: i64,
    pub denied: i64,
    pub secret_breaches: i64,
    pub prompts_blocked: i64,
    pub tools_blocked: i64,
    /// Counted in `user_activity`, not `governance_decisions`.
    pub tool_calls: i64,
}

#[derive(Debug, Clone)]
pub struct PerPolicyCounts {
    pub policy: String,
    pub allowed: i64,
    pub denied: i64,
    pub last_at: Option<chrono::DateTime<chrono::Utc>>,
}
