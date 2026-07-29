//! The window a telemetry read looks through.
//!
//! Every governance number the homepage pane shows is reachable two ways: the
//! conversation in front of you, and everything this account has ever run. The
//! panes render both at once, so the queries behind them take this rather than
//! a bare conversation id — one compile-time-checked statement serves both
//! windows, and the two can never drift apart by being written twice.
//!
//! Ownership is always the `user_id` column, never the conversation join. The
//! join only narrows. That is what makes the account-wide read agree with the
//! credit meter to the cent: both are `WHERE user_id = $1` over the same table.
//! Establishing ownership through `pi_conversation_sessions` instead would
//! silently drop every row written before a conversation binding existed, or
//! by a client that never opened one.
//!
//! Archived conversations are in scope. The governance rows outlive the
//! conversation that explains them, so a user's totals never shrink.

use systemprompt::identifiers::{ContextId, UserId};

#[derive(Debug, Clone, Copy)]
pub struct StatsScope<'a> {
    user_id: &'a UserId,
    conversation_id: Option<&'a ContextId>,
}

impl<'a> StatsScope<'a> {
    pub const fn all(user_id: &'a UserId) -> Self {
        Self {
            user_id,
            conversation_id: None,
        }
    }

    pub const fn conversation(user_id: &'a UserId, conversation_id: &'a ContextId) -> Self {
        Self {
            user_id,
            conversation_id: Some(conversation_id),
        }
    }

    pub fn user(&self) -> &str {
        self.user_id.as_str()
    }

    /// The conversation filter as the queries bind it: `NULL` widens the read
    /// to every conversation the user owns.
    pub fn conversation_filter(&self) -> Option<&str> {
        self.conversation_id.map(ContextId::as_str)
    }
}
