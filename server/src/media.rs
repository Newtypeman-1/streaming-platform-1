//! 미디어 서버(MediaMTX) 클라이언트.
//!
//! 브라우저는 SDP(접속 정보)를 이 API 서버에 보내고, 서버가 키·상태를 확인한 뒤 MediaMTX 에 대신 전달한다.
//! 그래서 MediaMTX 의 신호·API 포트는 외부에 열 필요가 없다.

use std::collections::HashSet;

use axum::http::{StatusCode, header};
use serde::Deserialize;

#[derive(Clone)]
pub struct MediaServer {
    http: reqwest::Client,
    webrtc_url: String,
    api_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// 시청하려는 경로에 아직(또는 더 이상) 영상이 들어오지 않는다
    #[error("아직 방송 영상이 들어오지 않았습니다.")]
    NotStreaming,
    #[error("미디어 서버 요청 실패: {0}")]
    Failed(String),
}

impl From<reqwest::Error> for MediaError {
    fn from(e: reqwest::Error) -> Self {
        MediaError::Failed(e.to_string())
    }
}

/// WebRTC 송출(WHIP)인지 시청(WHEP)인지
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Publish,
    Read,
}

#[derive(Deserialize)]
struct PathList {
    items: Vec<PathItem>,
}

#[derive(Deserialize)]
struct PathItem {
    name: String,
    ready: bool,
}

impl MediaServer {
    /// `webrtc_url`: WHIP/WHEP 주소 (기본 8889), `api_url`: Control API 주소 (기본 9997)
    pub fn new(webrtc_url: &str, api_url: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("HTTP 클라이언트 생성 실패"),
            webrtc_url: webrtc_url.trim_end_matches('/').to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
        }
    }

    /// SDP offer 를 MediaMTX 에 넘기고 answer 를 받는다.
    pub async fn exchange_sdp(&self, path: &str, direction: Direction, offer: String) -> Result<String, MediaError> {
        let endpoint = match direction {
            Direction::Publish => "whip",
            Direction::Read => "whep",
        };
        let res = self
            .http
            .post(format!("{}/{path}/{endpoint}", self.webrtc_url))
            .header(header::CONTENT_TYPE, "application/sdp")
            .body(offer)
            .send()
            .await?;
        match res.status() {
            StatusCode::CREATED => {}
            StatusCode::NOT_FOUND => return Err(MediaError::NotStreaming),
            status => {
                return Err(MediaError::Failed(format!("{endpoint} {status}: {}", res.text().await.unwrap_or_default())));
            }
        }
        Ok(res.text().await?)
    }

    /// 지금 영상이 들어오고 있는 경로들
    pub async fn ready_paths(&self) -> Result<HashSet<String>, MediaError> {
        // 이 서비스 규모에선 한 페이지로 충분하다
        let list: PathList = self
            .http
            .get(format!("{}/v3/paths/list?itemsPerPage=1000", self.api_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(list.items.into_iter().filter(|p| p.ready).map(|p| p.name).collect())
    }
}
