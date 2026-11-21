//! 라이브 방송 API 통합 테스트. 실제 PostgreSQL 과 가짜 MediaMTX 위에서 시작~송출~시청~종료를 검증한다.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::Duration;
use common::{TestApp, app, get, with_cookie};
use server::{AppState, end_dropped_streams, router};
use shared::{BroadcastResponse, BroadcastStatus, ErrorResponse, StartLiveResponse};
use tower::ServiceExt;

const OFFER: &str = "v=0 offer";

fn start_request(title: &str) -> Request<Body> {
    Request::post("/api/broadcasts/live")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({ "title": title }).to_string()))
        .unwrap()
}

fn sdp_request(uri: &str, key: Option<&str>) -> Request<Body> {
    let mut req = Request::post(uri).header(header::CONTENT_TYPE, "application/sdp");
    if let Some(key) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    req.body(Body::from(OFFER)).unwrap()
}

/// `cookie` 사용자로 종료 요청 (None 이면 로그인하지 않은 요청)
fn end_request(id: i64, cookie: Option<&str>) -> Request<Body> {
    let req = Request::post(shared::end_url(id)).body(Body::empty()).unwrap();
    match cookie {
        Some(cookie) => with_cookie(req, cookie),
        None => req,
    }
}

impl TestApp {
    /// 소유자 쿠키와 함께 라이브를 시작한다
    async fn start_live(&self, cookie: &str, title: &str) -> StartLiveResponse {
        let (status, _, body) = self.send(with_cookie(start_request(title), cookie)).await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice(&body).unwrap()
    }

    async fn detail(&self, id: i64) -> BroadcastResponse {
        let (status, _, body) = self.send(get(&format!("/api/broadcasts/{id}"))).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice(&body).unwrap()
    }
}

fn error_detail(body: &[u8]) -> String {
    serde_json::from_slice::<ErrorResponse>(body).unwrap().detail
}

#[tokio::test]
async fn 라이브를_시작하면_방송_중_상태와_송출_키를_돌려준다() {
    let app = app().await;
    let owner = app.signup("owner").await;

    let (status, headers, body) = app.send(with_cookie(start_request("  폰 방송 "), &owner)).await;

    assert_eq!(status, StatusCode::CREATED);
    let started: StartLiveResponse = serde_json::from_slice(&body).unwrap();
    let b = &started.broadcast;
    assert_eq!(b.title, "폰 방송");
    assert_eq!(b.status, BroadcastStatus::Live);
    assert_eq!(b.video_url, None);
    assert_eq!(b.ended_at, None);
    assert_eq!(b.owner.as_ref().unwrap().username, "owner");
    assert!(started.stream_key.len() >= 32, "송출 키는 추측할 수 없을 만큼 길어야 한다");
    assert_eq!(headers[header::LOCATION], format!("/api/broadcasts/{}", b.id));
}

#[tokio::test]
async fn 송출_키는_목록과_상세에_노출되지_않는다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let started = app.start_live(&owner, "비밀").await;

    let (_, _, list) = app.send(get("/api/broadcasts")).await;
    let (_, _, detail) = app.send(get(&format!("/api/broadcasts/{}", started.broadcast.id))).await;

    for body in [list, detail] {
        assert!(!String::from_utf8(body).unwrap().contains(&started.stream_key));
    }
}

#[tokio::test]
async fn 라이브_제목이_비어있으면_400() {
    let app = app().await;
    let owner = app.signup("owner").await;

    let (status, _, body) = app.send(with_cookie(start_request("   "), &owner)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_detail(&body), "제목을 입력해주세요.");
}

#[tokio::test]
async fn 송출_키가_맞으면_미디어_서버로_송출을_연결한다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let started = app.start_live(&owner, "송출").await;
    let id = started.broadcast.id;

    let (status, headers, body) = app.send(sdp_request(&shared::whip_url(id), Some(&started.stream_key))).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers[header::CONTENT_TYPE], "application/sdp");
    assert_eq!(body, format!("answer:{OFFER}").as_bytes());
    assert_eq!(*app.media.offers.lock().unwrap(), [(format!("/live/{id}/whip"), OFFER.to_string())]);
}

