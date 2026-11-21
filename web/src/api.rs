//! 백엔드 API 호출. 요청/응답 타입은 서버와 같은 shared 크레이트를 쓴다.

use gloo_net::http::{Request, Response};
use serde::de::DeserializeOwned;
use shared::{
    BroadcastResponse, CommentRequest, CommentResponse, ConfirmDonationRequest, Credentials, DonationOrderResponse,
    DonationRequest, DonationSummary, ErrorResponse, FailDonationRequest, StartLiveRequest, StartLiveResponse,
    UpdateBroadcastRequest, UserResponse,
};
use web_sys::{File, FormData};

async fn parse<T: DeserializeOwned>(res: Response) -> Result<T, String> {
    if res.ok() {
        return res.json().await.map_err(|e| e.to_string());
    }
    Err(error_message(res).await)
}

/// 본문 없는 성공 응답(204)
async fn expect_ok(res: Response) -> Result<(), String> {
    if res.ok() {
        return Ok(());
    }
    Err(error_message(res).await)
}

async fn error_message(res: Response) -> String {
    let status = res.status();
    res.json::<ErrorResponse>().await.map(|e| e.detail).unwrap_or_else(|_| format!("요청 실패 ({status})"))
}

async fn send(req: Result<Request, gloo_net::Error>) -> Result<Response, String> {
    req.map_err(|e| e.to_string())?.send().await.map_err(|e| e.to_string())
}

// ── 로그인 ── 세션은 HttpOnly 쿠키라 JS 가 직접 다루지 않는다. 같은 출처 요청이라 브라우저가 알아서 쿠키를 보낸다.

/// 로그인 상태. 로그인하지 않았으면 None.
pub async fn me() -> Result<Option<UserResponse>, String> {
    let res = Request::get("/api/auth/me").send().await.map_err(|e| e.to_string())?;
    if res.status() == 401 {
        return Ok(None);
    }
    parse(res).await.map(Some)
}

pub async fn signup(username: &str, password: &str) -> Result<UserResponse, String> {
    let req = Credentials { username: username.into(), password: password.into() };
    parse(send(Request::post("/api/auth/signup").json(&req)).await?).await
}

pub async fn login(username: &str, password: &str) -> Result<UserResponse, String> {
    let req = Credentials { username: username.into(), password: password.into() };
    parse(send(Request::post("/api/auth/login").json(&req)).await?).await
}

pub async fn logout() -> Result<(), String> {
    expect_ok(Request::post("/api/auth/logout").send().await.map_err(|e| e.to_string())?).await
}

pub async fn fetch_broadcasts() -> Result<Vec<BroadcastResponse>, String> {
    parse(Request::get("/api/broadcasts").send().await.map_err(|e| e.to_string())?).await
}

pub async fn fetch_broadcast(id: i64) -> Result<BroadcastResponse, String> {
    parse(Request::get(&format!("/api/broadcasts/{id}")).send().await.map_err(|e| e.to_string())?).await
}

/// 브라우저의 File 객체를 그대로 FormData 에 담아 보낸다.
/// (WASM 메모리로 파일 전체를 읽어 들이지 않으므로 큰 영상도 부담이 적다)
pub async fn register_broadcast(title: &str, video: &File) -> Result<BroadcastResponse, String> {
    let js_err = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let form = FormData::new().map_err(js_err)?;
    form.append_with_str("title", title).map_err(js_err)?;
    form.append_with_blob_and_filename("video", video, &video.name()).map_err(js_err)?;
    let res = Request::post("/api/broadcasts")
        .body(form)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(res).await
}

/// 라이브 방송을 만들고 송출 키를 받는다
pub async fn start_live(title: &str) -> Result<StartLiveResponse, String> {
    let req = StartLiveRequest { title: title.to_string() };
    let res = Request::post("/api/broadcasts/live")
        .json(&req)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(res).await
}

/// 라이브 종료. 로그인 쿠키로 소유자인지 서버가 확인한다.
pub async fn end_live(id: i64) -> Result<BroadcastResponse, String> {
    let res = Request::post(&shared::end_url(id))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    parse(res).await
}

pub async fn update_broadcast(id: i64, title: &str) -> Result<BroadcastResponse, String> {
    let req = UpdateBroadcastRequest { title: title.into() };
    parse(send(Request::patch(&format!("/api/broadcasts/{id}")).json(&req)).await?).await
}

pub async fn delete_broadcast(id: i64) -> Result<(), String> {
    expect_ok(Request::delete(&format!("/api/broadcasts/{id}")).send().await.map_err(|e| e.to_string())?).await
}

// ── 댓글 ──

pub async fn fetch_comments(broadcast_id: i64) -> Result<Vec<CommentResponse>, String> {
    parse(Request::get(&shared::comments_url(broadcast_id)).send().await.map_err(|e| e.to_string())?).await
}

pub async fn write_comment(broadcast_id: i64, body: &str) -> Result<CommentResponse, String> {
    let req = CommentRequest { body: body.into() };
    parse(send(Request::post(&shared::comments_url(broadcast_id)).json(&req)).await?).await
}

pub async fn edit_comment(id: i64, body: &str) -> Result<CommentResponse, String> {
    let req = CommentRequest { body: body.into() };
    parse(send(Request::patch(&shared::comment_url(id)).json(&req)).await?).await
}

pub async fn delete_comment(id: i64) -> Result<(), String> {
    expect_ok(Request::delete(&shared::comment_url(id)).send().await.map_err(|e| e.to_string())?).await
}

// ── 후원 ──

pub async fn fetch_donations(broadcast_id: i64) -> Result<DonationSummary, String> {
    parse(Request::get(&shared::donations_url(broadcast_id)).send().await.map_err(|e| e.to_string())?).await
}

pub async fn order_donation(broadcast_id: i64, amount: i64) -> Result<DonationOrderResponse, String> {
    parse(send(Request::post(&shared::donations_url(broadcast_id)).json(&DonationRequest { amount })).await?).await
}

pub async fn confirm_donation(req: &ConfirmDonationRequest) -> Result<DonationOrderResponse, String> {
    parse(send(Request::post(shared::DONATION_CONFIRM_URL).json(req)).await?).await
}

pub async fn fail_donation(order_id: &str, req: &FailDonationRequest) -> Result<DonationOrderResponse, String> {
    parse(send(Request::post(&shared::donation_fail_url(order_id)).json(req)).await?).await
}

/// 1234567 → "1,234,567원" (브라우저 숫자 형식)
pub fn won(amount: i64) -> String {
    let n: String = js_sys::Number::from(amount as f64).to_locale_string("ko-KR").into();
    format!("{n}원")
}

/// RFC 3339 시각을 브라우저 로캘 형식으로 보여준다.
pub fn local_time(rfc3339: &str) -> String {
    js_sys::Date::new(&rfc3339.into()).to_locale_string("ko-KR", &js_sys::Object::new()).into()
}
