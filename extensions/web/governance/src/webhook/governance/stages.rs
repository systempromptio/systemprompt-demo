//! The policy chain as data, for surfaces that show it rather than enforce it.
//!
//! The audit spine's `ChainEntryOutcome` is a database blob whose shape answers
//! to the governance schema. These two types are the same facts shaped for a
//! reader: no `PolicyId` newtype to unwrap, no serde tagging, and a result enum
//! that a UI can match on exhaustively.
//!
//! Separate from `inproc` because deciding a call and describing the decision
//! are different jobs, and only the second one is allowed to change for
//! cosmetic reasons.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageResult {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug)]
pub struct StageOutcome {
    pub policy: String,
    pub result: StageResult,
    pub detail: String,
    // Why: zero means "never ran", not "instant" — readers render it as absent.
    pub duration_ms: f64,
}
