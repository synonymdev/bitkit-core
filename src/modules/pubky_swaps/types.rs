use serde::{Deserialize, Serialize};

pub use crate::modules::paykit_route::{
    PaykitEndpoint, PaykitRoute, PaykitRouteRequest, ServiceRouteLeg,
};

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
    /// Payment Endpoint Identifier the route delivers to, such as `btc-regtest-p2wpkh`.
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

/// A firm `swap/1` quote for one route, with everything the review screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PubkySwapRouteQuote {
    pub provider_pubky: String,
    /// The wallet's id for this route, which `pubky_swaps_commit_route` takes. The provider's own
    /// quote id stays inside bitkit-core.
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
    /// The claim to the contact's address is broadcast but not confirmed yet. The provider settles
    /// the invoice once it sees the preimage; keep calling until the claim confirms.
    ClaimBroadcast {
        txid: String,
    },
    /// The claim to the contact's address is confirmed.
    Claimed {
        txid: String,
    },
    Failed {
        reason: String,
    },
}

/// A Lightning payment the wallet's node sent, which `pubky_swaps_restore` matches to a listed
/// swap by its hash.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PubkySwapSentPayment {
    pub payment_hash_hex: String,
    /// What the payment paid the provider, routing fees excluded.
    pub amount_sat: u64,
}

/// What `pubky_swaps_restore` found, by swap id.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct PubkySwapRestoreReport {
    /// Swaps rebuilt now; drive each with `pubky_swaps_advance`.
    pub restored: Vec<String>,
    /// Swaps already on record, listed again; drive the unfinished ones with `pubky_swaps_advance`.
    pub known: Vec<String>,
    /// Swaps the provider lists as finished, which get no record.
    pub finished: Vec<String>,
    /// Providers whose list was not read in full; restore again later to finish it.
    pub incomplete_providers: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PubkySwapRouteStatus {
    pub swap_id: String,
    pub state: PubkySwapRouteState,
    pub provider_pubky: String,
    pub delivered_sat: u64,
    pub destination_address: String,
}
