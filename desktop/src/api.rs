//! 서버 API 호출. 웹 앱과 달리 다른 출처(서버 도메인)로 요청하므로 모든 주소는 서버 주소를 붙인 절대 주소다.

use serde::de::DeserializeOwned;
use shared::{BroadcastResponse, ErrorResponse};

/// 배포한 서비스 주소
pub const DEFAULT_SERVER_URL: &str = "https://nadri.me";

/// 접속할 서버 주소. 실행할 때 준 `NADRI_SERVER_URL` → 빌드할 때 준 `NADRI_SERVER_URL` → 기본값 순.
pub fn server_url(runtime: Option<&str>, built: Option<&str>) -> String {
    [runtime, built].into_iter().flatten().find(|url| !url.is_empty()).unwrap_or(DEFAULT_SERVER_URL).to_string()
}

#[derive(Clone)]
pub struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: &str) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), http: reqwest::Client::new() }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// 녹화 영상 재생 주소. 영상이 없는 방송(라이브 등)은 `None`.
    pub fn video_url(&self, broadcast: &BroadcastResponse) -> Option<String> {
        broadcast.video_url.as_ref().map(|path| format!("{}{path}", self.base))
    }

    /// 방송 목록 (최신순)
    pub async fn broadcasts(&self) -> Result<Vec<BroadcastResponse>, String> {
        parse(self.send(self.http.get(format!("{}/api/broadcasts", self.base))).await?).await
    }

    /// 라이브 시청(WHEP): 웹뷰가 만든 SDP offer 를 서버로 보내고 answer 를 받는다.
    /// 웹뷰 안의 JS 가 직접 보내면 출처가 달라 막히므로(CORS) 앱이 대신 보낸다.
    pub async fn watch(&self, id: i64, offer: &str) -> Result<String, String> {
        let req = self
            .http
            .post(format!("{}{}", self.base, shared::whep_url(id)))
            .header(reqwest::header::CONTENT_TYPE, "application/sdp")
            .body(offer.to_string());
        let res = self.send(req).await?;
        if res.status().is_success() {
            return res.text().await.map_err(|e| e.to_string());
        }
        Err(error_message(res).await)
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        req.send().await.map_err(|_| format!("서버({})에 연결할 수 없습니다. 인터넷 연결을 확인해 주세요.", self.base))
    }
}

/// 성공 응답은 JSON 본문을, 오류 응답은 서버가 준 오류 메시지(`ErrorResponse.detail`)를 돌려준다.
async fn parse<T: DeserializeOwned>(res: reqwest::Response) -> Result<T, String> {
    if res.status().is_success() {
        return res.json().await.map_err(|e| e.to_string());
    }
    Err(error_message(res).await)
}

async fn error_message(res: reqwest::Response) -> String {
    let status = res.status();
    match res.json::<ErrorResponse>().await {
        Ok(error) => error.detail,
        Err(_) => format!("서버 오류 ({status})"),
    }
}
