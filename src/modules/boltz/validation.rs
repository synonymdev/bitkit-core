use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::types::BoltzNetwork;
use boltz_client::swaps::bitcoin::BtcSwapScript;
use boltz_client::swaps::boltz::{CreateReverseResponse, CreateSubmarineResponse};
use boltz_client::util::secrets::Preimage;
use boltz_client::Bolt11Invoice;
use std::str::FromStr;

/// Validate an optional fee rate before any fund-moving work begins.
///
/// The rate flows into `Fee::Relative(f64)` unchecked otherwise: negative
/// values and `NaN` silently become a zero fee and infinity saturates, so an
/// invalid configuration would only surface after a claim or refund is
/// already being constructed.
pub(crate) fn validate_fee_rate(fee_rate_sat_per_vb: Option<f64>) -> Result<(), BoltzError> {
    if let Some(rate) = fee_rate_sat_per_vb {
        if !rate.is_finite() || rate <= 0.0 {
            return Err(BoltzError::InvalidInput {
                error_details: format!(
                    "fee_rate_sat_per_vb must be a finite, positive sat/vB value, got {}",
                    rate
                ),
            });
        }
    }
    Ok(())
}

/// Parse `address` and require it to belong to `network`, returning its
/// canonical string form.
///
/// Fund destinations must be checked before they are acted on: a reverse
/// swap's claim address is persisted at creation and cannot be replaced after
/// the caller has paid the invoice, and a refund address receives the funds
/// directly.
pub(crate) fn validate_onchain_address(
    address: &str,
    network: BoltzNetwork,
) -> Result<String, BoltzError> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err(BoltzError::InvalidInput {
            error_details: "address must not be empty".to_string(),
        });
    }
    let parsed = trimmed
        .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
        .map_err(|e| BoltzError::InvalidInput {
            error_details: format!("Invalid Bitcoin address: {}", e),
        })?;
    let address = parsed
        .require_network(network.as_bitcoin_network())
        .map_err(|_| BoltzError::InvalidInput {
            error_details: format!(
                "Address does not belong to the {} network",
                network.as_str()
            ),
        })?;
    Ok(address.to_string())
}

/// Fully validate a submarine swap creation response before it is persisted
/// or the caller is handed a lockup address to fund.
///
/// `response.validate` proves the returned address commits to the script in
/// the response (with our refund key), but on Bitcoin it leaves the script's
/// hashlock unbound: Boltz could return a consistent script/address pair whose
/// hashlock does not match the invoice, letting it claim the lockup with a
/// preimage of its own choosing without ever paying the invoice. Binding the
/// hashlock to the invoice's payment hash closes that.
pub(crate) fn validate_submarine_response(
    response: &CreateSubmarineResponse,
    invoice: &str,
    refund_public_key: &bitcoin::PublicKey,
    network: BoltzNetwork,
) -> Result<(), BoltzError> {
    response
        .validate(invoice, refund_public_key, network.as_chain())
        .map_err(|e| BoltzError::SwapError {
            error_details: format!("Boltz submarine response failed validation: {}", e),
        })?;

    let invoice_preimage =
        Preimage::from_invoice_str(invoice).map_err(|e| BoltzError::InvalidInput {
            error_details: format!("Invalid BOLT11 invoice: {}", e),
        })?;
    let script =
        BtcSwapScript::submarine_from_swap_resp(response, *refund_public_key).map_err(|e| {
            BoltzError::SwapError {
                error_details: format!("Boltz submarine response failed validation: {}", e),
            }
        })?;
    if script.hashlock != invoice_preimage.hash160 {
        return Err(BoltzError::SwapError {
            error_details: format!(
                "Boltz submarine response failed validation: script hashlock {} does not match the invoice payment hash160 {}",
                script.hashlock, invoice_preimage.hash160
            ),
        });
    }
    Ok(())
}

