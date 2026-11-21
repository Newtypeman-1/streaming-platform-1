//! 방송 도메인 규칙.

pub const TITLE_MAX_CHARS: usize = 100;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum BroadcastError {
    #[error("제목을 입력해주세요.")]
    EmptyTitle,
    #[error("제목은 {TITLE_MAX_CHARS}자 이하여야 합니다.")]
    TitleTooLong,
    #[error("영상 파일만 등록할 수 있습니다.")]
    NotAVideo,
    #[error("영상 파일이 비어 있습니다.")]
    EmptyVideo,
    #[error("방송 소유자만 할 수 있습니다.")]
    NotOwner,
    #[error("방송 중에는 삭제할 수 없습니다. 먼저 방송을 종료해주세요.")]
    StillLive,
}

/// 방송 수정·삭제·종료는 소유자만. 소유자가 없는 방송(3단계 이전에 만든 방송)은 아무도 할 수 없다.
pub fn ensure_owner(owner_id: Option<i64>, user_id: i64) -> Result<(), BroadcastError> {
    match owner_id {
        Some(owner_id) if owner_id == user_id => Ok(()),
        _ => Err(BroadcastError::NotOwner),
    }
}

/// 방송 중인 라이브는 지울 수 없다. 송출이 미디어 서버에 남은 채 기록만 사라지는 것을 막는다.
pub fn ensure_deletable(status: shared::BroadcastStatus) -> Result<(), BroadcastError> {
    match status {
        shared::BroadcastStatus::Live => Err(BroadcastError::StillLive),
        _ => Ok(()),
    }
}

/// 제목 규칙: 앞뒤 공백 제거, 비어있지 않을 것, 100자(글자 수 기준) 이하.
pub fn normalize_title(title: &str) -> Result<String, BroadcastError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(BroadcastError::EmptyTitle);
    }
    if trimmed.chars().count() > TITLE_MAX_CHARS {
        return Err(BroadcastError::TitleTooLong);
    }
    Ok(trimmed.to_string())
}

/// `video/*` 타입만 허용한다.
pub fn validate_video_content_type(content_type: Option<&str>) -> Result<(), BroadcastError> {
    match content_type {
        Some(ct) if ct.starts_with("video/") => Ok(()),
        _ => Err(BroadcastError::NotAVideo),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 제목은_앞뒤_공백을_제거한다() {
        assert_eq!(normalize_title("  첫 방송  "), Ok("첫 방송".to_string()));
    }

    #[test]
    fn 빈_제목은_거부한다() {
        assert_eq!(normalize_title(""), Err(BroadcastError::EmptyTitle));
        assert_eq!(normalize_title("   "), Err(BroadcastError::EmptyTitle));
    }

    #[test]
    fn 제목_길이는_바이트가_아니라_글자_수로_센다() {
        // 한글 100자 = 300바이트지만 허용되어야 한다
        assert!(normalize_title(&"가".repeat(100)).is_ok());
        assert_eq!(normalize_title(&"가".repeat(101)), Err(BroadcastError::TitleTooLong));
    }

    #[test]
    fn 소유자만_통과한다() {
        assert_eq!(ensure_owner(Some(1), 1), Ok(()));
        assert_eq!(ensure_owner(Some(1), 2), Err(BroadcastError::NotOwner));
    }

    #[test]
    fn 소유자가_없는_방송은_아무도_관리할_수_없다() {
        assert_eq!(ensure_owner(None, 1), Err(BroadcastError::NotOwner));
    }

    #[test]
    fn 방송_중인_라이브만_삭제를_막는다() {
        use shared::BroadcastStatus::*;
        assert_eq!(ensure_deletable(Live), Err(BroadcastError::StillLive));
        assert_eq!(ensure_deletable(Ended), Ok(()));
        assert_eq!(ensure_deletable(Recorded), Ok(()));
    }

    #[test]
    fn 영상_타입만_허용한다() {
        assert_eq!(validate_video_content_type(Some("video/mp4")), Ok(()));
        assert_eq!(validate_video_content_type(Some("video/webm")), Ok(()));
        assert_eq!(validate_video_content_type(Some("text/plain")), Err(BroadcastError::NotAVideo));
        assert_eq!(validate_video_content_type(None), Err(BroadcastError::NotAVideo));
    }
}
