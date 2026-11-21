//! 여러 인스턴스로 띄울 때 필요한 동작: 상태 확인, 응답한 인스턴스 표시, 동시 기동.

mod common;

use axum::http::StatusCode;
use common::{app, get};

#[tokio::test]
async fn 상태_확인은_db에_연결되면_200() {
    let app = app().await;

    let (status, _, _) = app.send(get("/api/health")).await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn 상태_확인은_db에_연결할_수_없으면_503() {
    let app = app().await;
    app.db.stop().await.unwrap();

    let (status, _, _) = app.send(get("/api/health")).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
