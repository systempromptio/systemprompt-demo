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
    // Why: manual review is disabled — only an explicit denial gates an account.
    let row = sqlx::query!(
        r#"
        SELECT
            u.roles,
            COALESCE(upe.department, 'Default') AS "department!",
            COALESCE(ua.status <> 'denied', true) AS "is_approved!"
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

pub async fn find_display_name(
    pool: &PgPool,
    user_id: &UserId,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE(u.display_name, u.full_name, u.name) AS name
           FROM users u WHERE u.id = $1"#,
        user_id.as_str(),
    )
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}
