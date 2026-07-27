//! Listing, replaying, naming and hiding a viewer's own conversations.
//!
//! None of these routes need a running child, so all four authorize through
//! [`authorize_conversation`] — a query against `pi_conversations` scoped to
//! the token's user — rather than through the live-session registry. That is
//! what makes a reload non-destructive: it displaces the process, not the
//! record.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::auth::{authenticate, authorize_conversation, problem, unauthorized};
use super::registry::PiRegistry;
use crate::repositories::pi::{conversations as repo, events as event_repo};

/// How many conversations the picker shows. A visitor accumulates these one
/// reload at a time, and a list longer than this is an archive, not a picker.
const LIST_LIMIT: i64 = 50;

/// Frames returned in one history page. A long conversation is fetched in
/// several requests rather than one response the browser has to parse at once.
const HISTORY_LIMIT: i64 = 2000;

/// Longest title a viewer may set.
const MAX_TITLE: usize = 120;

#[derive(Debug, Deserialize)]
pub(super) struct TokenOnly {
    token: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    token: String,
    /// Resume point. Defaults to the beginning, so an ordinary restore asks for
    /// everything and a reconnect asks only for what it missed.
    #[serde(default)]
    after_seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RenameBody {
    token: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct ConversationSummary {
    id: String,
    title: Option<String>,
    last_seq: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    live: bool,
}

#[derive(Debug, Serialize)]
struct HistoryPage {
    conversation_id: String,
    title: Option<String>,
    /// Highest `seq` in this page, which is where the caller should attach the
    /// live stream. Zero for an empty conversation.
    last_seq: i64,
    /// True when the page stopped at [`HISTORY_LIMIT`] rather than at the end.
    more: bool,
    /// Whole frames in the shape the SSE stream sends, so the widget replays
    /// them through the renderers it already has.
    events: Vec<serde_json::Value>,
}

/// `GET /api/public/pi/conversations?token=…`
pub(super) async fn list(State(pool): State<Arc<PgPool>>, Query(q): Query<TokenOnly>) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &q.token).await else {
        return unauthorized();
    };
    match repo::list_conversations(&pool, &user_id, LIST_LIMIT).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|r| ConversationSummary {
                    id: r.id,
                    title: r.title,
                    last_seq: r.last_seq,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    live: r.live,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not list pi conversations");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not list conversations",
            )
        },
    }
}

/// `GET /api/public/pi/conversations/{id}/history?token=…&after_seq=…`
pub(super) async fn history(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(row) = authorize_conversation(&pool, &q.token, &conversation_id).await else {
        return problem(StatusCode::NOT_FOUND, "no such conversation");
    };
    let after = q.after_seq.unwrap_or(0);
    match event_repo::list_conversation_events(&pool, &conversation_id, after, HISTORY_LIMIT).await
    {
        Ok(events) => {
            let more = i64::try_from(events.len()).unwrap_or(i64::MAX) == HISTORY_LIMIT;
            // The page's own high-water mark, not the conversation's: a truncated
            // page must not tell the caller to attach the live stream past frames
            // it has not been sent.
            let last_seq = events.last().map_or(0, |e| e.seq);
            Json(HistoryPage {
                conversation_id,
                title: row.title,
                last_seq,
                more,
                events: events.into_iter().map(|e| e.body).collect(),
            })
            .into_response()
        },
        Err(e) => {
            tracing::error!(error = %e, "could not read a pi conversation's history");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not read history",
            )
        },
    }
}

/// `PATCH /api/public/pi/conversations/{id}`
pub(super) async fn rename(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &body.token).await else {
        return unauthorized();
    };
    let title: String = body.title.trim().chars().take(MAX_TITLE).collect();
    if title.is_empty() {
        return problem(StatusCode::BAD_REQUEST, "a title cannot be empty");
    }
    match repo::update_conversation_title(&pool, &conversation_id, &user_id, &title).await {
        // Zero rows is "not yours" or "not there", conflated deliberately.
        Ok(0) => problem(StatusCode::NOT_FOUND, "no such conversation"),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "could not rename a pi conversation");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not rename the conversation",
            )
        },
    }
}

/// `DELETE /api/public/pi/conversations/{id}`
///
/// Hides the conversation and ends its child if one is running. The row is
/// soft-deleted rather than dropped: the governance and spend rows it explains
/// are not the viewer's to erase, and a transcript that vanished while its
/// audit trail remained would leave the two disagreeing.
pub(super) async fn remove(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<String>,
    Json(body): Json<TokenOnly>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &body.token).await else {
        return unauthorized();
    };
    match repo::delete_conversation(&pool, &conversation_id, &user_id).await {
        Ok(0) => problem(StatusCode::NOT_FOUND, "no such conversation"),
        Ok(_) => {
            // After the row is hidden, so a child that outlives the request
            // cannot write more frames into a conversation the viewer deleted.
            registry.remove(&conversation_id, None).await;
            StatusCode::NO_CONTENT.into_response()
        },
        Err(e) => {
            tracing::error!(error = %e, "could not delete a pi conversation");
            problem(
                StatusCode::INTERNAL_SERVER_ERROR, /* lint-ok: http-error — logged above; the
                                                    * client is told nothing about why */
                "could not delete the conversation",
            )
        },
    }
}
