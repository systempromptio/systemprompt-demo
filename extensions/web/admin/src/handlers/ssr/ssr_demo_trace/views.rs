//! Row-to-view shaping for the trace timeline.

use serde::Serialize;

use crate::repositories::governance::demo_trace::DemoTraceRow;

#[derive(Debug, Serialize)]
pub(super) struct StageView {
    policy: String,
    result: String,
    detail: String,
    duration: String,
}

#[derive(Debug, Serialize)]
pub(super) struct EventView {
    id: String,
    at: String,
    kind: String,
    subject: String,
    outcome: String,
    policy: String,
    detail: String,
    pub(super) is_denied: bool,
    stages: Vec<StageView>,
    has_stages: bool,
    approver: Option<ApproverView>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApproverView {
    name: String,
    action: String,
    at: String,
    user_id: systemprompt::identifiers::UserId,
}

pub(super) fn event_view(row: &DemoTraceRow) -> EventView {
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
