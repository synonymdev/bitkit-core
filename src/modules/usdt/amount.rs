use super::errors::UsdtError;
use alloy_primitives::U256;

#[uniffi::export]
pub fn usdt_parse_amount(value: String) -> Result<u64, UsdtError> {
    let mut parts = value.trim().split('.');
    let whole = parts.next().ok_or(UsdtError::InvalidAmount)?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(UsdtError::InvalidAmount);
    }
    let whole: u64 = whole.parse().map_err(|_| UsdtError::InvalidAmount)?;
    let fraction: u64 = format!("{fraction:0<6}")
        .parse()
        .map_err(|_| UsdtError::InvalidAmount)?;
    whole
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(fraction))
        .ok_or(UsdtError::InvalidAmount)
}

#[uniffi::export]
pub fn usdt_format_amount(amount: u64) -> String {
    let value = format!("{}.{:06}", amount / 1_000_000, amount % 1_000_000);
    value
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

pub(super) fn with_margin(value: U256, percent: u8) -> Result<U256, UsdtError> {
    let hundred = U256::from(100);
    let percent = U256::from(percent);
    let margin = (value / hundred)
        .checked_mul(percent)
        .and_then(|margin| margin.checked_add(value % hundred * percent / hundred))
        .ok_or(UsdtError::InvalidResponse)?;
    value
        .checked_add(margin)
        .and_then(|value| value.checked_add(U256::from(1)))
        .ok_or(UsdtError::InvalidResponse)
}

pub(super) fn token_amount(value: U256) -> Result<u64, UsdtError> {
    value.try_into().map_err(|_| UsdtError::InvalidAmount)
}

// ERC-681 token amounts are integers in atomic units, optionally in scientific notation.
pub(super) fn parse_atomic_amount(value: &str) -> Result<u64, UsdtError> {
    let (mantissa, exponent) = match value.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<u32>()
                .map_err(|_| UsdtError::InvalidAmount)?,
        ),
        None => (value, 0),
    };
    let mantissa = mantissa.strip_prefix('+').unwrap_or(mantissa);
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(UsdtError::InvalidAmount);
    }
    let fraction = fraction.trim_end_matches('0');
    let power = exponent
        .checked_sub(fraction.len() as u32)
        .ok_or(UsdtError::InvalidAmount)?;
    let digits = format!("0{whole}{fraction}");
    let amount = digits
        .parse::<u64>()
        .map_err(|_| UsdtError::InvalidAmount)?;
    if amount == 0 {
        return Ok(0);
    }
    amount
        .checked_mul(10u64.checked_pow(power).ok_or(UsdtError::InvalidAmount)?)
        .ok_or(UsdtError::InvalidAmount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_preserve_rounding_without_intermediate_overflow() {
        for percent in [5, 10, 20] {
            for value in [
                U256::ZERO,
                U256::from(19),
                U256::from(20),
                U256::MAX / U256::from(2),
            ] {
                let expected = value + value / U256::from(100 / percent) + U256::from(1);
                assert_eq!(with_margin(value, percent).unwrap(), expected);
            }
            assert!(matches!(
                with_margin(U256::MAX, percent),
                Err(UsdtError::InvalidResponse)
            ));
        }
    }
}
