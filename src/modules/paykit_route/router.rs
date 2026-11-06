//! Picks how to pay a Paykit contact. A contact the wallet can pay from its own balance is paid
//! directly, over Lightning first. Otherwise the cheapest service leg that delivers the exact
//! amount to an endpoint the contact published is used. The user never picks the route.

use super::errors::PaykitRouteError;
use super::types::{PaykitEndpoint, PaykitRoute, PaykitRouteRequest, ServiceRouteLeg};

/// Lightning Payment Endpoint Identifiers in the order Bitkit prefers to pay them.
const LIGHTNING_PREFERENCE: [&str; 2] = ["btc-lightning-bolt11", "btc-lightning-lnurl"];

/// On-chain address types in the order Bitkit prefers to pay them.
const ONCHAIN_PREFERENCE: [&str; 4] = ["p2tr", "p2wpkh", "p2sh", "p2pkh"];

/// The rail segment of an on-chain Payment Endpoint Identifier, as the apps publish it.
fn network_label(network: bitcoin::Network) -> &'static str {
    match network {
        bitcoin::Network::Bitcoin => "bitcoin",
        bitcoin::Network::Regtest => "regtest",
        bitcoin::Network::Signet => "signet",
        _ => "testnet",
    }
}

fn preferred(
    endpoints: &[PaykitEndpoint],
    method_ids: impl IntoIterator<Item = String>,
) -> Option<&PaykitEndpoint> {
    method_ids
        .into_iter()
        .find_map(|id| endpoints.iter().find(|e| e.method_id == id))
}

fn lightning_endpoint(endpoints: &[PaykitEndpoint]) -> Option<&PaykitEndpoint> {
    preferred(endpoints, LIGHTNING_PREFERENCE.map(String::from))
}

fn onchain_endpoint(
    endpoints: &[PaykitEndpoint],
    network: bitcoin::Network,
) -> Option<&PaykitEndpoint> {
    let rail = network_label(network);
    preferred(
        endpoints,
        ONCHAIN_PREFERENCE.map(|kind| format!("btc-{rail}-{kind}")),
    )
}

/// The cheapest service leg that delivers exactly `amount_sat` to an on-chain endpoint for
/// `network` the contact published and whose quote still holds at `now_unix`.
fn best_service_leg(
    legs: &[ServiceRouteLeg],
    endpoints: &[PaykitEndpoint],
    amount_sat: u64,
    network: bitcoin::Network,
    now_unix: u64,
) -> Option<ServiceRouteLeg> {
    let rail = network_label(network);
    legs.iter()
        .filter(|leg| endpoints.contains(&leg.endpoint))
        .filter(|leg| {
            ONCHAIN_PREFERENCE
                .iter()
                .any(|kind| leg.endpoint.method_id == format!("btc-{rail}-{kind}"))
        })
        .filter(|leg| leg.delivered_sat == amount_sat && leg.debit_sat >= leg.delivered_sat)
        .filter(|leg| leg.valid_until_unix == 0 || leg.valid_until_unix > now_unix)
        .min_by_key(|leg| leg.debit_sat - leg.delivered_sat)
        .cloned()
}

