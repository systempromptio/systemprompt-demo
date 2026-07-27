//! One policy stage, as the browser sees it.
//!
//! A distinct type from the governance chain's own [`StageOutcome`]: that one is
//! shaped for Rust callers, and this one is a wire contract for a UI. Keeping
//! them apart means a rendering tweak stays a rendering tweak rather than
//! reaching into the module that decides whether calls are allowed.

use serde::Serialize;

use super::super::webhook::governance::stages::{StageOutcome, StageResult};

#[derive(Debug, Clone, Serialize)]
pub(super) struct PolicyStage {
    pub(super) policy: String,
    /// `pass`, `fail`, or `skip`. Skip is its own state so a policy that never
    /// ran can be dimmed rather than implying it cleared the call.
    pub(super) result: &'static str,
    pub(super) detail: String,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_carries_the_policys_own_wording() {
        // The detail is not re-authored for the UI: it is the same string the
        // audit spine stores, so a card and a trace row cannot disagree.
        let stage = PolicyStage::from_outcome(&StageOutcome {
            policy: "secret_scan".to_owned(),
            result: StageResult::Fail,
            detail: "matched anthropic_api_key".to_owned(),
        });
        assert_eq!(stage.result, "fail");
        assert_eq!(stage.detail, "matched anthropic_api_key");
    }

    #[test]
    fn skip_survives_as_itself() {
        // Collapsing skip into pass would tell the viewer a check cleared the
        // call when it never ran — the one thing this type exists to prevent.
        let stage = PolicyStage::from_outcome(&StageOutcome {
            policy: "rate_limit".to_owned(),
            result: StageResult::Skip,
            detail: "not reached".to_owned(),
        });
        assert_eq!(stage.result, "skip");
    }
}
