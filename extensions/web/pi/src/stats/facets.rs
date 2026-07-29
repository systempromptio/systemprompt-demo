//! Deriving the pane's display facets from the repository rows.
//!
//! Everything here is a pure projection of what the queries in the parent
//! module already returned; nothing reaches the database except the credit
//! balance, which has its own extension-level cache.

use sqlx::PgPool;

use super::super::format;
use super::{PiCredit, PiModelShare, PiPolicyStage};
use systemprompt_web_governance::repositories::analytics::session_detail;
use systemprompt_web_governance::repositories::governance::stages;

pub(super) async fn credit_position(
    pool: &PgPool,
    user_id: &systemprompt::identifiers::UserId,
) -> PiCredit {
    let balance = systemprompt_credits::get_balance(pool, user_id.as_str())
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not read a credit balance for the pi stats pane");
            systemprompt_credits::CreditBalance {
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
    rows: &[systemprompt_web_governance::repositories::governance::PerPolicyCounts],
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


#[derive(Default)]
struct ModelTally {
    requests: i64,
    latency_sum_ms: i64,
    latency_count: i64,
    cost_microdollars: i64,
}

pub(super) fn model_mix(requests: &[session_detail::SessionRequestRow]) -> Vec<PiModelShare> {
    // Why: a request rejected before routing has no model, and counting it in
    // the denominator would make the shares of the models that did run add up
    // to less than 100% with nothing on screen to explain the shortfall.
    let mut tally: Vec<(String, ModelTally)> = Vec::new();
    let mut total = 0i64;
    for row in requests {
        let Some(model) = row.model.as_ref() else {
            continue;
        };
        total += 1;
        let idx = tally
            .iter()
            .position(|(m, _)| m == model)
            .unwrap_or_else(|| {
                tally.push((model.clone(), ModelTally::default()));
                tally.len() - 1
            });
        let entry = &mut tally[idx].1;
        entry.requests += 1;
        entry.cost_microdollars += row.cost_microdollars;
        if let Some(latency) = row.latency_ms {
            entry.latency_sum_ms += i64::from(latency);
            entry.latency_count += 1;
        }
    }
    if total == 0 {
        return Vec::new();
    }
    tally.sort_by_key(|(_, t)| std::cmp::Reverse(t.requests));
    tally
        .into_iter()
        .map(|(model, t)| PiModelShare {
            model,
            percent: t.requests.saturating_mul(100) / total,
            requests: t.requests,
            avg_latency_ms: (t.latency_count > 0)
                .then(|| i32::try_from(t.latency_sum_ms / t.latency_count).unwrap_or(i32::MAX)),
            cost_display: format::cost(t.cost_microdollars),
        })
        .collect()
}

pub(super) struct Facets {
    pub(super) latency_last_ms: Option<i32>,
    pub(super) latency_p50_ms: Option<i32>,
    pub(super) latency_p95_ms: Option<i32>,
    pub(super) latency_avg_ms: Option<i32>,
    pub(super) model: Option<String>,
    pub(super) provider: Option<String>,
    pub(super) route_match: Option<String>,
    pub(super) requested_model: Option<String>,
}

pub(super) fn facets(requests: &[session_detail::SessionRequestRow]) -> Facets {
    let latest = requests.first();
    let latencies: Vec<i32> = requests.iter().filter_map(|r| r.latency_ms).collect();
    let latency_avg_ms = (!latencies.is_empty()).then(|| {
        let sum: i64 = latencies.iter().copied().map(i64::from).sum();
        i32::try_from(sum / latencies.len() as i64).unwrap_or(i32::MAX)
    });
    let model = latest.and_then(|r| r.model.clone());
    Facets {
        latency_last_ms: latest.and_then(|r| r.latency_ms),
        latency_p50_ms: format::median(latencies.clone()),
        latency_p95_ms: format::percentile(latencies, 95),
        latency_avg_ms,
        provider: latest.and_then(|r| r.provider.clone()),
        route_match: latest.and_then(|r| r.route_match.clone()),
        requested_model: latest
            .and_then(|r| r.requested_model.clone())
            .filter(|asked| Some(asked) != model.as_ref()),
        model,
    }
}
