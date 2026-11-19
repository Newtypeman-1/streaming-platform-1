//! 방송 API 통합 테스트. 실제 PostgreSQL(Testcontainers) 위에서 요청~DB 저장까지 검증한다.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use server::{AppState, router};
use shared::{BroadcastResponse, ErrorResponse};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tower::ServiceExt;

const VIDEO: &[u8] = b"0123456789";
const BOUNDARY: &str = "test-boundary";

struct TestApp {
    router: Router,
    storage_dir: tempfile::TempDir,
    _db: ContainerAsync<Postgres>,
}

async fn app() -> TestApp {
    let db = Postgres::default().with_tag("17").start().await.unwrap();
    let url = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        db.get_host().await.unwrap(),
        db.get_host_port_ipv4(5432).await.unwrap()
    );
    let storage_dir = tempfile::tempdir().unwrap();
    let state = AppState::connect(&url, storage_dir.path()).await.unwrap();
    TestApp { router: router(state), storage_dir, _db: db }
}

enum Part<'a> {
    Text(&'a str, &'a str),
    File { name: &'a str, filename: &'a str, content_type: &'a str, bytes: &'a [u8] },
}

fn multipart(parts: &[Part]) -> Request<Body> {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(name, value) => {
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
            }
            Part::File { name, filename, content_type, bytes } => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
                );
                body.extend_from_slice(bytes);
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    Request::post("/api/broadcasts")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"))
        .body(Body::from(body))
        .unwrap()
}

fn video_part(bytes: &[u8]) -> Part<'_> {
    Part::File { name: "video", filename: "clip.mp4", content_type: "video/mp4", bytes }
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

impl TestApp {
    async fn send(&self, req: Request<Body>) -> (StatusCode, header::HeaderMap, Vec<u8>) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        (parts.status, parts.headers, body.collect().await.unwrap().to_bytes().to_vec())
    }

    async fn register(&self, title: &str) -> BroadcastResponse {
        let (status, _, body) = self.send(multipart(&[Part::Text("title", title), video_part(VIDEO)])).await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice(&body).unwrap()
    }

    fn stored_files(&self) -> usize {
        std::fs::read_dir(self.storage_dir.path()).unwrap().count()
    }
}

#[tokio::test]
async fn 영상과_제목을_등록하면_201과_상세_정보를_돌려준다() {
    let app = app().await;

    let (status, headers, body) = app.send(multipart(&[Part::Text("title", "  첫 방송 "), video_part(VIDEO)])).await;

    assert_eq!(status, StatusCode::CREATED);
    let created: BroadcastResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(created.title, "첫 방송");
    assert_eq!(created.video_url, format!("/api/broadcasts/{}/video", created.id));
    assert_eq!(headers[header::LOCATION], format!("/api/broadcasts/{}", created.id));
    assert_eq!(app.stored_files(), 1);
}

#[tokio::test]
async fn 목록은_최신_등록순으로_조회된다() {
    let app = app().await;
    app.register("먼저").await;
    app.register("나중").await;

    let (status, _, body) = app.send(get("/api/broadcasts")).await;

    assert_eq!(status, StatusCode::OK);
    let list: Vec<BroadcastResponse> = serde_json::from_slice(&body).unwrap();
    let titles: Vec<_> = list.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, ["나중", "먼저"]);
}

#[tokio::test]
async fn 상세를_조회할_수_있다() {
    let app = app().await;
    let created = app.register("상세 방송").await;

    let (status, _, body) = app.send(get(&format!("/api/broadcasts/{}", created.id))).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<BroadcastResponse>(&body).unwrap(), created);
}

#[tokio::test]
async fn 없는_방송은_404() {
    let app = app().await;

    let (status, _, body) = app.send(get("/api/broadcasts/999999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(serde_json::from_slice::<ErrorResponse>(&body).unwrap().status, 404);

    let (status, _, _) = app.send(get("/api/broadcasts/999999/video")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 등록한_영상을_재생용으로_내려준다() {
    let app = app().await;
    let created = app.register("재생").await;

    let (status, headers, body) = app.send(get(&created.video_url)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(body, VIDEO);
}

#[tokio::test]
async fn 영상은_range_요청으로_일부만_받을_수_있다() {
    let app = app().await;
    let created = app.register("탐색").await;

    let req = Request::get(&created.video_url).header(header::RANGE, "bytes=2-5").body(Body::empty()).unwrap();
    let (status, headers, body) = app.send(req).await;

    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(headers[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(body, b"2345");
}

async fn assert_rejected(app: &TestApp, parts: &[Part<'_>]) {
    let (status, _, body) = app.send(multipart(parts)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert!(!error.detail.is_empty());
    assert_eq!(app.stored_files(), 0, "거절된 요청의 영상 파일이 남으면 안 된다");
}

#[tokio::test]
async fn 제목이_비어있으면_400이고_파일도_남지_않는다() {
    let app = app().await;
    assert_rejected(&app, &[Part::Text("title", "  "), video_part(VIDEO)]).await;
    // 영상이 제목보다 먼저 와도 마찬가지
    assert_rejected(&app, &[video_part(VIDEO), Part::Text("title", "")]).await;
    assert_rejected(&app, &[video_part(VIDEO)]).await;
}

#[tokio::test]
async fn 영상_파일이_없으면_400() {
    let app = app().await;
    assert_rejected(&app, &[Part::Text("title", "영상 없음")]).await;
}

#[tokio::test]
async fn 영상이_아닌_파일은_400() {
    let app = app().await;
    let text = Part::File { name: "video", filename: "a.txt", content_type: "text/plain", bytes: b"hi" };
    assert_rejected(&app, &[Part::Text("title", "텍스트"), text]).await;
}

#[tokio::test]
async fn 빈_파일은_400() {
    let app = app().await;
    assert_rejected(&app, &[Part::Text("title", "빈 파일"), video_part(b"")]).await;
}

#[tokio::test]
async fn 서버를_다시_띄워도_등록_정보와_영상이_남아있다() {
    let first = app().await;
    let created = first.register("재시작").await;

    // 같은 DB, 같은 저장소 디렉터리로 앱 상태를 새로 만든다 (= 서버 재시작)
    let url = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        first._db.get_host().await.unwrap(),
        first._db.get_host_port_ipv4(5432).await.unwrap()
    );
    let restarted = router(AppState::connect(&url, first.storage_dir.path()).await.unwrap());

    let res = restarted.clone().oneshot(get(&format!("/api/broadcasts/{}", created.id))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = restarted.oneshot(get(&created.video_url)).await.unwrap();
    assert_eq!(res.into_body().collect().await.unwrap().to_bytes().as_ref(), VIDEO);
}
