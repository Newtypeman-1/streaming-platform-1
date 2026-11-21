mod common;

use axum::http::StatusCode;
use common::{TestApp, app, get, json, with_cookie};
use shared::{BroadcastResponse, DONATION_CONFIRM_URL, DonationOrderResponse, DonationStatus, DonationSummary, ErrorResponse, PAYMENT_WEBHOOK_URL, donation_fail_url, donations_url};

/// 방송자(owner)의 녹화 방송 하나와 후원할 시청자(viewer) 로그인 쿠키
async fn setup() -> (TestApp, BroadcastResponse, String) {
    let app = app().await;
    let owner = app.signup("owner").await;
    let broadcast = app.register(&owner, "후원 받을 방송").await;
    let viewer = app.signup("viewer").await;
    (app, broadcast, viewer)
}

/// `cookie` 사용자로 후원 주문
async fn order(app: &TestApp, cookie: &str, broadcast_id: i64, amount: i64) -> DonationOrderResponse {
    let (status, _, body) =
        app.send(with_cookie(json("POST", &donations_url(broadcast_id), serde_json::json!({ "amount": amount })), cookie)).await;
    assert_eq!(status, StatusCode::CREATED);
    serde_json::from_slice(&body).unwrap()
}

/// 결제창이 successUrl 로 넘겨 준 값으로 승인 확인
async fn confirm(app: &TestApp, cookie: &str, payment_key: &str, order_id: &str, amount: i64) -> (StatusCode, Vec<u8>) {
    let req = json("POST", DONATION_CONFIRM_URL, serde_json::json!({ "paymentKey": payment_key, "orderId": order_id, "amount": amount }));
    let (status, _, body) = app.send(with_cookie(req, cookie)).await;
    (status, body)
}

