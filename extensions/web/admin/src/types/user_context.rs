//! The resolved caller identity carried through admin request handling.

use serde::Serialize;
use systemprompt::identifiers::{Email, UserId};

#[derive(Debug, Clone, Serialize)]
pub struct UserContext {
    pub user_id: UserId,
    pub username: String,
    pub email: Email,
    pub department: String,
    pub roles: Vec<String>,
    pub is_admin: bool,
    pub email_verified: bool,
    /// Whether an admin has approved this account. Registration is open, but a
    /// pending account reaches nothing but `/admin/pending` and draws no credit.
    pub is_approved: bool,
}
