//! Recipient amounts and fee schedules for externally addressed Pubky swaps.
use super::{BoltzError, BoltzNetwork, BoltzSwapType, SwapRecord};
use boltz_client::util::fees::Fee;
use pubky_swap_boltz::swap_common::SwapDirection;

/// Public native terms. Amount limits refer to the provider lockup, before the claim fee.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PubkySendTerms {
    pub pair_hash: String,
    pub minimum_lockup_sat: u64,
    pub maximum_lockup_sat: u64,
    pub base_fee_sat: u64,
    pub fee_ppm: u64,
    pub lockup_fee_sat: u64,
    pub claim_fee_sat: u64,
}

pub async fn get_send_terms(network: BoltzNetwork) -> Result<PubkySendTerms, BoltzError> {
    let offer = super::pubky::bridge(network)
        .await?
        .offer()
        .await
        .map_err(super::api::bridge_error)?;
    if !offer.directions.contains(&SwapDirection::Reverse) {
        return Err(invalid(
            "Provider does not support payments to Bitcoin addresses",
        ));
    }
    Ok(PubkySendTerms {
        pair_hash: pubky_swap_boltz::fees::pair_hash(&offer, SwapDirection::Reverse)
            .map_err(super::api::bridge_error)?,
        minimum_lockup_sat: offer.effective_min_amount_sat(),
        maximum_lockup_sat: offer.max_amount_sat,
        base_fee_sat: offer.base_fee_sat,
        fee_ppm: offer.fee_ppm,
        lockup_fee_sat: offer.onchain_fee_sat,
        claim_fee_sat: offer
            .fee_rate_sat_vb
            .max(2)
            .checked_mul(180)
            .ok_or_else(|| invalid("Claim fee overflow"))?,
    })
}

pub(crate) fn validate_recipient(record: &SwapRecord) -> Result<(), BoltzError> {
    if let Some(recipient) = record.recipient_amount_sat {
        if record.backend_binding.is_none()
            || record.swap_type != BoltzSwapType::Reverse
            || recipient == 0
            || record
                .onchain_amount_sat
                .is_none_or(|value| value <= recipient)
        {
            return Err(invalid("Invalid recipient amount in swap recovery record"));
        }
    }
    Ok(())
}

pub(crate) fn claim_fee(record: &SwapRecord, rate: Option<f64>) -> Result<Fee, BoltzError> {
    validate_recipient(record)?;
    match record.recipient_amount_sat {
        Some(recipient) => Ok(Fee::Absolute(
            record.onchain_amount_sat.unwrap_or(0) - recipient,
        )),
        None => Ok(Fee::Relative(
            rate.unwrap_or(super::claim::DEFAULT_FEERATE_SAT_PER_VB),
        )),
    }
}

pub(crate) fn validate_claim_output(
    record: &SwapRecord,
    tx: &bitcoin::Transaction,
) -> Result<(), BoltzError> {
    let Some(recipient) = record.recipient_amount_sat else {
        return Ok(());
    };
    let address = record
        .onchain_address
        .as_deref()
        .ok_or_else(|| invalid("Missing recipient address"))?;
    let address = super::validation::validate_onchain_address(address, record.network)?;
    use std::str::FromStr;
    let script = bitcoin::Address::from_str(&address)
        .map_err(|_| invalid("Invalid recipient address"))?
        .require_network(record.network.as_bitcoin_network())
        .map_err(|_| invalid("Invalid recipient network"))?
        .script_pubkey();
    if tx.output.len() != 1
        || tx.output[0].script_pubkey != script
        || tx.output[0].value.to_sat() < recipient
    {
        return Err(invalid("Claim transaction would underpay the recipient"));
    }
    Ok(())
}

fn invalid(message: &str) -> BoltzError {
    BoltzError::InvalidInput {
        error_details: message.into(),
    }
}