#[tokio::test]
async fn 로그인하지_않으면_후원_주문을_만들_수_없다() {
    let (app, broadcast, _) = setup().await;
    let (status, _, _) = app.send(json("POST", &donations_url(broadcast.id), serde_json::json!({ "amount": 5000 }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn 후원_주문은_방송_소유자를_받는_사람으로_결제_대기_상태로_만든다() {
    let (app, broadcast, viewer) = setup().await;
    let (status, _, body) =
        app.send(with_cookie(json("POST", &donations_url(broadcast.id), serde_json::json!({ "amount": 5000 })), &viewer)).await;
    assert_eq!(status, StatusCode::CREATED);
    let order: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(order.recipient.username, "owner");
    assert_eq!(order.broadcast_id, Some(broadcast.id));
    assert_eq!(order.amount, 5000);
    assert_eq!(order.status, DonationStatus::Pending);
    assert_eq!(order.client_key, common::CLIENT_KEY);
    // 결제 서비스 규칙: 영문·숫자·-_= 6~64자
    assert!((6..=64).contains(&order.order_id.len()));
    assert!(order.order_id.chars().all(|c| c.is_ascii_alphanumeric() || "-_=".contains(c)));
    assert!(!order.order_name.is_empty());
}

#[tokio::test]
async fn 금액이_범위를_벗어나면_400() {
    let (app, broadcast, viewer) = setup().await;
    for amount in [0, 999, 1_000_001] {
        let (status, _, _) =
            app.send(with_cookie(json("POST", &donations_url(broadcast.id), serde_json::json!({ "amount": amount })), &viewer)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "amount {amount}");
    }
}

#[tokio::test]
async fn 자기_방송에는_후원할_수_없다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let broadcast = app.register(&owner, "내 방송").await;
    let (status, _, _) =
        app.send(with_cookie(json("POST", &donations_url(broadcast.id), serde_json::json!({ "amount": 5000 })), &owner)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn 없는_방송에는_후원할_수_없다() {
    let (app, _, viewer) = setup().await;
    let (status, _, _) = app.send(with_cookie(json("POST", &donations_url(9999), serde_json::json!({ "amount": 5000 })), &viewer)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 결제_서비스_승인을_확인하면_후원이_확정된다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (status, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::OK);
    let done: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(done.status, DonationStatus::Done);
    assert_eq!(app.toss.confirms(), 1);
}

/// 주문을 다시 불러온다 (결제 실패 기록 API 는 같은 주문을 돌려준다)
async fn order_status(app: &TestApp, order_id: &str) -> DonationStatus {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let model = server::entity::donation::Entity::find()
        .filter(server::entity::donation::Column::OrderId.eq(order_id))
        .one(&app.state.db)
        .await
        .unwrap()
        .unwrap();
    model.status()
}

#[tokio::test]
async fn 금액이_주문과_다르면_승인을_요청하지_않고_실패로_기록한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    // successUrl 의 amount 를 바꾼 경우
    let (status, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 100).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(app.toss.confirms(), 0);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}

#[tokio::test]
async fn 후원한_본인만_승인을_확인할_수_있다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let body = serde_json::json!({ "paymentKey": "pay_1", "orderId": order.order_id, "amount": 5000 });
    let (status, _, _) = app.send(json("POST", DONATION_CONFIRM_URL, body)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let other = app.signup("other").await;
    let (status, _) = confirm(&app, &other, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.toss.confirms(), 0);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Pending);
}

#[tokio::test]
async fn 결제_서비스가_승인을_거절하면_실패로_기록하고_사유를_알려준다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    *app.toss.reject.lock().unwrap() = Some((403, "REJECT_CARD_COMPANY".into()));
    let (status, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert!(error.detail.contains("결제 승인이 거절되었습니다."), "{}", error.detail);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}

#[tokio::test]
async fn 승인이_거절되어도_결제_서비스에_조회해_이미_승인된_결제면_확정한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    // 첫 승인 응답이 끊긴 뒤 다시 확인한 상황: 결제 서비스는 이미 승인했고, 다시 승인하면 거절한다
    toss_approved(&app, "pay_1", &order.order_id, 5000);
    *app.toss.reject.lock().unwrap() = Some((400, "ALREADY_PROCESSED_PAYMENT".into()));
    let (status, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Done);
}

#[tokio::test]
async fn 결제_서비스에_연결하지_못하면_502_이고_결제_대기로_남긴다() {
    let (mut app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    app.router = server::router(server::AppState { payments: common::unreachable_payments(), ..app.state.clone() });
    let (status, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert!(error.detail.contains("결제 서비스"), "{}", error.detail);
    // 승인됐는지 알 수 없으므로 실패로 단정하지 않는다
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Pending);
}

#[tokio::test]
async fn 확정된_주문을_다시_확인하면_승인을_다시_요청하지_않고_같은_결과를_준다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (first, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    let (again, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!((first, again), (StatusCode::OK, StatusCode::OK));
    let done: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(done.status, DonationStatus::Done);
    assert_eq!(app.toss.confirms(), 1);
}

/// 결제창이 failUrl 로 넘겨 준 코드로 실패 기록
async fn report_fail(app: &TestApp, cookie: &str, order_id: &str, code: &str) -> (StatusCode, Vec<u8>) {
    let req = json("POST", &donation_fail_url(order_id), serde_json::json!({ "code": code, "message": "결제창 메시지" }));
    let (status, _, body) = app.send(with_cookie(req, cookie)).await;
    (status, body)
}

#[tokio::test]
async fn 결제창을_닫으면_취소로_기록한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (status, body) = report_fail(&app, &viewer, &order.order_id, "PAY_PROCESS_CANCELED").await;
    assert_eq!(status, StatusCode::OK);
    let canceled: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(canceled.status, DonationStatus::Canceled);
}

#[tokio::test]
async fn PC_결제창을_닫아_받은_오류_코드도_취소로_기록한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    // 결제창 SDK 는 결제창을 닫으면 이 코드로 오류를 낸다
    let (status, body) = report_fail(&app, &viewer, &order.order_id, "USER_CANCEL").await;
    assert_eq!(status, StatusCode::OK);
    let canceled: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(canceled.status, DonationStatus::Canceled);
}

#[tokio::test]
async fn 결제창에서_실패하면_실패로_기록한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (status, body) = report_fail(&app, &viewer, &order.order_id, "PAY_PROCESS_ABORTED").await;
    assert_eq!(status, StatusCode::OK);
    let failed: DonationOrderResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(failed.status, DonationStatus::Failed);
    assert_eq!(failed.failure.as_deref(), Some("결제창 메시지"));
}

#[tokio::test]
async fn 확정된_주문은_실패로_바꿀_수_없다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    let (status, _) = report_fail(&app, &viewer, &order.order_id, "PAY_PROCESS_CANCELED").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Done);
}

