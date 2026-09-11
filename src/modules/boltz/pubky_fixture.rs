//! Regtest-only deterministic provider fixture for official Boltz SDK conformance.

use async_trait::async_trait;
use bitcoin::{
    hashes::{sha256, Hash},
    secp256k1::{Secp256k1, SecretKey},
    Network, PublicKey, Transaction, Txid,
};
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use pubky_swap_boltz::swap_common::{
    messages::{
        Quote, QuoteRequest, SwapAccept, SwapOffer, SwapRequest, SwapScript, SwapStatusSnapshot,
        PROTOCOL_VERSION,
    },
    taproot::BoltzTaprootSwap,
    NetworkSpec, SwapDirection, SwapState,
};
use pubky_swap_boltz::{
    chain::{Chain, Observation},
    model::TransactionInfo,
    provider::Provider,
    service::BridgeSettings,
    store::Store,
    Bridge, Error, Result,
};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

const CHAIN_TIP: u32 = 100;
const TIMEOUT_BLOCKS: u32 = 144;
pub fn fixture_identity() -> String {
    pubky_swap_boltz::identity_from_secret(&[1; 32])
}

pub fn fixture_provider() -> String {
    pubky_swap_boltz::identity_from_secret(&[2; 32])
}

pub fn fixture_binding() -> String {
    format!("{}|{}|regtest", fixture_identity(), fixture_provider())
}

/// Construct the same bridge used by the standalone fixture and integration tests.
pub fn fixture_bridge() -> Result<Arc<Bridge>> {
    fixture_bridge_with_store(Store::memory(&fixture_binding())?)
}

pub fn fixture_bridge_with_store(store: Store) -> Result<Arc<Bridge>> {
    let settings = BridgeSettings {
        network: Network::Regtest,
        max_fee_bps: 500,
        max_amount_sat: 1_000_000,
    };
    Ok(Bridge::new(
        Arc::new(FixtureProvider::default()),
        Arc::new(FixtureChain),
        store,
        settings,
    ))
}

/// In-memory native provider for deterministic regtest protocol tests.
#[derive(Default)]
pub struct FixtureProvider {
    quotes: Mutex<HashMap<Uuid, Quote>>,
    swaps: Mutex<HashMap<Uuid, SwapAccept>>,
}

#[async_trait]
impl Provider for FixtureProvider {
    fn identity(&self) -> String {
        fixture_identity()
    }

    fn provider_key(&self) -> String {
        fixture_provider()
    }

    async fn offer(&self) -> Result<SwapOffer> {
        Ok(fixture_offer())
    }

    async fn quote(&self, request: QuoteRequest) -> Result<Quote> {
        if request.offer_id != fixture_offer().offer_id
            || request.client_pkarr != fixture_identity()
        {
            return Err(Error::Validation);
        }
        let quote = fixture_quote(request);
        self.quotes
            .lock()
            .await
            .insert(quote.quote_id, quote.clone());
        Ok(quote)
    }

    async fn create(&self, request: SwapRequest) -> Result<SwapAccept> {
        if request.script_type != SwapScript::TaprootBoltz
            || request.client_pkarr != fixture_identity()
        {
            return Err(Error::Validation);
        }
        let mut swaps = self.swaps.lock().await;
        if let Some(accept) = swaps
            .values()
            .find(|swap| swap.quote_id == request.quote_id)
        {
            return Ok(accept.clone());
        }
        let quote = self
            .quotes
            .lock()
            .await
            .get(&request.quote_id)
            .cloned()
            .ok_or(Error::NotFound)?;
        let accept = fixture_accept(&request, &quote)?;
        swaps.insert(accept.swap_id, accept.clone());
        Ok(accept)
    }

    async fn snapshot_by_quote(&self, quote_id: Uuid) -> Result<SwapStatusSnapshot> {
        let swap_id = self
            .swaps
            .lock()
            .await
            .values()
            .find(|accept| accept.quote_id == quote_id)
            .map(|accept| accept.swap_id)
            .ok_or(Error::NotFound)?;
        self.snapshot(swap_id).await
    }

    async fn snapshot(&self, swap_id: Uuid) -> Result<SwapStatusSnapshot> {
        let accept = self
            .swaps
            .lock()
            .await
            .get(&swap_id)
            .cloned()
            .ok_or(Error::NotFound)?;
        Ok(SwapStatusSnapshot {
            request_id: None,
            accept,
            network: NetworkSpec::Regtest,
            state: SwapState::Created,
            funding_txid_hex: None,
            funding_vout: None,
            spend_txid_hex: None,
            required_confirmations: 1,
            updated_at_unix: now(),
            observed_at_unix: now(),
        })
    }
}

