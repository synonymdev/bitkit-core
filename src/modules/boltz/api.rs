use crate::modules::boltz::client::build_boltz_client;
use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::models::{derive_swap_keypair, BoltzDB, SwapRecord};
use crate::modules::boltz::types::{
    BoltzNetwork, BoltzPairInfo, BoltzSwapType, ReverseSwapResponse, SubmarineSwapResponse,
};
use crate::modules::boltz::validation::{
    validate_onchain_address, validate_reverse_response, validate_submarine_response,
};
use boltz_client::swaps::boltz::{CreateReverseRequest, CreateSubmarineRequest};
use boltz_client::util::secrets::Preimage;

/// Raw status string a freshly created swap starts in.
const STATUS_CREATED: &str = "swap.created";

/// Fetch fees and limits for submarine swaps (onchain -> Lightning) on `network`.
pub async fn get_submarine_limits(network: BoltzNetwork) -> Result<BoltzPairInfo, BoltzError> {
    let client = build_boltz_client(network);
    let pairs = client
        .get_submarine_pairs()
        .await
        .map_err(map_api_err("fetch submarine pairs"))?;
    let pair = pairs.get_btc_to_btc_pair().ok_or(BoltzError::ApiError {
        error_details: "BTC submarine pair unavailable".to_string(),
    })?;
    Ok(BoltzPairInfo {
        hash: pair.hash,
        rate: pair.rate,
        minimal_sat: pair.limits.minimal,
        maximal_sat: pair.limits.maximal,
        fee_percentage: pair.fees.percentage,
        miner_fees_sat: pair.fees.miner_fees,
    })
}

