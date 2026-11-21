//! 방송 상세 화면의 댓글. 수정·삭제 버튼은 내 댓글에만 보이지만, 권한은 서버가 다시 검사한다.

use dioxus::prelude::*;
use gloo_timers::future::TimeoutFuture;
use shared::CommentResponse;

use crate::Route;
use crate::api;
use crate::auth::{Session, use_session};

/// 라이브 중 새 댓글을 불러오는 주기
const POLL_MS: u32 = 3000;

pub fn confirm(message: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(message).ok()).unwrap_or(false)
}

/// `live` 이면 주기적으로 목록을 다시 불러온다. 방송이 바뀌거나 라이브가 끝나면 부모가 key 로 새로 띄운다.
#[component]
pub fn Comments(broadcast_id: i64, live: bool) -> Element {
    let session = use_session();
    let mut list = use_signal(|| None::<Result<Vec<CommentResponse>, String>>);
    let mut draft = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // 다시 불러오는 동안에도 이전 목록을 그대로 보여 준다
    let reload = move || async move { list.set(Some(api::fetch_comments(broadcast_id).await)) };
    use_future(move || async move {
        loop {
            reload().await;
            if !live {
                return;
            }
            TimeoutFuture::new(POLL_MS).await;
        }
    });

    let onsubmit = move |evt: FormEvent| async move {
        evt.prevent_default();
        sending.set(true);
        error.set(None);
        match api::write_comment(broadcast_id, &draft()).await {
            Ok(_) => {
                draft.set(String::new());
                reload().await;
            }
            Err(e) => error.set(Some(e)),
        }
        sending.set(false);
    };

    rsx! {
        section { class: "comments",
            match &*list.read() {
                Some(Ok(comments)) => rsx! {
                    h2 { "댓글 {comments.len()}" }
                    ul {
                        for c in comments.iter().cloned() {
                            CommentItem { key: "{c.id}", comment: c, on_change: move |_| async move { reload().await } }
                        }
                    }
                },
                Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
                None => rsx! { h2 { "댓글" } },
            }
            match session() {
                Session::User(_) => rsx! {
                    form { class: "comment-form", onsubmit,
                        textarea {
                            value: "{draft}",
                            placeholder: "댓글을 남겨 보세요 (500자까지)",
                            maxlength: 500,
                            required: true,
                            oninput: move |e| draft.set(e.value()),
                        }
                        if let Some(e) = error() {
                            p { class: "error", "{e}" }
                        }
                        div { class: "row",
                            button { class: "button", disabled: sending(), "댓글 등록" }
                        }
                    }
                },
                Session::Guest => rsx! {
                    p { class: "meta",
                        Link { to: Route::Login {}, class: "link", "로그인" }
                        "하면 댓글을 쓸 수 있습니다."
                    }
                },
                Session::Checking => rsx! {},
            }
        }
    }
}

#[component]
fn CommentItem(comment: CommentResponse, on_change: EventHandler<()>) -> Element {
    let session = use_session();
    let mut editing = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let id = comment.id;
    let mine = session().is(comment.author.id);
    let body = comment.body.clone();

    let save = move |evt: FormEvent| async move {
        evt.prevent_default();
        let Some(text) = editing() else { return };
        match api::edit_comment(id, &text).await {
            Ok(_) => {
                editing.set(None);
                error.set(None);
                on_change.call(());
            }
            Err(e) => error.set(Some(e)),
        }
    };

    let delete = move |_| async move {
        if !confirm("댓글을 삭제할까요?") {
            return;
        }
        match api::delete_comment(id).await {
            Ok(()) => on_change.call(()),
            Err(e) => error.set(Some(e)),
        }
    };

    rsx! {
        li { class: "comment",
            div { class: "row",
                strong { "{comment.author.username}" }
                span { class: "meta",
                    {api::local_time(&comment.created_at)}
                    if comment.updated_at.is_some() { " (수정됨)" }
                }
                if mine && editing().is_none() {
                    button { class: "link-button", onclick: move |_| editing.set(Some(body.clone())), "수정" }
                    button { class: "link-button danger", onclick: delete, "삭제" }
                }
            }
            if let Some(text) = editing() {
                form { class: "comment-form", onsubmit: save,
                    textarea { value: "{text}", maxlength: 500, required: true, oninput: move |e| editing.set(Some(e.value())) }
                    div { class: "row",
                        button { class: "button", "저장" }
                        button {
                            class: "button secondary",
                            r#type: "button",
                            onclick: move |_| {
                                editing.set(None);
                                error.set(None);
                            },
                            "취소"
                        }
                    }
                }
            } else {
                p { "{comment.body}" }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
        }
    }
}
