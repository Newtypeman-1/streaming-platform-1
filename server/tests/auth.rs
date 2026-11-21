//! 회원가입·로그인·로그아웃 통합 테스트.

mod common;

use axum::http::{StatusCode, header};
use common::{PASSWORD, app, get, json, session_cookie, with_cookie};
use serde_json::json;
use shared::{ErrorResponse, UserResponse};

fn signup(username: &str, password: &str) -> axum::http::Request<axum::body::Body> {
    json("POST", "/api/auth/signup", json!({ "username": username, "password": password }))
}

fn login(username: &str, password: &str) -> axum::http::Request<axum::body::Body> {
    json("POST", "/api/auth/login", json!({ "username": username, "password": password }))
}

fn error_detail(body: &[u8]) -> String {
    serde_json::from_slice::<ErrorResponse>(body).unwrap().detail
}

#[tokio::test]
async fn 가입하면_201과_사용자_정보를_돌려주고_바로_로그인된다() {
    let app = app().await;

    let (status, headers, body) = app.send(signup("  Alice ", PASSWORD)).await;

    assert_eq!(status, StatusCode::CREATED);
    let user: UserResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(user.username, "alice");
    let set_cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set_cookie.contains("HttpOnly"), "JS 에서 세션 쿠키를 읽을 수 없어야 한다");
    assert!(set_cookie.contains("SameSite=Lax"));

    let (status, _, body) = app.send(with_cookie(get("/api/auth/me"), &session_cookie(&headers).unwrap())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<UserResponse>(&body).unwrap(), user);
}

#[tokio::test]
async fn 응답에_비밀번호나_해시가_나오지_않는다() {
    let app = app().await;

    let (_, _, body) = app.send(signup("alice", PASSWORD)).await;

    let body = String::from_utf8(body).unwrap();
    assert!(!body.contains(PASSWORD));
    assert!(!body.contains("argon2"));
}

#[tokio::test]
async fn 이미_있는_아이디로_가입하면_409() {
    let app = app().await;
    app.signup("alice").await;

    let (status, headers, body) = app.send(signup("ALICE", "another-password")).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_detail(&body), "이미 사용 중인 아이디입니다.");
    assert!(headers.get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn 아이디나_비밀번호_규칙에_맞지_않으면_400() {
    let app = app().await;

    for (username, password) in [("ab", PASSWORD), ("한글아이디", PASSWORD), ("alice", "short")] {
        let (status, _, _) = app.send(signup(username, password)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{username} / {password}");
    }
}

#[tokio::test]
async fn 맞는_비밀번호로_로그인하면_세션_쿠키를_준다() {
    let app = app().await;
    app.signup("alice").await;

    let (status, headers, body) = app.send(login("Alice", PASSWORD)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(serde_json::from_slice::<UserResponse>(&body).unwrap().username, "alice");
    let (status, _, _) = app.send(with_cookie(get("/api/auth/me"), &session_cookie(&headers).unwrap())).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn 비밀번호가_틀리거나_없는_아이디면_401이고_같은_메시지다() {
    let app = app().await;
    app.signup("alice").await;

    let (status, headers, wrong_password) = app.send(login("alice", "wrong-password")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers.get(header::SET_COOKIE).is_none());

    let (status, _, no_user) = app.send(login("nobody", PASSWORD)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // 어떤 아이디가 가입돼 있는지 알려 주지 않는다
    assert_eq!(error_detail(&wrong_password), error_detail(&no_user));
}

#[tokio::test]
async fn 로그인하지_않았거나_모르는_세션이면_내_정보는_401() {
    let app = app().await;

    let (status, _, _) = app.send(get("/api/auth/me")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.send(with_cookie(get("/api/auth/me"), "nadri_session=made-up")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn 로그아웃하면_그_세션은_더_이상_쓸_수_없다() {
    let app = app().await;
    let cookie = app.signup("alice").await;

    let (status, headers, _) = app.send(with_cookie(json("POST", "/api/auth/logout", json!({})), &cookie)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"), "브라우저 쿠키도 지운다");
    let (status, _, _) = app.send(with_cookie(get("/api/auth/me"), &cookie)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "쿠키를 지우지 않은 채 다시 보내도 서버가 거절해야 한다");
}

#[tokio::test]
async fn 로그아웃은_다른_기기의_세션에_영향이_없다() {
    let app = app().await;
    let first = app.signup("alice").await;
    let (_, headers, _) = app.send(login("alice", PASSWORD)).await;
    let second = session_cookie(&headers).unwrap();

    app.send(with_cookie(json("POST", "/api/auth/logout", json!({})), &first)).await;

    let (status, _, _) = app.send(with_cookie(get("/api/auth/me"), &second)).await;
    assert_eq!(status, StatusCode::OK);
}
