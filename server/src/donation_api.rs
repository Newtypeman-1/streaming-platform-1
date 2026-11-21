//! 후원 API: 주문(로그인), 결제 승인 확인·실패 기록(후원한 본인), 결제 통지(결제 서비스), 방송별 내역·합계(누구나).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use shared::{ConfirmDonationRequest, DonationOrderResponse, FailDonationRequest, DonationRequest, DonationStatus};

use crate::AppState;
use crate::api::find;
use crate::auth_api::CurrentUser;
use crate::donation::{DonationError, recipient_of, validate_amount, verify_payment};
use crate::entity::{broadcast, donation, user};
use crate::error::{AppError, JsonBody};
use crate::payment::PaymentError;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/broadcasts/{id}/donations", post(order))
        .route("/api/donations/confirm", post(confirm))
        .route("/api/donations/{order_id}/fail", post(record_failure))
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

async fn find_order(state: &AppState, order_id: &str) -> Result<donation::Model, AppError> {
    donation::Entity::find()
        .filter(donation::Column::OrderId.eq(order_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("후원 주문을 찾을 수 없습니다: {order_id}")))
}

/// 주문 확인·실패 기록은 후원한 본인만
async fn find_own_order(state: &AppState, order_id: &str, user: &user::Model) -> Result<donation::Model, AppError> {
    let model = find_order(state, order_id).await?;
    if model.donor_id != user.id {
        return Err(DonationError::NotDonor.into());
    }
    Ok(model)
}

async fn order_json(state: &AppState, model: donation::Model) -> Result<Json<DonationOrderResponse>, AppError> {
    let recipient = user::Entity::find_by_id(model.recipient_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal(format!("받는 사람이 없습니다: {}", model.recipient_id)))?;
    Ok(Json(order_response(state, model, recipient)))
}

async fn confirm(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    JsonBody(req): JsonBody<ConfirmDonationRequest>,
) -> Result<Json<DonationOrderResponse>, AppError> {
    let model = find_own_order(&state, &req.order_id, &user).await?;
    // 성공 화면을 새로고침하는 등 다시 확인하면 이미 확정한 결과를 그대로 준다
    match model.status() {
        DonationStatus::Done => return order_json(&state, model).await,
        DonationStatus::Failed | DonationStatus::Canceled => return Err(DonationError::Ended.into()),
        DonationStatus::Pending => {}
    }
    if req.amount != model.amount {
        return Err(fail(&state, &model, DonationError::AmountMismatch).await);
    }
    // 받는 사람: 주문한 방송이 아직 있고 소유자가 주문 때와 같아야 한다
    let broadcast = match model.broadcast_id {
        Some(id) => broadcast::Entity::find_by_id(id).one(&state.db).await?,
        None => None,
    };
    if broadcast.and_then(|b| b.user_id) != Some(model.recipient_id) {
        return Err(fail(&state, &model, DonationError::RecipientGone).await);
    }
    let payment = match state.payments.confirm(&req.payment_key, &model.order_id, model.amount).await {
        Ok(payment) => payment,
        Err(PaymentError::Rejected { message, .. }) => return Err(fail(&state, &model, DonationError::Rejected(message)).await),
        // 연결 실패는 승인됐는지 알 수 없으므로 결제 대기로 둔다
        Err(e) => return Err(e.into()),
    };
    if let Err(e) = verify_payment(&model.order_id, model.amount, &payment) {
        tracing::error!("승인된 결제가 주문과 다름: {model:?} {payment:?}");
        return Err(fail(&state, &model, e).await);
    }
    order_json(&state, mark_done(&state, model, &payment.payment_key).await?).await
}

/// 결제 대기 중인 주문만 확정한다. 이미 다른 요청이 상태를 바꿨으면 그 결과를 돌려준다.
async fn mark_done(state: &AppState, model: donation::Model, payment_key: &str) -> Result<donation::Model, AppError> {
    donation::Entity::update_many()
        .col_expr(donation::Column::Status, DonationStatus::Done.as_str().into())
        .col_expr(donation::Column::PaymentKey, payment_key.into())
        .col_expr(donation::Column::ApprovedAt, sea_orm::sea_query::Expr::current_timestamp().into())
        .filter(donation::Column::Id.eq(model.id))
        .filter(donation::Column::Status.eq(DonationStatus::Pending.as_str()))
        .exec(&state.db)
        .await?;
    find_order(state, &model.order_id).await
}

/// 결제 대기 중인 주문을 실패로 기록하고, 그 사유를 응답 오류로 돌려준다.
async fn fail(state: &AppState, model: &donation::Model, reason: impl Into<AppError> + ToString) -> AppError {
    if let Err(e) = mark_ended(state, model, DonationStatus::Failed, &reason.to_string()).await {
        return e;
    }
    reason.into()
}

/// 결제 대기 중인 주문만 실패·취소로 바꾼다.
async fn mark_ended(state: &AppState, model: &donation::Model, status: DonationStatus, failure: &str) -> Result<(), AppError> {
    donation::Entity::update_many()
        .col_expr(donation::Column::Status, status.as_str().into())
        .col_expr(donation::Column::Failure, failure.into())
        .filter(donation::Column::Id.eq(model.id))
        .filter(donation::Column::Status.eq(DonationStatus::Pending.as_str()))
        .exec(&state.db)
        .await?;
    Ok(())
}

/// 결제창에서 실패·취소되어 failUrl 로 돌아왔을 때 화면이 알려 준다.
async fn record_failure(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(order_id): Path<String>,
    JsonBody(req): JsonBody<FailDonationRequest>,
) -> Result<Json<DonationOrderResponse>, AppError> {
    let model = find_own_order(&state, &order_id, &user).await?;
    if model.status() == DonationStatus::Done {
        return Err(DonationError::AlreadyDone.into());
    }
    // 구매자가 결제창을 닫으면 결제 서비스가 이 코드를 준다
    let status = if req.code == "PAY_PROCESS_CANCELED" { DonationStatus::Canceled } else { DonationStatus::Failed };
    mark_ended(&state, &model, status, &req.message.unwrap_or(req.code)).await?;
    order_json(&state, find_order(&state, &order_id).await?).await
}
