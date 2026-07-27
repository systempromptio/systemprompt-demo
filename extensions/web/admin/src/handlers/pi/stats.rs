//! What the agent just cost, and what policy let through.
//!
//! The homepage pane shows a user their own governance spine while the terminal
//! beside it is still running. That data already exists — `ai_requests` for
//! spend and latency, `governance_decisions` for verdicts, `user_activity`
//! for tool fires — so this endpoint composes the same repository functions the
//! retired admin pages used rather than adding queries.
//!
//! It is deliberately *not* an admin route. The interactive site has no admin
//! concept: authority here comes from the embed token, which resolves to
//! exactly one user, and the conversation must belong to that user. A caller
//! therefore reads their own session and nobody else's, with the same opaque
//! 404 for "not yours" as for "does not exist".
//!
//! The ownership check reads `pi_conversations`, not the live-session registry.
//! Every number below is a database query, and so is the attested session id
//! they key on — which is what makes these stats survive a reload and a server
//! restart. Resolving that id from memory instead would make a conversation
//! uncostable the moment its child exited, with every row explaining it still
//! sitting in Postgres.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::identifiers::SessionId;

use super::api::TokenQuery;
use super::auth::{authorize_conversation, problem};
use super::format;
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::{demo_trace, stages};

const TRACE_LIMIT: i64 = 120;

#[derive(Debug, Serialize)]
struct PiStats {
    conversation_id: String,
    model: Option<String>,
    requested_model: Option<String>,
    provider: Option<String>,
    route_match: Option<String>,
    requests: i64,
    errors: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
    cache_hit_percent: i64,
    cost_microdollars: i64,
    cost_display: String,
    cost_per_request_display: String,
    latency_p50_ms: Option<i32>,
    latency_p95_ms: Option<i32>,
    latency_last_ms: Option<i32>,
    allowed: i64,
    denied: i64,
    prompts_blocked: i64,
    tools_blocked: i64,
    tool_calls: i64,
    secrets_caught: i64,
    policy_stages: Vec<PiPolicyStage>,
    model_mix: Vec<PiModelShare>,
    credit: PiCredit,
    events: Vec<PiStatEvent>,
}

#[derive(Debug, Serialize)]
struct PiPolicyStage {
    id: String,
    label: String,
    passed: i64,
    failed: i64,
    active: bool,
}

#[derive(Debug, Serialize)]
struct PiModelShare {
    model: String,
    requests: i64,
    percent: i64,
}

#[derive(Debug, Serialize)]
struct PiCredit {
    granted_microdollars: i64,
    spent_microdollars: i64,
    remaining_microdollars: i64,
    granted_display: String,
    spent_display: String,
    remaining_display: String,
    remaining_percent: i64,
    exhausted: bool,
}

#[derive(Debug, Serialize)]
struct PiStatEvent {
    id: String,
    at: chrono::DateTime<chrono::Utc>,
    kind: String,
    subject: String,
    outcome: String,
    policy: String,
    detail: String,
}

pub(super) async fn stats(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<String>,
    Query(q): Query<TokenQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(row) = authorize_conversation(&pool, q.token(), &conversation_id).await else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };

    match collect(
        &pool,
        &conversation_id,
        &row.attested_session_id,
        &row.user_id,
    )
    .await
    {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not read pi session stats");
            problem(StatusCode::INTERNAL_SERVER_ERROR, "could not read stats") // lint-ok: http-error — logged above; the client is told nothing about why
        },
    }
}

async fn credit_position(pool: &PgPool, user_id: &systemprompt::identifiers::UserId) -> PiCredit {
    let balance = systemprompt_credits_extension::get_balance(pool, user_id.as_str())
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not read a credit balance for the pi stats pane");
            systemprompt_credits_extension::CreditBalance {
                balance_microdollars: 0,
                granted_microdollars: 0,
                spent_microdollars: 0,
            }
        });

    let remaining_percent = if balance.granted_microdollars > 0 {
        let pct = balance.balance_microdollars.saturating_mul(100) / balance.granted_microdollars;
        pct.clamp(0, 100)
    } else {
        0
    };

    PiCredit {
        granted_microdollars: balance.granted_microdollars,
        spent_microdollars: balance.spent_microdollars,
        remaining_microdollars: balance.balance_microdollars,
        granted_display: format::cost_round(balance.granted_microdollars),
        spent_display: format::cost(balance.spent_microdollars),
        remaining_display: format::cost(balance.balance_microdollars.max(0)),
        remaining_percent,
        exhausted: balance.granted_microdollars > 0 && balance.balance_microdollars <= 0,
    }
}

