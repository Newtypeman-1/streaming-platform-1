use dioxus::prelude::*;

mod api;
mod live;
mod pages;

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
}

const STYLE: Asset = asset!("/assets/main.css");
const LIVE_JS: Asset = asset!("/assets/live.js");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: STYLE }
        document::Script { src: LIVE_JS }
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
            }
        }
        main { class: "main", Outlet::<Route> {} }
    }
}
