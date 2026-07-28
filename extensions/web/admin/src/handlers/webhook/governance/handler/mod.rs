//! Governance webhook entrypoint: authenticate, evaluate the policy chain, and
//! record an audit row before returning the `PreToolUse` decision.

mod authn;
pub(super) mod evaluate;

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use sqlx::PgPool;
use systemprompt::api::services::middleware::attest_session;
use systemprompt::identifiers::{SessionId, UserId};
use systemprompt::traits::{AnalyticsProvider, SessionAnalytics};
use systemprompt_security::authz::Decision;
use systemprompt_security::policy::types::AccessScope;

use crate::types::webhook::{GovernQuery, HookEventPayload};

use super::types::{
    AuditTarget, AuthDenialParams, ChainEntryOutcome, ChainEntryResult, DecisionAudit,
    GovernanceDecision, GovernanceDeps, GovernanceResponse, HookSpecificOutput, PrincipalSnapshot,
};
use super::{audit, scope};

use authn::{authenticate_request, deny_for_auth_failure};
use evaluate::{EvaluateInput, evaluate};

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

fn build_response(decision: &Decision) -> Response {
    // lint-ok: http-error — builds the decision body itself
    let permission_decision = GovernanceDecision::from_decision(decision);
    let permission_decision_reason = match decision {
        Decision::Allow { .. } => None,
        Decision::Deny { reason } => Some(format!("[GOVERNANCE] {reason}")),
    };
    let response = GovernanceResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PreToolUse",
            permission_decision,
            permission_decision_reason,
        },
    };
    (StatusCode::OK, Json(response)).into_response()
}

const UNATTESTED_PREFIX: &str = "unattested_";

pub(super) async fn attested_session_id(
    analytics: &Arc<dyn AnalyticsProvider>,
    claimed: &SessionId,
    user_id: &UserId,
) -> SessionId {
    match attest_session(analytics, claimed, user_id, "hooks/govern").await {
        Ok(()) => claimed.clone(),
        Err(e) => {
            tracing::warn!(
                claimed_session_id = %claimed,
                user_id = %user_id,
                reason = %e,
                "govern hook credential names a session the server cannot attest; \
                 auditing under an unattested id"
            );
            SessionId::new(format!("{UNATTESTED_PREFIX}{claimed}"))
        },
    }
}

pub(crate) async fn govern_tool_use(
    State(pool): State<Arc<PgPool>>,
    Extension(deps): Extension<GovernanceDeps>,
    headers: HeaderMap,
    Query(query): Query<GovernQuery>,
    // JSON: hook bodies must never 422 — `from_value` degrades leniently
    Json(raw): Json<serde_json::Value>,
) -> Response {
    // lint-ok: http-error — a hook answers 200 with a decision; an error status
    // reads as "hook unavailable" and lets the call through
    let (payload, _warnings) = HookEventPayload::from_value(raw);
    let GovernanceDeps {
        session_service,
        analytics,
    } = deps;

    let tool_name = payload.tool_name().unwrap_or("unknown");
    let agent_session = SessionId::new(payload.session_id());
    let agent_id = payload.common.agent_id.as_ref();
    let plugin_id = query.plugin_id.as_ref();

    let denial_params = AuthDenialParams {
        pool: &pool,
        session_id: &agent_session,
        tool_name,
        agent_id,
        plugin_id,
        session_service: &session_service,
        headers: &headers,
    };

    let principal = match authenticate_request(&headers, &denial_params) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let user_id = principal.user_id;
    let session_id = attested_session_id(&analytics, &principal.session_id, &user_id).await;

    let db_scope = scope::scope_from_user_roles(&pool, &user_id).await;
    let principal_scope = scope::higher_privilege(principal.token_scope, db_scope);
    let access_scope = agent_id.map_or(principal_scope, |id| {
        scope::higher_privilege(principal_scope, scope::resolve_agent_scope(id))
    });

    // Why: the rate-limit policy keys on the *agent's* session, not the
    // credential's — one long-lived plugin token drives many agent runs, and a
    // per-credential bucket would let one runaway run throttle every other.
    let (decision, chain) = evaluate(&EvaluateInput {
        tool_name,
        session_id: &agent_session,
        user_id: &user_id,
        access_scope,
        tool_input: payload.tool_input(),
    });

    let audit = DecisionAudit {
        decision: decision.clone(),
        principal: PrincipalSnapshot {
            user_id,
            session_id: session_id.clone(),
            agent_session: Some(agent_session),
            agent_id: agent_id.cloned(),
            agent_scope: access_scope,
        },
        target: AuditTarget {
            tool_name: tool_name.to_owned(),
            plugin_id: plugin_id.cloned(),
        },
        chain,
    };
    spawn_audit_recording(&pool, audit);

    build_response(&decision)
}

fn spawn_auth_denial(params: &AuthDenialParams<'_>, reason: &str) {
    let pool = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(params.pool);
    let reason = reason.to_owned();
    let session_id = params.session_id.clone();
    let tool_name = params.tool_name.to_owned();
    let agent_id = params.agent_id.cloned();
    let plugin_id = params.plugin_id.cloned();
    let session_service = Arc::clone(params.session_service);
    let headers = params.headers.clone();

    tokio::spawn(async move {
        // Why: Every UserId must be a real `users` row, so provision the anonymous
        // principal for this fingerprint (idempotent upsert) to carry the audit's
        // foreign key. Only user agent + locale are set because
        // `compute_fingerprint` falls back to exactly those two signals.
        let analytics = SessionAnalytics {
            user_agent: header_str(&headers, header::USER_AGENT),
            preferred_locale: header_str(&headers, header::ACCEPT_LANGUAGE),
            ..SessionAnalytics::default()
        };
        let user_id = match session_service.ensure_anonymous_user(&analytics).await {
            Ok((uid, _fingerprint)) => uid,
            Err(e) => {
                tracing::error!(
                    target: "governance.audit.write_failed",
                    error = %e,
                    session_id = %session_id,
                    "could not resolve anonymous principal; auth-denial audit dropped",
                );
                return;
            },
        };
        let audit = DecisionAudit {
            decision: deny_for_auth_failure(&reason),
            principal: PrincipalSnapshot {
                user_id,
                // Why: this path fires *because* authentication failed, so there
                // is no credential to attest a session from. Prefixing keeps the
                // invariant that an un-prefixed `session_id` in this table was
                // always checked against `user_sessions`.
                session_id: SessionId::new(format!("{UNATTESTED_PREFIX}{session_id}")),
                agent_session: Some(session_id.clone()),
                agent_id,
                agent_scope: AccessScope::Unknown,
            },
            target: AuditTarget {
                tool_name,
                plugin_id,
            },
            chain: vec![ChainEntryOutcome {
                policy_id: systemprompt::identifiers::PolicyId::new("authentication"),
                result: ChainEntryResult::Fail,
                detail: reason,
            }],
        };
        if let Err(e) = audit::record_decision(&pool, &audit).await {
            tracing::error!(
                target: "governance.audit.write_failed",
                error = %e,
                session_id = %session_id,
                "governance audit write failed; row dropped",
            );
        }
    });
}

fn spawn_audit_recording(pool: &Arc<PgPool>, audit: DecisionAudit) {
    let p = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(pool);
    tokio::spawn(async move {
        let session_id = audit.principal.session_id.clone();
        if let Err(e) = audit::record_decision(&p, &audit).await {
            tracing::error!(
                target: "governance.audit.write_failed",
                error = %e,
                session_id = %session_id,
                "governance audit write failed; row dropped",
            );
        }
    });
}
