//! 통합 테스트 공용 준비물: 실제 PostgreSQL(Testcontainers), 임시 영상 저장소, 가짜 MediaMTX.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing;
use http_body_util::BodyExt;
use server::{AppState, MediaServer, router};
use shared::BroadcastResponse;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt};
use tower::ServiceExt;

pub struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub storage_dir: tempfile::TempDir,
    pub media: FakeMedia,
    pub db: ContainerAsync<Postgres>,
}

pub async fn app() -> TestApp {
    let db = Postgres::default().with_tag("17").start().await.unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let media = FakeMedia::start().await;
    let state = AppState::connect(&db_url(&db).await, storage_dir.path(), media.server()).await.unwrap();
    TestApp { router: router(state.clone()), state, storage_dir, media, db }
}

pub async fn db_url(db: &ContainerAsync<Postgres>) -> String {
    format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        db.get_host().await.unwrap(),
        db.get_host_port_ipv4(5432).await.unwrap()
    )
}

/// 아무 것도 듣고 있지 않은 주소 → 미디어 서버가 꺼진 상황
pub fn unreachable_media() -> MediaServer {
    MediaServer::new("http://127.0.0.1:9", "http://127.0.0.1:9")
}

impl TestApp {
    pub async fn send(&self, req: Request<Body>) -> (StatusCode, header::HeaderMap, Vec<u8>) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        (parts.status, parts.headers, body.collect().await.unwrap().to_bytes().to_vec())
    }
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

pub fn json(method: &str, uri: &str, value: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap()
}

/// 로그인 쿠키(`nadri_session=...`)를 실어 보낸다
pub fn with_cookie(mut req: Request<Body>, cookie: &str) -> Request<Body> {
    req.headers_mut().insert(header::COOKIE, cookie.parse().unwrap());
    req
}

/// 응답의 Set-Cookie 에서 요청에 실어 보낼 `이름=값` 부분만 꺼낸다
pub fn session_cookie(headers: &header::HeaderMap) -> Option<String> {
    let set_cookie = headers.get(header::SET_COOKIE)?.to_str().ok()?;
    Some(set_cookie.split(';').next()?.to_string())
}

pub const PASSWORD: &str = "password123";

impl TestApp {
    /// 가입하고 로그인 쿠키를 돌려준다
    pub async fn signup(&self, username: &str) -> String {
        let (status, headers, _) =
            self.send(json("POST", "/api/auth/signup", serde_json::json!({ "username": username, "password": PASSWORD }))).await;
        assert_eq!(status, StatusCode::CREATED);
        session_cookie(&headers).unwrap()
    }
}

/// MediaMTX 흉내. WHIP/WHEP 요청은 받은 offer 를 기록하고 "answer:<offer>" 를 돌려준다.
/// `ready` 에 넣어 둔 경로만 송출 중인 것으로 보고, 그 경로만 시청할 수 있다.
#[derive(Clone, Default)]
pub struct FakeMedia {
    pub url: String,
    pub ready: Arc<Mutex<Vec<String>>>,
    /// (요청 경로, offer)
    pub offers: Arc<Mutex<Vec<(String, String)>>>,
}

impl FakeMedia {
    async fn start() -> Self {
        let mut fake = FakeMedia::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        fake.url = format!("http://{}", listener.local_addr().unwrap());

        let (ready, ready_for_read, offers) = (fake.ready.clone(), fake.ready.clone(), fake.offers.clone());
        let app = Router::new()
            .route(
                "/v3/paths/list",
                routing::get(move || async move {
                    let items: Vec<_> =
                        ready.lock().unwrap().iter().map(|name| serde_json::json!({ "name": name, "ready": true })).collect();
                    axum::Json(serde_json::json!({ "items": items }))
                }),
            )
            .route(
                "/{*path}",
                routing::post(move |axum::extract::Path(path): axum::extract::Path<String>, offer: String| async move {
                    offers.lock().unwrap().push((format!("/{path}"), offer.clone()));
                    // MediaMTX 는 영상이 들어오지 않는 경로를 시청하려 하면 404 를 준다
                    if let Some(media_path) = path.strip_suffix("/whep")
                        && !ready_for_read.lock().unwrap().iter().any(|p| p == media_path)
                    {
                        return (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "application/json")], "{}".to_string());
                    }
                    (StatusCode::CREATED, [(header::CONTENT_TYPE, "application/sdp")], format!("answer:{offer}"))
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        fake
    }

    pub fn server(&self) -> MediaServer {
        MediaServer::new(&self.url, &self.url)
    }

    pub fn set_ready(&self, paths: &[&str]) {
        *self.ready.lock().unwrap() = paths.iter().map(|p| p.to_string()).collect();
    }
}

pub const VIDEO: &[u8] = b"0123456789";
pub const BOUNDARY: &str = "test-boundary";

pub enum Part<'a> {
    Text(&'a str, &'a str),
    File { name: &'a str, filename: &'a str, content_type: &'a str, bytes: &'a [u8] },
}

pub fn multipart(parts: &[Part]) -> Request<Body> {
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

pub fn video_part(bytes: &[u8]) -> Part<'_> {
    Part::File { name: "video", filename: "clip.mp4", content_type: "video/mp4", bytes }
}

impl TestApp {
    /// `cookie` 사용자로 녹화 방송 등록
    pub async fn register(&self, cookie: &str, title: &str) -> BroadcastResponse {
        let (status, _, body) = self.send(with_cookie(multipart(&[Part::Text("title", title), video_part(VIDEO)]), cookie)).await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice(&body).unwrap()
    }

    pub fn stored_files(&self) -> usize {
        std::fs::read_dir(self.storage_dir.path()).unwrap().count()
    }
}

