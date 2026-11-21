//! 라이브 방송 API: 시작, 송출(WHIP), 시청(WHEP), 종료, 끊긴 방송 정리.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use shared::{BroadcastResponse, BroadcastStatus, StartLiveRequest, StartLiveResponse};

use crate::AppState;
use crate::api::{find, to_response};
use crate::auth_api::CurrentUser;
use crate::broadcast::{ensure_owner, normalize_title};
use crate::entity::broadcast;
use crate::error::AppError;
use crate::live::{dropped_streams, ensure_live, media_path, verify_stream_key};
use crate::media::Direction;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/broadcasts/live", post(start))
        .route("/api/broadcasts/{id}/whip", post(publish))
        .route("/api/broadcasts/{id}/whep", post(watch))
        .route("/api/broadcasts/{id}/end", post(end))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

fn sdp(answer: String) -> Response {
    (StatusCode::CREATED, [(header::CONTENT_TYPE, "application/sdp")], answer).into_response()
}

async fn start(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<StartLiveRequest>,
) -> Result<Response, AppError> {
    let stream_key = uuid::Uuid::new_v4().simple().to_string();
    let model = broadcast::ActiveModel {
        title: Set(normalize_title(&req.title)?),
        status: Set(BroadcastStatus::Live.as_str().into()),
        stream_key: Set(Some(stream_key.clone())),
        user_id: Set(Some(user.id)),
        created_at: Set(Utc::now().into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    let location = format!("/api/broadcasts/{}", model.id);
    let body = StartLiveResponse { broadcast: to_response(model, Some(user)), stream_key };
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(body)).into_response())
}

/// 송출은 송출 키로 확인한다. 키는 라이브를 시작한 소유자에게만 한 번 발급된다.
/// (브라우저가 아닌 송출 프로그램(ffmpeg 등)도 쓸 수 있도록 로그인 쿠키 대신 키를 쓴다)
async fn publish(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    offer: String,
) -> Result<Response, AppError> {
    let model = find(&state, id).await?;
    verify_stream_key(model.stream_key.as_deref(), bearer(&headers))?;
    ensure_live(model.status())?;
    Ok(sdp(state.media.exchange_sdp(&media_path(id), Direction::Publish, offer).await?))
}

async fn watch(State(state): State<AppState>, Path(id): Path<i64>, offer: String) -> Result<Response, AppError> {
    ensure_live(find(&state, id).await?.status())?;
    Ok(sdp(state.media.exchange_sdp(&media_path(id), Direction::Read, offer).await?))
}

/// 종료는 소유자 로그인으로만 한다.
async fn end(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> Result<Json<BroadcastResponse>, AppError> {
    let model = find(&state, id).await?;
    ensure_owner(model.user_id, user.id)?;
    let model = if model.status() == BroadcastStatus::Live { mark_ended(&state, model).await? } else { model };
    Ok(Json(to_response(model, Some(user))))
}

async fn mark_ended(state: &AppState, model: broadcast::Model) -> Result<broadcast::Model, AppError> {
    let mut active = model.into_active_model();
    active.status = Set(BroadcastStatus::Ended.as_str().into());
    active.ended_at = Set(Some(Utc::now().into()));
    Ok(active.update(&state.db).await?)
}

/// 방송 중으로 남아 있지만 영상이 끊긴 라이브를 종료 처리하고, 끝낸 방송 id 를 돌려준다.
/// (휴대폰 브라우저를 그냥 닫거나 네트워크가 끊기면 종료 요청이 오지 않는다)
/// 미디어 서버 상태를 모르면 아무 것도 끝내지 않는다.
pub async fn end_dropped_streams(state: &AppState, grace: Duration) -> Result<Vec<i64>, AppError> {
    let ready = state.media.ready_paths().await?;
    let live = broadcast::Entity::find()
        .filter(broadcast::Column::Status.eq(BroadcastStatus::Live.as_str()))
        .all(&state.db)
        .await?;
    let started: Vec<_> = live.iter().map(|b| (b.id, b.created_at.to_utc())).collect();
    let dropped = dropped_streams(&started, &ready, Utc::now(), grace);
    for model in live.into_iter().filter(|b| dropped.contains(&b.id)) {
        mark_ended(state, model).await?;
    }
    Ok(dropped)
}
