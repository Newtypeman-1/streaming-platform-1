use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::TryStreamExt;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};
use shared::{BroadcastResponse, BroadcastStatus};
use tokio_util::io::StreamReader;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::AppState;
use crate::broadcast::{BroadcastError, normalize_title, validate_video_content_type};
use crate::entity::broadcast;
use crate::error::AppError;

const MAX_UPLOAD_BYTES: usize = 2 * 1024 * 1024 * 1024; // 2GB

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/broadcasts",
            get(list).post(register).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route("/api/broadcasts/{id}", get(detail))
        .route("/api/broadcasts/{id}/video", get(video))
        .merge(crate::live_api::routes())
        .merge(crate::auth_api::routes())
        .with_state(state)
}

pub(crate) fn to_response(model: broadcast::Model) -> BroadcastResponse {
    BroadcastResponse {
        id: model.id,
        status: model.status(),
        title: model.title,
        video_url: model.video_key.is_some().then(|| shared::video_url(model.id)),
        created_at: model.created_at.to_rfc3339(),
        ended_at: model.ended_at.map(|t| t.to_rfc3339()),
    }
}

pub(crate) async fn find(state: &AppState, id: i64) -> Result<broadcast::Model, AppError> {
    broadcast::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("방송을 찾을 수 없습니다: {id}")))
}

async fn register(State(state): State<AppState>, multipart: Multipart) -> Result<Response, AppError> {
    let mut saved_key = None;
    let result = save_broadcast(&state, multipart, &mut saved_key).await;
    // 어느 단계에서 실패하든, 이미 디스크에 쓴 영상은 지워서 고아 파일을 남기지 않는다.
    if result.is_err()
        && let Some(key) = saved_key
        && let Err(e) = state.storage.delete(&key).await
    {
        tracing::warn!("업로드 실패 후 영상 정리 실패 ({key}): {e}");
    }
    let created = result?;
    let location = format!("/api/broadcasts/{}", created.id);
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(to_response(created))).into_response())
}

async fn save_broadcast(
    state: &AppState,
    mut multipart: Multipart,
    saved_key: &mut Option<String>,
) -> Result<broadcast::Model, AppError> {
    let mut title = None;
    let mut content_type = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("title") => title = Some(field.text().await?),
            Some("video") => {
                if saved_key.is_some() {
                    return Err(AppError::BadRequest("영상은 하나만 올릴 수 있습니다.".into()));
                }
                let ct = field.content_type().map(str::to_owned);
                validate_video_content_type(ct.as_deref())?;
                let filename = field.file_name().map(str::to_owned);
                let reader = StreamReader::new(field.map_err(std::io::Error::other));
                *saved_key = Some(state.storage.save(filename.as_deref(), reader).await?);
                content_type = ct;
            }
            _ => {}
        }
    }

    let title = normalize_title(title.as_deref().unwrap_or_default())?;
    let (Some(key), Some(content_type)) = (saved_key.clone(), content_type) else {
        return Err(AppError::BadRequest("영상 파일을 선택해주세요.".into()));
    };
    let size = state.storage.path_of(&key).map(|p| p.metadata().map(|m| m.len())).transpose()?;
    if size.unwrap_or(0) == 0 {
        return Err(BroadcastError::EmptyVideo.into());
    }

    let model = broadcast::ActiveModel {
        title: Set(title),
        status: Set(BroadcastStatus::Recorded.as_str().into()),
        video_key: Set(Some(key)),
        video_content_type: Set(Some(content_type)),
        created_at: Set(chrono::Utc::now().into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok(model)
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<BroadcastResponse>>, AppError> {
    let mut list: Vec<_> =
        broadcast::Entity::find().order_by_desc(broadcast::Column::Id).all(&state.db).await?.into_iter().map(to_response).collect();
    // 방송 중인 라이브를 맨 앞으로. 안정 정렬이라 나머지는 최신순 그대로다.
    list.sort_by_key(|b| b.status != BroadcastStatus::Live);
    Ok(Json(list))
}

async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Json<BroadcastResponse>, AppError> {
    Ok(Json(to_response(find(&state, id).await?)))
}

/// tower-http 의 ServeFile 이 Range 요청(206 Partial Content)을 처리해 준다.
/// 브라우저 <video> 는 Range 로 필요한 구간만 받아 가므로 이게 있어야 탐색(seek)이 된다.
async fn video(State(state): State<AppState>, Path(id): Path<i64>, req: Request) -> Result<Response, AppError> {
    let model = find(&state, id).await?;
    let path = model
        .video_key
        .as_deref()
        .and_then(|key| state.storage.path_of(key))
        .ok_or_else(|| AppError::NotFound(format!("영상 파일을 찾을 수 없습니다: {id}")))?;

    // ServeFile 의 에러 타입은 Infallible 이라 실패하지 않는다
    let Ok(mut res) = ServeFile::new(path).oneshot(req).await;
    if let Some(Ok(ct)) = model.video_content_type.as_deref().map(HeaderValue::from_str) {
        res.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    Ok(res.map(Body::new))
}
