//! Wallet-local route selection for a Paykit payment (Paykit Capabilities §9.1, one `swap/1`
//! leg at most).
//!
//! A contact the wallet can pay from its own balance is paid directly. Otherwise the router
//! turns each trusted provider's offer into `swap/1` edges, keeps the edges that connect a rail
//! the wallet holds to an endpoint the contact published, pre-ranks them by indicative fee,
//! asks the best few for a firm quote, and picks the one that debits the wallet least for the
//! exact amount the contact must receive.

use swap_common::messages::SwapOffer;
use swap_common::SwapDirection;

use super::types::{PaykitEndpoint, SwapRouteEdge};

pub(crate) const LIGHTNING_BOLT11: &str = "btc-lightning-bolt11";
pub(crate) const LIGHTNING_LNURL: &str = "btc-lightning-lnurl";
/// Providers asked for a firm quote per payment (§9.1 `fanout_budget`).
pub(crate) const FANOUT_BUDGET: usize = 3;

/// On-chain address types in the order Bitkit prefers to pay them.
const ONCHAIN_PREFERENCE: [&str; 4] = ["p2tr", "p2wpkh", "p2sh", "p2pkh"];

pub(crate) fn network_label(network: bitcoin::Network) -> &'static str {
    match network {
        bitcoin::Network::Bitcoin => "bitcoin",
        bitcoin::Network::Regtest => "regtest",
        bitcoin::Network::Signet => "signet",
        _ => "testnet",
    }
}

pub(crate) fn is_lightning(method_id: &str) -> bool {
    method_id == LIGHTNING_BOLT11 || method_id == LIGHTNING_LNURL
}

/// The contact's on-chain endpoint for `network`, preferring the cheapest address type.
pub(crate) fn onchain_endpoint(
    endpoints: &[PaykitEndpoint],
    network: bitcoin::Network,
) -> Option<PaykitEndpoint> {
    let prefix = format!("btc-{}-", network_label(network));
    ONCHAIN_PREFERENCE.iter().find_map(|kind| {
        endpoints
            .iter()
            .find(|e| e.method_id == format!("{prefix}{kind}"))
            .cloned()
    })
}

/// The `swap/1` edges an offer advertises (Paykit Capabilities §13.1), one per on-chain Payment
/// Endpoint Identifier the provider pays or accepts. The fee, minimum and `fund_within_secs` are
/// offer-wide, so every edge carries the same numbers.
pub(crate) fn edges_from_offer(offer: &SwapOffer) -> Vec<SwapRouteEdge> {
    let network = network_label(offer.network.to_bitcoin_network());
    let mut edges = vec![];
    for direction in &offer.directions {
        for kind in ONCHAIN_PREFERENCE {
            let onchain = format!("btc-{network}-{kind}");
            let (dir, from, to) = match direction {
                SwapDirection::Reverse => ("reverse", LIGHTNING_BOLT11.to_string(), onchain),
                SwapDirection::Submarine => ("submarine", onchain, LIGHTNING_BOLT11.to_string()),
            };
            edges.push(SwapRouteEdge {
                provider_pubky: offer.provider_pkarr.clone(),
                offer_id: offer.offer_id.to_string(),
                direction: dir.to_string(),
                from_endpoint: from,
                to_endpoint: to,
                min_sat: offer.effective_min_amount_sat(),
                max_sat: offer.max_amount_sat,
                base_fee_sat: offer.base_fee_sat,
                fee_ppm: offer.fee_ppm,
                onchain_fee_sat: offer.onchain_fee_sat,
                atomicity: "hash".to_string(),
                fund_within_secs: offer.fund_within_secs,
            });
        }
    }
    edges
}

/// Indicative fee of an edge for `amount_sat`: base + ppm + on-chain (§6.2).
pub(crate) fn indicative_fee(edge: &SwapRouteEdge, amount_sat: u64) -> u64 {
    let proportional = (u128::from(amount_sat) * u128::from(edge.fee_ppm) / 1_000_000) as u64;
    edge.base_fee_sat
        .saturating_add(proportional)
        .saturating_add(edge.onchain_fee_sat)
}

