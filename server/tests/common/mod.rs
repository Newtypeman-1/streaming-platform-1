//! 통합 테스트 공용 준비물: 실제 PostgreSQL(Testcontainers), 임시 영상 저장소, 가짜 MediaMTX.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing;
use http_body_util::BodyExt;
use server::{AppState, MediaServer, PaymentGateway, router};
use shared::{BroadcastResponse, ErrorResponse};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt};
use tower::ServiceExt;

pub struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub storage_dir: tempfile::TempDir,
    pub media: FakeMedia,
    pub toss: FakeToss,
    pub db: ContainerAsync<Postgres>,
}

pub async fn app() -> TestApp {
    let db = start_postgres().await;
    let storage_dir = tempfile::tempdir().unwrap();
    let media = FakeMedia::start().await;
    let toss = FakeToss::start().await;
    let state = AppState::connect(&db_url(&db).await, storage_dir.path(), media.server(), toss.gateway()).await.unwrap();
    TestApp { router: router(state.clone()), state, storage_dir, media, toss, db }
}

/// 테스트마다 컨테이너를 띄우므로 수십 개가 동시에 뜬다. 그러면 가끔 Docker 가 포트를 연결하지 못한
/// 컨테이너(`PortNotExposed`)가 생긴다. 그런 컨테이너는 버리고 새로 띄운다.
async fn start_postgres() -> ContainerAsync<Postgres> {
    for _ in 0..3 {
        let db = Postgres::default().with_tag("17").start().await.unwrap();
        if db.get_host_port_ipv4(5432).await.is_ok() {
            return db;
        }
    }
    panic!("PostgreSQL 테스트 컨테이너의 포트를 열지 못했습니다");
}

pub async fn db_url(db: &ContainerAsync<Postgres>) -> String {
    format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        db.get_host().await.unwrap(),
        db.get_host_port_ipv4(5432).await.unwrap()
    )
}

pub const CLIENT_KEY: &str = "test_ck_fake";
pub const SECRET_KEY: &str = "test_sk_fake";

/// 아무 것도 듣고 있지 않은 주소 → 결제 서비스에 연결할 수 없는 상황
pub fn unreachable_payments() -> PaymentGateway {
    PaymentGateway::new("http://127.0.0.1:9", SECRET_KEY, CLIENT_KEY)
}

/// 아무 것도 듣고 있지 않은 주소 → 미디어 서버가 꺼진 상황
pub fn unreachable_media() -> MediaServer {
    MediaServer::new("http://127.0.0.1:9", "http://127.0.0.1:9")
}

impl TestApp {
    /// 요청을 보내고 응답을 돌려준다. 오류 응답(4xx·5xx)은 모두 README 가 약속한
    /// `ErrorResponse` JSON 이어야 하므로, 어떤 테스트에서든 아니면 여기서 실패한다.
    pub async fn send(&self, req: Request<Body>) -> (StatusCode, header::HeaderMap, Vec<u8>) {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        let body = body.collect().await.unwrap().to_bytes().to_vec();
        if parts.status.is_client_error() || parts.status.is_server_error() {
            assert_error_format(parts.status, &parts.headers, &body);
        }
        (parts.status, parts.headers, body)
    }
}

