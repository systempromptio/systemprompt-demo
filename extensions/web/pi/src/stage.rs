//! One policy stage, as the browser sees it.
//!
//! A distinct type from the governance chain's own [`StageOutcome`]: that one
//! is shaped for Rust callers, and this one is a wire contract for a UI.
//! Keeping them apart means a rendering tweak stays a rendering tweak rather
//! than reaching into the module that decides whether calls are allowed.

use serde::Serialize;

use systemprompt_web_governance::webhook::governance::stages::{StageOutcome, StageResult};

#[derive(Debug, Clone, Serialize)]
pub struct PolicyStage {
    pub policy: String,
    /// `pass`, `fail`, or `skip`. Skip is its own state so a policy that never
    /// ran can be dimmed rather than implying it cleared the call.
    pub result: &'static str,
    pub detail: String,
    /// Milliseconds spent evaluating this policy; zero if it never ran.
    pub duration_ms: f64,
}

impl PolicyStage {
    pub(super) fn from_outcome(outcome: &StageOutcome) -> Self {
        Self {
            policy: outcome.policy.clone(),
            result: match outcome.result {
                StageResult::Pass => "pass",
                StageResult::Fail => "fail",
                StageResult::Skip => "skip",
            },
            detail: outcome.detail.clone(),
            duration_ms: outcome.duration_ms,
        }
    }
}
