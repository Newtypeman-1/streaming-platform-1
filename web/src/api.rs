//! 백엔드 API 호출. 요청/응답 타입은 서버와 같은 shared 크레이트를 쓴다.

use gloo_net::http::{Request, Response};
use serde::de::DeserializeOwned;
use shared::{BroadcastResponse, ErrorResponse, StartLiveRequest, StartLiveResponse};
use web_sys::{File, FormData};

async fn parse<T: DeserializeOwned>(res: Response) -> Result<T, String> {
    if res.ok() {
        return res.json().await.map_err(|e| e.to_string());
    }
    let status = res.status();
    Err(res
        .json::<ErrorResponse>()
        .await
        .map(|e| e.detail)
        .unwrap_or_else(|_| format!("요청 실패 ({status})")))
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

/// RFC 3339 시각을 브라우저 로캘 형식으로 보여준다.
pub fn local_time(rfc3339: &str) -> String {
    js_sys::Date::new(&rfc3339.into()).to_locale_string("ko-KR", &js_sys::Object::new()).into()
}
