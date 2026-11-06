//! Picks how to pay a Paykit contact. A contact the wallet can pay from its own balance is paid
//! directly, over Lightning first. Otherwise the cheapest service leg that delivers the exact
//! amount to an endpoint the contact published is used. The user never picks the route.

use super::errors::PaykitRouteError;
use super::types::{PaykitEndpoint, PaykitRoute, PaykitRouteRequest, ServiceRouteLeg};
use lightning_invoice::{Bolt11Invoice, Currency};
use std::str::FromStr;
use std::time::Duration;

/// Lightning Payment Endpoint Identifiers in the order Bitkit prefers to pay them.
const LIGHTNING_PREFERENCE: [&str; 2] = ["btc-lightning-bolt11", "btc-lightning-lnurl"];

/// On-chain address types in the order Bitkit prefers to pay them.
const ONCHAIN_PREFERENCE: [&str; 4] = ["p2tr", "p2wpkh", "p2sh", "p2pkh"];

/// The rail segment of an on-chain Payment Endpoint Identifier, as the apps publish it. The
/// spec has no Testnet4 rail, so a Testnet4 wallet routes nothing on chain.
fn network_label(network: bitcoin::Network) -> Option<&'static str> {
    match network {
        bitcoin::Network::Bitcoin => Some("bitcoin"),
        bitcoin::Network::Regtest => Some("regtest"),
        bitcoin::Network::Signet => Some("signet"),
        bitcoin::Network::Testnet => Some("testnet"),
        _ => None,
    }
}

/// The wallet's on-chain Payment Endpoint Identifiers, in the order Bitkit prefers to pay them.
fn onchain_method_ids(network: bitcoin::Network) -> Vec<String> {
    network_label(network)
        .map(|rail| {
            ONCHAIN_PREFERENCE
                .iter()
                .map(|kind| format!("btc-{rail}-{kind}"))
                .collect()
        })
        .unwrap_or_default()
}

fn preferred(
    endpoints: &[PaykitEndpoint],
    method_ids: impl IntoIterator<Item = String>,
) -> Option<&PaykitEndpoint> {
    method_ids
        .into_iter()
        .find_map(|id| endpoints.iter().find(|e| e.method_id == id))
}

/// Whether a Lightning endpoint's value can pay `amount_sat` now. A BOLT11 invoice must be for
/// the wallet's network, unexpired at `now_unix` and amountless or for exactly the amount. An
/// LNURL is only checked for a value, since checking it needs a network call.
fn lightning_payable(
    endpoint: &PaykitEndpoint,
    amount_sat: u64,
    network: bitcoin::Network,
    now_unix: u64,
) -> bool {
    if endpoint.method_id != "btc-lightning-bolt11" {
        return !endpoint.value.trim().is_empty();
    }
    let Ok(invoice) = Bolt11Invoice::from_str(endpoint.value.trim()) else {
        return false;
    };
    invoice.currency() == Currency::from(network)
        && !invoice.would_expire(Duration::from_secs(now_unix))
        && invoice
            .amount_milli_satoshis()
            .is_none_or(|msat| msat == amount_sat.saturating_mul(1000))
}

fn lightning_endpoint(
    endpoints: &[PaykitEndpoint],
    amount_sat: u64,
    network: bitcoin::Network,
    now_unix: u64,
) -> Option<&PaykitEndpoint> {
    LIGHTNING_PREFERENCE.iter().find_map(|id| {
        endpoints
            .iter()
            .find(|e| e.method_id == *id && lightning_payable(e, amount_sat, network, now_unix))
    })
}

