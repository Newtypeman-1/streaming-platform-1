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
    // 서버에 만든 주문. 있으면 금액 선택 대신 주문서(결제수단·약관)를 보여 준다
    let mut checkout = use_signal(|| None::<DonationOrderResponse>);

    // 주문서 영역이 그려진 뒤 그 안에 결제 UI 를 그린다
    use_effect(move || {
        let Some(order) = checkout() else { return };
        spawn(async move {
            if let Some(failure) = call_payment(&format!("render({}, {})", js(&order.client_key), js(&order))).await {
                end_checkout(&order, failure, checkout, error).await;
            }
        });
    });

    let start = move |evt: FormEvent| async move {
        evt.prevent_default();
        busy.set(true);
        error.set(None);
        match api::order_donation(id, amount()).await {
            Ok(order) => checkout.set(Some(order)),
            Err(e) => error.set(Some(e)),
        }
        busy.set(false);
    };

    // 결제하기: 고른 결제수단의 결제창을 연다. 인증에 성공하면 성공 화면으로 이동한다
    let pay = move |_| async move {
        let Some(order) = checkout() else { return };
        busy.set(true);
        error.set(None);
        match document::eval(&format!("return await window.payment.request({});", js(&order))).await {
            Ok(serde_json::Value::Null) => {}
            Ok(v) if v["notice"].is_string() => error.set(v["notice"].as_str().map(str::to_string)),
            other => end_checkout(&order, failure_of(other), checkout, error).await,
        }
        busy.set(false);
    };

    // 다른 금액을 고르려고 주문서를 닫으면 이 주문은 취소로 기록한다
    let close = move |_| async move {
        let Some(order) = checkout() else { return };
        let failure = FailDonationRequest { code: "USER_CANCEL".into(), message: Some("주문서를 닫았습니다.".into()) };
        end_checkout(&order, failure, checkout, error).await;
        error.set(None);
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
            match (session(), checkout()) {
                (Session::User(_), Some(order)) => rsx! {
                    div { class: "donation-checkout",
                        p { "{order.recipient.username} 님에게 " strong { {api::won(order.amount)} } " 후원" }
                        div { id: "payment-method" }
                        div { id: "agreement" }
                        div { class: "row",
                            button { class: "button", disabled: busy(), onclick: pay, "결제하기" }
                            button { class: "button secondary", disabled: busy(), onclick: close, "다른 금액 선택" }
                        }
                        p { class: "meta", "테스트 결제라 실제 돈이 빠져나가지 않습니다." }
                        if let Some(e) = error() {
                            p { class: "error", "{e}" }
                        }
                    }
                },
                (Session::User(user), None) if user.id != owner.id => rsx! {
                    form { class: "donation-form", onsubmit: start,
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
                (Session::Guest, _) => rsx! {
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

fn js<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

/// `window.payment` 의 함수를 부른다. 실패하면 그 코드·메시지
async fn call_payment(call: &str) -> Option<FailDonationRequest> {
    match document::eval(&format!("return await window.payment.{call};")).await {
        Ok(serde_json::Value::Null) => None,
        other => Some(failure_of(other)),
    }
}

fn failure_of(result: Result<serde_json::Value, document::EvalError>) -> FailDonationRequest {
    match result {
        Ok(v) => FailDonationRequest {
            code: v["code"].as_str().unwrap_or("UNKNOWN").to_string(),
            message: v["message"].as_str().map(str::to_string),
        },
        Err(e) => FailDonationRequest { code: "UNKNOWN".into(), message: Some(format!("{e:?}")) },
    }
}

/// 주문서를 닫는다. 서버에 취소·실패를 기록하고 그 사유를 보여 준다.
async fn end_checkout(
    order: &DonationOrderResponse,
    failure: FailDonationRequest,
    mut checkout: Signal<Option<DonationOrderResponse>>,
    mut error: Signal<Option<String>>,
) {
    document::eval("window.payment.clear()");
    checkout.set(None);
    error.set(Some(match api::fail_donation(&order.order_id, &failure).await {
        Ok(recorded) => failure_text(&recorded),
        Err(e) => e,
    }));
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
