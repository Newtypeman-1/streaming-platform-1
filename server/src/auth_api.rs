//! 회원가입·로그인·로그아웃 API 와, 로그인한 사용자를 꺼내는 추출기(`CurrentUser`).

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, SqlErr};
use shared::{Credentials, UserResponse};

use crate::AppState;
use crate::auth::{
    AuthError, SESSION_DAYS, expired_session_cookie, hash_password, hash_session_token, new_session_token,
    normalize_username, session_cookie, session_token_from_cookie, validate_password, verify_password,
};
use crate::entity::{session, user};
use crate::error::AppError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
}

/// 로그인한 사용자. 핸들러 인자로 받으면 로그인하지 않은 요청은 핸들러에 닿기 전에 401 로 끝난다.
pub struct CurrentUser(pub user::Model);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = session_token(&parts.headers).ok_or(AuthError::LoginRequired)?;
        let found = session::Entity::find()
            .filter(session::Column::TokenHash.eq(hash_session_token(token)))
            .filter(session::Column::ExpiresAt.gt(Utc::now()))
            .find_also_related(user::Entity)
            .one(&state.db)
            .await?;
        match found {
            Some((_, Some(user))) => Ok(CurrentUser(user)),
            _ => Err(AuthError::LoginRequired.into()),
        }
    }
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(session_token_from_cookie)
}

/// 새 세션을 만들고 사용자 정보와 로그인 쿠키를 응답한다.
async fn logged_in(state: &AppState, status: StatusCode, user: user::Model) -> Result<Response, AppError> {
    let token = new_session_token();
    let now = Utc::now();
    session::ActiveModel {
        user_id: Set(user.id),
        token_hash: Set(hash_session_token(&token)),
        expires_at: Set((now + Duration::days(SESSION_DAYS)).into()),
        created_at: Set(now.into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok((status, [(header::SET_COOKIE, session_cookie(&token))], Json(UserResponse::from(user))).into_response())
}

/// Argon2 는 일부러 느리게 만든 계산이라 비동기 작업 스레드를 막지 않도록 따로 돌린다.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::Internal(e.to_string()))
}

async fn signup(State(state): State<AppState>, Json(req): Json<Credentials>) -> Result<Response, AppError> {
    let username = normalize_username(&req.username)?;
    validate_password(&req.password)?;
    let password_hash = blocking(move || hash_password(&req.password)).await?;

    let inserted = user::ActiveModel {
        username: Set(username),
        password_hash: Set(password_hash),
        created_at: Set(Utc::now().into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await;
    let user = match inserted {
        Err(e) if matches!(e.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            return Err(AuthError::UsernameTaken.into());
        }
        result => result?,
    };
    logged_in(&state, StatusCode::CREATED, user).await
}

async fn login(State(state): State<AppState>, Json(req): Json<Credentials>) -> Result<Response, AppError> {
    // 형식이 틀린 아이디도 "없는 아이디"와 같은 응답을 준다
    let found = match normalize_username(&req.username) {
        Ok(username) => user::Entity::find().filter(user::Column::Username.eq(username)).one(&state.db).await?,
        Err(_) => None,
    };
    let Some(user) = found else {
        return Err(AuthError::WrongCredentials.into());
    };
    let hash = user.password_hash.clone();
    if !blocking(move || verify_password(&req.password, &hash)).await? {
        return Err(AuthError::WrongCredentials.into());
    }
    logged_in(&state, StatusCode::OK, user).await
}

/// 이 세션만 지운다(다른 기기의 로그인은 그대로). 로그인하지 않은 상태로 불러도 204.
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(token) = session_token(&headers) {
        session::Entity::delete_many()
            .filter(session::Column::TokenHash.eq(hash_session_token(token)))
            .exec(&state.db)
            .await?;
    }
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, expired_session_cookie())]).into_response())
}

async fn me(CurrentUser(user): CurrentUser) -> Json<UserResponse> {
    Json(user.into())
}