fn onchain_endpoint(
    endpoints: &[PaykitEndpoint],
    network: bitcoin::Network,
) -> Option<&PaykitEndpoint> {
    preferred(endpoints, onchain_method_ids(network))
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
    let onchain = onchain_method_ids(network);
    legs.iter()
        .filter(|leg| endpoints.contains(&leg.endpoint))
        .filter(|leg| onchain.contains(&leg.endpoint.method_id))
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
            lightning_endpoint(endpoints, amount, network, now_unix),
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
    use bitcoin::hashes::{sha256, Hash as _};
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{InvoiceBuilder, PaymentSecret};

    const NOW: u64 = 1_000;

    /// A signed invoice created at `NOW - 10` that expires `expiry_secs` later.
    fn bolt11(currency: Currency, amount_msat: Option<u64>, expiry_secs: u64) -> String {
        let secp = Secp256k1::new();
        let key = SecretKey::from_slice(&[0xab; 32]).unwrap();
        let mut builder = InvoiceBuilder::new(currency)
            .payment_hash(sha256::Hash::from_byte_array([1u8; 32]))
            .payment_secret(PaymentSecret([2u8; 32]))
            .duration_since_epoch(Duration::from_secs(NOW - 10))
            .expiry_time(Duration::from_secs(expiry_secs))
            .min_final_cltv_expiry_delta(144)
            .description("paykit".into());
        if let Some(msat) = amount_msat {
            builder = builder.amount_milli_satoshis(msat);
        }
        builder
            .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &key))
            .unwrap()
            .to_string()
    }

    /// An endpoint the contact published; a BOLT11 endpoint holds a payable regtest invoice.
    fn endpoint(method_id: &str) -> PaykitEndpoint {
        let value = match method_id {
            "btc-lightning-bolt11" => bolt11(Currency::Regtest, Some(20_000_000), 3_600),
            _ => "v".into(),
        };
        PaykitEndpoint {
            method_id: method_id.into(),
            value,
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
    fn skips_bolt11_invoices_it_cannot_pay_now() {
        let unpayable = [
            bolt11(Currency::Bitcoin, Some(20_000_000), 3_600),
            bolt11(Currency::Regtest, Some(20_000_000), 5),
            bolt11(Currency::Regtest, Some(19_000_000), 3_600),
            "not an invoice".into(),
        ];
        for value in unpayable {
            let mut request = request(&["btc-regtest-p2wpkh"], 20_000, 20_000, vec![]);
            request.contact_endpoints.push(PaykitEndpoint {
                method_id: "btc-lightning-bolt11".into(),
                value,
            });
            assert_eq!(route(request), direct("btc-regtest-p2wpkh"));
        }
        let mut amountless = request(&[], 20_000, 0, vec![]);
        amountless.contact_endpoints.push(PaykitEndpoint {
            method_id: "btc-lightning-bolt11".into(),
            value: bolt11(Currency::Regtest, None, 3_600),
        });
        assert!(matches!(route(amountless), PaykitRoute::Direct { .. }));
        let mut empty_lnurl = request(&[], 20_000, 0, vec![]);
        empty_lnurl.contact_endpoints.push(PaykitEndpoint {
            method_id: "btc-lightning-lnurl".into(),
            value: " ".into(),
        });
        assert!(matches!(route(empty_lnurl), PaykitRoute::None { .. }));
    }

    #[test]
    fn ignores_legs_to_an_address_the_contact_did_not_publish() {
        let mut other = leg("btc-regtest-p2wpkh", 20_000, 20_001, 0);
        other.endpoint.value = "other".into();
        assert!(matches!(
            route(request(&["btc-regtest-p2wpkh"], 0, 0, vec![other])),
            PaykitRoute::None { .. }
        ));
    }

    #[test]
    fn pays_on_chain_endpoints_of_each_network_rail() {
        let rails = [
            (bitcoin::Network::Bitcoin, "btc-bitcoin-p2tr"),
            (bitcoin::Network::Testnet, "btc-testnet-p2tr"),
            (bitcoin::Network::Signet, "btc-signet-p2tr"),
            (bitcoin::Network::Regtest, "btc-regtest-p2tr"),
        ];
        let published: Vec<&str> = rails.iter().map(|(_, id)| *id).collect();
        for (network, id) in rails {
            let request = request(&published, 0, 20_000, vec![]);
            assert_eq!(route_payment(&request, network, NOW).unwrap(), direct(id));
        }
        let legs = vec![leg("btc-testnet-p2tr", 20_000, 20_001, 0)];
        let request = request(&published, 0, 20_000, legs);
        assert!(matches!(
            route_payment(&request, bitcoin::Network::Testnet4, NOW).unwrap(),
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
