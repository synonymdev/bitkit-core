use crate::DecodingError;

pub fn parse_amount_as_satoshis(amount: &str) -> Result<u64, DecodingError> {
    amount.parse::<f64>()
        .map_err(|_| DecodingError::InvalidAmount)
        .map(|btc| (btc * 100_000_000.0) as u64)
}