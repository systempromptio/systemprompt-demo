//! `/trace/{conversation_id}` — the audit report behind the terminal's rail.
//!
//! The terminal shows a four-pip summary per governed call; this page is the
//! evidence it links to: the merged, time-ordered union of governance
//! decisions, AI requests, and tool fires for one conversation, with every
//! policy stage and its measured cost. The path segment names the conversation
//! and a `#call-<id>` fragment deep-links one governance decision by its
//! `governance_decisions.id`.
//!
//! The page is public on purpose: an audit trail you cannot hand to someone
//! who wasn't in the session is not evidence. The conversation id is an
//! unguessable capability — holding the link is the authorization, the same
//! posture as the artifact viewer routes. An unknown id renders the empty
//! state, indistinguishable from a conversation with no activity.

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use crate::error::{AdminHtmlError, AdminHtmlResult};
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::demo_trace::{self, DemoTraceRow};
use crate::repositories::pi::conversations;
use crate::templates::AdminTemplateEngine;

use super::branding_context;

const TRACE_LIMIT: i64 = 200;

#[derive(Debug, Serialize)]
struct StageView {
    policy: String,
    result: String,
    detail: String,
    duration: String,
}

#[derive(Debug, Serialize)]
struct EventView {
    id: String,
    at: String,
    kind: String,
    subject: String,
    outcome: String,
    policy: String,
    detail: String,
    is_denied: bool,
    stages: Vec<StageView>,
    has_stages: bool,
    approver: Option<ApproverView>,
}

#[derive(Debug, Serialize)]
struct ApproverView {
    name: String,
    action: String,
    at: String,
    user_id: systemprompt::identifiers::UserId,
}

#[derive(Debug, Serialize)]
struct ArtifactView {
    artifact_type: String,
    title: String,
    server_name: String,
    at: String,
    ui_href: String,
}

#[derive(Debug, Serialize)]
struct SummaryView {
    events: usize,
    denials: usize,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    cost_display: String,
}

pub(crate) async fn demo_trace_page(
    Extension(engine): Extension<AdminTemplateEngine>,
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<String>,
) -> AdminHtmlResult<Response> {
    let mut ctx = branding_context(&engine);
    let serde_json::Value::Object(obj) = &mut ctx else {
        return Err(AdminHtmlError::internal(
            "branding context is not an object".to_owned(),
        ));
    };

    let id = ContextId::new(conversation_id);
    let conversation = conversations::find_conversation_with_owner(&pool, &id)
        .await
        .map_err(|e| {
            AdminHtmlError::internal(format!("demo trace conversation read failed: {e:?}"))
        })?;

    if let Some((row, owner_name)) = conversation {
        let attested = &row.attested_session_id;
        let trace = demo_trace::list_demo_trace(&pool, &row.id, TRACE_LIMIT)
            .await
            // lint-ok: http-error — every read failure on this page is a 500.
            .map_err(|e| AdminHtmlError::internal(format!("demo trace read failed: {e:?}")))?;
        let kpis = session_detail::get_conversation_kpis(&pool, &row.id)
            .await
            // lint-ok: http-error — every read failure on this page is a 500.
            .map_err(|e| AdminHtmlError::internal(format!("demo trace kpi read failed: {e:?}")))?;

        insert_artifacts(obj, &pool, &row.id, &row.user_id).await?;

        let events: Vec<EventView> = trace.iter().map(event_view).collect();
        let denials = events.iter().filter(|e| e.is_denied).count();

        obj.insert("has_session".to_owned(), true.into());
        obj.insert("username".to_owned(), owner_name.into());
        obj.insert("conversation_id".to_owned(), row.id.to_string().into());
        obj.insert("session_id".to_owned(), attested.to_string().into());
        if let Some(title) = &row.title {
            obj.insert("conversation_title".to_owned(), title.clone().into());
        }
        obj.insert(
            "summary".to_owned(),
            serde_json::to_value(SummaryView {
                events: events.len(),
                denials,
                requests: kpis.request_count,
                input_tokens: kpis.total_input_tokens,
                output_tokens: kpis.total_output_tokens,
                cost_display: format!("${:.4}", kpis.total_cost_microdollars as f64 / 1_000_000.0),
            })
            .unwrap_or(serde_json::Value::Null),
        );
        obj.insert(
            "events".to_owned(),
            serde_json::to_value(events).unwrap_or_else(|_| serde_json::Value::Array(vec![])),
        );
    } else {
        obj.insert("has_session".to_owned(), false.into());
    }

    let html = engine
        .render("demo-trace", &ctx)
        // lint-ok: http-error — every template render failure is a 500.
        .map_err(|e| AdminHtmlError::internal(format!("demo-trace page render failed: {e:?}")))?;
    Ok(Html(html).into_response())
}

