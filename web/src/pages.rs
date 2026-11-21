use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use shared::{BroadcastResponse, BroadcastStatus};

use crate::Route;
use crate::api;
use crate::auth::{RequireLogin, use_session};
use crate::comments::{Comments, confirm};
use crate::live::LivePlayer;

#[component]
pub fn BroadcastList() -> Element {
    let broadcasts = use_resource(api::fetch_broadcasts);

    match &*broadcasts.read() {
        None => rsx! { p { "불러오는 중..." } },
        Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
        Some(Ok(list)) if list.is_empty() => rsx! { p { "아직 방송이 없습니다. 라이브를 시작하거나 녹화 방송을 등록해 보세요." } },
        Some(Ok(list)) => rsx! {
            h1 { "방송 목록" }
            ul { class: "grid",
                for b in list {
                    li { key: "{b.id}",
                        Link { to: Route::BroadcastDetail { id: b.id }, class: "card",
                            div { class: "thumb",
                                if let Some(url) = &b.video_url {
                                    // #t=0.1 : 첫 프레임을 썸네일처럼 보여준다
                                    video { src: "{url}#t=0.1", preload: "metadata", muted: true }
                                }
                                StatusBadge { status: b.status }
                            }
                            strong { "{b.title}" }
                            span { class: "meta",
                                if let Some(owner) = &b.owner {
                                    "{owner.username} · "
                                }
                                {api::local_time(&b.created_at)}
                            }
                        }
                    }
                }
            }
        },
    }
}

#[component]
fn StatusBadge(status: BroadcastStatus) -> Element {
    match status {
        BroadcastStatus::Live => rsx! { span { class: "badge live", "LIVE" } },
        BroadcastStatus::Ended => rsx! { span { class: "badge ended", "종료" } },
        BroadcastStatus::Recorded => rsx! { span { class: "badge", "녹화" } },
    }
}

#[component]
pub fn BroadcastDetail(id: i64) -> Element {
    // id 가 바뀌면(다른 방송으로 이동) 다시 불러온다
    let mut broadcast = use_resource(use_reactive!(|id| api::fetch_broadcast(id)));

    match &*broadcast.read() {
        None => rsx! { p { "불러오는 중..." } },
        Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
        Some(Ok(b)) => rsx! {
            article {
                match (&b.status, &b.video_url) {
                    // 라이브가 끝나면 다시 불러와 종료 화면으로 바꾼다
                    (BroadcastStatus::Live, _) => rsx! { LivePlayer { id: b.id, on_end: move |_| broadcast.restart() } },
                    (_, Some(url)) => rsx! { video { class: "player", src: "{url}", controls: true, autoplay: true, playsinline: true } },
                    (_, None) => rsx! {
                        div { class: "player ended",
                            p { "방송이 종료되었습니다." }
                            if let Some(at) = b.ended_at.as_deref() {
                                p { class: "meta", {api::local_time(at)} " 종료" }
                            }
                        }
                    },
                }
                TitleBar { key: "{b.id}", broadcast: b.clone() }
                p { class: "meta",
                    if let Some(owner) = &b.owner {
                        "{owner.username} · "
                    }
                    {api::local_time(&b.created_at)}
                    " 시작"
                }
                Comments { key: "{b.id}-{b.status.as_str()}", broadcast_id: b.id, live: b.status == BroadcastStatus::Live }
            }
        },
    }
}

const VIDEO_INPUT_ID: &str = "video-input";

/// Dioxus 이벤트로 받은 파일 대신 브라우저의 원본 File 객체를 꺼낸다.
fn selected_video() -> Option<web_sys::File> {
    web_sys::window()?
        .document()?
        .get_element_by_id(VIDEO_INPUT_ID)?
        .dyn_into::<HtmlInputElement>()
        .ok()?
        .files()?
        .get(0)
}

/// 제목과, 소유자에게만 보이는 제목 수정·삭제 버튼. 버튼을 숨기는 것과 별개로 서버가 소유자인지 다시 검사한다.
#[component]
fn TitleBar(broadcast: BroadcastResponse) -> Element {
    let session = use_session();
    let navigator = use_navigator();
    let mut title = use_signal(|| broadcast.title.clone());
    let mut editing = use_signal(|| None::<String>);
    let mut error = use_signal(|| None::<String>);
    let id = broadcast.id;
    let is_owner = broadcast.owner.as_ref().is_some_and(|owner| session().is(owner.id));
    let live = broadcast.status == BroadcastStatus::Live;

    let save = move |evt: FormEvent| async move {
        evt.prevent_default();
        let Some(text) = editing() else { return };
        match api::update_broadcast(id, &text).await {
            Ok(updated) => {
                title.set(updated.title);
                editing.set(None);
                error.set(None);
            }
            Err(e) => error.set(Some(e)),
        }
    };

    let delete = move |_| async move {
        if !confirm("방송을 삭제할까요? 영상과 댓글도 함께 지워집니다.") {
            return;
        }
        match api::delete_broadcast(id).await {
            Ok(()) => {
                navigator.push(Route::BroadcastList {});
            }
            Err(e) => error.set(Some(e)),
        }
    };

    rsx! {
        div { class: "title-row",
            if let Some(text) = editing() {
                form { class: "row", style: "flex: 1", onsubmit: save,
                    input { class: "inline-input", value: "{text}", maxlength: 100, required: true, oninput: move |e| editing.set(Some(e.value())) }
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
            } else {
                h1 { StatusBadge { status: broadcast.status } " {title}" }
                if is_owner {
                    div { class: "row",
                        button { class: "button secondary", onclick: move |_| editing.set(Some(title())), "제목 수정" }
                        // 방송 중인 라이브는 서버도 삭제를 거절한다(409). 송출 화면에서 먼저 종료한다.
                        if !live {
                            button { class: "button danger", onclick: delete, "삭제" }
                        }
                    }
                }
            }
        }
        if let Some(e) = error() {
            p { class: "error", "{e}" }
        }
    }
}

#[component]
pub fn BroadcastNew() -> Element {
    rsx! { RequireLogin { RegisterForm {} } }
}

#[component]
fn RegisterForm() -> Element {
    let mut title = use_signal(String::new);
    let mut submitting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let navigator = use_navigator();

    let onsubmit = move |evt: FormEvent| async move {
        evt.prevent_default();
        let Some(video) = selected_video() else {
            error.set(Some("영상 파일을 선택해주세요.".into()));
            return;
        };
        submitting.set(true);
        error.set(None);
        match api::register_broadcast(&title(), &video).await {
            Ok(created) => {
                navigator.push(Route::BroadcastDetail { id: created.id });
            }
            Err(e) => {
                error.set(Some(e));
                submitting.set(false);
            }
        }
    };

    rsx! {
        form { class: "form", onsubmit,
            h1 { "방송 등록" }
            label {
                "제목"
                input {
                    value: "{title}",
                    maxlength: 100,
                    required: true,
                    oninput: move |e| title.set(e.value()),
                }
            }
            label {
                "녹화 영상"
                input { id: VIDEO_INPUT_ID, r#type: "file", accept: "video/*", required: true }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
            button { class: "button", disabled: submitting(),
                if submitting() { "업로드 중..." } else { "등록" }
            }
        }
    }
}
