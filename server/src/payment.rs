//! 결제 서비스(토스페이먼츠) 설정.
//!
//! 브라우저는 클라이언트 키로 결제창만 띄운다.

#[derive(Clone)]
pub struct PaymentGateway {
    /// 결제창을 띄울 때 브라우저가 쓰는 공개 키
    pub client_key: String,
}

impl PaymentGateway {
    pub fn new(client_key: &str) -> Self {
        Self { client_key: client_key.to_string() }
    }
}