// Why: same conversation key as the trace; rendered by the same viewer routes
// the terminal uses, scoped to the conversation's owner — the id in the URL is
// the capability, so the owner's artifacts are what the link discloses.
async fn insert_artifacts(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    pool: &PgPool,
    conversation_id: &ContextId,
    owner_id: &systemprompt::identifiers::UserId,
) -> AdminHtmlResult<()> {
    let artifacts: Vec<ArtifactView> =
        crate::repositories::pi::artifacts::list_artifacts_for_conversation(
            pool,
            conversation_id,
            owner_id,
            TRACE_LIMIT,
        )
        .await
        // lint-ok: http-error — every read failure on this page is a 500.
        .map_err(|e| AdminHtmlError::internal(format!("demo trace artifact read failed: {e:?}")))?
        .into_iter()
        .map(|a| ArtifactView {
            ui_href: format!("/api/public/pi/artifacts/{}/ui", a.artifact_id),
            title: a.title.unwrap_or_else(|| a.server_name.clone()),
            artifact_type: a.artifact_type,
            server_name: a.server_name,
            at: a.created_at.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
        })
        .collect();
    obj.insert("has_artifacts".to_owned(), (!artifacts.is_empty()).into());
    obj.insert(
        "artifacts".to_owned(),
        serde_json::to_value(artifacts).unwrap_or_else(|_| serde_json::Value::Array(vec![])),
    );
    Ok(())
}

fn event_view(row: &DemoTraceRow) -> EventView {
    let stages = row
        .evaluated_rules
        .as_ref()
        .map(stage_views)
        .unwrap_or_default();
    EventView {
        id: row.id.clone(),
        at: row.at.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
        kind: row.kind.clone(),
        subject: row.subject.clone(),
        outcome: row.outcome.clone(),
        policy: row.policy.clone(),
        detail: row.detail.clone(),
        is_denied: matches!(
            row.outcome.as_str(),
            "deny" | "denied" | "failed" | "rejected"
        ),
        has_stages: !stages.is_empty(),
        stages,
        approver: row.evaluated_rules.as_ref().and_then(approver_view),
    }
}

fn approver_view(evaluated_rules: &serde_json::Value) -> Option<ApproverView> {
    let approver = evaluated_rules.get("approver")?;
    let text = |key: &str| {
        approver
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let at = approver
        .get("decided_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string())
        .unwrap_or_default();
    Some(ApproverView {
        name: text("username"),
        action: text("action"),
        at,
        user_id: systemprompt::identifiers::UserId::new(text("user_id")),
    })
}

fn stage_views(evaluated_rules: &serde_json::Value) -> Vec<StageView> {
    let Some(chain) = evaluated_rules.get("chain").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    chain
        .iter()
        .map(|entry| {
            let text = |key: &str| {
                entry
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned()
            };
            StageView {
                policy: text("policy_id"),
                result: text("result"),
                detail: text("detail"),
                duration: format_duration(
                    entry
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0),
                ),
            }
        })
        .collect()
}

// Why: 0 is the backfill sentinel for "recorded before timings existed" (see
// migrations/2026-07-28-governance-chain-duration-backfill.sql) — it renders
// as an em dash, never as a measured figure.
fn format_duration(ms: f64) -> String {
    if ms > 0.0 && ms < 1.0 {
        "<1ms".to_owned()
    } else if ms >= 1.0 {
        format!("{}ms", ms.round() as i64)
    } else {
        "—".to_owned()
    }
}
