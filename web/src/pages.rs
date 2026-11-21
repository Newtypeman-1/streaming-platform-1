use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

use crate::Route;
use crate::api;

#[component]
pub fn BroadcastList() -> Element {
    let broadcasts = use_resource(api::fetch_broadcasts);

    match &*broadcasts.read() {
        None => rsx! { p { "불러오는 중..." } },
        Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
        Some(Ok(list)) if list.is_empty() => rsx! { p { "아직 등록된 방송이 없습니다. 첫 방송을 등록해 보세요." } },
        Some(Ok(list)) => rsx! {
            h1 { "녹화 방송" }
            ul { class: "grid",
                for b in list {
                    li { key: "{b.id}",
                        Link { to: Route::BroadcastDetail { id: b.id }, class: "card",
                            if let Some(url) = &b.video_url {
                                // #t=0.1 : 첫 프레임을 썸네일처럼 보여준다
                                video { src: "{url}#t=0.1", preload: "metadata", muted: true }
                            }
                            strong { "{b.title}" }
                            span { class: "meta", {api::local_time(&b.created_at)} }
                        }
                    }
                }
            }
        },
    }
}

#[component]
pub fn BroadcastDetail(id: i64) -> Element {
    // id 가 바뀌면(다른 방송으로 이동) 다시 불러온다
    let broadcast = use_resource(use_reactive!(|id| api::fetch_broadcast(id)));

    match &*broadcast.read() {
        None => rsx! { p { "불러오는 중..." } },
        Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
        Some(Ok(b)) => rsx! {
            article {
                if let Some(url) = &b.video_url {
                    video { class: "player", src: "{url}", controls: true, autoplay: true }
                }
                h1 { "{b.title}" }
                p { class: "meta", {api::local_time(&b.created_at)} " 등록" }
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

#[component]
pub fn BroadcastNew() -> Element {
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