/// Return the stable zero-fee regtest offer used by this fixture.
pub fn fixture_offer() -> SwapOffer {
    SwapOffer {
        request_id: None,
        offer_id: Uuid::from_u128(1),
        provider_pkarr: fixture_provider(),
        network: NetworkSpec::Regtest,
        directions: vec![SwapDirection::Submarine, SwapDirection::Reverse],
        min_amount_sat: 10_000,
        max_amount_sat: 1_000_000,
        base_fee_sat: 0,
        fee_ppm: 0,
        required_confirmations: 1,
        htlc_timeout_blocks: TIMEOUT_BLOCKS,
        lightning_node_id: None,
        valid_until_unix: now() + 600,
        onchain_fee_sat: 0,
        fee_rate_sat_vb: 1,
        protocol_version: PROTOCOL_VERSION,
        features: vec!["boltz-taproot-v1".into(), "swap-status-v1".into()],
    }
}

fn fixture_quote(request: QuoteRequest) -> Quote {
    Quote {
        request_id: request.request_id,
        quote_id: Uuid::new_v4(),
        offer_id: request.offer_id,
        direction: request.direction,
        amount_sat: request.amount_sat,
        fee_sat: 0,
        service_fee_sat: 0,
        onchain_fee_sat: 0,
        fee_rate_sat_vb: 1,
        total_sat: request.amount_sat,
        htlc_timeout_blocks: TIMEOUT_BLOCKS,
        required_confirmations: 1,
        valid_until_unix: now() + 600,
        protocol_version: PROTOCOL_VERSION,
    }
}

fn fixture_accept(request: &SwapRequest, quote: &Quote) -> Result<SwapAccept> {
    let contract = fixture_contract(request)?;
    let invoice = match request.direction {
        SwapDirection::Reverse => Some(fixture_invoice(request, quote)?),
        SwapDirection::Submarine => None,
    };
    Ok(SwapAccept {
        script_type: SwapScript::TaprootBoltz,
        swap_tree: Some(contract.swap_tree()),
        quote_id: request.quote_id,
        swap_id: Uuid::new_v4(),
        direction: request.direction,
        htlc_script_hex: String::new(),
        htlc_address: contract.address(Network::Regtest).to_string(),
        onchain_amount_sat: quote.amount_sat,
        timeout_block_height: CHAIN_TIP + TIMEOUT_BLOCKS,
        provider_pubkey_hex: provider_public_key().to_string(),
        invoice,
    })
}

fn fixture_contract(request: &SwapRequest) -> Result<BoltzTaprootSwap> {
    let provider = provider_public_key();
    let (claim, refund) = match request.direction {
        SwapDirection::Submarine => (
            provider,
            parse_key(request.client_refund_pubkey_hex.as_deref())?,
        ),
        SwapDirection::Reverse => (
            parse_key(request.client_claim_pubkey_hex.as_deref())?,
            provider,
        ),
    };
    let hash = sha256::Hash::from_str(&request.payment_hash_hex).map_err(|_| Error::Validation)?;
    BoltzTaprootSwap::new(
        request.direction,
        &hash.to_byte_array(),
        &claim,
        &refund,
        CHAIN_TIP + TIMEOUT_BLOCKS,
    )
    .map_err(|_| Error::Validation)
}

fn fixture_invoice(request: &SwapRequest, quote: &Quote) -> Result<String> {
    let hash = sha256::Hash::from_str(&request.payment_hash_hex).map_err(|_| Error::Validation)?;
    InvoiceBuilder::new(Currency::Regtest)
        .amount_milli_satoshis(quote.total_sat * 1000)
        .description("Pubky Swap interoperability fixture".into())
        .payment_hash(hash)
        .payment_secret(PaymentSecret([7; 32]))
        .current_timestamp()
        .expiry_time(Duration::from_secs(86_400))
        .min_final_cltv_expiry_delta(240)
        .build_signed(|message| {
            Secp256k1::new().sign_ecdsa_recoverable(message, &provider_secret_key())
        })
        .map(|invoice| invoice.to_string())
        .map_err(|_| Error::Validation)
}

fn parse_key(key: Option<&str>) -> Result<PublicKey> {
    PublicKey::from_str(key.ok_or(Error::Validation)?).map_err(|_| Error::Validation)
}

fn provider_secret_key() -> SecretKey {
    let mut scalar = [0; 32];
    scalar[31] = 1;
    SecretKey::from_slice(&scalar).expect("invariant: one is a valid fixture scalar")
}

fn provider_public_key() -> PublicKey {
    PublicKey::new(bitcoin::secp256k1::PublicKey::from_secret_key(
        &Secp256k1::new(),
        &provider_secret_key(),
    ))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("invariant: fixture clock is after Unix epoch")
        .as_secs()
}

/// Chain backend with a fixed tip and no funded outputs.
pub struct FixtureChain;

#[async_trait]
impl Chain for FixtureChain {
    async fn tip(&self) -> Result<u32> {
        Ok(CHAIN_TIP)
    }

    async fn fee(&self) -> Result<u64> {
        Ok(1)
    }

    async fn observe(&self, _: &SwapAccept) -> Result<Option<Observation>> {
        Ok(None)
    }

    async fn transaction(&self, _: Txid) -> Result<TransactionInfo> {
        Err(Error::NotFound)
    }

    async fn broadcast(&self, transaction: Transaction) -> Result<Txid> {
        Ok(transaction.compute_txid())
    }
}
