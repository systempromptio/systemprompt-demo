//! Per-IP limits on the two endpoints that mint things.
//!
//! `/session` spawns a process and `/embed-token` mints a credential; every
//! other pi route requires one of those to have succeeded first, so these two
//! are the abuse surface and the only ones throttled. State is in-process —
//! same trade as the governance `rate_limit` policy — and per [`Lane`] so a
//! flood of token mints cannot starve session creation's budget.
//!
//! The client IP comes from `x-forwarded-for`/`x-real-ip`, the same headers
//! the magic-link limiter already trusts on this deployment; behind no proxy
//! at all the socket's peer would be better, but every deployment of this
//! repo serves through one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Idle-bucket sweep threshold: past this many keys, a check also drops every
/// bucket with no hit inside the window, so the map tracks active IPs rather
/// than every IP ever seen.
const SWEEP_AT: usize = 1024;

pub(super) struct Lane {
    limit: usize,
    window: Duration,
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Lane {
    pub(super) fn new(limit: usize, window: Duration) -> Arc<Self> {
        Arc::new(Self {
            limit,
            window,
            buckets: Mutex::new(HashMap::new()),
        })
    }

    fn allow(&self, ip: &str) -> bool {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        let Ok(mut buckets) = self.buckets.lock() else {
            // Why: fail open — a poisoned limiter must not take the demo down.
            return true;
        };
        if buckets.len() > SWEEP_AT {
            buckets.retain(|_, hits| {
                hits.retain(|t| *t > cutoff);
                !hits.is_empty()
            });
        }
        let hits = buckets.entry(ip.to_owned()).or_default();
        hits.retain(|t| *t > cutoff);
        if hits.len() >= self.limit {
            return false;
        }
        hits.push(now);
        true
    }
}

pub(super) async fn per_ip(State(lane): State<Arc<Lane>>, req: Request, next: Next) -> Response {
    // lint-ok: http-error — the 429 is the feature, not a failure to classify
    if lane.limit == 0 || lane.allow(&client_ip(req.headers())) {
        return next.run(req).await;
    }
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(
            axum::http::header::RETRY_AFTER,
            lane.window.as_secs().to_string(),
        )],
        "too many requests from this address",
    )
        .into_response()
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()))
        .map_or_else(|| "unknown".to_owned(), |ip| ip.trim().to_owned())
}
