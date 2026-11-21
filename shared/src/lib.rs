//! 서버와 웹 클라이언트가 함께 쓰는 API 타입.
//! 한쪽에서 필드를 바꾸면 다른 쪽이 컴파일 단계에서 바로 알 수 있다.

use serde::{Deserialize, Serialize};

/// 방송 상태. 녹화 방송은 처음부터 끝까지 `Recorded`, 라이브는 `Live` → `Ended`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BroadcastStatus {
    Recorded,
    Live,
    Ended,
}

impl BroadcastStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Live => "live",
            Self::Ended => "ended",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Recorded, Self::Live, Self::Ended].into_iter().find(|status| status.as_str() == s)
    }
}

/// 방송 목록/상세 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastResponse {
    pub id: i64,
    pub title: String,
    pub status: BroadcastStatus,
    /// 방송한 사람. 3단계(로그인) 이전에 만든 방송은 없다.
    pub owner: Option<UserResponse>,
    /// 녹화 영상 재생 주소 (Range 요청 지원). 라이브 방송은 없다.
    pub video_url: Option<String>,
    /// RFC 3339 (예: 2026-11-19T10:00:00Z)
    pub created_at: String,
    /// 라이브가 끝난 시각
    pub ended_at: Option<String>,
}

/// 방송 정보 수정 (소유자만)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateBroadcastRequest {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartLiveRequest {
    pub title: String,
}

/// 라이브 시작 응답. 송출 키는 송출한 사람만 이 응답으로 한 번 받는다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartLiveResponse {
    pub broadcast: BroadcastResponse,
    pub stream_key: String,
}

/// 회원가입·로그인 요청.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// 로그인한 사용자, 방송 소유자, 댓글 작성자.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
}

/// 오류 응답. RFC 9457 Problem Details 의 필드 이름을 따른다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub status: u16,
    pub detail: String,
}

pub fn video_url(id: i64) -> String {
    format!("/api/broadcasts/{id}/video")
}

/// WebRTC 송출(WHIP). `Authorization: Bearer <송출 키>` 와 SDP offer 를 보낸다.
pub fn whip_url(id: i64) -> String {
    format!("/api/broadcasts/{id}/whip")
}

/// WebRTC 시청(WHEP). SDP offer 를 보낸다.
pub fn whep_url(id: i64) -> String {
    format!("/api/broadcasts/{id}/whep")
}

/// 라이브 종료. 방송 소유자로 로그인해 있어야 한다.
pub fn end_url(id: i64) -> String {
    format!("/api/broadcasts/{id}/end")
}
