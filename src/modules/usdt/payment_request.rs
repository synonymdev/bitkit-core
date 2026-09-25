use super::{
    keys::parse_address,
    types::{CHAIN_ID, TOKEN},
    UsdtError, UsdtPaymentRequest,
};
use alloy_primitives::Address;

#[uniffi::export]
pub fn usdt_parse_payment_request(value: String) -> Result<UsdtPaymentRequest, UsdtError> {
    let (recipient, amount, chain_id) = parse_request(&value)?;
    Ok(UsdtPaymentRequest {
        recipient: recipient.to_checksum(None),
        amount,
        chain_id,
    })
}

fn parse_request(value: &str) -> Result<(Address, Option<u64>, Option<u64>), UsdtError> {
    let value = value.trim();
    if value.len() > 2048 {
        return Err(UsdtError::InvalidAddress);
    }
    let Some((scheme, uri)) = value.split_once(':') else {
        return Ok((parse_address(value)?, None, None));
    };
    if !scheme.eq_ignore_ascii_case("ethereum") {
        return Err(UsdtError::InvalidAddress);
    }
    let (target, query) = uri.split_once('?').unwrap_or((uri, ""));
    let (address, chain) = target
        .strip_prefix("pay-")
        .unwrap_or(target)
        .split_once('@')
        .ok_or(UsdtError::WrongNetwork)?;
    let Some(chain) = chain.strip_suffix("/transfer") else {
        if chain != CHAIN_ID.to_string() || !query.is_empty() {
            return Err(UsdtError::WrongNetwork);
        }
        return Ok((parse_address(address)?, None, Some(CHAIN_ID)));
    };
    if chain != CHAIN_ID.to_string() || parse_address(address)? != TOKEN {
        return Err(UsdtError::WrongNetwork);
    }
    let mut recipient = None;
    let mut amount = None;
    // ERC-681 uses a literal plus for positive numbers, not HTML form spaces.
    let query = query.replace('+', "%2B");
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "address" if recipient.is_none() => recipient = Some(parse_address(&value)?),
            "uint256" if amount.is_none() => {
                amount = Some(super::amount::parse_atomic_amount(&value)?)
            }
            _ => return Err(UsdtError::InvalidAddress),
        }
    }
    Ok((
        recipient.ok_or(UsdtError::InvalidAddress)?,
        amount,
        Some(CHAIN_ID),
    ))
}
