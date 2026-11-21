//! 라이브 송출 화면과 라이브 플레이어. WebRTC 는 assets/live.js 가 맡고, 여기서는 상태와 화면만 다룬다.

use dioxus::prelude::*;
use gloo_timers::future::TimeoutFuture;
use shared::{BroadcastResponse, BroadcastStatus};

use crate::Route;
use crate::api;
use crate::auth::RequireLogin;

/// JS 문자열 리터럴로 안전하게 넣는다
fn js_str(s: &str) -> String {
    serde_json::to_string(s).expect("문자열 직렬화는 실패하지 않는다")
}

/// live.js 함수를 부르고, JS 에서 던진 오류는 메시지로 돌려준다.
async fn call_js(code: &str) -> Result<(), String> {
    let wrapped = format!("try {{ {code}; return null; }} catch (e) {{ return e.message || String(e); }}");
    match document::eval(&wrapped).await {
        Ok(serde_json::Value::Null) => Ok(()),
        Ok(serde_json::Value::String(message)) => Err(message),
        Ok(other) => Err(other.to_string()),
        Err(e) => Err(format!("{e:?}")),
    }
}

/// 연결 상태 알림을 받는 eval 을 띄운다. 콜백 `send` 로 'connected' | 'unstable' | 'disconnected',
/// 연결 실패 시 'error:<메시지>' 가 온다.
fn listen_js(code: String, mut on_message: impl FnMut(String) + 'static) {
    spawn(async move {
        let mut eval = document::eval(&format!(
            "const send = (s) => dioxus.send(s); try {{ {code}; }} catch (e) {{ send('error:' + (e.message || e)); }}"
        ));
        while let Ok(message) = eval.recv::<String>().await {
            on_message(message);
        }
    });
}

const STUDIO_VIDEO_ID: &str = "studio-video";

#[derive(Clone, PartialEq)]
enum Studio {
    /// 카메라 꺼짐
    Off,
    Preview,
    Starting,
    Live { id: i64, connected: bool, unstable: bool },
    Ending,
    Ended(BroadcastResponse),
}

#[component]
pub fn LiveStudio() -> Element {
    rsx! { RequireLogin { StudioScreen {} } }
}

#[component]
fn StudioScreen() -> Element {
    let mut studio = use_signal(|| Studio::Off);
    let mut title = use_signal(String::new);
    let mut front = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);

    // 화면을 떠나면 카메라를 끄고, 방송 중이었다면 종료 요청도 보낸다
    use_drop(|| {
        document::eval("window.live.stopPublish(true); window.live.stopPreview();");
    });

    let turn_on_camera = move |use_front: bool| async move {
        error.set(None);
        let facing = if use_front { "user" } else { "environment" };
        match call_js(&format!("await window.live.startPreview({}, {})", js_str(STUDIO_VIDEO_ID), js_str(facing))).await {
            Ok(()) => {
                front.set(use_front);
                studio.set(Studio::Preview);
            }
            Err(e) => error.set(Some(e)),
        }
    };

    let end = move |id: i64, reason: Option<String>| async move {
        studio.set(Studio::Ending);
        let _ = call_js("window.live.stopPublish(false)").await;
        match api::end_live(id).await {
            Ok(ended) => studio.set(Studio::Ended(ended)),
            // 종료 요청이 실패해도 송출이 끊겼으니 서버가 곧 종료 처리한다
            Err(e) => error.set(Some(e)),
        }
        if reason.is_some() {
            error.set(reason);
        }
    };

    let start = move |evt: FormEvent| async move {
        evt.prevent_default();
        error.set(None);
        studio.set(Studio::Starting);
        let started = match api::start_live(&title()).await {
            Ok(started) => started,
            Err(e) => {
                error.set(Some(e));
                studio.set(Studio::Preview);
                return;
            }
        };
        let id = started.broadcast.id;
        let key = started.stream_key;
        studio.set(Studio::Live { id, connected: false, unstable: false });

        let code = format!(
            "await window.live.publish({}, {}, {}, send)",
            js_str(&shared::whip_url(id)),
            js_str(&shared::end_url(id)),
            js_str(&key)
        );
        listen_js(code, move |message| {
            let Studio::Live { id, .. } = studio() else { return };
            match message.as_str() {
                "connected" => studio.set(Studio::Live { id, connected: true, unstable: false }),
                "unstable" => studio.set(Studio::Live { id, connected: true, unstable: true }),
                "disconnected" => {
                    spawn(end(id, Some("송출 연결이 끊겨 방송을 종료했습니다.".into())));
                }
                failed => {
                    // 송출 연결에 실패: 만든 방송은 지우지 않고 종료로 남긴다. 미리보기는 그대로라 다시 시작할 수 있다.
                    let reason = failed.strip_prefix("error:").unwrap_or(failed).to_string();
                    spawn(async move {
                        let _ = api::end_live(id).await;
                        error.set(Some(reason));
                        studio.set(Studio::Preview);
                    });
                }
            }
        });
    };

    let state = studio();
    let camera_on = matches!(state, Studio::Preview | Studio::Starting | Studio::Live { .. } | Studio::Ending);

    rsx! {
        section { class: "studio",
            h1 { "라이브 방송" }
            div { class: "studio-video",
                // 전면 카메라는 거울처럼 보여야 자연스럽다 (송출 영상은 뒤집지 않는다)
                video {
                    id: STUDIO_VIDEO_ID,
                    class: if front() { "mirror" },
                    hidden: !camera_on,
                    autoplay: true,
                    playsinline: true,
                    muted: true,
                }
                if let Studio::Live { connected, unstable, .. } = &state {
                    span { class: "badge live", "LIVE" }
                    span { class: "studio-state",
                        if !connected { "송출 연결 중..." } else if *unstable { "연결 불안정" } else { "방송 중" }
                    }
                }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
            match state {
                Studio::Off => rsx! {
                    p { class: "meta", "카메라와 마이크 권한을 허용해야 방송할 수 있습니다." }
                    button { class: "button", onclick: move |_| turn_on_camera(true), "카메라 켜기" }
                },
                Studio::Preview | Studio::Starting => rsx! {
                    form { class: "form", onsubmit: start,
                        label {
                            "방송 제목"
                            input {
                                value: "{title}",
                                maxlength: 100,
                                required: true,
                                oninput: move |e| title.set(e.value()),
                            }
                        }
                        div { class: "row",
                            button {
                                class: "button secondary",
                                r#type: "button",
                                disabled: studio() == Studio::Starting,
                                onclick: move |_| turn_on_camera(!front()),
                                if front() { "후면 카메라로" } else { "전면 카메라로" }
                            }
                            button { class: "button", disabled: studio() == Studio::Starting,
                                if studio() == Studio::Starting { "시작하는 중..." } else { "방송 시작" }
                            }
                        }
                    }
                },
                Studio::Live { id, .. } => rsx! {
                    div { class: "row",
                        button {
                            class: "button danger",
                            onclick: move |_| end(id, None),
                            "방송 종료"
                        }
                        Link { to: Route::BroadcastDetail { id }, new_tab: true, class: "meta", "시청 화면 열기 ↗" }
                    }
                },
                Studio::Ending => rsx! { p { "종료하는 중..." } },
                Studio::Ended(b) => rsx! {
                    p { "방송이 종료되었습니다." }
                    p { class: "meta", "{b.title} · " {b.ended_at.as_deref().map(api::local_time)} " 종료" }
                    div { class: "row",
                        button {
                            class: "button",
                            onclick: move |_| {
                                error.set(None);
                                studio.set(Studio::Off);
                            },
                            "새 방송 준비"
                        }
                        Link { to: Route::BroadcastList {}, class: "button secondary", "목록으로" }
                    }
                },
            }
        }
    }
}

