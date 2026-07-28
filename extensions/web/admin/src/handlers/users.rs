//! HTTP handlers for user administration.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;

use systemprompt::identifiers::UserId;

use crate::activity::{self, ActivityEntity, NewActivity};
use crate::error::{AdminError, AdminResult};
use crate::repositories;
use crate::types::{CreateUserRequest, EventsQuery, UpdateUserRequest, UserContext};

pub(crate) use systemprompt_web_governance::identity::extract_user_from_cookie;

use super::responses::{EventsListResponse, UsersListResponse};

pub(crate) async fn dashboard_handler(State(pool): State<Arc<PgPool>>) -> AdminResult<Response> {
    let data =
        repositories::dashboard::get_dashboard_data(&pool, "7 days", "4 hours", "today", "7d")
            .await?;
    Ok(Json(data).into_response())
}

pub(crate) async fn list_users_handler(State(pool): State<Arc<PgPool>>) -> AdminResult<Response> {
    let users = repositories::users::queries::list_users(&pool).await?;
    Ok(Json(UsersListResponse { users }).into_response())
}

pub(crate) async fn user_detail_handler(
    State(pool): State<Arc<PgPool>>,
    Path(user_id_raw): Path<String>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    let detail = repositories::users::queries::find_user_detail(&pool, &user_id)
        .await?
        .ok_or_else(|| AdminError::NotFound("User not found".to_owned()))?;
    Ok(Json(detail).into_response())
}

pub(crate) async fn user_usage_handler(
    State(pool): State<Arc<PgPool>>,
    Path(user_id_raw): Path<String>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    let events = repositories::users::queries::list_user_usage(&pool, &user_id).await?;
    Ok(Json(EventsListResponse { events }).into_response())
}

pub(crate) async fn create_user_handler(
    State(pool): State<Arc<PgPool>>,
    Extension(user_ctx): Extension<UserContext>,
    Json(body): Json<CreateUserRequest>,
) -> AdminResult<Response> {
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    let user = repositories::users::mutations::create_user(&*pool, &body).await?;
    // Why: an admin creating an account by hand has already made the decision
    // the review gate exists to capture, so it is approved on the spot rather
    // than landing in its creator's own queue.
    repositories::users::approvals::approve_on_create(&pool, &user.user_id, &user_ctx.user_id)
        .await;
    let p = Arc::clone(&pool);
    let uid = user_ctx.user_id.clone();
    let new_user_id = user.user_id.clone();
    let name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.user_id.as_str().to_owned());
    tokio::spawn(async move {
        activity::record(
            &p,
            NewActivity::entity_created(&uid, ActivityEntity::User, new_user_id.as_str(), &name),
        )
        .await;
    });
    Ok((StatusCode::CREATED, Json(user)).into_response())
}

pub(crate) async fn update_user_handler(
    State(pool): State<Arc<PgPool>>,
    Extension(user_ctx): Extension<UserContext>,
    Path(user_id_raw): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    let user = repositories::users::mutations::update_user(&pool, &user_id, &body)
        .await?
        .ok_or_else(|| AdminError::NotFound("User not found".to_owned()))?;
    let p = Arc::clone(&pool);
    let uid = user_ctx.user_id.clone();
    let target_user_id = user.user_id.clone();
    let name = user
        .display_name
        .clone()
        .unwrap_or_else(|| user.user_id.as_str().to_owned());
    tokio::spawn(async move {
        activity::record(
            &p,
            NewActivity::entity_updated(&uid, ActivityEntity::User, target_user_id.as_str(), &name),
        )
        .await;
    });
    Ok(Json(user).into_response())
}

pub(crate) async fn delete_user_handler(
    State(pool): State<Arc<PgPool>>,
    Extension(user_ctx): Extension<UserContext>,
    Path(user_id_raw): Path<String>,
) -> AdminResult<Response> {
    let user_id = UserId::new(user_id_raw);
    if !user_ctx.is_admin {
        return Err(AdminError::Forbidden("Admin access required".to_owned()));
    }
    if !repositories::users::mutations::delete_user(&pool, &user_id).await? {
        return Err(AdminError::NotFound("User not found".to_owned()));
    }
    let p = Arc::clone(&pool);
    let uid = user_ctx.user_id.clone();
    let target = user_id.clone();
    tokio::spawn(async move {
        activity::record(
            &p,
            NewActivity::entity_deleted(
                &uid,
                ActivityEntity::User,
                target.as_str(),
                target.as_str(),
            ),
        )
        .await;
    });
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn list_events_handler(
    State(pool): State<Arc<PgPool>>,
    Query(query): Query<EventsQuery>,
) -> AdminResult<Response> {
    let response = repositories::dashboard::list_events(&pool, &query).await?;
    Ok(Json(response).into_response())
}
