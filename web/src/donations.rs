//! 후원: 방송 상세 화면의 내역·합계와 후원하기, 결제창에서 돌아오는 성공·실패 화면.
//! 화면의 성공 표시는 서버가 결제 승인을 확인한 응답을 받은 뒤에만 보여 준다.

use dioxus::prelude::*;
use shared::{BroadcastResponse, ConfirmDonationRequest, DonationOrderResponse, DonationStatus, FailDonationRequest};

use crate::Route;
use crate::api;
use crate::auth::{Session, use_session};

const PRESETS: [i64; 4] = [1_000, 5_000, 10_000, 50_000];

#[component]
pub fn Donations(broadcast: BroadcastResponse) -> Element {
    let session = use_session();
    let id = broadcast.id;
    let summary = use_resource(move || api::fetch_donations(id));
    let mut amount = use_signal(|| 5_000i64);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let donate = move |evt: FormEvent| async move {
        evt.prevent_default();
        busy.set(true);
        error.set(None);
        let result = match api::order_donation(id, amount()).await {
            Ok(order) => open_payment(&order).await,
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            error.set(Some(e));
        }
        busy.set(false);
    };

    let Some(owner) = broadcast.owner.clone() else {
        // 로그인 기능 전에 만든 방송은 받을 사람이 없다
        return rsx! {};
    };

    rsx! {
        section { class: "donations",
            match &*summary.read() {
                Some(Ok(s)) => rsx! {
                    h2 { "후원 " span { class: "donation-total", {api::won(s.total)} } span { class: "meta", " · {s.count}건" } }
                    if !s.donations.is_empty() {
                        ul {
                            for d in s.donations.iter() {
                                li { key: "{d.id}", class: "donation",
                                    strong { "{d.donor.username}" }
                                    span { {api::won(d.amount)} }
                                    span { class: "meta", {api::local_time(&d.approved_at)} }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! { p { class: "error", "{e}" } },
                None => rsx! { h2 { "후원" } },
            }
            match session() {
                Session::User(user) if user.id != owner.id => rsx! {
                    form { class: "donation-form", onsubmit: donate,
                        div { class: "row",
                            for preset in PRESETS {
                                button {
                                    r#type: "button",
                                    class: if amount() == preset { "button" } else { "button secondary" },
                                    onclick: move |_| amount.set(preset),
                                    {api::won(preset)}
                                }
                            }
                        }
                        div { class: "row",
                            input {
                                class: "inline-input",
                                r#type: "number",
                                min: 1000,
                                max: 1000000,
                                step: 1000,
                                value: "{amount}",
                                oninput: move |e| amount.set(e.value().parse().unwrap_or(0)),
                            }
                            button { class: "button", disabled: busy(), "{owner.username} 님에게 후원하기" }
                        }
                        p { class: "meta", "1,000원 ~ 1,000,000원 · 테스트 결제라 실제 돈이 빠져나가지 않습니다." }
                        if let Some(e) = error() {
                            p { class: "error", "{e}" }
                        }
                    }
                },
                Session::Guest => rsx! {
                    p { class: "meta",
                        Link { to: Route::Login {}, class: "link", "로그인" }
                        "하면 후원할 수 있습니다."
                    }
                },
                _ => rsx! {},
            }
        }
    }
}

/// 결제창을 띄운다. 인증에 성공하면 결제창이 성공 화면으로 이동하므로 돌아오지 않는다.
/// 결제창을 닫거나 실패하면 서버에 기록하고 그 사유를 돌려준다.
async fn open_payment(order: &DonationOrderResponse) -> Result<(), String> {
    let code = format!(
        "return await window.payment.request({}, {});",
        serde_json::to_string(&order.client_key).unwrap(),
        serde_json::to_string(order).unwrap()
    );
    let failure = match document::eval(&code).await {
        Ok(serde_json::Value::Null) => return Ok(()),
        Ok(v) => FailDonationRequest {
            code: v["code"].as_str().unwrap_or("UNKNOWN").to_string(),
            message: v["message"].as_str().map(str::to_string),
        },
        Err(e) => FailDonationRequest { code: "UNKNOWN".into(), message: Some(format!("{e:?}")) },
    };
    let recorded = api::fail_donation(&order.order_id, &failure).await?;
    Err(failure_text(&recorded))
}

fn failure_text(order: &DonationOrderResponse) -> String {
    match order.status {
        DonationStatus::Canceled => "결제를 취소했습니다.".to_string(),
        _ => format!("결제에 실패했습니다. {}", order.failure.as_deref().unwrap_or("")),
    }
}

/// 쿼리 문자열(`a=1&b=2`)에서 값 하나
fn query_param(query: &str, name: &str) -> Option<String> {
    web_sys::UrlSearchParams::new_with_str(query).ok()?.get(name)
}

fn back_link(order: &DonationOrderResponse) -> Element {
    match order.broadcast_id {
        Some(id) => {
            rsx! { Link { to: Route::BroadcastDetail { id }, class: "button", "방송으로 돌아가기" } }
        }
        None => rsx! { Link { to: Route::BroadcastList {}, class: "button", "방송 목록" } },
    }
}

/// 결제창 successUrl. 결제창이 붙여 준 paymentKey·orderId·amount 를 서버에 보내 승인을 확인받는다.
#[component]
pub fn DonationSuccess(query: String) -> Element {
    let result = use_resource(move || {
        let query = query.clone();
        async move {
            let query_param = |name| query_param(&query, name);
            let req = ConfirmDonationRequest {
                payment_key: query_param("paymentKey").ok_or("결제 정보가 없습니다.")?,
                order_id: query_param("orderId").ok_or("결제 정보가 없습니다.")?,
                amount: query_param("amount").and_then(|a| a.parse().ok()).ok_or("결제 정보가 없습니다.")?,
            };
            api::confirm_donation(&req).await
        }
    });

    rsx! {
        section { class: "donation-result",
            match &*result.read() {
                None => rsx! { p { "결제 승인을 확인하는 중..." } },
                Some(Ok(order)) => rsx! {
                    h1 { "후원 완료" }
                    p { "{order.recipient.username} 님에게 " strong { {api::won(order.amount)} } "을 후원했습니다." }
                    {back_link(order)}
                },
                Some(Err(e)) => rsx! {
                    h1 { "후원을 확정하지 못했습니다" }
                    p { class: "error", "{e}" }
                    Link { to: Route::BroadcastList {}, class: "button secondary", "방송 목록" }
                },
            }
        }
    }
}

/// 결제창 failUrl. 결제창이 붙여 준 code·message 를 서버에 기록한다.
#[component]
pub fn DonationFail(query: String) -> Element {
    let result = use_resource(move || {
        let query = query.clone();
        async move {
            let query_param = |name| query_param(&query, name);
            let order_id = query_param("order").ok_or("주문 정보가 없습니다.")?;
            let req = FailDonationRequest {
                code: query_param("code").unwrap_or_else(|| "UNKNOWN".into()),
                message: query_param("message"),
            };
            api::fail_donation(&order_id, &req).await
        }
    });

    rsx! {
        section { class: "donation-result",
            match &*result.read() {
                None => rsx! { p { "처리 중..." } },
                Some(Ok(order)) => rsx! {
                    h1 { if order.status == DonationStatus::Canceled { "후원을 취소했습니다" } else { "후원에 실패했습니다" } }
                    p { class: "meta", {failure_text(order)} }
                    {back_link(order)}
                },
                Some(Err(e)) => rsx! {
                    h1 { "후원에 실패했습니다" }
                    p { class: "error", "{e}" }
                    Link { to: Route::BroadcastList {}, class: "button secondary", "방송 목록" }
                },
            }
        }
    }
}
