//! 댓글 도메인 규칙.

pub const BODY_MAX_CHARS: usize = 500;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommentError {
    #[error("댓글 내용을 입력해주세요.")]
    EmptyBody,
    #[error("댓글은 {BODY_MAX_CHARS}자 이하여야 합니다.")]
    BodyTooLong,
    #[error("댓글 작성자만 할 수 있습니다.")]
    NotAuthor,
}

/// 내용 규칙: 앞뒤 공백 제거, 비어있지 않을 것, 500자(글자 수 기준) 이하.
pub fn normalize_body(body: &str) -> Result<String, CommentError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(CommentError::EmptyBody);
    }
    if trimmed.chars().count() > BODY_MAX_CHARS {
        return Err(CommentError::BodyTooLong);
    }
    Ok(trimmed.to_string())
}

/// 댓글 수정·삭제는 작성자만. 방송 소유자라도 남의 댓글은 건드릴 수 없다.
pub fn ensure_author(author_id: i64, user_id: i64) -> Result<(), CommentError> {
    if author_id != user_id {
        return Err(CommentError::NotAuthor);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 내용은_앞뒤_공백을_제거한다() {
        assert_eq!(normalize_body("  안녕하세요\n"), Ok("안녕하세요".to_string()));
    }

    #[test]
    fn 빈_내용은_거부한다() {
        assert_eq!(normalize_body(""), Err(CommentError::EmptyBody));
        assert_eq!(normalize_body(" \n "), Err(CommentError::EmptyBody));
    }

    #[test]
    fn 내용_길이는_글자_수로_500자까지() {
        assert!(normalize_body(&"가".repeat(500)).is_ok());
        assert_eq!(normalize_body(&"가".repeat(501)), Err(CommentError::BodyTooLong));
    }

    #[test]
    fn 작성자만_통과한다() {
        assert_eq!(ensure_author(1, 1), Ok(()));
        assert_eq!(ensure_author(1, 2), Err(CommentError::NotAuthor));
    }
}