#[tokio::test]
async fn 실패하거나_취소된_주문은_승인을_요청하지_않는다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    report_fail(&app, &viewer, &order.order_id, "PAY_PROCESS_CANCELED").await;
    let (status, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(app.toss.confirms(), 0);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Canceled);
}

#[tokio::test]
async fn 후원한_본인만_실패를_기록할_수_있다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let other = app.signup("other").await;
    let (status, _) = report_fail(&app, &other, &order.order_id, "PAY_PROCESS_CANCELED").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Pending);
}

#[tokio::test]
async fn 결제_서비스가_주문과_다른_금액을_승인했다고_하면_확정하지_않는다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    *app.toss.approved_amount.lock().unwrap() = Some(100);
    let (status, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}

#[tokio::test]
async fn 주문_뒤_방송이_지워졌으면_승인을_요청하지_않는다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let broadcast = app.register(&owner, "곧 지울 방송").await;
    let viewer = app.signup("viewer").await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (status, _, _) =
        app.send(with_cookie(axum::http::Request::delete(format!("/api/broadcasts/{}", broadcast.id)).body(axum::body::Body::empty()).unwrap(), &owner)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(app.toss.confirms(), 0);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}

/// 결제 서비스가 승인을 마쳤다고 기록해 둔다 (승인 응답이 이 서버에 오기 전에 끊긴 상황)
fn toss_approved(app: &TestApp, payment_key: &str, order_id: &str, amount: i64) {
    app.toss.payments.lock().unwrap().insert(
        payment_key.to_string(),
        serde_json::json!({ "paymentKey": payment_key, "orderId": order_id, "status": "DONE", "totalAmount": amount }),
    );
}

/// 결제 상태 변경 통지(PAYMENT_STATUS_CHANGED). data 는 결제 객체다.
async fn notify(app: &TestApp, payment_key: &str, order_id: &str, amount: i64) -> StatusCode {
    let body = serde_json::json!({
        "eventType": "PAYMENT_STATUS_CHANGED",
        "createdAt": "2026-11-21T12:00:01.000000",
        "data": { "paymentKey": payment_key, "orderId": order_id, "status": "DONE", "totalAmount": amount },
    });
    app.send(json("POST", PAYMENT_WEBHOOK_URL, body)).await.0
}

#[tokio::test]
async fn 승인_통지를_받으면_결제_서비스에_조회해_확정한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    toss_approved(&app, "pay_1", &order.order_id, 5000);
    assert_eq!(notify(&app, "pay_1", &order.order_id, 5000).await, StatusCode::OK);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Done);
}

/// 방송의 확정된 후원 (건수, 합계)와 그 주문의 승인 시각
async fn done_rows(app: &TestApp, order_id: &str) -> (usize, i64, Option<String>) {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use server::entity::donation;
    let done = donation::Entity::find().filter(donation::Column::Status.eq("done")).all(&app.state.db).await.unwrap();
    let approved = done.iter().find(|d| d.order_id == order_id).and_then(|d| d.approved_at).map(|t| t.to_rfc3339());
    (done.len(), done.iter().map(|d| d.amount).sum(), approved)
}

#[tokio::test]
async fn 같은_승인_통지가_반복되어도_한_번만_적립한다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let (status, _) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::OK);
    let before = done_rows(&app, &order.order_id).await;
    assert_eq!((before.0, before.1), (1, 5000));

    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert_eq!(notify(&app, "pay_1", &order.order_id, 5000).await, StatusCode::OK);
    }
    assert_eq!(done_rows(&app, &order.order_id).await, before);
}

#[tokio::test]
async fn 결제_서비스에_없는_결제의_통지는_확정하지_않는다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    // 누구나 보낼 수 있는 주소라 통지 내용만으로는 믿지 않는다
    assert_eq!(notify(&app, "forged", &order.order_id, 5000).await, StatusCode::BAD_REQUEST);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Pending);
}

