use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::models::{derive_swap_keypair, BoltzDB, CreationIntent, SwapRecord};
use crate::modules::boltz::types::{
    BoltzNetwork, BoltzPairInfo, BoltzSwapType, ReverseSwapResponse, SubmarineSwapResponse,
};
use crate::modules::boltz::validation::{
    validate_onchain_address, validate_reverse_response, validate_submarine_response,
};
use bitcoin::hashes::{sha256, Hash};
use boltz_client::swaps::boltz::{
    CreateReverseResponse, CreateSubmarineResponse, ReversePair, SubmarinePair,
};
use boltz_client::util::secrets::Preimage;
use pubky_swap_boltz::model::{CreateRequest, ReverseRequest, SubmarineRequest};
use pubky_swap_boltz::swap_common::SwapDirection;
use serde_json::json;

pub(crate) fn bridge_error(error: pubky_swap_boltz::Error) -> BoltzError {
    BoltzError::ApiError {
        error_details: format!("Pubky swap: {error}"),
    }
}

/// Fetch the configured Pubky provider's Bitcoin submarine terms.
pub async fn get_submarine_limits(network: BoltzNetwork) -> Result<BoltzPairInfo, BoltzError> {
    let pairs = super::pubky::bridge(network)
        .await?
        .pairs(SwapDirection::Submarine)
        .await
        .map_err(bridge_error)?;
    // Rust boltz-client 0.4.1 requires a Liquid map on its outer response.
    // Parse the supported Bitcoin pair directly, preserving the SDK fee types.
    let pair: SubmarinePair = serde_json::from_value(pairs["BTC"]["BTC"].clone())?;
    Ok(BoltzPairInfo {
        hash: pair.hash,
        rate: pair.rate,
        minimal_sat: pair.limits.minimal,
        maximal_sat: pair.limits.maximal,
        fee_percentage: pair.fees.percentage,
        miner_fees_sat: pair.fees.miner_fees,
    })
}

/// Fetch the configured Pubky provider's Bitcoin reverse terms.
pub async fn get_reverse_limits(network: BoltzNetwork) -> Result<BoltzPairInfo, BoltzError> {
    let pairs = super::pubky::bridge(network)
        .await?
        .pairs(SwapDirection::Reverse)
        .await
        .map_err(bridge_error)?;
    let pair: ReversePair = serde_json::from_value(pairs["BTC"]["BTC"].clone())?;
    Ok(BoltzPairInfo {
        hash: pair.hash,
        rate: pair.rate,
        minimal_sat: pair.limits.minimal,
        maximal_sat: pair.limits.maximal,
        fee_percentage: pair.fees.percentage,
        miner_fees_sat: pair.fees.miner_fees.lockup + pair.fees.miner_fees.claim,
    })
}

impl BoltzDB {
    /// Persist the recovery intent before negotiating, retaining wallet signing.
    pub async fn create_submarine_swap(
        &self,
        network: BoltzNetwork,
        electrum_url: String,
        invoice: String,
        mnemonic: String,
        bip39_passphrase: Option<String>,
    ) -> Result<SubmarineSwapResponse, BoltzError> {
        let _operation = self.recovery_gate.read().await;
        let invoice = invoice.trim().to_string();
        if invoice.is_empty() {
            return Err(BoltzError::InvalidInput {
                error_details: "invoice must not be empty".into(),
            });
        }
        let binding = super::pubky::binding(network).await?;
        let electrum_url =
            super::pubky::electrum_for_binding(network, &binding, &electrum_url).await?;
        let wallet = wallet_fingerprint(&mnemonic, bip39_passphrase.as_deref(), network)?;
        let index = self.reserve_swap_index().await?;
        let keys = derive_swap_keypair(&mnemonic, bip39_passphrase.as_deref(), network, index)?;
        let request_key = digest(&json!([binding, wallet, network, "submarine", invoice]))?;
        let intent = self
            .save_intent(&CreationIntent {
                id: uuid::Uuid::new_v4().to_string(),
                request_key,
                wallet_fingerprint: wallet,
                backend_binding: binding,
                network,
                electrum_url,
                swap_index: index,
                recipient_amount_sat: None,
                claim_address: None,
                created_at: now_secs(),
                request: CreateRequest::Submarine(SubmarineRequest {
                    from: "BTC".into(),
                    to: "BTC".into(),
                    invoice,
                    refund_public_key: bitcoin::PublicKey::new(keys.public_key()).to_string(),
                    pair_hash: String::new(),
                    referral_id: String::new(),
                    error: String::new(),
                }),
            })
            .await?;
        let response: CreateSubmarineResponse = serde_json::from_value(
            self.resume_intent(&intent, &mnemonic, bip39_passphrase.as_deref())
                .await?,
        )?;
        Ok(SubmarineSwapResponse {
            id: response.id,
            address: response.address,
            bip21: response.bip21,
            expected_amount_sat: response.expected_amount,
            accept_zero_conf: response.accept_zero_conf,
            timeout_block_height: response.timeout_block_height,
        })
    }

