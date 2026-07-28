//! Serving a tool call's persisted artifact back to the widget.
//!
//! Two views of the same `mcp_artifacts` row: the raw JSON, and the same
//! server-side render the MCP `resources/read` path produces — so the preview
//! in the terminal is byte-for-byte what an MCP host would show, not a second
//! renderer to keep honest.
//!
//! The token travels as a query parameter because the HTML view is loaded via
//! `<iframe src>`, which cannot set headers — the same trade the SSE stream
//! route already makes. Ownership is enforced in SQL (`user_id` in the WHERE
//! clause), so a wrong-owner id and an unknown id are the same 404.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sqlx::PgPool;
use systemprompt::identifiers::{ArtifactId, ContextId};
use systemprompt::mcp::services::ui_renderer::{MCP_APP_MIME_TYPE, RenderTarget};

use axum::http::HeaderMap;
use serde::Deserialize;
use systemprompt::identifiers::UserId;

use super::auth::{authenticate, problem, unauthorized};
use systemprompt_web_governance::identity::extract_user_from_cookie;
use crate::repositories::artifacts::{McpArtifactRow, find_artifact_for_user};

// Why: the embed token is optional here, unlike on the stream — the
// audit-trail page links to these routes from an admin session, where the
// identity is the cookie's. Either credential resolves to the same `user_id`
// filter.
#[derive(Debug, Deserialize)]
pub(super) struct OptionalTokenQuery {
    token: Option<String>,
}

#[derive(Debug, Serialize)]
struct ArtifactView {
    artifact_id: ArtifactId,
    artifact_type: String,
    title: Option<String>,
    server_name: String,
    created_at: chrono::DateTime<chrono::Utc>,
    // JSON: the stored tool response, shape owned by the tool that made it
    data: serde_json::Value,
}

pub(super) async fn show(
    State(pool): State<Arc<PgPool>>,
    Path(artifact_id): Path<ArtifactId>,
    Query(q): Query<OptionalTokenQuery>,
    headers: HeaderMap,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let row = match fetch(&pool, &artifact_id, q.token.as_deref(), &headers).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    Json(ArtifactView {
        artifact_id: row.artifact_id,
        artifact_type: row.artifact_type,
        title: row.title,
        server_name: row.server_name,
        created_at: row.created_at,
        data: row.data,
    })
    .into_response()
}

pub(super) async fn show_ui(
    State(pool): State<Arc<PgPool>>,
    Path(artifact_id): Path<ArtifactId>,
    Query(q): Query<OptionalTokenQuery>,
    headers: HeaderMap,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let row = match fetch(&pool, &artifact_id, q.token.as_deref(), &headers).await {
        Ok(row) => row,
        Err(response) => return response,
    };

    // Why: the stored row wraps the typed payload in the full tool response;
    // the renderer wants the payload alone.
    let Some(payload) = row.data.get("artifact") else {
        return problem(StatusCode::UNPROCESSABLE_ENTITY, "artifact has no payload");
    };

    let target = RenderTarget {
        artifact_id: &row.artifact_id,
        artifact_type: &row.artifact_type,
        payload,
        context_id: row.context_id.clone().unwrap_or_else(ContextId::generate),
        title: row.title.clone(),
    };

    match systemprompt::mcp::services::ui_renderer::artifact_ui_resource(&target).await {
        Ok(resource) => {
            let mut response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, MCP_APP_MIME_TYPE)
                .header(
                    header::CONTENT_SECURITY_POLICY,
                    resource.csp.to_header_value(),
                )
                .body(axum::body::Body::from(resource.html))
                // lint-ok: http-error — headers are static, so this arm is unreachable
                .unwrap_or_else(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "render failed"));
            // Why: the global security middleware stamps X-Frame-Options: DENY
            // sitewide and ignores a raw header; this marker is the sanctioned
            // way to say "the terminal may iframe this, others may not".
            response
                .extensions_mut()
                .insert(systemprompt::extension::FrameOptionsOverride(
                    systemprompt::extension::FrameOptions::SameOrigin,
                ));
            response
        },
        Err(e) => {
            tracing::warn!(error = %e, artifact_id = %row.artifact_id, "artifact render failed");
            problem(StatusCode::UNPROCESSABLE_ENTITY, "unrenderable artifact")
        },
    }
}

async fn fetch(
    pool: &Arc<PgPool>,
    artifact_id: &ArtifactId,
    token: Option<&str>,
    headers: &HeaderMap,
) -> Result<McpArtifactRow, Response> {
    let Some(user_id) = identify(pool, token, headers).await else {
        return Err(unauthorized());
    };
    find_artifact_for_user(pool, artifact_id, &user_id)
        .await
        .inspect_err(
            |e| tracing::warn!(error = %e, artifact_id = %artifact_id, "artifact lookup failed"),
        )
        .ok()
        .flatten()
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "no such artifact"))
}

async fn identify(pool: &Arc<PgPool>, token: Option<&str>, headers: &HeaderMap) -> Option<UserId> {
    if let Some(token) = token {
        return authenticate(pool, token).await;
    }
    extract_user_from_cookie(headers).ok().map(|s| s.user_id)
}
