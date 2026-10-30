use serde::{Deserialize, Serialize};

/// A `swap/1` edge from a provider's offer, in Paykit Capabilities terms: the payment endpoint
/// the owner pays from, the endpoint the route delivers to, its bounds and its indicative fee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct SwapRouteEdge {
    pub provider_pubky: String,
    pub offer_id: String,
    /// `reverse` (Lightning to on-chain) or `submarine` (on-chain to Lightning).
    pub direction: String,
    /// Payment Endpoint Identifier the owner pays with, such as `btc-lightning-bolt11`.
    pub from_endpoint: String,
    /// Payment Endpoint Identifier family the route delivers to, such as `btc-regtest-*`.
    pub to_endpoint: String,
    /// The minimum the provider enforces, not its configured floor.
    pub min_sat: u64,
    pub max_sat: u64,
    pub base_fee_sat: u64,
    pub fee_ppm: u64,
    pub onchain_fee_sat: u64,
    /// Atomicity class of the edge. Pubky Swaps legs are always `hash`.
    pub atomicity: String,
    pub fund_within_secs: u64,
}

/// What the wallet knows when it pays a Paykit contact: the amount, the endpoints the contact
/// published and what the wallet can spend on each rail.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PaykitRouteRequest {
    pub amount_sat: u64,
    /// Payment Endpoint Identifiers paired with their values, as Paykit resolved them.
    pub contact_endpoints: Vec<PaykitEndpoint>,
    pub lightning_spendable_sat: u64,
    pub onchain_spendable_sat: u64,
    /// Candidate swap providers as pubkys, usually the user's contacts. Each is asked for its
    /// offer; those that run no provider are skipped.
    pub providers: Vec<String>,
    /// Fee rate for the wallet's own claim transaction.
    pub fee_rate_sat_vb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PaykitEndpoint {
    pub method_id: String,
    pub value: String,
}

/// The route the router picked. A direct route means no swap is needed.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum PaykitRoute {
    /// The wallet can pay this endpoint from its own balance.
    Direct { endpoint: PaykitEndpoint },
    /// The wallet pays over Lightning and a Pubky Swaps provider delivers on-chain.
    Swap { quote: PubkySwapRouteQuote },
    /// Nothing can pay this contact. `reason` says why, for the user.
    None { reason: String },
}

/// A firm `swap/1` quote for one route, with everything the review screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PubkySwapRouteQuote {
    pub provider_pubky: String,
    pub quote_id: String,
    /// The contact endpoint the swap delivers to.
    pub endpoint: PaykitEndpoint,
    /// What the contact receives.
    pub delivered_sat: u64,
    /// What the provider locks on chain: the delivered amount plus the wallet's claim fee.
    pub lock_sat: u64,
    /// What the wallet pays over Lightning.
    pub debit_sat: u64,
    pub service_fee_sat: u64,
    pub provider_onchain_fee_sat: u64,
    pub claim_fee_sat: u64,
    /// `debit_sat - delivered_sat`.
    pub total_fee_sat: u64,
    pub atomicity: String,
    pub valid_until_unix: u64,
    /// How many providers were asked for a quote.
    pub providers_quoted: u32,
}

/// A committed swap: the hold invoice the wallet pays to start it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PubkySwapRoutePayment {
    pub swap_id: String,
    pub invoice: String,
    pub debit_sat: u64,
    pub delivered_sat: u64,
    pub destination_address: String,
    pub htlc_address: String,
    pub timeout_block_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PubkySwapRouteState {
    /// Waiting for the hold invoice to be paid.
    AwaitingPayment,
    /// Paid; waiting for the provider's on-chain lockup, or for its confirmations.
    AwaitingLockup {
        confirmations: u32,
        required: u32,
    },
    /// The claim to the contact's address is broadcast. The provider settles the invoice
    /// once it sees the preimage.
    Claimed {
        txid: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PubkySwapRouteStatus {
    pub swap_id: String,
    pub state: PubkySwapRouteState,
    pub provider_pubky: String,
    pub delivered_sat: u64,
    pub destination_address: String,
}
