//! `retention` job: bounded data retention for the demo.
//!
//! Replaces core's `cleanup_empty_contexts` and `database_cleanup`, both
//! disabled in `services/scheduler/config.yaml`. Core's windows are Rust
//! constants and its orphan sweep had no age guard at all, so this deployment
//! could not stop it deleting same-day governance audit rows.
//!
//! Governance audit rows are kept for a full quarter so the dashboard's
//! lifetime aggregates stay truthful and a visitor can watch the observation
//! system accumulate evidence. Empty scaffolding contexts, which carry no
//! statistic, are collected far sooner.
//!
//! Destructive by opt-in only: without `enforce: true` the job reports what it
//! would delete and changes nothing, matching the scheduler's documented
//! contract for destructive jobs.

use sqlx::PgPool;
use systemprompt::database::DbPool;
use systemprompt::traits::{Job, JobContext, JobResult};

use crate::error::JobError;
use systemprompt_web_admin::repositories::retention::{self, Days, RetentionWindow};

const DEFAULT_AUDIT_DAYS: i32 = 90;
const DEFAULT_EMPTY_CONTEXT_DAYS: i32 = 7;
const DEFAULT_INACTIVE_DAYS: i32 = 7;

#[derive(Debug, Clone, Copy, Default)]
pub struct RetentionJob;

impl RetentionJob {
    fn window(ctx: &JobContext) -> RetentionWindow {
        RetentionWindow {
            audit: Self::parameter(ctx, "audit_days", DEFAULT_AUDIT_DAYS),
            empty_context: Self::parameter(ctx, "empty_context_days", DEFAULT_EMPTY_CONTEXT_DAYS),
            inactive: Self::parameter(ctx, "inactive_days", DEFAULT_INACTIVE_DAYS),
        }
    }

    fn parameter(ctx: &JobContext, key: &str, default: i32) -> Days {
        let Some(raw) = ctx.get_parameter(key) else {
            return Days(default);
        };

        match raw.parse::<i32>() {
            Ok(value) if value > 0 => Days(value),
            _ => {
                tracing::warn!(
                    key,
                    value = %raw,
                    default,
                    "Ignoring unusable retention parameter"
                );
                Days(default)
            },
        }
    }

    async fn report(pool: &PgPool, window: &RetentionWindow) -> Result<JobResult, JobError> {
        let executions = retention::count_expired_tool_executions(pool, window).await?;
        let contexts = retention::count_expired_empty_contexts(pool, window).await?;

        tracing::info!(
            expired_tool_executions = executions,
            expired_empty_contexts = contexts,
            audit_days = window.audit.0,
            empty_context_days = window.empty_context.0,
            "enforce disabled: rows qualify for retention but were not deleted"
        );

        Ok(JobResult::success().with_message(format!(
            "observe-only: {executions} tool execution(s) and {contexts} context(s) qualify"
        )))
    }

    async fn sweep(pool: &PgPool, window: &RetentionWindow) -> Result<JobResult, JobError> {
        let contexts = retention::delete_expired_empty_contexts(pool, window).await?;
        let executions = retention::delete_expired_tool_executions(pool, window).await?;

        tracing::info!(
            deleted_tool_executions = executions,
            deleted_empty_contexts = contexts,
            audit_days = window.audit.0,
            empty_context_days = window.empty_context.0,
            "Retention sweep completed"
        );

        Ok(JobResult::success().with_stats(executions + contexts, 0))
    }
}

#[async_trait::async_trait]
impl Job for RetentionJob {
    fn name(&self) -> &'static str {
        "retention"
    }

    fn description(&self) -> &'static str {
        "Deletes governance audit rows past the retention window and empty contexts that hold no statistic"
    }

    fn schedule(&self) -> &'static str {
        "0 0 4 * * *"
    }

    async fn execute(
        &self,
        ctx: &JobContext,
    ) -> Result<JobResult, systemprompt::traits::ProviderError> {
        let start = std::time::Instant::now();

        let db = ctx
            .db_pool::<DbPool>()
            .ok_or(JobError::MissingContext("DbPool"))?;

        let pool = db
            .write_pool()
            .ok_or(JobError::MissingContext("write PgPool"))?;

        let window = Self::window(ctx);

        let result = if ctx.enforce() {
            Self::sweep(&pool, &window).await?
        } else {
            Self::report(&pool, &window).await?
        };

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(result.with_duration(duration_ms))
    }
}

systemprompt::traits::submit_job!(&RetentionJob);
