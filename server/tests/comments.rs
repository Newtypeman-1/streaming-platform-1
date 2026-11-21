//! 댓글 API 통합 테스트.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestApp, app, get, json, with_cookie};
use serde_json::json;
use shared::{CommentResponse, ErrorResponse};

fn write(broadcast_id: i64, body: &str) -> Request<Body> {
    json("POST", &shared::comments_url(broadcast_id), json!({ "body": body }))
}

fn edit(id: i64, body: &str) -> Request<Body> {
    json("PATCH", &shared::comment_url(id), json!({ "body": body }))
}

fn delete(id: i64) -> Request<Body> {
    Request::delete(shared::comment_url(id)).body(Body::empty()).unwrap()
}

impl TestApp {
    async fn comment(&self, cookie: &str, broadcast_id: i64, body: &str) -> CommentResponse {
        let (status, _, res) = self.send(with_cookie(write(broadcast_id, body), cookie)).await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice(&res).unwrap()
    }

    async fn comments(&self, broadcast_id: i64) -> Vec<CommentResponse> {
        let (status, _, body) = self.send(get(&shared::comments_url(broadcast_id))).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_slice(&body).unwrap()
    }
}

/// 방송 하나와 (방송 소유자, 다른 사용자) 쿠키
async fn setup(app: &TestApp) -> (i64, String, String) {
    let owner = app.signup("owner").await;
    let viewer = app.signup("viewer").await;
    let id = app.register(&owner, "댓글 달 방송").await.id;
    (id, owner, viewer)
}

#[tokio::test]
async fn 로그인한_사용자는_댓글을_쓸_수_있고_작성자가_기록된다() {
    let app = app().await;
    let (id, _, viewer) = setup(&app).await;

    let created = app.comment(&viewer, id, "  재밌어요 ").await;

    assert_eq!(created.broadcast_id, id);
    assert_eq!(created.body, "재밌어요");
    assert_eq!(created.author.username, "viewer");
    assert_eq!(created.updated_at, None);
}

#[tokio::test]
async fn 로그인하지_않으면_댓글을_쓸_수_없다() {
    let app = app().await;
    let (id, _, _) = setup(&app).await;

    let (status, _, _) = app.send(write(id, "익명 댓글")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(app.comments(id).await.is_empty());
}

#[tokio::test]
async fn 댓글_목록은_누구나_볼_수_있고_오래된_순이다() {
    let app = app().await;
    let (id, owner, viewer) = setup(&app).await;
    app.comment(&viewer, id, "첫 댓글").await;
    app.comment(&owner, id, "답글").await;

    let list = app.comments(id).await;

    let summary: Vec<_> = list.iter().map(|c| (c.author.username.as_str(), c.body.as_str())).collect();
    assert_eq!(summary, [("viewer", "첫 댓글"), ("owner", "답글")]);
}

#[tokio::test]
async fn 댓글_목록은_방송별로_나뉜다() {
    let app = app().await;
    let (first, owner, viewer) = setup(&app).await;
    let second = app.register(&owner, "다른 방송").await.id;
    app.comment(&viewer, first, "첫 방송 댓글").await;

    assert_eq!(app.comments(first).await.len(), 1);
    assert!(app.comments(second).await.is_empty());
}

#[tokio::test]
async fn 빈_댓글이나_500자_넘는_댓글은_400() {
    let app = app().await;
    let (id, _, viewer) = setup(&app).await;

    for body in ["   ".to_string(), "가".repeat(501)] {
        let (status, _, _) = app.send(with_cookie(write(id, &body), &viewer)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _, _) = app.send(with_cookie(write(id, &"가".repeat(500)), &viewer)).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn 없는_방송의_댓글은_404() {
    let app = app().await;
    let (_, _, viewer) = setup(&app).await;

    let (status, _, _) = app.send(get(&shared::comments_url(999))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = app.send(with_cookie(write(999, "댓글"), &viewer)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 작성자는_댓글을_고칠_수_있고_수정_시각이_남는다() {
    let app = app().await;
    let (id, _, viewer) = setup(&app).await;
    let created = app.comment(&viewer, id, "오타 있는 댓글").await;

    let (status, _, body) = app.send(with_cookie(edit(created.id, " 고친 댓글 "), &viewer)).await;

    assert_eq!(status, StatusCode::OK);
    let edited: CommentResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(edited.body, "고친 댓글");
    assert!(edited.updated_at.is_some());
    assert_eq!(app.comments(id).await, [edited]);
}

#[tokio::test]
async fn 비로그인은_401_다른_사용자는_403으로_남의_댓글을_고칠_수_없다() {
    let app = app().await;
    let (id, owner, viewer) = setup(&app).await;
    let created = app.comment(&viewer, id, "원래 댓글").await;

    let (status, _, _) = app.send(edit(created.id, "바꿈")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // 방송 소유자라도 남의 댓글은 고칠 수 없다
    let (status, _, body) = app.send(with_cookie(edit(created.id, "바꿈"), &owner)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(serde_json::from_slice::<ErrorResponse>(&body).unwrap().detail, "댓글 작성자만 할 수 있습니다.");

    assert_eq!(app.comments(id).await, [created]);
}

#[tokio::test]
async fn 작성자는_댓글을_지울_수_있다() {
    let app = app().await;
    let (id, _, viewer) = setup(&app).await;
    let created = app.comment(&viewer, id, "지울 댓글").await;

    let (status, _, _) = app.send(with_cookie(delete(created.id), &viewer)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.comments(id).await.is_empty());
}

#[tokio::test]
async fn 비로그인은_401_다른_사용자는_403으로_남의_댓글을_지울_수_없다() {
    let app = app().await;
    let (id, owner, viewer) = setup(&app).await;
    let created = app.comment(&viewer, id, "남길 댓글").await;

    let (status, _, _) = app.send(delete(created.id)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = app.send(with_cookie(delete(created.id), &owner)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert_eq!(app.comments(id).await, [created]);
}

#[tokio::test]
async fn 없는_댓글을_고치거나_지우면_404() {
    let app = app().await;
    let (_, _, viewer) = setup(&app).await;

    for req in [edit(999, "댓글"), delete(999)] {
        let (status, _, _) = app.send(with_cookie(req, &viewer)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn 방송을_지우면_댓글도_함께_지워진다() {
    let app = app().await;
    let (id, owner, viewer) = setup(&app).await;
    let created = app.comment(&viewer, id, "방송과 함께 사라질 댓글").await;

    let (status, _, _) = app.send(with_cookie(Request::delete(format!("/api/broadcasts/{id}")).body(Body::empty()).unwrap(), &owner)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = app.send(with_cookie(delete(created.id), &viewer)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
