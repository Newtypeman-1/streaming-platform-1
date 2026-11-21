//! 후원 도메인 규칙.

use crate::payment::Payment;

pub const MIN_AMOUNT: i64 = 1_000;
pub const MAX_AMOUNT: i64 = 1_000_000;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DonationError {
    #[error("후원 금액은 {MIN_AMOUNT}원 이상 {MAX_AMOUNT}원 이하여야 합니다.")]
    InvalidAmount,
    #[error("방송자가 없는 방송에는 후원할 수 없습니다.")]
    NoRecipient,
    #[error("자기 방송에는 후원할 수 없습니다.")]
    SelfDonation,
    #[error("결제 금액이 주문과 다릅니다.")]
    AmountMismatch,
    #[error("후원한 본인만 할 수 있습니다.")]
    NotDonor,
    #[error("결제가 승인되지 않았습니다: {0}")]
    Rejected(String),
    #[error("이미 확정된 후원입니다.")]
    AlreadyDone,
    #[error("이미 실패했거나 취소된 주문입니다. 다시 후원해주세요.")]
    Ended,
    #[error("결제 서비스 응답이 주문과 달라 후원을 확정하지 않았습니다.")]
    PaymentMismatch,
    #[error("후원 받을 방송이 없어져 결제를 승인하지 않았습니다.")]
    RecipientGone,
}

/// 결제 서비스가 승인했다고 알려 준 결제가 이 주문의 것인지: 같은 주문번호, 같은 금액, 승인 완료(DONE).
pub fn verify_payment(order_id: &str, amount: i64, payment: &Payment) -> Result<(), DonationError> {
    if payment.order_id != order_id || payment.total_amount != amount || payment.status != "DONE" {
        return Err(DonationError::PaymentMismatch);
    }
    Ok(())
}

/// 후원은 방송 소유자가 받는다.
pub fn recipient_of(owner_id: Option<i64>, donor_id: i64) -> Result<i64, DonationError> {
    match owner_id {
        None => Err(DonationError::NoRecipient),
        Some(owner) if owner == donor_id => Err(DonationError::SelfDonation),
        Some(owner) => Ok(owner),
    }
}

pub fn validate_amount(amount: i64) -> Result<i64, DonationError> {
    if !(MIN_AMOUNT..=MAX_AMOUNT).contains(&amount) {
        return Err(DonationError::InvalidAmount);
    }
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 금액은_1000원부터_100만원까지() {
        assert_eq!(validate_amount(1_000), Ok(1_000));
        assert_eq!(validate_amount(1_000_000), Ok(1_000_000));
        assert_eq!(validate_amount(999), Err(DonationError::InvalidAmount));
        assert_eq!(validate_amount(1_000_001), Err(DonationError::InvalidAmount));
        assert_eq!(validate_amount(-5_000), Err(DonationError::InvalidAmount));
    }

    #[test]
    fn 받는_사람은_방송_소유자이고_자기_자신은_안된다() {
        assert_eq!(recipient_of(Some(7), 1), Ok(7));
        assert_eq!(recipient_of(None, 1), Err(DonationError::NoRecipient));
        assert_eq!(recipient_of(Some(1), 1), Err(DonationError::SelfDonation));
    }

    fn paid(order_id: &str, amount: i64, status: &str) -> Payment {
        Payment { payment_key: "pay_1".into(), order_id: order_id.into(), status: status.into(), total_amount: amount }
    }

    #[test]
    fn 승인된_결제는_주문번호_금액이_같고_완료_상태여야_한다() {
        assert_eq!(verify_payment("order-1", 5000, &paid("order-1", 5000, "DONE")), Ok(()));
        assert_eq!(verify_payment("order-1", 5000, &paid("order-2", 5000, "DONE")), Err(DonationError::PaymentMismatch));
        assert_eq!(verify_payment("order-1", 5000, &paid("order-1", 100, "DONE")), Err(DonationError::PaymentMismatch));
        assert_eq!(verify_payment("order-1", 5000, &paid("order-1", 5000, "CANCELED")), Err(DonationError::PaymentMismatch));
    }
}
