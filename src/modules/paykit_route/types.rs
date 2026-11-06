use serde::{Deserialize, Serialize};

use crate::modules::pubky_swaps::PubkySwapRouteQuote;

/// A Payment Endpoint the contact published: its Payment Endpoint Identifier, such as
/// `btc-lightning-bolt11` or `btc-bitcoin-p2tr`, and the value a payer pays to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct PaykitEndpoint {
    pub method_id: String,
    pub value: String,
}

/// What the wallet knows when it pays a Paykit contact: the amount, the endpoints the contact
/// published and what the wallet can spend on each rail.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PaykitRouteRequest {
    pub amount_sat: u64,
    /// The contact's current Payment Endpoints, as Paykit resolved them.
    pub contact_endpoints: Vec<PaykitEndpoint>,
    /// What the wallet can pay over Lightning, its Spark balance included, net of the fees a
    /// payment of `amount_sat` costs.
    pub lightning_spendable_sat: u64,
    /// What the wallet can pay on chain, net of the miner fee a payment of `amount_sat` costs.
    pub onchain_spendable_sat: u64,
    /// Legs through services the wallet holds a balance on, such as a Spark withdrawal to the
    /// contact's on-chain address, each already quoted by the app's engine for that service.
    pub service_legs: Vec<ServiceRouteLeg>,
    /// Candidate Pubky Swaps providers as pubkys, usually the user's contacts. Only
    /// `pubky_swaps_route_paykit_payment` asks them; a contact that runs no provider is skipped.
    pub providers: Vec<String>,
    /// Fee rate for the wallet's own claim transaction on a swap route.
    pub fee_rate_sat_vb: u64,
}

/// A one-leg route through a service the wallet holds a balance on, such as a Spark withdrawal
/// to the contact's on-chain address. The app quotes it and runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ServiceRouteLeg {
    /// The service, such as `spark`.
    pub service: String,
    /// The contact endpoint the service delivers to. It must be one the contact published.
    pub endpoint: PaykitEndpoint,
    /// What the contact receives.
    pub delivered_sat: u64,
    /// What the wallet's balance on the service is debited.
    pub debit_sat: u64,
    /// Who the wallet trusts to complete the leg: `trusted` for Spark's operators.
    pub atomicity: String,
    /// When the quote stops holding, or 0 when it does not expire.
    pub valid_until_unix: u64,
}

/// The route the router picked.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PaykitRoute {
    /// The wallet can pay this endpoint from its own balance.
    Direct { endpoint: PaykitEndpoint },
    /// A service the wallet holds a balance on delivers to the contact. The app runs the leg.
    Service { leg: ServiceRouteLeg },
    /// The wallet pays over Lightning and a Pubky Swaps provider delivers on-chain.
    Swap { quote: PubkySwapRouteQuote },
    /// Nothing can pay this contact. `reason` says why, for the user.
    None { reason: String },
}
