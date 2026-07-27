//! The policy chain as data, for surfaces that show it rather than enforce it.
//!
//! The audit spine's `ChainEntryOutcome` is a database blob whose shape answers
//! to the governance schema. These two types are the same facts shaped for a
//! reader: no `PolicyId` newtype to unwrap, no serde tagging, and a result enum
//! that a UI can match on exhaustively.
//!
//! Separate from `inproc` because deciding a call and describing the decision are
//! different jobs, and only the second one is allowed to change for cosmetic
//! reasons.

/// How one stage of the chain ended.
///
/// Three states, not a bool: a skipped policy is not a passing one. A policy
/// disabled in config and a policy never reached because an earlier one denied
/// both arrive here as [`Self::Skip`], and anything that rendered them as passes
/// would claim a check ran that did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StageResult {
    Pass,
    Fail,
    Skip,
}

/// One stage of the chain as it actually ran.
pub(crate) struct StageOutcome {
    pub(crate) policy: String,
    pub(crate) result: StageResult,
    /// The policy's own explanation — the same string the audit row stores, so a
    /// card and a trace row cannot disagree about why.
    pub(crate) detail: String,
}