#[tokio::test]
async fn 송출_키가_없거나_틀리면_401이고_미디어_서버에_닿지_않는다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let started = app.start_live(&owner, "송출").await;
    let url = shared::whip_url(started.broadcast.id);

    for key in [None, Some("wrong")] {
        let (status, _, _) = app.send(sdp_request(&url, key)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    assert!(app.media.offers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn 녹화_방송에는_송출할_수_없다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let recorded = app.register(&owner, "녹화").await;

    let (status, _, _) = app.send(sdp_request(&shared::whip_url(recorded.id), Some(""))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn 방송_중인_라이브는_누구나_시청할_수_있다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "시청").await.broadcast.id;
    app.media.set_ready(&[&format!("live/{id}")]);

    let (status, _, body) = app.send(sdp_request(&shared::whep_url(id), None)).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, format!("answer:{OFFER}").as_bytes());
    assert_eq!(app.media.offers.lock().unwrap()[0].0, format!("/live/{id}/whep"));
}

#[tokio::test]
async fn 아직_영상이_들어오지_않은_라이브를_시청하면_409() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "송출 준비 중").await.broadcast.id;

    let (status, _, body) = app.send(sdp_request(&shared::whep_url(id), None)).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_detail(&body), "아직 방송 영상이 들어오지 않았습니다.");
}

