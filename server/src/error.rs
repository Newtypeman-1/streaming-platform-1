use axum::Json;
use axum::extract::multipart::MultipartError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use shared::ErrorResponse;

use crate::auth::AuthError;
use crate::broadcast::BroadcastError;
use crate::live::LiveError;
use crate::media::MediaError;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    BadGateway(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            AppError::BadRequest(detail) => (StatusCode::BAD_REQUEST, detail),
            AppError::Unauthorized(detail) => (StatusCode::UNAUTHORIZED, detail),
            AppError::Forbidden(detail) => (StatusCode::FORBIDDEN, detail),
            AppError::NotFound(detail) => (StatusCode::NOT_FOUND, detail),
            AppError::Conflict(detail) => (StatusCode::CONFLICT, detail),
            AppError::BadGateway(detail) => {
                tracing::warn!("{detail}");
                (StatusCode::BAD_GATEWAY, "방송 서버에 연결하지 못했습니다. 잠시 후 다시 시도해주세요.".to_string())
            }
            AppError::Internal(detail) => {
                tracing::error!("{detail}");
                (StatusCode::INTERNAL_SERVER_ERROR, "서버 오류가 발생했습니다.".to_string())
            }
        };
        (status, Json(ErrorResponse { status: status.as_u16(), detail })).into_response()
    }
}

impl From<AuthError> for AppError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::InvalidUsername | AuthError::InvalidPassword => AppError::BadRequest(e.to_string()),
            AuthError::UsernameTaken => AppError::Conflict(e.to_string()),
            AuthError::WrongCredentials | AuthError::LoginRequired => AppError::Unauthorized(e.to_string()),
        }
    }
}

impl From<BroadcastError> for AppError {
    fn from(e: BroadcastError) -> Self {
        match e {
            BroadcastError::NotOwner => AppError::Forbidden(e.to_string()),
            BroadcastError::StillLive => AppError::Conflict(e.to_string()),
            _ => AppError::BadRequest(e.to_string()),
        }
    }
}

impl From<LiveError> for AppError {
    fn from(e: LiveError) -> Self {
        match e {
            LiveError::InvalidStreamKey => AppError::Unauthorized(e.to_string()),
            LiveError::NotLive => AppError::Conflict(e.to_string()),
        }
    }
}

impl From<MediaError> for AppError {
    fn from(e: MediaError) -> Self {
        match e {
            MediaError::NotStreaming => AppError::Conflict(e.to_string()),
            MediaError::Failed(_) => AppError::BadGateway(e.to_string()),
        }
    }
}

impl From<MultipartError> for AppError {
    fn from(e: MultipartError) -> Self {
        AppError::BadRequest(e.body_text())
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(e: sea_orm::DbErr) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}