#[derive(Clone, PartialEq)]
enum Playback {
    Idle,
    Connecting,
    Playing,
    Waiting(String),
}

const PLAYER_VIDEO_ID: &str = "live-player";
/// 방송 상태 확인 · 끊긴 영상 재연결 주기
const POLL_MS: u32 = 3000;

/// 라이브 시청. 방송 상태를 주기적으로 확인해 끝나면 `on_end` 로 알리고, 영상이 끊기면 다시 연결한다.
#[component]
pub fn LivePlayer(id: i64, on_end: EventHandler<BroadcastResponse>) -> Element {
    let mut playback = use_signal(|| Playback::Idle);

    use_drop(|| {
        document::eval("window.live.stopPlay()");
    });

    use_future(move || async move {
        loop {
            if let Ok(latest) = api::fetch_broadcast(id).await
                && latest.status != BroadcastStatus::Live
            {
                let _ = call_js("window.live.stopPlay()").await;
                on_end.call(latest);
                return;
            }
            if matches!(playback(), Playback::Idle | Playback::Waiting(_)) {
                playback.set(Playback::Connecting);
                let code = format!("await window.live.play({}, {}, send)", js_str(PLAYER_VIDEO_ID), js_str(&shared::whep_url(id)));
                listen_js(code, move |message| match message.as_str() {
                    "connected" => playback.set(Playback::Playing),
                    "unstable" => playback.set(Playback::Waiting("연결이 불안정합니다...".into())),
                    "disconnected" => playback.set(Playback::Waiting("영상 연결이 끊겼습니다. 다시 연결하는 중...".into())),
                    failed => {
                        let reason = failed.strip_prefix("error:").unwrap_or(failed);
                        playback.set(Playback::Waiting(format!("{reason} 잠시 후 다시 연결합니다.")));
                    }
                });
            }
            TimeoutFuture::new(POLL_MS).await;
        }
    });

    rsx! {
        div { class: "player-wrap",
            video { id: PLAYER_VIDEO_ID, class: "player", autoplay: true, playsinline: true, muted: true, controls: true }
            match playback() {
                Playback::Playing => rsx! {},
                Playback::Idle | Playback::Connecting => rsx! { p { class: "overlay", "연결 중..." } },
                Playback::Waiting(message) => rsx! { p { class: "overlay", "{message}" } },
            }
        }
        p { class: "meta", "소리가 들리지 않으면 플레이어의 음소거를 해제하세요." }
    }
}
