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
use systemprompt::identifiers::ContextId;

use super::auth::{authenticate, authorize_conversation, problem, unauthorized};
use super::registry::PiRegistry;
use crate::repositories::{conversations as repo, events as event_repo};

const LIST_LIMIT: i64 = 50;

const HISTORY_LIMIT: i64 = 2000;

const MAX_TITLE: usize = 120;

#[derive(Debug, Deserialize)]
pub(super) struct TokenOnly {
    token: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    token: String,
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
    id: ContextId,
    title: Option<String>,
    last_seq: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    live: bool,
}

#[derive(Debug, Serialize)]
struct HistoryPage {
    conversation_id: ContextId,
    title: Option<String>,
    last_seq: i64,
    more: bool,
    events: Vec<serde_json::Value>,
}

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

pub(super) async fn history(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<ContextId>,
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
            let last_seq = events.last().map_or(0, |e| e.seq);
            Json(HistoryPage {
                conversation_id,
                title: row.title,
                last_seq,
                more,
                events: collapse_duplicate_errors(events.into_iter().map(|e| e.body).collect()),
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

/// The read-side mirror of `PiSession::is_repeat_error`, for stored rows the
/// emit-level dedupe never saw.
///
/// Error frames are upgraded to the current
/// vocabulary first (`events::upgrade_legacy_error`) so differently-worded
/// duplicates of one failed request become identical and collapse.
/// `turn_start`/`turn_end` are transparent for the same reason as in the
/// live path: they are what a provider retry interleaves. A turn stripped
/// empty by that suppression — retries carry nothing but the duplicate — is
/// dropped whole, so a restored transcript does not replay a run of no-op
/// busy flickers.
pub fn collapse_duplicate_errors(events: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut out = Vec::with_capacity(events.len());
    let mut last_error: Option<serde_json::Value> = None;
    let mut pending_turn_start: Option<serde_json::Value> = None;
    for mut event in events {
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("error") => {
                super::events::upgrade_legacy_error(&mut event);
                if last_error.as_ref().is_some_and(|l| same_error(l, &event)) {
                    continue;
                }
                last_error = Some(event.clone());
                out.extend(pending_turn_start.take());
                out.push(event);
            },
            Some("turn_start") => {
                out.extend(pending_turn_start.take());
                pending_turn_start = Some(event);
            },
            Some("turn_end") => {
                if pending_turn_start.take().is_some() {
                    continue;
                }
                out.push(event);
            },
            _ => {
                last_error = None;
                out.extend(pending_turn_start.take());
                out.push(event);
            },
        }
    }
    // Why: a conversation captured mid-turn ends on its `turn_start`; it must
    // survive so the replayed view still shows the turn in progress.
    out.extend(pending_turn_start);
    out
}

fn same_error(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    ["kind", "code", "message"]
        .iter()
        .all(|field| a.get(field) == b.get(field))
}

pub(super) async fn rename(
    State(pool): State<Arc<PgPool>>,
    Path(conversation_id): Path<ContextId>,
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

pub(super) async fn remove(
    State(pool): State<Arc<PgPool>>,
    Extension(registry): Extension<PiRegistry>,
    Path(conversation_id): Path<ContextId>,
    Json(body): Json<TokenOnly>,
) -> Response {
    // lint-ok: http-error — this module hand-shapes opaque statuses on purpose
    let Some(user_id) = authenticate(&pool, &body.token).await else {
        return unauthorized();
    };
    match repo::delete_conversation(&pool, &conversation_id, &user_id).await {
        Ok(0) => problem(StatusCode::NOT_FOUND, "no such conversation"),
        Ok(_) => {
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
