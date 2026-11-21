//! 여러 인스턴스로 띄울 때 필요한 동작: 상태 확인, 응답한 인스턴스 표시, 동시 기동.

mod common;

use axum::http::StatusCode;
use common::{app, db_url, get, unreachable_media, unreachable_payments};
use server::{AppState, with_instance_name};
use tower::ServiceExt;

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

/// 새 DB 에 인스턴스 여러 개가 한꺼번에 떠도 마이그레이션이 서로 부딪혀 기동에 실패하지 않는다.
#[tokio::test]
async fn 인스턴스_여러_개가_동시에_떠도_모두_기동한다() {
    let app = app().await;
    let url = db_url(&app.db).await;
    // 공용 준비물이 이미 마이그레이션을 적용했으므로 빈 DB 로 되돌린다
    sea_orm::ConnectionTrait::execute_unprepared(&app.state.db, "DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .await
        .unwrap();

    let starts = (0..4).map(|_| {
        let url = url.clone();
        let dir = app.storage_dir.path().to_path_buf();
        tokio::spawn(async move {
            AppState::connect(&url, dir, unreachable_media(), unreachable_payments()).await.map(|_| ()).map_err(|e| e.to_string())
        })
    });
    let results: Vec<_> = futures_util::future::join_all(starts).await.into_iter().map(Result::unwrap).collect();

    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

/// 여러 인스턴스 앞에서 요청이 어디로 갔는지 알 수 있게, 응답마다 처리한 인스턴스 이름을 붙인다.
#[tokio::test]
async fn 응답에_처리한_인스턴스_이름을_붙인다() {
    let app = app().await;
    let router = with_instance_name(app.router.clone(), "nadri-server-abc");

    let res = router.oneshot(get("/api/broadcasts")).await.unwrap();

    assert_eq!(res.headers()["x-instance"], "nadri-server-abc");
}
