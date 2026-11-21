use axum::Json;
use axum::extract::multipart::MultipartError;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use shared::ErrorResponse;

use crate::auth::AuthError;
use crate::broadcast::BroadcastError;
use crate::comment::CommentError;
use crate::live::LiveError;
use crate::media::MediaError;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    /// 외부 서비스(미디어·결제 서버) 연결 실패. 사용자에게 보여 줄 문구를 담는다. 원인은 변환할 때 로그로 남긴다.
    BadGateway(&'static str),
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
            AppError::BadGateway(detail) => (StatusCode::BAD_GATEWAY, detail.to_string()),
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

impl From<CommentError> for AppError {
    fn from(e: CommentError) -> Self {
        match e {
            CommentError::NotAuthor => AppError::Forbidden(e.to_string()),
            CommentError::EmptyBody | CommentError::BodyTooLong => AppError::BadRequest(e.to_string()),
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
            MediaError::Failed(_) => {
                tracing::warn!("{e}");
                AppError::BadGateway("방송 서버에 연결하지 못했습니다. 잠시 후 다시 시도해주세요.")
            }
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

/// `Json<T>` 대신 쓰는 요청 본문 추출기. JSON 이 아니거나 필드가 맞지 않을 때
/// axum 기본 응답(영문 텍스트, 415/422) 대신 다른 오류와 같은 형식의 400 을 준다.
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    Json<T>: FromRequest<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, AppError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(_) => Err(AppError::BadRequest("요청 본문(JSON)이 올바르지 않습니다.".into())),
        }
    }
}

impl From<crate::donation::DonationError> for AppError {
    fn from(e: crate::donation::DonationError) -> Self {
        use crate::donation::DonationError;
        match e {
            DonationError::NotDonor => AppError::Forbidden(e.to_string()),
            DonationError::AlreadyDone | DonationError::Ended | DonationError::RecipientGone => AppError::Conflict(e.to_string()),
            DonationError::PaymentMismatch => AppError::BadGateway("결제 서비스 응답이 주문과 달라 후원을 확정하지 않았습니다."),
            _ => AppError::BadRequest(e.to_string()),
        }
    }
}

impl From<crate::payment::PaymentError> for AppError {
    fn from(e: crate::payment::PaymentError) -> Self {
        tracing::warn!("{e}");
        AppError::BadGateway("결제 서비스에 연결하지 못했습니다. 잠시 후 다시 시도해주세요.")
    }
}
