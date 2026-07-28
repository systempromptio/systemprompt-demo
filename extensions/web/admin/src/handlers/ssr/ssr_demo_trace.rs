//! `/admin/demo/trace` — the audit report behind the terminal's rail.
//!
//! The terminal shows a four-pip summary per governed call; this page is the
//! evidence it links to: the merged, time-ordered union of governance
//! decisions, AI requests, and tool fires for one conversation, with every
//! policy stage and its measured cost. `?session=` names the conversation and
//! `?call=` deep-links one governance decision by its
//! `governance_decisions.id`.
//!
//! Authority is the signed-in user's own: the conversation is resolved through
//! `find_conversation`, which scopes to the owner in the `WHERE` clause, so a
//! foreign or unknown id renders the same empty state as no id at all.

use std::sync::Arc;

use axum::Extension;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use systemprompt::identifiers::ContextId;

use crate::error::{AdminHtmlError, AdminHtmlResult};
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::demo_trace::{self, DemoTraceRow};
use crate::repositories::pi::conversations;
use crate::templates::AdminTemplateEngine;
use crate::types::UserContext;

use super::branding_context;

const TRACE_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
pub(crate) struct TraceQuery {
    session: Option<String>,
    call: Option<String>,
}

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
    is_focus: bool,
    stages: Vec<StageView>,
    has_stages: bool,
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
    Extension(user_ctx): Extension<UserContext>,
    Extension(engine): Extension<AdminTemplateEngine>,
    State(pool): State<Arc<PgPool>>,
    Query(q): Query<TraceQuery>,
) -> AdminHtmlResult<Response> {
    let mut ctx = branding_context(&engine);
    let serde_json::Value::Object(obj) = &mut ctx else {
        return Err(AdminHtmlError::internal(
            "branding context is not an object".to_owned(),
        ));
    };
    obj.insert("username".to_owned(), user_ctx.username.clone().into());

    let conversation = match &q.session {
        Some(session) => {
            let id = ContextId::new(session.clone());
            conversations::find_conversation(&pool, &id, &user_ctx.user_id)
                .await
                .map_err(|e| {
                    AdminHtmlError::internal(format!("demo trace conversation read failed: {e:?}"))
                })?
        },
        None => None,
    };

    if let Some(row) = conversation {
        let attested = &row.attested_session_id;
        let trace = demo_trace::list_demo_trace(&pool, attested, TRACE_LIMIT)
            .await
            // lint-ok: http-error — every read failure on this page is a 500.
            .map_err(|e| AdminHtmlError::internal(format!("demo trace read failed: {e:?}")))?;
        let kpis = session_detail::get_session_kpis(&pool, attested)
            .await
            // lint-ok: http-error — every read failure on this page is a 500.
            .map_err(|e| AdminHtmlError::internal(format!("demo trace kpi read failed: {e:?}")))?;

        insert_artifacts(obj, &pool, attested, &user_ctx).await?;

        let events: Vec<EventView> = trace
            .iter()
            .map(|r| event_view(r, q.call.as_deref()))
            .collect();
        let denials = events.iter().filter(|e| e.is_denied).count();

        obj.insert("has_session".to_owned(), true.into());
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

// Why: same session key as the trace; rendered by the same viewer routes the
// terminal uses (cookie auth), not a parallel scheme.
async fn insert_artifacts(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    pool: &PgPool,
    attested: &systemprompt::identifiers::SessionId,
    user_ctx: &UserContext,
) -> AdminHtmlResult<()> {
    let artifacts: Vec<ArtifactView> =
        crate::repositories::pi::artifacts::list_artifacts_for_session(
            pool,
            attested,
            &user_ctx.user_id,
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

fn event_view(row: &DemoTraceRow, focus: Option<&str>) -> EventView {
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
        is_focus: focus == Some(row.id.as_str()),
        has_stages: !stages.is_empty(),
        stages,
    }
}

// Why: the persisted `DecisionAudit` blob is read leniently — rows written
// before the chain carried timings have no `duration_ms`, and those render as
// `—` rather than a fabricated zero.
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
                    entry.get("duration_ms").and_then(serde_json::Value::as_f64),
                ),
            }
        })
        .collect()
}

fn format_duration(ms: Option<f64>) -> String {
    match ms {
        Some(ms) if ms > 0.0 && ms < 1.0 => "<1ms".to_owned(),
        Some(ms) if ms >= 1.0 => format!("{}ms", ms.round() as i64),
        _ => "—".to_owned(),
    }
}