#[tokio::test]
async fn 이_서버의_주문이_아니거나_다른_종류의_통지는_무시하고_200() {
    let (app, _, _) = setup().await;
    // 200 이 아니면 결제 서비스가 통지를 계속 다시 보낸다
    assert_eq!(notify(&app, "pay_x", "unknown-order", 5000).await, StatusCode::OK);
    let other = serde_json::json!({ "eventType": "DEPOSIT_CALLBACK", "createdAt": "2026-11-21T12:00:01", "data": {} });
    assert_eq!(app.send(json("POST", PAYMENT_WEBHOOK_URL, other)).await.0, StatusCode::OK);
}

#[tokio::test]
async fn 조회한_결제가_승인_완료가_아니면_결제_대기로_둔다() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    app.toss.payments.lock().unwrap().insert(
        "pay_1".into(),
        serde_json::json!({ "paymentKey": "pay_1", "orderId": order.order_id, "status": "IN_PROGRESS", "totalAmount": 5000 }),
    );
    assert_eq!(notify(&app, "pay_1", &order.order_id, 5000).await, StatusCode::OK);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Pending);
}

#[tokio::test]
async fn 조회한_결제_금액이_주문과_다르면_실패로_기록하고_200() {
    let (app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    toss_approved(&app, "pay_1", &order.order_id, 100);
    assert_eq!(notify(&app, "pay_1", &order.order_id, 5000).await, StatusCode::OK);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}

async fn summary(app: &TestApp, broadcast_id: i64) -> DonationSummary {
    let (status, _, body) = app.send(get(&donations_url(broadcast_id))).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).unwrap()
}

/// 주문하고 승인까지 확인한다
async fn donate(app: &TestApp, cookie: &str, broadcast_id: i64, amount: i64) {
    let order = order(app, cookie, broadcast_id, amount).await;
    let (status, _) = confirm(app, cookie, &format!("pay-{}", order.order_id), &order.order_id, amount).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn 방송별_후원_내역과_합계는_확정된_후원만_최근_순으로_누구나_본다() {
    let (app, broadcast, viewer) = setup().await;
    let fan = app.signup("fan").await;
    donate(&app, &viewer, broadcast.id, 5000).await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    donate(&app, &fan, broadcast.id, 3000).await;
    // 결제 대기·취소는 세지 않는다
    order(&app, &viewer, broadcast.id, 10_000).await;
    let canceled = order(&app, &viewer, broadcast.id, 20_000).await;
    report_fail(&app, &viewer, &canceled.order_id, "PAY_PROCESS_CANCELED").await;

    let summary = summary(&app, broadcast.id).await;
    assert_eq!((summary.total, summary.count), (8000, 2));
    let rows: Vec<_> = summary.donations.iter().map(|d| (d.donor.username.as_str(), d.amount)).collect();
    assert_eq!(rows, [("fan", 3000), ("viewer", 5000)]);
}

#[tokio::test]
async fn 후원_내역은_방송별로_따로_센다() {
    let (app, broadcast, viewer) = setup().await;
    let other_owner = app.signup("owner2").await;
    let other = app.register(&other_owner, "다른 방송").await;
    donate(&app, &viewer, broadcast.id, 5000).await;
    donate(&app, &viewer, other.id, 7000).await;
    let summary = summary(&app, other.id).await;
    assert_eq!((summary.total, summary.count), (7000, 1));
}

#[tokio::test]
async fn 없는_방송의_후원_내역은_404() {
    let (app, _, _) = setup().await;
    let (status, _, _) = app.send(get(&donations_url(9999))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 테스트_코드를_설정하면_결제_서비스에_실어_보내_승인_거절을_재현한다() {
    let (mut app, broadcast, viewer) = setup().await;
    let order = order(&app, &viewer, broadcast.id, 5000).await;
    let payments = app.toss.gateway().with_test_code("INVALID_REJECT_CARD");
    app.router = server::router(server::AppState { payments, ..app.state.clone() });
    let (status, body) = confirm(&app, &viewer, "pay_1", &order.order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert!(error.detail.contains("INVALID_REJECT_CARD"), "{}", error.detail);
    assert_eq!(order_status(&app, &order.order_id).await, DonationStatus::Failed);
}
