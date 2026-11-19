//! 서버와 웹 클라이언트가 함께 쓰는 API 타입.
//! 한쪽에서 필드를 바꾸면 다른 쪽이 컴파일 단계에서 바로 알 수 있다.

use serde::{Deserialize, Serialize};

/// 방송 목록/상세 응답.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastResponse {
    pub id: i64,
    pub title: String,
    /// 영상 재생 주소 (Range 요청 지원)
    pub video_url: String,
    /// RFC 3339 (예: 2026-11-19T10:00:00Z)
    pub created_at: String,
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