/// Fetch fees and limits for reverse swaps (Lightning -> onchain) on `network`.
pub async fn get_reverse_limits(network: BoltzNetwork) -> Result<BoltzPairInfo, BoltzError> {
    let client = build_boltz_client(network);
    let pairs = client
        .get_reverse_pairs()
        .await
        .map_err(map_api_err("fetch reverse pairs"))?;
    let pair = pairs.get_btc_to_btc_pair().ok_or(BoltzError::ApiError {
        error_details: "BTC reverse pair unavailable".to_string(),
    })?;
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
    /// Create a submarine swap (onchain -> Lightning).
    ///
    /// `invoice` is the BOLT11 invoice the caller's Lightning node generated for
    /// the amount it wants to receive. Boltz returns a lockup address the caller
    /// funds from its onchain wallet. The refund key is derived deterministically
    /// from `mnemonic` at a freshly reserved index (persisted, not the key), so
    /// the swap can be refunded — and recovered from the seed — if Boltz fails to
    /// pay the invoice.
    pub async fn create_submarine_swap(
        &self,
        network: BoltzNetwork,
        electrum_url: String,
        invoice: String,
        mnemonic: String,
        bip39_passphrase: Option<String>,
    ) -> Result<SubmarineSwapResponse, BoltzError> {
        if invoice.trim().is_empty() {
            return Err(BoltzError::InvalidInput {
                error_details: "invoice must not be empty".to_string(),
            });
        }

        let swap_index = self.reserve_swap_index().await?;
        let keypair =
            derive_swap_keypair(&mnemonic, bip39_passphrase.as_deref(), network, swap_index)?;
        let refund_public_key = bitcoin::PublicKey::new(keypair.public_key());

        let client = build_boltz_client(network);
        let request = CreateSubmarineRequest {
            from: "BTC".to_string(),
            to: "BTC".to_string(),
            invoice: invoice.clone(),
            refund_public_key,
            pair_hash: None,
            referral_id: None,
            webhook: None,
        };
        let response = client
            .post_swap_req(&request)
            .await
            .map_err(map_api_err("create submarine swap"))?;

        // Prove Boltz's response before persisting it or handing the caller a
        // lockup address to fund: the returned address must commit to a script
        // holding our refund key, and that script's hashlock must match the
        // invoice's payment hash. Without both, a malicious or buggy server
        // could hand back an address we cannot refund from, or one it can
        // claim without ever paying the invoice.
        validate_submarine_response(&response, &invoice, &refund_public_key, network)?;

        let record = SwapRecord {
            id: response.id.clone(),
            swap_type: BoltzSwapType::Submarine,
            status: STATUS_CREATED.to_string(),
            network,
            electrum_url,
            swap_index,
            invoice: Some(invoice),
            lockup_address: Some(response.address.clone()),
            onchain_address: None,
            amount_sat: response.expected_amount,
            onchain_amount_sat: None,
            timeout_block_height: response.timeout_block_height,
            create_response_json: serde_json::to_string(&response)?,
            claim_tx_id: None,
            refund_tx_id: None,
            created_at: now_secs(),
        };
        self.insert_swap(&record).await?;

        Ok(SubmarineSwapResponse {
            id: response.id,
            address: response.address,
            bip21: response.bip21,
            expected_amount_sat: response.expected_amount,
            accept_zero_conf: response.accept_zero_conf,
            timeout_block_height: response.timeout_block_height,
        })
    }

    /// Create a reverse swap (Lightning -> onchain).
    ///
    /// `amount_sat` is the Lightning amount the caller will pay; `claim_address`
    /// is the onchain address the received funds are claimed to (typically a
    /// fresh address from the caller's onchain wallet). The claim key and the
    /// preimage are derived deterministically from `mnemonic` at a freshly
    /// reserved index (the preimage is `sha256(swap_key)`), so the claim can be
    /// made — and recovered from the seed — once Boltz locks the onchain funds.
    pub async fn create_reverse_swap(
        &self,
        network: BoltzNetwork,
        electrum_url: String,
        amount_sat: u64,
        claim_address: String,
        mnemonic: String,
        bip39_passphrase: Option<String>,
    ) -> Result<ReverseSwapResponse, BoltzError> {
        if amount_sat == 0 {
            return Err(BoltzError::InvalidInput {
                error_details: "amount_sat must be greater than 0".to_string(),
            });
        }
        // The claim address is persisted at creation and cannot be replaced
        // once the caller has paid the invoice, so a parse or wrong-network
        // failure must surface now, not at claim time.
        let claim_address = validate_onchain_address(&claim_address, network)?;

        let swap_index = self.reserve_swap_index().await?;
        let keypair =
            derive_swap_keypair(&mnemonic, bip39_passphrase.as_deref(), network, swap_index)?;
        let claim_public_key = bitcoin::PublicKey::new(keypair.public_key());
        let preimage = Preimage::from_swap_key(&keypair);

        let client = build_boltz_client(network);
        let request = CreateReverseRequest {
            from: "BTC".to_string(),
            to: "BTC".to_string(),
            claim_public_key,
            invoice: None,
            invoice_amount: Some(amount_sat),
            preimage_hash: Some(preimage.sha256),
            description: None,
            description_hash: None,
            address: None,
            address_signature: None,
            referral_id: None,
            webhook: None,
        };
        let response = client
            .post_reverse_req(request)
            .await
            .map_err(map_api_err("create reverse swap"))?;

        // Prove Boltz's response before persisting it or handing the caller an
        // invoice to pay: the invoice must commit to our preimage, the lockup
        // address must commit to a script holding our claim key whose hashlock
        // matches that preimage, and the invoice amount must be what was
        // requested. Without those, the caller could pay an invoice for funds
        // it cannot claim, or pay more than it asked to.
        validate_reverse_response(&response, &preimage, &claim_public_key, amount_sat, network)?;

        let invoice = response.invoice.clone().ok_or(BoltzError::ApiError {
            error_details: "Reverse swap response missing invoice".to_string(),
        })?;
        let timeout_block_height = response.timeout_block_height as u64;
        let onchain_amount = response.onchain_amount;

        let record = SwapRecord {
            id: response.id.clone(),
            swap_type: BoltzSwapType::Reverse,
            status: STATUS_CREATED.to_string(),
            network,
            electrum_url,
            swap_index,
            invoice: Some(invoice.clone()),
            lockup_address: Some(response.lockup_address.clone()),
            onchain_address: Some(claim_address),
            amount_sat,
            onchain_amount_sat: Some(onchain_amount),
            timeout_block_height,
            create_response_json: serde_json::to_string(&response)?,
            claim_tx_id: None,
            refund_tx_id: None,
            created_at: now_secs(),
        };
        self.insert_swap(&record).await?;

        Ok(ReverseSwapResponse {
            id: response.id,
            invoice,
            lockup_address: response.lockup_address,
            onchain_amount_sat: onchain_amount,
            timeout_block_height,
        })
    }
}

fn now_secs() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn map_api_err(context: &'static str) -> impl Fn(boltz_client::error::Error) -> BoltzError {
    move |e| BoltzError::ApiError {
        error_details: format!("Failed to {}: {}", context, e),
    }
}
