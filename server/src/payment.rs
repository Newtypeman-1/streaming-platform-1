//! 결제 서비스(토스페이먼츠) 클라이언트.
//!
//! 브라우저는 클라이언트 키로 결제창만 띄운다. 승인은 이 서버가 시크릿 키로 요청하고 결과를 확인한다.

#[derive(Clone)]
pub struct PaymentGateway {
    http: reqwest::Client,
    api_url: String,
    secret_key: String,
    /// 결제창을 띄울 때 브라우저가 쓰는 공개 키
    pub client_key: String,
    /// 결제 승인 실패를 재현하는 테스트 코드 (테스트 키에서만 동작)
    test_code: Option<String>,
}

impl PaymentGateway {
    /// `api_url`: 결제 API 주소 (https://api.tosspayments.com). 테스트 키(`test_`)면 실제 돈이 오가지 않는다.
    pub fn new(api_url: &str, secret_key: &str, client_key: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("HTTP 클라이언트 생성 실패"),
            api_url: api_url.trim_end_matches('/').to_string(),
            secret_key: secret_key.to_string(),
            client_key: client_key.to_string(),
            test_code: None,
        }
    }
}

/// 결제 서비스의 결제 객체 중 이 서버가 확인하는 필드
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub payment_key: String,
    pub order_id: String,
    /// READY, IN_PROGRESS, DONE, CANCELED, ABORTED, EXPIRED 등. 승인 완료는 DONE
    pub status: String,
    pub total_amount: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    /// 결제 서비스가 요청을 거절했다 (카드 거절, 잘못된 키 등)
    #[error("{message} ({code})")]
    Rejected { code: String, message: String },
    /// 결제 서비스에 연결하지 못했거나 알 수 없는 응답
    #[error("결제 서비스 요청 실패: {0}")]
    Failed(String),
}

impl From<reqwest::Error> for PaymentError {
    fn from(e: reqwest::Error) -> Self {
        PaymentError::Failed(e.to_string())
    }
}

#[derive(serde::Deserialize)]
struct TossError {
    code: String,
    message: String,
}

impl PaymentGateway {
    /// 결제 승인 요청에 테스트 코드 헤더를 붙여 실패를 재현한다. 라이브 키에서는 결제 서비스가 무시한다.
    pub fn with_test_code(mut self, code: &str) -> Self {
        self.test_code = Some(code.to_string());
        self
    }

    /// 결제 승인. 결제창 인증을 마친 결제를 실제로 승인한다.
    /// 주문번호를 멱등키로 실어, 응답이 끊겨 다시 요청해도 결제 서비스가 첫 응답을 돌려주게 한다.
    pub async fn confirm(&self, payment_key: &str, order_id: &str, amount: i64) -> Result<Payment, PaymentError> {
        let mut req = self
            .http
            .post(format!("{}/v1/payments/confirm", self.api_url))
            .header("Idempotency-Key", order_id)
            .json(&serde_json::json!({ "paymentKey": payment_key, "orderId": order_id, "amount": amount }));
        if let Some(code) = &self.test_code {
            req = req.header("TossPayments-Test-Code", code);
        }
        self.send(req).await
    }

    /// 결제 조회. 통지(웹훅) 내용은 서명이 없으므로 이 조회 결과로 확인한다.
    pub async fn find(&self, payment_key: &str) -> Result<Payment, PaymentError> {
        self.send(self.http.get(format!("{}/v1/payments/{payment_key}", self.api_url))).await
    }

    /// 시크릿 키 Basic 인증(`시크릿키:` 의 Base64)으로 보내고 결제 객체를 받는다.
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Payment, PaymentError> {
        let res = req.basic_auth(&self.secret_key, None::<&str>).send().await?;
        let status = res.status();
        if status.is_success() {
            return Ok(res.json().await?);
        }
        match res.json::<TossError>().await {
            Ok(e) if status.is_client_error() => Err(PaymentError::Rejected { code: e.code, message: e.message }),
            Ok(e) => Err(PaymentError::Failed(format!("{status} {}: {}", e.code, e.message))),
            Err(_) => Err(PaymentError::Failed(format!("{status}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 결제_API_주소가_https_여도_TLS_연결을_시도한다() {
        // 접속만 받는 로컬 서버: TLS 를 지원하면 핸드셰이크를 보내고, 지원하지 않으면 요청을 보내지 못한다
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handshake = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let wait = std::time::Duration::from_secs(2);
            let (mut socket, _) = tokio::time::timeout(wait, listener.accept()).await.ok()?.ok()?;
            let mut first = [0u8; 1];
            tokio::time::timeout(wait, socket.read_exact(&mut first)).await.ok()?.ok()?;
            Some(first[0])
        });
        let gateway = PaymentGateway::new(&format!("https://{addr}"), "test_sk", "test_ck");
        let send = gateway.http.get(format!("{}/v1/payments/x", gateway.api_url)).timeout(std::time::Duration::from_secs(2)).send();
        let (_, first) = tokio::join!(send, handshake);
        // TLS 핸드셰이크 레코드는 0x16 으로 시작한다
        assert_eq!(first.unwrap(), Some(0x16));
    }
}
