mod common;

use axum::http::StatusCode;
use common::{TestApp, app, json, with_cookie};
use shared::{BroadcastResponse, DonationOrderResponse, DonationStatus, donations_url};

/// 방송자(owner)의 녹화 방송 하나와 후원할 시청자(viewer) 로그인 쿠키
async fn setup() -> (TestApp, BroadcastResponse, String) {
    let app = app().await;
    let owner = app.signup("owner").await;
    let broadcast = app.register(&owner, "후원 받을 방송").await;
    let viewer = app.signup("viewer").await;
    (app, broadcast, viewer)
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
