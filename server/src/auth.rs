//! 회원·로그인 도메인 규칙.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use sha2::{Digest, Sha256};

pub const USERNAME_MIN_CHARS: usize = 3;
pub const USERNAME_MAX_CHARS: usize = 20;
pub const PASSWORD_MIN_CHARS: usize = 8;
pub const PASSWORD_MAX_CHARS: usize = 64;
/// 로그인 유지 기간
pub const SESSION_DAYS: i64 = 30;
pub const SESSION_COOKIE: &str = "nadri_session";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("아이디는 영문 소문자·숫자·밑줄(_)로 {USERNAME_MIN_CHARS}~{USERNAME_MAX_CHARS}자여야 합니다.")]
    InvalidUsername,
    #[error("비밀번호는 {PASSWORD_MIN_CHARS}~{PASSWORD_MAX_CHARS}자여야 합니다.")]
    InvalidPassword,
    #[error("이미 사용 중인 아이디입니다.")]
    UsernameTaken,
    #[error("아이디 또는 비밀번호가 올바르지 않습니다.")]
    WrongCredentials,
    #[error("로그인이 필요합니다.")]
    LoginRequired,
}

/// 아이디 규칙: 앞뒤 공백 제거, 대문자는 소문자로. 영문 소문자·숫자·밑줄만, 3~20자.
pub fn normalize_username(username: &str) -> Result<String, AuthError> {
    let username = username.trim().to_lowercase();
    let len = username.chars().count();
    let allowed = username.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !allowed || !(USERNAME_MIN_CHARS..=USERNAME_MAX_CHARS).contains(&len) {
        return Err(AuthError::InvalidUsername);
    }
    Ok(username)
}

/// 비밀번호 규칙: 8~64자(글자 수 기준). 공백도 비밀번호의 일부로 본다.
pub fn validate_password(password: &str) -> Result<(), AuthError> {
    let len = password.chars().count();
    if !(PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&len) {
        return Err(AuthError::InvalidPassword);
    }
    Ok(())
}

/// Argon2id 해시(PHC 문자열). 솔트는 매번 새로 만든다.
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).expect("Argon2 해시는 실패하지 않는다").to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}

/// 로그인 세션 토큰(256비트 난수, 16진수). 브라우저 쿠키에만 원문이 있고 DB 에는 해시만 저장한다.
pub fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

pub fn hash_session_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `Cookie` 헤더 값에서 세션 토큰을 꺼낸다.
pub fn session_token_from_cookie(cookie_header: &str) -> Option<&str> {
    cookie_header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

/// 로그인 쿠키. JS 에서 읽을 수 없고(HttpOnly), 다른 사이트에서 보낸 POST 에는 실리지 않는다(SameSite=Lax).
pub fn session_cookie(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}", SESSION_DAYS * 24 * 60 * 60)
}

pub fn expired_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 아이디는_공백을_빼고_소문자로_맞춘다() {
        assert_eq!(normalize_username("  Nadri_01 "), Ok("nadri_01".to_string()));
    }

    #[test]
    fn 아이디는_영문_숫자_밑줄_3에서_20자만_허용한다() {
        assert!(normalize_username("abc").is_ok());
        assert!(normalize_username(&"a".repeat(20)).is_ok());
        for bad in ["ab", &"a".repeat(21), "나드리", "a b c", "abc!", ""] {
            assert_eq!(normalize_username(bad), Err(AuthError::InvalidUsername), "{bad:?}");
        }
    }

    #[test]
    fn 비밀번호는_8에서_64자() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password(&"가".repeat(64)).is_ok());
        assert_eq!(validate_password("1234567"), Err(AuthError::InvalidPassword));
        assert_eq!(validate_password(&"a".repeat(65)), Err(AuthError::InvalidPassword));
    }

    #[test]
    fn 비밀번호_해시는_원문을_담지_않고_같은_비밀번호만_통과한다() {
        let hash = hash_password("correct horse");
        assert!(!hash.contains("correct horse"));
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong horse", &hash));
        assert!(!verify_password("correct horse", "망가진 해시"));
    }

    #[test]
    fn 같은_비밀번호도_해시는_매번_다르다() {
        assert_ne!(hash_password("same password"), hash_password("same password"));
    }

    #[test]
    fn 세션_토큰은_추측할_수_없는_길이이고_매번_다르다() {
        let token = new_session_token();
        assert_eq!(token.len(), 64);
        assert_ne!(token, new_session_token());
        assert_ne!(hash_session_token(&token), token);
        assert_eq!(hash_session_token(&token), hash_session_token(&token));
    }

    #[test]
    fn 쿠키_헤더에서_세션_토큰을_찾는다() {
        assert_eq!(session_token_from_cookie("a=1; nadri_session=tok; b=2"), Some("tok"));
        assert_eq!(session_token_from_cookie("nadri_session=tok"), Some("tok"));
        assert_eq!(session_token_from_cookie("other=tok"), None);
        assert_eq!(session_token_from_cookie("nadri_session="), None);
    }
}
