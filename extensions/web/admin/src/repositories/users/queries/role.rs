//! Role, department and approval lookup for a single user.

use sqlx::PgPool;
use systemprompt::identifiers::UserId;

#[derive(Debug)]
pub struct UserAccess {
    pub roles: Vec<String>,
    pub department: String,
    pub is_approved: bool,
}

pub async fn find_user_access(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<Option<UserAccess>, sqlx::Error> {
    // Why: a missing user_approvals row is pending, not approved — the gate has
    // to fail closed for any account created down a path that skips the review.
    let row = sqlx::query!(
        r#"
        SELECT
            u.roles,
            COALESCE(upe.department, 'Default') AS "department!",
            COALESCE(ua.status = 'approved', false) AS "is_approved!"
        FROM users u
        LEFT JOIN user_profile_ext upe ON upe.user_id = u.id
        LEFT JOIN user_approvals ua ON ua.user_id = u.id
        WHERE u.id = $1
        "#,
        user_id.as_str()
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| UserAccess {
        roles: r.roles,
        department: r.department,
        is_approved: r.is_approved,
    }))
}