fn policy_stages(rows: &[crate::repositories::governance::PerPolicyCounts]) -> Vec<PiPolicyStage> {
    stages::STAGES
        .iter()
        .map(|(id, label)| {
            let found = rows.iter().find(|r| r.policy == *id);
            PiPolicyStage {
                id: (*id).to_owned(),
                label: (*label).to_owned(),
                passed: found.map_or(0, |r| r.allowed),
                failed: found.map_or(0, |r| r.denied),
                active: found.is_some(),
            }
        })
        .collect()
}

fn model_mix(requests: &[session_detail::SessionRequestRow]) -> Vec<PiModelShare> {
    let total = requests.len() as i64;
    if total == 0 {
        return Vec::new();
    }
    let mut tally: Vec<(String, i64)> = Vec::new();
    for row in requests {
        match tally.iter_mut().find(|(m, _)| *m == row.model) {
            Some((_, n)) => *n += 1,
            None => tally.push((row.model.clone(), 1)),
        }
    }
    tally.sort_by(|a, b| b.1.cmp(&a.1));
    tally
        .into_iter()
        .map(|(model, requests)| PiModelShare {
            model,
            percent: requests.saturating_mul(100) / total,
            requests,
        })
        .collect()
}

async fn collect(
    pool: &PgPool,
    conversation_id: &str,
    attested: &SessionId,
    user_id: &systemprompt::identifiers::UserId,
) -> Result<PiStats, sqlx::Error> {
    let credit = credit_position(pool, user_id).await;
    let kpis = session_detail::get_session_kpis(pool, attested).await?;
    let requests = session_detail::list_session_requests(pool, attested).await?;
    let trace = demo_trace::list_demo_trace(pool, attested, TRACE_LIMIT).await?;
    let counts = stages::get_session_governance_counts(pool, attested).await?;
    let stage_rows = stages::list_session_policy_stages(pool, attested).await?;

    let latest = requests.first();
    let latency_last_ms = latest.and_then(|r| r.latency_ms);
    let latencies: Vec<i32> = requests.iter().filter_map(|r| r.latency_ms).collect();
    let latency_p50_ms = format::median(latencies.clone());
    let latency_p95_ms = format::percentile(latencies, 95);
    let model = latest.map(|r| r.model.clone());
    let provider = latest.map(|r| r.provider.clone());
    let route_match = latest.and_then(|r| r.route_match.clone());
    let requested_model = latest
        .and_then(|r| r.requested_model.clone())
        .filter(|asked| Some(asked) != model.as_ref());

    let cache_hit_percent = if kpis.request_count > 0 {
        kpis.cache_hit_count.saturating_mul(100) / kpis.request_count
    } else {
        0
    };
    let cost_per_request_display = if kpis.request_count > 0 {
        format::cost(kpis.total_cost_microdollars / kpis.request_count)
    } else {
        format::cost(0)
    };

    let counted = |kind: &str, outcome: &str| {
        trace
            .iter()
            .filter(|r| r.kind == kind && r.outcome == outcome)
            .count() as i64
    };

    Ok(PiStats {
        conversation_id: conversation_id.to_owned(),
        model,
        requested_model,
        provider,
        route_match,
        requests: kpis.request_count,
        errors: kpis.error_count,
        input_tokens: kpis.total_input_tokens,
        output_tokens: kpis.total_output_tokens,
        cache_read_tokens: kpis.total_cache_read_tokens,
        cache_creation_tokens: kpis.total_cache_creation_tokens,
        cache_hit_percent,
        cost_microdollars: kpis.total_cost_microdollars,
        cost_display: format::cost(kpis.total_cost_microdollars),
        cost_per_request_display,
        latency_p50_ms,
        latency_p95_ms,
        latency_last_ms,
        secrets_caught: counts.secret_breaches,
        policy_stages: policy_stages(&stage_rows),
        model_mix: model_mix(&requests),
        allowed: trace.iter().filter(|r| r.outcome == "allow").count() as i64,
        denied: trace.iter().filter(|r| r.outcome == "deny").count() as i64,
        prompts_blocked: counted("prompt", "deny"),
        tools_blocked: counted("tool", "deny"),
        tool_calls: trace.iter().filter(|r| r.kind == "fire").count() as i64,
        credit,
        events: trace
            .into_iter()
            .map(|r| PiStatEvent {
                id: r.id,
                at: r.at,
                kind: r.kind,
                subject: r.subject,
                outcome: r.outcome,
                policy: r.policy,
                detail: r.detail,
            })
            .collect(),
    })
}
