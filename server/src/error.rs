use axum::Json;
use axum::extract::multipart::MultipartError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use shared::ErrorResponse;

use crate::broadcast::BroadcastError;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            AppError::BadRequest(detail) => (StatusCode::BAD_REQUEST, detail),
            AppError::NotFound(detail) => (StatusCode::NOT_FOUND, detail),
            AppError::Internal(detail) => {
                tracing::error!("{detail}");
                (StatusCode::INTERNAL_SERVER_ERROR, "서버 오류가 발생했습니다.".to_string())
            }
        };
        (status, Json(ErrorResponse { status: status.as_u16(), detail })).into_response()
    }
}

impl From<BroadcastError> for AppError {
    fn from(e: BroadcastError) -> Self {
        AppError::BadRequest(e.to_string())
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