fn assert_error_format(status: StatusCode, headers: &header::HeaderMap, body: &[u8]) {
    let text = String::from_utf8_lossy(body);
    assert_eq!(
        headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("application/json"),
        "오류 응답은 JSON 이어야 한다: {status} {text}"
    );
    let error: ErrorResponse =
        serde_json::from_slice(body).unwrap_or_else(|_| panic!("오류 응답이 ErrorResponse 형식이 아니다: {status} {text}"));
    assert_eq!(error.status, status.as_u16(), "본문의 status 가 응답 코드와 다르다: {text}");
    assert!(!error.detail.is_empty(), "오류 메시지가 비어 있다: {status}");
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


/// 토스페이먼츠 결제 API 흉내.
/// 승인 요청(`POST /v1/payments/confirm`)은 시크릿 키를 확인하고 요청 그대로 승인(DONE)해 기록한다.
/// 조회(`GET /v1/payments/{paymentKey}`)는 기록한 결제를 돌려준다.
#[derive(Clone, Default)]
pub struct FakeToss {
    pub url: String,
    /// paymentKey → 결제 객체
    pub payments: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    /// 승인 요청 횟수
    pub confirm_count: Arc<Mutex<u32>>,
    /// Some((HTTP 상태, 오류 코드)) 이면 승인 요청을 거절한다
    pub reject: Arc<Mutex<Option<(u16, String)>>>,
    /// Some 이면 요청과 다른 금액으로 승인했다고 응답한다
    pub approved_amount: Arc<Mutex<Option<i64>>>,
}

/// `SECRET_KEY` + ":" 의 Base64
const TOSS_AUTH: &str = "Basic dGVzdF9za19mYWtlOg==";

fn toss_error(status: StatusCode, code: &str, message: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    (status, axum::Json(serde_json::json!({ "code": code, "message": message }))).into_response()
}

impl FakeToss {
    async fn start() -> Self {
        use axum::response::IntoResponse;
        let mut fake = FakeToss::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        fake.url = format!("http://{}", listener.local_addr().unwrap());

        let confirm_state = fake.clone();
        let find_state = fake.clone();
        let app = Router::new()
            .route(
                "/v1/payments/confirm",
                routing::post(move |headers: header::HeaderMap, axum::Json(req): axum::Json<serde_json::Value>| {
                    let fake = confirm_state.clone();
                    async move {
                        if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(TOSS_AUTH) {
                            return toss_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED_KEY", "인증되지 않은 시크릿 키입니다.");
                        }
                        *fake.confirm_count.lock().unwrap() += 1;
                        // 테스트 환경에서 오류를 재현하는 헤더: 그 코드로 승인을 거절한다
                        if let Some(code) = headers.get("TossPayments-Test-Code").and_then(|v| v.to_str().ok()) {
                            return toss_error(StatusCode::FORBIDDEN, code, &format!("테스트 코드로 거절: {code}"));
                        }
                        if let Some((status, code)) = fake.reject.lock().unwrap().clone() {
                            return toss_error(StatusCode::from_u16(status).unwrap(), &code, "결제 승인이 거절되었습니다.");
                        }
                        let amount = fake.approved_amount.lock().unwrap().unwrap_or(req["amount"].as_i64().unwrap());
                        let payment = serde_json::json!({
                            "paymentKey": req["paymentKey"],
                            "orderId": req["orderId"],
                            "status": "DONE",
                            "totalAmount": amount,
                            "approvedAt": "2026-11-21T12:00:00+09:00",
                        });
                        fake.payments.lock().unwrap().insert(req["paymentKey"].as_str().unwrap().to_string(), payment.clone());
                        axum::Json(payment).into_response()
                    }
                }),
            )
            .route(
                "/v1/payments/{payment_key}",
                routing::get(move |headers: header::HeaderMap, axum::extract::Path(key): axum::extract::Path<String>| {
                    let fake = find_state.clone();
                    async move {
                        if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) != Some(TOSS_AUTH) {
                            return toss_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED_KEY", "인증되지 않은 시크릿 키입니다.");
                        }
                        match fake.payments.lock().unwrap().get(&key) {
                            Some(payment) => axum::Json(payment.clone()).into_response(),
                            None => toss_error(StatusCode::NOT_FOUND, "NOT_FOUND_PAYMENT", "존재하지 않는 결제 정보 입니다."),
                        }
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        fake
    }

    pub fn gateway(&self) -> PaymentGateway {
        PaymentGateway::new(&self.url, SECRET_KEY, CLIENT_KEY)
    }

    pub fn confirms(&self) -> u32 {
        *self.confirm_count.lock().unwrap()
    }
}
