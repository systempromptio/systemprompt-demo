//! Deriving the pane's display facets from the repository rows.
//!
//! Everything here is a pure projection of what the queries in the parent
//! module already returned; nothing reaches the database except the credit
//! balance, which has its own extension-level cache.

use sqlx::PgPool;

use super::super::format;
use super::{PiCredit, PiModelShare, PiPolicyStage};
use crate::repositories::analytics::session_detail;
use crate::repositories::governance::{demo_trace, stages};

pub(super) async fn credit_position(
    pool: &PgPool,
    user_id: &systemprompt::identifiers::UserId,
) -> PiCredit {
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


pub(super) fn policy_stages(
    rows: &[crate::repositories::governance::PerPolicyCounts],
) -> Vec<PiPolicyStage> {
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


pub(super) fn model_mix(requests: &[session_detail::SessionRequestRow]) -> Vec<PiModelShare> {
    // Why: a request rejected before routing has no model, and counting it in
    // the denominator would make the shares of the models that did run add up
    // to less than 100% with nothing on screen to explain the shortfall.
    let mut tally: Vec<(String, i64)> = Vec::new();
    let mut total = 0i64;
    for model in requests.iter().filter_map(|r| r.model.as_ref()) {
        total += 1;
        match tally.iter_mut().find(|(m, _)| m == model) {
            Some((_, n)) => *n += 1,
            None => tally.push((model.clone(), 1)),
        }
    }
    if total == 0 {
        return Vec::new();
    }
    tally.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    tally
        .into_iter()
        .map(|(model, requests)| PiModelShare {
            model,
            percent: requests.saturating_mul(100) / total,
            requests,
        })
        .collect()
}


pub(super) struct TraceCounts {
    pub(super) allowed: i64,
    pub(super) denied: i64,
    pub(super) prompts_blocked: i64,
    pub(super) tools_blocked: i64,
    pub(super) tool_calls: i64,
}

pub(super) fn trace_counts(trace: &[demo_trace::DemoTraceRow]) -> TraceCounts {
    let counted = |kind: &str, outcome: &str| {
        trace
            .iter()
            .filter(|r| r.kind == kind && r.outcome == outcome)
            .count() as i64
    };
    TraceCounts {
        allowed: trace.iter().filter(|r| r.outcome == "allow").count() as i64,
        denied: trace.iter().filter(|r| r.outcome == "deny").count() as i64,
        prompts_blocked: counted("prompt", "deny"),
        tools_blocked: counted("tool", "deny"),
        tool_calls: trace.iter().filter(|r| r.kind == "fire").count() as i64,
    }
}

pub(super) struct Facets {
    pub(super) latency_last_ms: Option<i32>,
    pub(super) latency_p50_ms: Option<i32>,
    pub(super) latency_p95_ms: Option<i32>,
    pub(super) model: Option<String>,
    pub(super) provider: Option<String>,
    pub(super) route_match: Option<String>,
    pub(super) requested_model: Option<String>,
}

pub(super) fn facets(requests: &[session_detail::SessionRequestRow]) -> Facets {
    let latest = requests.first();
    let latencies: Vec<i32> = requests.iter().filter_map(|r| r.latency_ms).collect();
    let model = latest.and_then(|r| r.model.clone());
    Facets {
        latency_last_ms: latest.and_then(|r| r.latency_ms),
        latency_p50_ms: format::median(latencies.clone()),
        latency_p95_ms: format::percentile(latencies, 95),
        provider: latest.and_then(|r| r.provider.clone()),
        route_match: latest.and_then(|r| r.route_match.clone()),
        requested_model: latest
            .and_then(|r| r.requested_model.clone())
            .filter(|asked| Some(asked) != model.as_ref()),
        model,
    }
}
