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

use super::types::{
    PaykitEndpoint, PaykitRoute, PubkySwapRouteQuote, ServiceRouteLeg, SwapRouteEdge,
};

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

/// The cheapest service leg that delivers exactly `amount_sat` to an endpoint the contact
/// published and whose quote still holds at `now_unix`.
pub(crate) fn best_service_leg(
    legs: &[ServiceRouteLeg],
    endpoints: &[PaykitEndpoint],
    amount_sat: u64,
    now_unix: u64,
) -> Option<ServiceRouteLeg> {
    legs.iter()
        .filter(|leg| endpoints.contains(&leg.endpoint))
        .filter(|leg| leg.delivered_sat == amount_sat && leg.debit_sat >= leg.delivered_sat)
        .filter(|leg| leg.valid_until_unix == 0 || leg.valid_until_unix > now_unix)
        .min_by_key(|leg| leg.debit_sat - leg.delivered_sat)
        .cloned()
}

/// The route that costs the wallet less in fees. A hash-locked swap wins a tie: the wallet
/// trusts no one to complete it.
pub(crate) fn cheaper_route(
    service: Option<ServiceRouteLeg>,
    swap: Option<PubkySwapRouteQuote>,
) -> Option<PaykitRoute> {
    match (service, swap) {
        (Some(leg), Some(quote)) if quote.total_fee_sat <= leg.debit_sat - leg.delivered_sat => {
            Some(PaykitRoute::Swap { quote })
        }
        (Some(leg), _) => Some(PaykitRoute::Service { leg }),
        (None, Some(quote)) => Some(PaykitRoute::Swap { quote }),
        (None, None) => None,
    }
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

    fn leg(
        service: &str,
        method_id: &str,
        delivered: u64,
        debit: u64,
        until: u64,
    ) -> ServiceRouteLeg {
        ServiceRouteLeg {
            service: service.into(),
            endpoint: endpoint(method_id),
            delivered_sat: delivered,
            debit_sat: debit,
            atomicity: "trusted".into(),
            valid_until_unix: until,
        }
    }

    #[test]
    fn picks_the_cheapest_service_leg_to_a_published_endpoint() {
        let endpoints = vec![endpoint("btc-regtest-p2wpkh")];
        let legs = vec![
            leg("spark", "btc-regtest-p2wpkh", 20_000, 20_900, 0),
            leg("spark", "btc-regtest-p2wpkh", 20_000, 20_400, 0),
            leg("unpublished", "btc-regtest-p2tr", 20_000, 20_001, 0),
        ];
        let picked = best_service_leg(&legs, &endpoints, 20_000, 100).unwrap();
        assert_eq!(picked.debit_sat, 20_400);
    }

    #[test]
    fn drops_service_legs_that_are_expired_or_deliver_another_amount() {
        let endpoints = vec![endpoint("btc-regtest-p2wpkh")];
        let legs = vec![
            leg("expired", "btc-regtest-p2wpkh", 20_000, 20_100, 100),
            leg("short", "btc-regtest-p2wpkh", 19_000, 19_100, 0),
            leg("negative-fee", "btc-regtest-p2wpkh", 20_000, 19_999, 0),
        ];
        assert!(best_service_leg(&legs, &endpoints, 20_000, 100).is_none());
        let live = leg("live", "btc-regtest-p2wpkh", 20_000, 20_100, 101);
        assert_eq!(
            best_service_leg(std::slice::from_ref(&live), &endpoints, 20_000, 100),
            Some(live)
        );
    }

    fn swap_quote(total_fee_sat: u64) -> PubkySwapRouteQuote {
        PubkySwapRouteQuote {
            provider_pubky: "p".into(),
            quote_id: "q".into(),
            endpoint: endpoint("btc-regtest-p2wpkh"),
            delivered_sat: 20_000,
            lock_sat: 20_200,
            debit_sat: 20_000 + total_fee_sat,
            service_fee_sat: 0,
            provider_onchain_fee_sat: 0,
            claim_fee_sat: 200,
            total_fee_sat,
            atomicity: "hash".into(),
            valid_until_unix: 0,
            providers_quoted: 1,
        }
    }

    #[test]
    fn picks_the_route_with_the_lower_fee_and_the_swap_on_a_tie() {
        let service = leg("spark", "btc-regtest-p2wpkh", 20_000, 20_500, 0);
        let pick = |swap_fee| cheaper_route(Some(service.clone()), Some(swap_quote(swap_fee)));
        assert!(matches!(pick(499), Some(PaykitRoute::Swap { .. })));
        assert!(matches!(pick(500), Some(PaykitRoute::Swap { .. })));
        assert!(matches!(pick(501), Some(PaykitRoute::Service { .. })));
        assert!(matches!(
            cheaper_route(Some(service), None),
            Some(PaykitRoute::Service { .. })
        ));
        assert!(cheaper_route(None, None).is_none());
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