#[tokio::test]
async fn 송출_키로_종료하면_종료_상태가_되고_더_이상_송출_시청할_수_없다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let started = app.start_live(&owner, "종료").await;
    let (id, key) = (started.broadcast.id, started.stream_key.as_str());

    let (status, _, body) = app.send(end_request(id, Some(&owner))).await;

    assert_eq!(status, StatusCode::OK);
    let ended: BroadcastResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(ended.status, BroadcastStatus::Ended);
    assert!(ended.ended_at.is_some());
    assert_eq!(app.detail(id).await, ended);

    let (status, _, body) = app.send(sdp_request(&shared::whip_url(id), Some(key))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_detail(&body), "방송 중이 아닙니다.");
    let (status, _, _) = app.send(sdp_request(&shared::whep_url(id), None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(app.media.offers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn 이미_끝난_방송을_다시_종료해도_종료_시각은_그대로다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "두 번 종료").await.broadcast.id;
    let (_, _, first) = app.send(end_request(id, Some(&owner))).await;

    let (status, _, second) = app.send(end_request(id, Some(&owner))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, second);
}

#[tokio::test]
async fn 로그인하지_않으면_라이브를_시작할_수_없다() {
    let app = app().await;

    let (status, _, _) = app.send(start_request("몰래 방송")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, _, body) = app.send(get("/api/broadcasts")).await;
    assert_eq!(serde_json::from_slice::<Vec<BroadcastResponse>>(&body).unwrap(), []);
}

#[tokio::test]
async fn 비로그인은_401_다른_사용자는_403으로_라이브를_종료할_수_없다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let other = app.signup("other").await;
    let started = app.start_live(&owner, "남의 방송").await;
    let id = started.broadcast.id;

    let (status, _, _) = app.send(end_request(id, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.send(end_request(id, Some(&other))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // 송출 키만 알아서도 안 된다: 종료는 소유자 로그인으로만
    let with_key = Request::post(shared::end_url(id))
        .header(header::AUTHORIZATION, format!("Bearer {}", started.stream_key))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = app.send(with_key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    assert_eq!(app.detail(id).await.status, BroadcastStatus::Live);
}

#[tokio::test]
async fn 다른_사용자는_남의_송출_키를_모르므로_송출할_수_없다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let other = app.signup("other").await;
    let mine = app.start_live(&owner, "내 방송").await;
    let theirs = app.start_live(&other, "다른 사람 방송").await;

    // 자기 송출 키로 남의 방송에 송출 시도
    let (status, _, _) = app.send(sdp_request(&shared::whip_url(mine.broadcast.id), Some(&theirs.stream_key))).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(app.media.offers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn 방송_중인_라이브는_삭제할_수_없고_종료하면_삭제할_수_있다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "라이브").await.broadcast.id;
    let delete = || with_cookie(Request::delete(format!("/api/broadcasts/{id}")).body(Body::empty()).unwrap(), &owner);

    let (status, _, body) = app.send(delete()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_detail(&body), "방송 중에는 삭제할 수 없습니다. 먼저 방송을 종료해주세요.");

    app.send(end_request(id, Some(&owner))).await;
    let (status, _, _) = app.send(delete()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn 없는_방송은_404() {
    let app = app().await;
    let owner = app.signup("owner").await;

    for req in [sdp_request(&shared::whip_url(999), Some("k")), sdp_request(&shared::whep_url(999), None), end_request(999, Some(&owner))] {
        let (status, _, _) = app.send(req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn 목록은_방송_중인_라이브를_먼저_보여주고_나머지는_최신순이다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let live = app.start_live(&owner, "라이브").await.broadcast;
    let ended = app.start_live(&owner, "끝난 라이브").await;
    app.send(end_request(ended.broadcast.id, Some(&owner))).await;
    app.register(&owner, "녹화").await;

    let (_, _, body) = app.send(get("/api/broadcasts")).await;

    let list: Vec<BroadcastResponse> = serde_json::from_slice(&body).unwrap();
    let summary: Vec<_> = list.iter().map(|b| (b.title.as_str(), b.status)).collect();
    assert_eq!(
        summary,
        [("라이브", BroadcastStatus::Live), ("녹화", BroadcastStatus::Recorded), ("끝난 라이브", BroadcastStatus::Ended)]
    );
    assert_eq!(list[0], live);
}

#[tokio::test]
async fn 송출이_끊긴_라이브는_자동으로_종료된다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let streaming = app.start_live(&owner, "송출 중").await.broadcast.id;
    let dropped = app.start_live(&owner, "끊김").await.broadcast.id;
    app.media.set_ready(&[&format!("live/{streaming}")]);

    let ended = end_dropped_streams(&app.state, Duration::zero()).await.unwrap();

    assert_eq!(ended, [dropped]);
    assert_eq!(app.detail(streaming).await.status, BroadcastStatus::Live);
    let dropped = app.detail(dropped).await;
    assert_eq!(dropped.status, BroadcastStatus::Ended);
    assert!(dropped.ended_at.is_some());
}

#[tokio::test]
async fn 막_시작한_라이브는_송출_연결을_기다려준다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "연결 중").await.broadcast.id;

    let ended = end_dropped_streams(&app.state, Duration::seconds(30)).await.unwrap();

    assert!(ended.is_empty());
    assert_eq!(app.detail(id).await.status, BroadcastStatus::Live);
}

/// 미디어 서버가 잠깐 죽었다고 모든 방송을 끝내면 안 된다.
async fn media_down(app: &TestApp) -> (axum::Router, AppState) {
    let state = AppState::connect(&common::db_url(&app.db).await, app.storage_dir.path(), common::unreachable_media(), common::payments())
        .await
        .unwrap();
    (router(state.clone()), state)
}

#[tokio::test]
async fn 미디어_서버에_닿지_않으면_자동_종료하지_않는다() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let id = app.start_live(&owner, "미디어 서버 꺼짐").await.broadcast.id;
    let (_, state) = media_down(&app).await;

    assert!(end_dropped_streams(&state, Duration::zero()).await.is_err());

    assert_eq!(app.detail(id).await.status, BroadcastStatus::Live);
}

#[tokio::test]
async fn 미디어_서버에_닿지_않으면_송출_시청은_502() {
    let app = app().await;
    let owner = app.signup("owner").await;
    let started = app.start_live(&owner, "미디어 서버 꺼짐").await;
    let id = started.broadcast.id;
    let (router, _) = media_down(&app).await;

    for req in [sdp_request(&shared::whip_url(id), Some(&started.stream_key)), sdp_request(&shared::whep_url(id), None)] {
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    }
}