/// Reverse edges that deliver to `endpoint`, can lock `lock_sat`, stay within the client's fee
/// cap (swap-common refuses a quote above it) and that the wallet can afford over Lightning,
/// cheapest first, at most [`FANOUT_BUDGET`].
pub(crate) fn rank_reverse_edges(
    edges: Vec<SwapRouteEdge>,
    endpoint: &str,
    lock_sat: u64,
    lightning_spendable_sat: u64,
    max_fee_bps: u16,
) -> Vec<SwapRouteEdge> {
    let max_fee = (u128::from(lock_sat) * u128::from(max_fee_bps) / 10_000) as u64;
    let mut usable: Vec<_> = edges
        .into_iter()
        .filter(|e| e.direction == "reverse" && e.to_endpoint == endpoint)
        .filter(|e| lock_sat >= e.min_sat && lock_sat <= e.max_sat)
        .filter(|e| indicative_fee(e, lock_sat) <= max_fee)
        .filter(|e| lock_sat.saturating_add(indicative_fee(e, lock_sat)) <= lightning_spendable_sat)
        .collect();
    usable.sort_by_key(|e| indicative_fee(e, lock_sat));
    usable.truncate(FANOUT_BUDGET);
    usable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(provider: &str, direction: &str, base: u64, min: u64, max: u64) -> SwapRouteEdge {
        SwapRouteEdge {
            provider_pubky: provider.into(),
            offer_id: "o".into(),
            direction: direction.into(),
            from_endpoint: LIGHTNING_BOLT11.into(),
            to_endpoint: "btc-regtest-p2wpkh".into(),
            min_sat: min,
            max_sat: max,
            base_fee_sat: base,
            fee_ppm: 2500,
            onchain_fee_sat: 500,
            atomicity: "hash".into(),
            fund_within_secs: 300,
        }
    }

    fn endpoint(method_id: &str) -> PaykitEndpoint {
        PaykitEndpoint {
            method_id: method_id.into(),
            value: "v".into(),
        }
    }

    #[test]
    fn picks_preferred_onchain_endpoint_for_the_network() {
        let endpoints = vec![
            endpoint("btc-regtest-p2pkh"),
            endpoint("btc-bitcoin-p2tr"),
            endpoint("btc-regtest-p2wpkh"),
        ];
        let picked = onchain_endpoint(&endpoints, bitcoin::Network::Regtest).unwrap();
        assert_eq!(picked.method_id, "btc-regtest-p2wpkh");
        assert!(
            onchain_endpoint(&[endpoint(LIGHTNING_BOLT11)], bitcoin::Network::Regtest).is_none()
        );
    }

    #[test]
    fn ranks_reverse_edges_by_fee_within_bounds_and_budget() {
        let edges = vec![
            edge("expensive", "reverse", 900, 10_000, 1_000_000),
            edge("cheap", "reverse", 100, 10_000, 1_000_000),
            edge("submarine", "submarine", 0, 10_000, 1_000_000),
            edge("too-small", "reverse", 0, 10_000, 15_000),
            edge("min-too-high", "reverse", 0, 50_000, 1_000_000),
        ];
        let ranked = rank_reverse_edges(edges, "btc-regtest-p2wpkh", 20_000, 1_000_000, 10_000);
        let names: Vec<_> = ranked.iter().map(|e| e.provider_pubky.as_str()).collect();
        assert_eq!(names, ["cheap", "expensive"]);
    }

    #[test]
    fn drops_edges_the_wallet_cannot_afford() {
        let edges = vec![edge("p", "reverse", 100, 10_000, 1_000_000)];
        // 20,000 locked + 100 + 50 + 500 fee = 20,650.
        let rank = |spendable| {
            rank_reverse_edges(edges.clone(), "btc-regtest-p2wpkh", 20_000, spendable, 500)
        };
        assert!(rank(20_649).is_empty());
        assert_eq!(rank(20_650).len(), 1);
    }

    #[test]
    fn indicative_fee_matches_the_offer_formula() {
        let e = edge("p", "reverse", 100, 0, u64::MAX);
        assert_eq!(indicative_fee(&e, 100_000), 100 + 250 + 500);
    }

    #[test]
    fn drops_edges_above_the_client_fee_cap_and_other_endpoints() {
        // 100 + 50 + 500 = 650 sats on 20,000 is 3.25%: under the 5% cap, over a 3% one.
        let edges = vec![edge("p", "reverse", 100, 10_000, 1_000_000)];
        assert_eq!(
            rank_reverse_edges(edges.clone(), "btc-regtest-p2wpkh", 20_000, 1_000_000, 500).len(),
            1
        );
        assert!(
            rank_reverse_edges(edges.clone(), "btc-regtest-p2wpkh", 20_000, 1_000_000, 300)
                .is_empty()
        );
        assert!(rank_reverse_edges(edges, "btc-regtest-p2tr", 20_000, 1_000_000, 500).is_empty());
    }
}
