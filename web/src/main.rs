use dioxus::prelude::*;

mod api;
mod auth;
mod comments;
mod donations;
mod live;
mod pages;

use auth::{AccountMenu, Login, Signup};
use donations::{DonationFail, DonationSuccess};
use live::LiveStudio;
use pages::{BroadcastDetail, BroadcastList, BroadcastNew};

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(Layout)]
    #[route("/")]
    BroadcastList {},
    #[route("/new")]
    BroadcastNew {},
    #[route("/live")]
    LiveStudio {},
    #[route("/broadcasts/:id")]
    BroadcastDetail { id: i64 },
    #[route("/login")]
    Login {},
    #[route("/signup")]
    Signup {},
    // 결제창이 돌아오는 주소. 결제창이 붙여 준 쿼리를 통째로 받는다.
    #[route("/donations/success?:..query")]
    DonationSuccess { query: String },
    #[route("/donations/fail?:..query")]
    DonationFail { query: String },
}

const STYLE: Asset = asset!("/assets/main.css");
const LIVE_JS: Asset = asset!("/assets/live.js");
const PAYMENT_JS: Asset = asset!("/assets/payment.js");
/// 토스페이먼츠 결제창 SDK v2
const TOSS_SDK: &str = "https://js.tosspayments.com/v2/standard";

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    auth::provide_session();
    rsx! {
        document::Stylesheet { href: STYLE }
        document::Script { src: LIVE_JS }
        document::Script { src: TOSS_SDK }
        document::Script { src: PAYMENT_JS }
        Router::<Route> {}
    }
}

#[component]
fn Layout() -> Element {
    rsx! {
        header { class: "header",
            Link { to: Route::BroadcastList {}, class: "logo", "streaming-platform" }
            nav { class: "row",
                Link { to: Route::BroadcastNew {}, class: "button secondary", "녹화 등록" }
                Link { to: Route::LiveStudio {}, class: "button", "라이브 시작" }
                AccountMenu {}
            }
        }
        main { class: "main", Outlet::<Route> {} }
    }
}
