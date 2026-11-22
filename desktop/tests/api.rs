//! 서버 API 호출 테스트. 실제 서버 대신 같은 경로·응답 형식을 흉내 낸 가짜 서버를 띄운다.

use axum::Router;
use axum::routing::get;
use desktop::api::Api;
use serde_json::json;

/// 가짜 서버를 빈 포트에 띄우고 주소를 돌려준다
async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn broadcast(id: i64, title: &str, status: &str) -> serde_json::Value {
    json!({
        "id": id,
        "title": title,
        "status": status,
        "owner": { "id": 1, "username": "alice" },
        "videoUrl": if status == "recorded" { Some(format!("/api/broadcasts/{id}/video")) } else { None },
        "createdAt": "2026-11-22T10:00:00Z",
        "endedAt": null,
    })
}

#[tokio::test]
async fn 서버의_방송_목록을_가져온다() {
    let base = serve(Router::new().route(
        "/api/broadcasts",
        get(|| async { axum::Json(json!([broadcast(2, "라이브", "live"), broadcast(1, "녹화", "recorded")])) }),
    ))
    .await;

    let list = Api::new(&base).broadcasts().await.unwrap();

    assert_eq!(list.iter().map(|b| (b.id, b.title.as_str())).collect::<Vec<_>>(), [(2, "라이브"), (1, "녹화")]);
}

#[tokio::test]
async fn 서버가_오류를_응답하면_오류_메시지를_돌려준다() {
    let base = serve(Router::new().route(
        "/api/broadcasts",
        get(|| async {
            let body = json!({ "status": 503, "detail": "DB에 연결할 수 없습니다." });
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(body))
        }),
    ))
    .await;

    let error = Api::new(&base).broadcasts().await.unwrap_err();

    assert_eq!(error, "DB에 연결할 수 없습니다.");
}

#[tokio::test]
async fn 프록시가_json_이_아닌_오류를_주면_상태_코드를_알려준다() {
    // 서버가 꺼져 있으면 앞단의 Caddy 가 본문 없이 502 를 준다
    let base = serve(Router::new().route("/api/broadcasts", get(|| async { axum::http::StatusCode::BAD_GATEWAY }))).await;

    let error = Api::new(&base).broadcasts().await.unwrap_err();

    assert_eq!(error, "서버 오류 (502 Bad Gateway)");
}

#[tokio::test]
async fn 서버에_연결할_수_없으면_주소와_함께_알려준다() {
    // 아무것도 듣고 있지 않은 포트
    let error = Api::new("http://127.0.0.1:9").broadcasts().await.unwrap_err();

    assert_eq!(error, "서버(http://127.0.0.1:9)에 연결할 수 없습니다. 인터넷 연결을 확인해 주세요.");
}

#[test]
fn 녹화_영상_주소는_서버_주소를_붙인_절대_주소다() {
    let api = Api::new("https://nadri.me/");
    let recorded: shared::BroadcastResponse = serde_json::from_value(broadcast(1, "녹화", "recorded")).unwrap();
    let live: shared::BroadcastResponse = serde_json::from_value(broadcast(2, "라이브", "live")).unwrap();

    assert_eq!(api.video_url(&recorded).as_deref(), Some("https://nadri.me/api/broadcasts/1/video"));
    assert_eq!(api.video_url(&live), None);
}

#[tokio::test]
async fn 라이브_시청은_sdp_offer_를_보내고_answer_를_받는다() {
    let base = serve(Router::new().route(
        "/api/broadcasts/{id}/whep",
        axum::routing::post(
            |axum::extract::Path(id): axum::extract::Path<i64>, headers: axum::http::HeaderMap, offer: String| async move {
                assert_eq!(headers[axum::http::header::CONTENT_TYPE], "application/sdp");
                let answer = format!("answer to {offer} for {id}");
                (axum::http::StatusCode::CREATED, [(axum::http::header::CONTENT_TYPE, "application/sdp")], answer)
            },
        ),
    ))
    .await;

    let answer = Api::new(&base).watch(7, "v=0 offer").await.unwrap();

    assert_eq!(answer, "answer to v=0 offer for 7");
}

#[tokio::test]
async fn 끝난_라이브를_시청하려_하면_서버의_오류_메시지를_돌려준다() {
    let base = serve(Router::new().route(
        "/api/broadcasts/{id}/whep",
        axum::routing::post(|| async {
            let body = json!({ "status": 409, "detail": "방송 중인 라이브가 아닙니다." });
            (axum::http::StatusCode::CONFLICT, axum::Json(body))
        }),
    ))
    .await;

    let error = Api::new(&base).watch(7, "v=0 offer").await.unwrap_err();

    assert_eq!(error, "방송 중인 라이브가 아닙니다.");
}

#[test]
fn 서버_주소는_실행_시_환경변수_빌드_시_값_기본값_순으로_정한다() {
    use desktop::api::{DEFAULT_SERVER_URL, server_url};

    assert_eq!(server_url(Some("http://localhost:8000"), Some("https://staging.nadri.me")), "http://localhost:8000");
    assert_eq!(server_url(None, Some("https://staging.nadri.me")), "https://staging.nadri.me");
    assert_eq!(server_url(Some(""), None), DEFAULT_SERVER_URL);
    assert_eq!(DEFAULT_SERVER_URL, "https://nadri.me");
}
