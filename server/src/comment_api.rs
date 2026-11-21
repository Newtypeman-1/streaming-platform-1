//! 댓글 API: 방송별 목록(누구나), 작성(로그인), 수정·삭제(작성자).

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, Set};
use shared::{CommentRequest, CommentResponse};

use crate::AppState;
use crate::api::find;
use crate::auth_api::CurrentUser;
use crate::comment::{ensure_author, normalize_body};
use crate::entity::{comment, user};
use crate::error::{AppError, JsonBody};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/broadcasts/{id}/comments", get(list).post(write))
        .route("/api/comments/{id}", patch(edit).delete(remove))
}

fn to_response(model: comment::Model, author: user::Model) -> CommentResponse {
    CommentResponse {
        id: model.id,
        broadcast_id: model.broadcast_id,
        author: author.into(),
        body: model.body,
        created_at: model.created_at.to_rfc3339(),
        updated_at: model.updated_at.map(|t| t.to_rfc3339()),
    }
}

async fn list(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<Vec<CommentResponse>>, AppError> {
    find(&state, id).await?;
    let comments = comment::Entity::find()
        .filter(comment::Column::BroadcastId.eq(id))
        .find_also_related(user::Entity)
        .order_by_asc(comment::Column::Id)
        .all(&state.db)
        .await?;
    // 작성자는 NOT NULL 외래 키라 항상 있다
    Ok(Json(comments.into_iter().filter_map(|(c, author)| Some(to_response(c, author?))).collect()))
}

async fn write(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    JsonBody(req): JsonBody<CommentRequest>,
) -> Result<Response, AppError> {
    find(&state, id).await?;
    let model = comment::ActiveModel {
        broadcast_id: Set(id),
        user_id: Set(user.id),
        body: Set(normalize_body(&req.body)?),
        created_at: Set(Utc::now().into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    let location = shared::comment_url(model.id);
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(to_response(model, user))).into_response())
}

async fn find_comment(state: &AppState, id: i64) -> Result<comment::Model, AppError> {
    comment::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("댓글을 찾을 수 없습니다: {id}")))
}

async fn edit(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
    JsonBody(req): JsonBody<CommentRequest>,
) -> Result<Json<CommentResponse>, AppError> {
    let model = find_comment(&state, id).await?;
    ensure_author(model.user_id, user.id)?;
    let mut active = model.into_active_model();
    active.body = Set(normalize_body(&req.body)?);
    active.updated_at = Set(Some(Utc::now().into()));
    Ok(Json(to_response(active.update(&state.db).await?, user)))
}

async fn remove(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let model = find_comment(&state, id).await?;
    ensure_author(model.user_id, user.id)?;
    comment::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