    pub async fn create_reverse_swap(
        &self,
        network: BoltzNetwork,
        electrum_url: String,
        amount_sat: u64,
        claim_address: String,
        mnemonic: String,
        bip39_passphrase: Option<String>,
    ) -> Result<ReverseSwapResponse, BoltzError> {
        self.create_reverse_swap_with_recipient(
            network,
            electrum_url,
            amount_sat,
            claim_address,
            mnemonic,
            bip39_passphrase,
            None,
            String::new(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_reverse_swap_with_recipient(
        &self,
        network: BoltzNetwork,
        electrum_url: String,
        amount_sat: u64,
        claim_address: String,
        mnemonic: String,
        bip39_passphrase: Option<String>,
        recipient_amount_sat: Option<u64>,
        pair_hash: String,
    ) -> Result<ReverseSwapResponse, BoltzError> {
        let _operation = self.recovery_gate.read().await;
        if amount_sat == 0 {
            return Err(BoltzError::InvalidInput {
                error_details: "amount_sat must be greater than 0".into(),
            });
        }
        let claim_address = validate_onchain_address(&claim_address, network)?;
        let binding = super::pubky::binding(network).await?;
        let electrum_url =
            super::pubky::electrum_for_binding(network, &binding, &electrum_url).await?;
        let wallet = wallet_fingerprint(&mnemonic, bip39_passphrase.as_deref(), network)?;
        let index = self.reserve_swap_index().await?;
        let keys = derive_swap_keypair(&mnemonic, bip39_passphrase.as_deref(), network, index)?;
        let request_key = digest(&json!([
            binding,
            wallet,
            network,
            "reverse",
            amount_sat,
            claim_address
        ]))?;
        let request_key = match recipient_amount_sat {
            Some(recipient) => digest(&json!([request_key, recipient, pair_hash]))?,
            None => request_key,
        };
        let intent = self
            .save_intent(&CreationIntent {
                id: uuid::Uuid::new_v4().to_string(),
                request_key,
                wallet_fingerprint: wallet,
                backend_binding: binding,
                network,
                electrum_url,
                swap_index: index,
                recipient_amount_sat,
                claim_address: Some(claim_address),
                created_at: now_secs(),
                request: CreateRequest::Reverse(ReverseRequest {
                    from: "BTC".into(),
                    to: "BTC".into(),
                    invoice_amount: amount_sat,
                    onchain_amount: 0,
                    preimage_hash: Preimage::from_swap_key(&keys).sha256.to_string(),
                    claim_public_key: bitcoin::PublicKey::new(keys.public_key()).to_string(),
                    pair_hash,
                    referral_id: String::new(),
                    error: String::new(),
                }),
            })
            .await?;
        let response: CreateReverseResponse = serde_json::from_value(
            self.resume_intent(&intent, &mnemonic, bip39_passphrase.as_deref())
                .await?,
        )?;
        Ok(ReverseSwapResponse {
            id: response.id,
            invoice: response.invoice.ok_or_else(|| BoltzError::SwapError {
                error_details: "Missing reverse invoice".into(),
            })?,
            lockup_address: response.lockup_address,
            onchain_amount_sat: response.onchain_amount,
            timeout_block_height: u64::from(response.timeout_block_height),
        })
    }

    pub(crate) async fn recover_creation_intents(
        &self,
        network: BoltzNetwork,
        mnemonic: &str,
        passphrase: Option<&str>,
    ) -> Result<(), BoltzError> {
        let _operation = self.recovery_gate.read().await;
        let wallet = wallet_fingerprint(mnemonic, passphrase, network)?;
        for intent in self.creation_intents().await? {
            if intent.network == network && intent.wallet_fingerprint == wallet {
                if let Err(error) = self.resume_intent(&intent, mnemonic, passphrase).await {
                    log::warn!("Pubky creation recovery remains pending: {}", error);
                }
            }
        }
        Ok(())
    }

    async fn resume_intent(
        &self,
        intent: &CreationIntent,
        mnemonic: &str,
        passphrase: Option<&str>,
    ) -> Result<serde_json::Value, BoltzError> {
        let _guard = super::guard::lock_swap(&intent.id).await;
        if super::pubky::binding(intent.network).await? != intent.backend_binding
            || wallet_fingerprint(mnemonic, passphrase, intent.network)?
                != intent.wallet_fingerprint
        {
            return Err(BoltzError::InvalidInput {
                error_details: "Creation recovery identity does not match".into(),
            });
        }
        let keys = derive_swap_keypair(mnemonic, passphrase, intent.network, intent.swap_index)?;
        let public_key = bitcoin::PublicKey::new(keys.public_key());
        if intent.request.client_key() != public_key.to_string() {
            return Err(BoltzError::InvalidInput {
                error_details: "Creation recovery key does not match".into(),
            });
        }
        let bridge =
            super::pubky::bridge_for_binding(intent.network, &intent.backend_binding).await?;
        self.negotiate_intent(intent, &keys, &bridge).await
    }

    async fn negotiate_intent(
        &self,
        intent: &CreationIntent,
        keys: &boltz_client::Keypair,
        bridge: &pubky_swap_boltz::Bridge,
    ) -> Result<serde_json::Value, BoltzError> {
        let response = bridge
            .create(intent.request.clone(), Some(intent.id.clone()))
            .await
            .map_err(bridge_error)?;
        let record = record_from_response(intent, &response, keys)?;
        super::send::validate_recipient(&record)?;
        self.complete_intent(intent, &record).await?;
        Ok(response)
    }
}

pub(super) fn record_from_response(
    intent: &CreationIntent,
    response: &serde_json::Value,
    keys: &boltz_client::Keypair,
) -> Result<SwapRecord, BoltzError> {
    let key = bitcoin::PublicKey::new(keys.public_key());
    let (id, swap_type, invoice, address, amount, onchain, timeout) = match &intent.request {
        CreateRequest::Submarine(request) => {
            let response: CreateSubmarineResponse = serde_json::from_value(response.clone())?;
            validate_submarine_response(&response, &request.invoice, &key, intent.network)?;
            (
                response.id,
                BoltzSwapType::Submarine,
                request.invoice.clone(),
                response.address,
                response.expected_amount,
                None,
                response.timeout_block_height,
            )
        }
        CreateRequest::Reverse(request) => {
            let response: CreateReverseResponse = serde_json::from_value(response.clone())?;
            validate_reverse_response(
                &response,
                &Preimage::from_swap_key(keys),
                &key,
                request.invoice_amount,
                intent.network,
            )?;
            let invoice = response.invoice.ok_or_else(|| BoltzError::SwapError {
                error_details: "Missing reverse invoice".into(),
            })?;
            (
                response.id,
                BoltzSwapType::Reverse,
                invoice,
                response.lockup_address,
                request.invoice_amount,
                Some(response.onchain_amount),
                u64::from(response.timeout_block_height),
            )
        }
    };
    Ok(SwapRecord {
        id,
        backend_binding: Some(intent.backend_binding.clone()),
        swap_type,
        status: if swap_type == BoltzSwapType::Submarine {
            "invoice.set"
        } else {
            "swap.created"
        }
        .into(),
        network: intent.network,
        electrum_url: intent.electrum_url.clone(),
        swap_index: intent.swap_index,
        invoice: Some(invoice),
        lockup_address: Some(address),
        onchain_address: intent.claim_address.clone(),
        amount_sat: amount,
        recipient_amount_sat: intent.recipient_amount_sat,
        onchain_amount_sat: onchain,
        timeout_block_height: timeout,
        create_response_json: serde_json::to_string(response)?,
        claim_tx_id: None,
        refund_tx_id: None,
        created_at: intent.created_at,
    })
}

fn wallet_fingerprint(
    mnemonic: &str,
    passphrase: Option<&str>,
    network: BoltzNetwork,
) -> Result<String, BoltzError> {
    let keys = derive_swap_keypair(mnemonic, passphrase, network, 0)?;
    Ok(sha256::Hash::hash(&keys.public_key().serialize()).to_string())
}

fn digest(value: &serde_json::Value) -> Result<String, BoltzError> {
    Ok(sha256::Hash::hash(&serde_json::to_vec(value)?).to_string())
}

fn now_secs() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

#[cfg(test)]
#[path = "pubky_tests.rs"]
mod pubky_tests;