/// Pick the route for paying `request.amount_sat` to a contact at `now_unix`.
pub fn route_payment(
    request: &PaykitRouteRequest,
    network: bitcoin::Network,
    now_unix: u64,
) -> Result<PaykitRoute, PaykitRouteError> {
    let amount = request.amount_sat;
    if amount == 0 {
        return Err(PaykitRouteError::InvalidRequest {
            error_details: "amount must be positive".into(),
        });
    }
    let endpoints = &request.contact_endpoints;
    let direct = [
        (
            lightning_endpoint(endpoints),
            request.lightning_spendable_sat,
        ),
        (
            onchain_endpoint(endpoints, network),
            request.onchain_spendable_sat,
        ),
    ];
    if let Some((Some(endpoint), _)) = direct
        .iter()
        .find(|(endpoint, spendable)| endpoint.is_some() && *spendable >= amount)
    {
        return Ok(PaykitRoute::Direct {
            endpoint: (*endpoint).clone(),
        });
    }
    Ok(
        match best_service_leg(&request.service_legs, endpoints, amount, network, now_unix) {
            Some(leg) => PaykitRoute::Service { leg },
            None => PaykitRoute::None {
                reason: "No balance can reach an endpoint this contact accepts.".into(),
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000;

    fn endpoint(method_id: &str) -> PaykitEndpoint {
        PaykitEndpoint {
            method_id: method_id.into(),
            value: "v".into(),
        }
    }

    fn leg(method_id: &str, delivered: u64, debit: u64, until: u64) -> ServiceRouteLeg {
        ServiceRouteLeg {
            service: "spark".into(),
            endpoint: endpoint(method_id),
            delivered_sat: delivered,
            debit_sat: debit,
            atomicity: "trusted".into(),
            valid_until_unix: until,
        }
    }

    fn request(
        endpoints: &[&str],
        lightning: u64,
        onchain: u64,
        legs: Vec<ServiceRouteLeg>,
    ) -> PaykitRouteRequest {
        PaykitRouteRequest {
            amount_sat: 20_000,
            contact_endpoints: endpoints.iter().map(|id| endpoint(id)).collect(),
            lightning_spendable_sat: lightning,
            onchain_spendable_sat: onchain,
            service_legs: legs,
        }
    }

    fn route(request: PaykitRouteRequest) -> PaykitRoute {
        route_payment(&request, bitcoin::Network::Regtest, NOW).unwrap()
    }

    fn direct(method_id: &str) -> PaykitRoute {
        PaykitRoute::Direct {
            endpoint: endpoint(method_id),
        }
    }

    #[test]
    fn pays_lightning_first_then_the_preferred_onchain_endpoint() {
        let all = [
            "btc-regtest-p2pkh",
            "btc-bitcoin-p2tr",
            "btc-regtest-p2wpkh",
            "btc-lightning-lnurl",
            "btc-lightning-bolt11",
        ];
        assert_eq!(
            route(request(&all, 20_000, 20_000, vec![])),
            direct("btc-lightning-bolt11")
        );
        assert_eq!(
            route(request(&all[..4], 20_000, 0, vec![])),
            direct("btc-lightning-lnurl")
        );
        assert_eq!(
            route(request(&all, 19_999, 20_000, vec![])),
            direct("btc-regtest-p2wpkh")
        );
    }

    #[test]
    fn pays_directly_before_any_service_leg() {
        let legs = vec![leg("btc-regtest-p2tr", 20_000, 20_000, 0)];
        assert_eq!(
            route(request(&["btc-regtest-p2tr"], 0, 20_000, legs)),
            direct("btc-regtest-p2tr")
        );
    }

    #[test]
    fn withdraws_through_the_cheapest_live_leg_to_a_published_endpoint() {
        let legs = vec![
            leg("btc-regtest-p2wpkh", 20_000, 20_900, 0),
            leg("btc-regtest-p2wpkh", 20_000, 20_400, NOW + 1),
            leg("btc-regtest-p2tr", 20_000, 20_001, 0),
            leg("btc-regtest-p2wpkh", 20_000, 20_100, NOW),
            leg("btc-regtest-p2wpkh", 19_000, 19_100, 0),
            leg("btc-regtest-p2wpkh", 20_000, 19_999, 0),
        ];
        let PaykitRoute::Service { leg } = route(request(&["btc-regtest-p2wpkh"], 0, 0, legs))
        else {
            panic!("expected a service route");
        };
        assert_eq!(leg.debit_sat, 20_400);
    }

    #[test]
    fn ignores_legs_to_endpoints_off_this_network_or_off_chain() {
        let legs = vec![
            leg("btc-bitcoin-p2tr", 20_000, 20_001, 0),
            leg("btc-lightning-bolt11", 20_000, 20_001, 0),
        ];
        let published = ["btc-bitcoin-p2tr", "btc-lightning-bolt11"];
        assert!(matches!(
            route(request(&published, 0, 0, legs)),
            PaykitRoute::None { .. }
        ));
    }

    #[test]
    fn explains_when_nothing_reaches_the_contact() {
        assert_eq!(
            route(request(&["btc-lightning-bolt11"], 19_999, 0, vec![])),
            PaykitRoute::None {
                reason: "No balance can reach an endpoint this contact accepts.".into()
            }
        );
        assert!(route_payment(
            &PaykitRouteRequest {
                amount_sat: 0,
                ..request(&[], 0, 0, vec![])
            },
            bitcoin::Network::Regtest,
            NOW
        )
        .is_err());
    }
}
