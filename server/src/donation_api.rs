//! 후원 API: 주문(로그인), 결제 승인 확인·실패 기록(후원한 본인), 결제 통지(결제 서비스), 방송별 내역·합계(누구나).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use shared::{DonationOrderResponse, DonationRequest, DonationStatus};

use crate::AppState;
use crate::api::find;
use crate::auth_api::CurrentUser;
use crate::donation::{recipient_of, validate_amount};
use crate::entity::{donation, user};
use crate::error::{AppError, JsonBody};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/broadcasts/{id}/donations", post(order))
}

fn order_response(state: &AppState, model: donation::Model, recipient: user::Model) -> DonationOrderResponse {
    DonationOrderResponse {
        status: model.status(),
        order_name: order_name(&recipient.username),
        order_id: model.order_id,
        broadcast_id: model.broadcast_id,
        recipient: recipient.into(),
        amount: model.amount,
        client_key: state.payments.client_key.clone(),
        failure: model.failure,
    }
}

/// 결제창에 보이는 상품명
fn order_name(recipient: &str) -> String {
    format!("나드리 {recipient} 님 후원")
}

async fn order(
    State(state): State<AppState>,
    CurrentUser(donor): CurrentUser,
    Path(id): Path<i64>,
    JsonBody(req): JsonBody<DonationRequest>,
) -> Result<Response, AppError> {
    let amount = validate_amount(req.amount)?;
    let broadcast = find(&state, id).await?;
    let recipient_id = recipient_of(broadcast.user_id, donor.id)?;
    let recipient = user::Entity::find_by_id(recipient_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal(format!("방송 소유자가 없습니다: {recipient_id}")))?;
    let model = donation::ActiveModel {
        // 결제 서비스 규칙: 영문·숫자·-_= 6~64자
        order_id: Set(format!("nadri-{}", uuid::Uuid::new_v4().simple())),
        broadcast_id: Set(Some(id)),
        donor_id: Set(donor.id),
        recipient_id: Set(recipient_id),
        amount: Set(amount),
        status: Set(DonationStatus::Pending.as_str().to_string()),
        created_at: Set(Utc::now().into()),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(order_response(&state, model, recipient))).into_response())
}
