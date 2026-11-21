//! 후원 도메인 규칙.

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
}
