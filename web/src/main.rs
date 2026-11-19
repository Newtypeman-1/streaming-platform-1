use dioxus::prelude::*;

mod api;
mod pages;

use pages::{BroadcastDetail, BroadcastList, BroadcastNew};

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(Layout)]
    #[route("/")]
    BroadcastList {},
    #[route("/new")]
    BroadcastNew {},
    #[route("/broadcasts/:id")]
    BroadcastDetail { id: i64 },
}

const STYLE: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: STYLE }
        Router::<Route> {}
    }
}

#[component]
fn Layout() -> Element {
    rsx! {
        header { class: "header",
            Link { to: Route::BroadcastList {}, class: "logo", "streaming-platform" }
            Link { to: Route::BroadcastNew {}, class: "button", "방송 등록" }
        }
        main { class: "main", Outlet::<Route> {} }
    }
}