/// Fully validate a reverse swap creation response before it is persisted or
/// the caller is handed an invoice to pay.
///
/// `response.validate` proves the invoice commits to our preimage's sha256 and
/// that the lockup address commits to the script in the response (with our
/// claim key), but it leaves two terms unbound: the script's hashlock is never
/// compared with the preimage (a mismatch would silently break our script-path
/// claim, leaving the funds claimable only if Boltz cooperates), and the
/// invoice amount is never compared with the requested `amount_sat`.
pub(crate) fn validate_reverse_response(
    response: &CreateReverseResponse,
    preimage: &Preimage,
    claim_public_key: &bitcoin::PublicKey,
    amount_sat: u64,
    network: BoltzNetwork,
) -> Result<(), BoltzError> {
    response
        .validate(preimage, claim_public_key, network.as_chain())
        .map_err(|e| BoltzError::SwapError {
            error_details: format!("Boltz reverse response failed validation: {}", e),
        })?;

    let script =
        BtcSwapScript::reverse_from_swap_resp(response, *claim_public_key).map_err(|e| {
            BoltzError::SwapError {
                error_details: format!("Boltz reverse response failed validation: {}", e),
            }
        })?;
    if script.hashlock != preimage.hash160 {
        return Err(BoltzError::SwapError {
            error_details: format!(
                "Boltz reverse response failed validation: script hashlock {} does not match our preimage hash160 {}",
                script.hashlock, preimage.hash160
            ),
        });
    }

    if let Some(invoice) = &response.invoice {
        let parsed = Bolt11Invoice::from_str(invoice).map_err(|e| BoltzError::SwapError {
            error_details: format!("Boltz returned an unparseable invoice: {}", e),
        })?;
        let expected_msat =
            amount_sat
                .checked_mul(1000)
                .ok_or_else(|| BoltzError::InvalidInput {
                    error_details: format!("amount_sat {} is too large", amount_sat),
                })?;
        match parsed.amount_milli_satoshis() {
            Some(msat) if msat == expected_msat => {}
            other => {
                return Err(BoltzError::SwapError {
                    error_details: format!(
                        "Boltz reverse response failed validation: invoice amount {:?} msat does not match the requested {} msat",
                        other, expected_msat
                    ),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_fee_rate, validate_onchain_address};
    use crate::modules::boltz::types::BoltzNetwork;

    #[test]
    fn accepts_valid_fee_rates_and_none() {
        assert!(validate_fee_rate(None).is_ok());
        assert!(validate_fee_rate(Some(1.5)).is_ok());
        assert!(validate_fee_rate(Some(500.0)).is_ok());
    }

    #[test]
    fn rejects_non_finite_and_non_positive_fee_rates() {
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                validate_fee_rate(Some(rate)).is_err(),
                "rate {} must be rejected",
                rate
            );
        }
    }

    #[test]
    fn accepts_an_address_on_the_selected_network() {
        let mainnet = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
        let canonical = validate_onchain_address(mainnet, BoltzNetwork::Mainnet).unwrap();
        assert_eq!(canonical, mainnet);

        // Surrounding whitespace is tolerated and stripped.
        let padded = format!("  {}\n", mainnet);
        assert_eq!(
            validate_onchain_address(&padded, BoltzNetwork::Mainnet).unwrap(),
            mainnet
        );
    }

    #[test]
    fn rejects_an_address_from_another_network() {
        // A valid mainnet address is not acceptable on testnet, and vice versa.
        let mainnet = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
        let testnet = "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx";
        assert!(validate_onchain_address(mainnet, BoltzNetwork::Testnet).is_err());
        assert!(validate_onchain_address(testnet, BoltzNetwork::Mainnet).is_err());
        assert!(validate_onchain_address(testnet, BoltzNetwork::Regtest).is_err());
    }

    #[test]
    fn rejects_garbage_and_empty_addresses() {
        assert!(validate_onchain_address("", BoltzNetwork::Mainnet).is_err());
        assert!(validate_onchain_address("   ", BoltzNetwork::Mainnet).is_err());
        assert!(validate_onchain_address("notanaddress", BoltzNetwork::Mainnet).is_err());
    }
}
