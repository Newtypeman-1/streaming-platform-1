//! 로그인 상태, 로그인·회원가입 화면.
//! 화면은 로그인 여부에 따라 버튼을 보이거나 숨길 뿐이고, 실제 권한 검사는 서버가 한다.

use dioxus::prelude::*;
use shared::UserResponse;

use crate::Route;
use crate::api;

#[derive(Clone, PartialEq)]
pub enum Session {
    /// 처음 화면을 열고 서버에 로그인 상태를 묻는 중
    Checking,
    Guest,
    User(UserResponse),
}

impl Session {
    pub fn user(&self) -> Option<&UserResponse> {
        match self {
            Session::User(user) => Some(user),
            _ => None,
        }
    }

    pub fn is(&self, user_id: i64) -> bool {
        self.user().is_some_and(|u| u.id == user_id)
    }
}

pub fn use_session() -> Signal<Session> {
    use_context()
}

/// 앱 시작 시 한 번 로그인 상태를 확인해 모든 화면이 함께 쓴다.
pub fn provide_session() {
    let mut session = use_context_provider(|| Signal::new(Session::Checking));
    use_future(move || async move {
        session.set(match api::me().await {
            Ok(Some(user)) => Session::User(user),
            _ => Session::Guest,
        });
    });
}

/// 로그인해야 쓸 수 있는 화면을 감싼다.
#[component]
pub fn RequireLogin(children: Element) -> Element {
    match use_session()() {
        Session::Checking => rsx! { p { "로그인 상태를 확인하는 중..." } },
        Session::Guest => rsx! {
            div { class: "form",
                p { "로그인이 필요합니다." }
                div { class: "row",
                    Link { to: Route::Login {}, class: "button", "로그인" }
                    Link { to: Route::Signup {}, class: "button secondary", "회원가입" }
                }
            }
        },
        Session::User(_) => children,
    }
}

/// 헤더의 로그인 영역
#[component]
pub fn AccountMenu() -> Element {
    let mut session = use_session();
    let navigator = use_navigator();

    match session() {
        Session::Checking => rsx! {},
        Session::Guest => rsx! {
            Link { to: Route::Login {}, class: "button secondary", "로그인" }
        },
        Session::User(user) => rsx! {
            span { class: "meta", "{user.username}" }
            button {
                class: "button secondary",
                onclick: move |_| async move {
                    let _ = api::logout().await;
                    session.set(Session::Guest);
                    navigator.push(Route::BroadcastList {});
                },
                "로그아웃"
            }
        },
    }
}

#[component]
pub fn Login() -> Element {
    rsx! { AuthForm { signup: false } }
}

#[component]
pub fn Signup() -> Element {
    rsx! { AuthForm { signup: true } }
}

#[component]
fn AuthForm(signup: bool) -> Element {
    let mut session = use_session();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut submitting = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let navigator = use_navigator();

    let onsubmit = move |evt: FormEvent| async move {
        evt.prevent_default();
        submitting.set(true);
        error.set(None);
        let result =
            if signup { api::signup(&username(), &password()).await } else { api::login(&username(), &password()).await };
        match result {
            Ok(user) => {
                session.set(Session::User(user));
                navigator.push(Route::BroadcastList {});
            }
            Err(e) => {
                error.set(Some(e));
                submitting.set(false);
            }
        }
    };

    rsx! {
        form { class: "form", onsubmit,
            h1 { if signup { "회원가입" } else { "로그인" } }
            label {
                "아이디"
                input {
                    value: "{username}",
                    autocomplete: "username",
                    required: true,
                    oninput: move |e| username.set(e.value()),
                }
                if signup {
                    span { class: "meta", "영문 소문자·숫자·밑줄(_) 3~20자" }
                }
            }
            label {
                "비밀번호"
                input {
                    r#type: "password",
                    value: "{password}",
                    autocomplete: if signup { "new-password" } else { "current-password" },
                    required: true,
                    oninput: move |e| password.set(e.value()),
                }
                if signup {
                    span { class: "meta", "8~64자" }
                }
            }
            if let Some(e) = error() {
                p { class: "error", "{e}" }
            }
            button { class: "button", disabled: submitting(),
                if signup { "가입하기" } else { "로그인" }
            }
            if signup {
                p { class: "meta", "이미 계정이 있나요? " Link { to: Route::Login {}, class: "link", "로그인" } }
            } else {
                p { class: "meta", "계정이 없나요? " Link { to: Route::Signup {}, class: "link", "회원가입" } }
            }
        }
    }
}
