//! 나드리 데스크톱 앱: 왼쪽에 방송 목록, 오른쪽에 플레이어.
//! 녹화 방송은 `<video>` 로, 라이브는 웹뷰의 WebRTC 로 재생한다. 서버 호출은 모두 `desktop::api` 가 맡는다.

use desktop::api::{Api, server_url};
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use shared::{BroadcastResponse, BroadcastStatus};

const STYLE: &str = include_str!("../assets/style.css");
const LIVE_JS: &str = include_str!("../assets/live.js");
const LIVE_VIDEO_ID: &str = "live-video";

fn main() {
    let window = WindowBuilder::new().with_title("나드리").with_inner_size(LogicalSize::new(1100.0, 720.0));
    dioxus::LaunchBuilder::desktop().with_cfg(Config::new().with_window(window)).launch(App);
}

#[component]
fn App() -> Element {
    let api = use_context_provider(|| {
        let runtime = std::env::var("NADRI_SERVER_URL").ok();
        Api::new(&server_url(runtime.as_deref(), option_env!("NADRI_SERVER_URL")))
    });
    let mut selected = use_signal(|| None::<BroadcastResponse>);
    let mut list = use_resource({
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.broadcasts().await }
        }
    });

    rsx! {
        style { {STYLE} }
        header {
            h1 { "나드리" }
            span { class: "server", "{api.base()}" }
            button { onclick: move |_| list.restart(), "새로고침" }
        }
        main {
            aside {
                match &*list.read() {
                    None => rsx! { p { class: "note", "방송 목록을 불러오는 중..." } },
                    Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
                    Some(Ok(broadcasts)) if broadcasts.is_empty() => rsx! { p { class: "note", "아직 방송이 없습니다." } },
                    Some(Ok(broadcasts)) => rsx! {
                        ul {
                            for b in broadcasts.clone() {
                                li {
                                    key: "{b.id}",
                                    class: if selected().is_some_and(|s| s.id == b.id) { "selected" },
                                    onclick: move |_| selected.set(Some(b.clone())),
                                    StatusBadge { status: b.status }
                                    div { class: "title", "{b.title}" }
                                    div { class: "meta", "{owner(&b)} · {short_time(&b.created_at)}" }
                                }
                            }
                        }
                    },
                }
            }
            section { class: "player",
                match selected() {
                    None => rsx! { p { class: "note", "왼쪽에서 방송을 고르세요." } },
                    Some(b) => rsx! { Player { key: "{b.id}", broadcast: b } },
                }
            }
        }
    }
}

#[component]
fn StatusBadge(status: BroadcastStatus) -> Element {
    let (class, label) = match status {
        BroadcastStatus::Live => ("badge live", "LIVE"),
        BroadcastStatus::Recorded => ("badge", "녹화"),
        BroadcastStatus::Ended => ("badge", "종료"),
    };
    rsx! { span { class, "{label}" } }
}

#[component]
fn Player(broadcast: BroadcastResponse) -> Element {
    let api = use_context::<Api>();
    rsx! {
        h2 { "{broadcast.title}" }
        p { class: "meta", "{owner(&broadcast)} · {short_time(&broadcast.created_at)}" }
        if broadcast.status == BroadcastStatus::Live {
            LivePlayer { id: broadcast.id }
        } else if let Some(src) = api.video_url(&broadcast) {
            video { controls: true, autoplay: true, src }
        } else {
            p { class: "note", "끝난 라이브입니다. 다시보기 영상이 없습니다." }
        }
    }
}

/// 라이브 재생. 웹뷰(assets/live.js)가 만든 SDP offer 를 받아 앱이 서버로 보내고(WHEP), answer 를 웹뷰에 돌려준다.
#[component]
fn LivePlayer(id: i64) -> Element {
    let api = use_context::<Api>();
    let mut state = use_signal(|| "연결 중...".to_string());
    let mut error = use_signal(|| None::<String>);

    use_drop(|| {
        document::eval("window.nadriLive && window.nadriLive.stop();");
    });

    let start = move |_| {
        let api = api.clone();
        spawn(async move {
            let mut eval = document::eval(&format!("{LIVE_JS}\nawait window.nadriLive.play({LIVE_VIDEO_ID:?}, dioxus);"));
            while let Ok(message) = eval.recv::<String>().await {
                if let Some(offer) = message.strip_prefix("offer:") {
                    let reply = match api.watch(id, offer).await {
                        Ok(answer) => answer,
                        Err(e) => {
                            error.set(Some(e.clone()));
                            format!("error:{e}")
                        }
                    };
                    let _ = eval.send(reply);
                } else if let Some(s) = message.strip_prefix("state:") {
                    state.set(connection_label(s).to_string());
                } else if let Some(e) = message.strip_prefix("error:") {
                    error.set(Some(e.to_string()));
                }
            }
        });
    };

    rsx! {
        video { id: LIVE_VIDEO_ID, controls: true, autoplay: true, playsinline: true, onmounted: start }
        p { class: "note", "{state}" }
        if let Some(e) = error() {
            p { class: "error", "{e}" }
        }
    }
}

fn connection_label(state: &str) -> &'static str {
    match state {
        "connected" => "라이브 재생 중 (소리를 켜려면 음소거를 해제하세요)",
        "disconnected" => "연결이 잠시 끊겼습니다. 다시 연결하는 중...",
        "failed" | "closed" => "라이브 연결이 끊겼습니다. 방송이 끝났을 수 있습니다.",
        _ => "연결 중...",
    }
}

fn owner(b: &BroadcastResponse) -> String {
    b.owner.as_ref().map_or_else(|| "알 수 없음".to_string(), |o| o.username.clone())
}

/// `2026-11-22T10:00:00Z` → `2026-11-22 10:00`
fn short_time(rfc3339: &str) -> String {
    rfc3339.get(..16).unwrap_or(rfc3339).replace('T', " ")
}
