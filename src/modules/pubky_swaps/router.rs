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

/// The `swap/1` edges an offer advertises (Paykit Capabilities §13.1). The fee, minimum and
/// `fund_within_secs` are offer-wide, so both directions carry the same numbers.
pub(crate) fn edges_from_offer(offer: &SwapOffer) -> Vec<SwapRouteEdge> {
    let onchain = format!(
        "btc-{}-*",
        network_label(offer.network.to_bitcoin_network())
    );
    offer
        .directions
        .iter()
        .map(|direction| {
            let (dir, from, to) = match direction {
                SwapDirection::Reverse => {
                    ("reverse", LIGHTNING_BOLT11.to_string(), onchain.clone())
                }
                SwapDirection::Submarine => {
                    ("submarine", onchain.clone(), LIGHTNING_BOLT11.to_string())
                }
            };
            SwapRouteEdge {
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
            }
        })
        .collect()
}

/// Indicative fee of an edge for `amount_sat`: base + ppm + on-chain (§6.2).
pub(crate) fn indicative_fee(edge: &SwapRouteEdge, amount_sat: u64) -> u64 {
    let proportional = (u128::from(amount_sat) * u128::from(edge.fee_ppm) / 1_000_000) as u64;
    edge.base_fee_sat
        .saturating_add(proportional)
        .saturating_add(edge.onchain_fee_sat)
}

/// Reverse edges that can lock `lock_sat` and that the wallet can afford over Lightning,
/// cheapest first, at most [`FANOUT_BUDGET`].
pub(crate) fn rank_reverse_edges(
    edges: Vec<SwapRouteEdge>,
    lock_sat: u64,
    lightning_spendable_sat: u64,
) -> Vec<SwapRouteEdge> {
    let mut usable: Vec<_> = edges
        .into_iter()
        .filter(|e| e.direction == "reverse")
        .filter(|e| lock_sat >= e.min_sat && lock_sat <= e.max_sat)
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
            to_endpoint: "btc-regtest-*".into(),
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
        let ranked = rank_reverse_edges(edges, 20_000, 1_000_000);
        let names: Vec<_> = ranked.iter().map(|e| e.provider_pubky.as_str()).collect();
        assert_eq!(names, ["cheap", "expensive"]);
    }

    #[test]
    fn drops_edges_the_wallet_cannot_afford() {
        let edges = vec![edge("p", "reverse", 100, 10_000, 1_000_000)];
        // 20,000 locked + 100 + 50 + 500 fee = 20,650.
        assert!(rank_reverse_edges(edges.clone(), 20_000, 20_649).is_empty());
        assert_eq!(rank_reverse_edges(edges, 20_000, 20_650).len(), 1);
    }

    #[test]
    fn indicative_fee_matches_the_offer_formula() {
        let e = edge("p", "reverse", 100, 0, u64::MAX);
        assert_eq!(indicative_fee(&e, 100_000), 100 + 250 + 500);
    }
}
