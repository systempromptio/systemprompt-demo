//! Retention sweeps for the governance spine.
//!
//! These replace core's `cleanup_empty_contexts` /
//! `delete_orphaned_mcp_executions` pair, which are disabled in
//! `services/scheduler/config.yaml`.
//!
//! Two rules core got wrong and this module fixes:
//!
//! - **Age is the only criterion for audit rows.** Core deleted
//!   `mcp_tool_executions` whose `context_id` was absent from `user_contexts`,
//!   with no age guard. That column carries no foreign key, so a missing
//!   context is not an integrity violation — it is a normal state, and treating
//!   it as one destroyed same-day audit rows.
//! - **A context holding audit rows is not empty.** Core defined "empty" as
//!   "has no `task_messages`", so a visitor who drove the tool surface without
//!   chatting had their context collected and their tool calls reaped with it.
//!
//! The two windows are deliberately different, because the two tables serve
//! different purposes. `mcp_tool_executions` *is* the observable evidence the
//! demo exists to show: the dashboard aggregates it as a lifetime total, so a
//! short window would make the published numbers decay and quietly understate
//! usage. It gets a long, flat window and nothing else — in particular, not the
//! owner's recent activity, which would erase a departed evaluator's history
//! precisely when it is most interesting. Contexts carrying no messages, no
//! tool calls and no recent sessions are pure scaffolding; they hold no
//! statistic and are collected on a short window.

use sqlx::PgPool;

#[derive(Debug, Clone, Copy)]
pub struct Days(pub i32);

impl Days {
    fn interval(self) -> String {
        format!("{} days", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionWindow {
    pub audit: Days,
    pub empty_context: Days,
    pub inactive: Days,
}

pub async fn count_expired_tool_executions(
    pool: &PgPool,
    window: &RetentionWindow,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!"
           FROM mcp_tool_executions
           WHERE created_at < NOW() - $1::text::interval"#,
        window.audit.interval(),
    )
    .fetch_one(pool)
    .await
}

pub async fn delete_expired_tool_executions(
    pool: &PgPool,
    window: &RetentionWindow,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"DELETE FROM mcp_tool_executions
           WHERE created_at < NOW() - $1::text::interval"#,
        window.audit.interval(),
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn count_expired_empty_contexts(
    pool: &PgPool,
    window: &RetentionWindow,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!"
           FROM user_contexts uc
           WHERE uc.created_at < NOW() - $1::text::interval
             AND (uc.kind != 'cli_session' OR uc.session_id IS NULL)
             AND NOT EXISTS (
                 SELECT 1 FROM task_messages tm WHERE tm.context_id = uc.context_id)
             AND NOT EXISTS (
                 SELECT 1 FROM mcp_tool_executions m WHERE m.context_id = uc.context_id)
             AND NOT EXISTS (
                 SELECT 1 FROM user_sessions s
                  WHERE s.user_id = uc.user_id
                    AND s.last_activity_at > NOW() - $2::text::interval)"#,
        window.empty_context.interval(),
        window.inactive.interval(),
    )
    .fetch_one(pool)
    .await
}

pub async fn delete_expired_empty_contexts(
    pool: &PgPool,
    window: &RetentionWindow,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"DELETE FROM user_contexts uc
           WHERE uc.created_at < NOW() - $1::text::interval
             AND (uc.kind != 'cli_session' OR uc.session_id IS NULL)
             AND NOT EXISTS (
                 SELECT 1 FROM task_messages tm WHERE tm.context_id = uc.context_id)
             AND NOT EXISTS (
                 SELECT 1 FROM mcp_tool_executions m WHERE m.context_id = uc.context_id)
             AND NOT EXISTS (
                 SELECT 1 FROM user_sessions s
                  WHERE s.user_id = uc.user_id
                    AND s.last_activity_at > NOW() - $2::text::interval)"#,
        window.empty_context.interval(),
        window.inactive.interval(),
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
