//! The `swap/1` flow for a Paykit payment: route, commit, claim.
//!
//! Message mapping (Paykit Capabilities §13.1): `rfq` is pubky-swap's `QuoteRequest`, `quote`
//! is `Quote`, `commit` is `SwapRequest`, `contract` is `SwapAccept`, `refusal` is `Reject`.
//! Here they travel over iroh `direct/1`, which §13.1 allows for the same payloads.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::{Address, ScriptBuf};
use electrum_client::{Client as ElectrumClient, ElectrumApi};
use once_cell::sync::Lazy;
use swap_common::htlc::build_htlc_script;
use swap_common::messages::{
    OfferRequest, Quote, QuoteRequest, Reject, SwapMessage, SwapOffer, SwapRequest, SwapScript,
    SwapStatusRequest, PROTOCOL_VERSION,
};
use swap_common::onchain::{build_claim_tx, estimate_spend_fee, spend_vsize};
use swap_common::timelock::TimelockParams;
use swap_common::validate::{
    validate_accept, validate_hold_invoice, validate_quote, verify_contract, ClientPolicy,
    DecodedHoldInvoice,
};
use swap_common::SwapDirection;
use uuid::Uuid;

use super::claim;
use super::errors::PubkySwapError;
use super::keys;
use super::router::{self, is_lightning};
use super::store::{self, SwapRecord};
use super::transport::{bare_pubky, DirectClient};
use super::types::*;

/// How long a candidate provider has to answer an offer request.
const OFFER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Most candidates asked for an offer per payment: each is a connection and an address lookup
/// through iroh's n0 discovery, which sees which keys the wallet looks up.
const MAX_CANDIDATES: usize = 30;
/// How long the wallet keeps asking about a swap request the provider is still admitting.
const PENDING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const PENDING_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Quotes the router fetched, kept until the wallet commits to one of them.
static QUOTES: Lazy<Mutex<HashMap<String, CachedQuote>>> = Lazy::new(Default::default);

#[derive(Clone)]
struct CachedQuote {
    route: PubkySwapRouteQuote,
    quote: Quote,
    network: bitcoin::Network,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn policy(network: bitcoin::Network) -> ClientPolicy {
    ClientPolicy::for_network(network, TimelockParams::default())
}

fn refused(reject: Reject) -> PubkySwapError {
    PubkySwapError::Refused {
        code: reject.code.unwrap_or_else(|| "unknown".into()),
        reason: reject.reason,
    }
}

fn unexpected(what: &str, got: &SwapMessage) -> PubkySwapError {
    PubkySwapError::transport(format!("expected {what}, got {got:?}"))
}

fn identity_pubky(secret: &[u8; 32]) -> String {
    pubky::Keypair::from_secret(secret).public_key().to_z32()
}

fn parse_address(address: &str, network: bitcoin::Network) -> Result<Address, PubkySwapError> {
    Address::from_str(address)
        .map_err(|e| PubkySwapError::invalid(format!("address: {e}")))?
        .require_network(network)
        .map_err(|e| PubkySwapError::invalid(format!("address: {e}")))
}

/// The wallet's fee for claiming the provider's lockup to `destination`.
fn claim_fee(destination: &ScriptBuf, fee_rate_sat_vb: u64) -> Result<u64, PubkySwapError> {
    // Only the shape matters for the size: a real contract has the same lengths.
    let key = keys::swap_keys(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        None,
        0,
    )?;
    let script = build_htlc_script(&[0; 32], &key.claim_pubkey, &key.claim_pubkey, 1_000_000);
    Ok(estimate_spend_fee(
        fee_rate_sat_vb.max(1),
        spend_vsize(&script, destination, true),
    ))
}

async fn fetch_offer(client: &DirectClient, provider: &str) -> Result<SwapOffer, PubkySwapError> {
    let request = SwapMessage::OfferRequest(OfferRequest {
        request_id: Some(Uuid::new_v4()),
    });
    match client.request(provider, &request).await? {
        SwapMessage::Offer(offer) => Ok(offer),
        SwapMessage::Reject(reject) => Err(refused(reject)),
        other => Err(unexpected("Offer", &other)),
    }
}

/// The `swap/1` edges a provider advertises right now.
pub async fn provider_edges(
    provider: String,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<Vec<SwapRouteEdge>, PubkySwapError> {
    let secret = keys::identity_secret(&mnemonic, bip39_passphrase.as_deref())?;
    let client = DirectClient::bind(secret).await?;
    let offer = fetch_offer(&client, bare_pubky(&provider)).await;
    client.close().await;
    Ok(router::edges_from_offer(&offer?))
}

async fn quote_edge(
    client: &DirectClient,
    identity: &str,
    offer: &SwapOffer,
    lock_sat: u64,
    network: bitcoin::Network,
) -> Result<Quote, PubkySwapError> {
    let request = QuoteRequest {
        request_id: Some(Uuid::new_v4()),
        offer_id: offer.offer_id,
        client_pkarr: identity.to_string(),
        direction: SwapDirection::Reverse,
        amount_sat: lock_sat,
        protocol_version: PROTOCOL_VERSION,
        features: vec![],
    };
    let reply = client
        .request(
            &offer.provider_pkarr,
            &SwapMessage::QuoteRequest(request.clone()),
        )
        .await?;
    let quote = match reply {
        SwapMessage::Quote(quote) => quote,
        SwapMessage::Reject(reject) => return Err(refused(reject)),
        other => return Err(unexpected("Quote", &other)),
    };
    validate_quote(&quote, &request, now_unix(), &policy(network))
        .map_err(PubkySwapError::validation)?;
    Ok(quote)
}

/// Pick the route for a Paykit payment. The user never picks a swap: one is used only when no
/// rail the wallet holds reaches an endpoint the contact published.
pub async fn route_paykit_payment(
    request: PaykitRouteRequest,
    network: bitcoin::Network,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<PaykitRoute, PubkySwapError> {
    let amount = request.amount_sat;
    if amount == 0 {
        return Err(PubkySwapError::invalid("amount must be positive"));
    }
    let endpoints = &request.contact_endpoints;
    if let Some(lightning) = endpoints.iter().find(|e| is_lightning(&e.method_id)) {
        if request.lightning_spendable_sat >= amount {
            return Ok(PaykitRoute::Direct {
                endpoint: lightning.clone(),
            });
        }
    }
    let onchain = router::onchain_endpoint(endpoints, network);
    if let Some(onchain) = &onchain {
        if request.onchain_spendable_sat >= amount {
            return Ok(PaykitRoute::Direct {
                endpoint: onchain.clone(),
            });
        }
    }
    let service = router::best_service_leg(&request.service_legs, endpoints, amount, now_unix());
    let swap = match &onchain {
        Some(onchain) if !request.providers.is_empty() => {
            match best_swap_route(
                &request,
                onchain.clone(),
                network,
                &mnemonic,
                bip39_passphrase.as_deref(),
            )
            .await
            {
                Ok(swap) => swap,
                Err(e) if service.is_some() => {
                    log::warn!("pubky swaps: no swap route, paying through a service: {e}");
                    None
                }
                Err(e) => return Err(e),
            }
        }
        _ => None,
    };
    Ok(
        router::cheaper_route(service, swap).unwrap_or_else(|| PaykitRoute::None {
            reason: if onchain.is_none() {
                "No balance can reach an endpoint this contact accepts."
            } else if request.providers.is_empty() {
                "No swap provider is set up."
            } else if network == bitcoin::Network::Bitcoin {
                "Swap routes are not available on mainnet yet."
            } else {
                "No swap provider can route this payment right now."
            }
            .into(),
        }),
    )
}

/// The cheapest firm quote from the trusted Pubky Swaps providers for delivering the request's
/// amount to `onchain`, cached for `commit_route`.
async fn best_swap_route(
    request: &PaykitRouteRequest,
    onchain: PaykitEndpoint,
    network: bitcoin::Network,
    mnemonic: &str,
    bip39_passphrase: Option<&str>,
) -> Result<Option<PubkySwapRouteQuote>, PubkySwapError> {
    // The claim is broadcast at one fee and only rebroadcast, never bumped: until it can be, a
    // claim stuck below the market could lose the whole debit, so mainnet takes no swap route.
    // Service legs still route there.
    if network == bitcoin::Network::Bitcoin {
        return Ok(None);
    }
    let amount = request.amount_sat;
    let destination = parse_address(&onchain.value, network)?.script_pubkey();
    let claim_fee_sat = claim_fee(&destination, request.fee_rate_sat_vb)?;
    let lock_sat = amount + claim_fee_sat;

    let secret = keys::identity_secret(mnemonic, bip39_passphrase)?;
    let identity = identity_pubky(&secret);
    let client = DirectClient::bind(secret).await?;

    let mut offers = HashMap::new();
    let mut edges = vec![];
    // Candidates are usually the user's contacts, and most of them run no provider: ask them all at
    // once and give each a short window to answer.
    let mut probes = tokio::task::JoinSet::new();
    // In the caller's order, so the cap keeps configured providers listed first.
    let mut seen = std::collections::HashSet::new();
    let candidates: Vec<String> = request
        .providers
        .iter()
        .map(|p| bare_pubky(p).to_string())
        .filter(|p| seen.insert(p.clone()))
        .take(MAX_CANDIDATES)
        .collect();
    for provider in candidates {
        let client = client.clone();
        probes.spawn(async move {
            let offer = tokio::time::timeout(OFFER_TIMEOUT, fetch_offer(&client, &provider)).await;
            (provider, offer)
        });
    }
    while let Some(probe) = probes.join_next().await {
        let Ok((provider, offer)) = probe else {
            continue;
        };
        match offer {
            Ok(Ok(offer)) if bare_pubky(&offer.provider_pkarr) != provider => {
                log::warn!("pubky swaps: {provider} answered with another provider's offer")
            }
            Ok(Ok(offer)) if offer.network.to_bitcoin_network() == network => {
                edges.extend(router::edges_from_offer(&offer));
                offers.insert(offer.provider_pkarr.clone(), offer);
            }
            Ok(Ok(_)) => log::warn!("pubky swaps: provider {provider} is on another network"),
            Ok(Err(e)) => log::debug!("pubky swaps: no offer from {provider}: {e}"),
            Err(_) => log::debug!("pubky swaps: {provider} did not answer in time"),
        }
    }
    let ranked = router::rank_reverse_edges(
        edges,
        &onchain.method_id,
        lock_sat,
        request.lightning_spendable_sat,
        policy(network).max_fee_bps,
    );
    let asked = ranked.len() as u32;

    let mut best: Option<(Quote, String)> = None;
    for edge in &ranked {
        let Some(offer) = offers.get(&edge.provider_pubky) else {
            continue;
        };
        match quote_edge(&client, &identity, offer, lock_sat, network).await {
            Ok(quote) if quote.total_sat <= request.lightning_spendable_sat => {
                if best
                    .as_ref()
                    .is_none_or(|(b, _)| quote.total_sat < b.total_sat)
                {
                    best = Some((quote, edge.provider_pubky.clone()));
                }
            }
            Ok(_) => {}
            Err(e) => log::warn!("pubky swaps: no quote from {}: {e}", edge.provider_pubky),
        }
    }
    client.close().await;

    let Some((quote, provider)) = best else {
        return Ok(None);
    };
    let route = PubkySwapRouteQuote {
        provider_pubky: provider,
        // The wallet's own id for this route: two providers may pick the same quote id.
        quote_id: Uuid::new_v4().to_string(),
        endpoint: onchain,
        delivered_sat: amount,
        lock_sat,
        debit_sat: quote.total_sat,
        service_fee_sat: quote.service_fee_sat,
        provider_onchain_fee_sat: quote.onchain_fee_sat,
        claim_fee_sat,
        total_fee_sat: quote.total_sat.saturating_sub(amount),
        atomicity: "hash".into(),
        valid_until_unix: quote.valid_until_unix,
        providers_quoted: asked,
    };
    let mut quotes = QUOTES.lock().unwrap();
    let now = now_unix();
    quotes.retain(|_, cached| !cached.quote.is_expired(now));
    quotes.insert(
        route.quote_id.clone(),
        CachedQuote {
            route: route.clone(),
            quote,
            network,
        },
    );
    Ok(Some(route))
}

fn is_pending(reply: &Result<SwapMessage, PubkySwapError>) -> bool {
    matches!(reply, Ok(SwapMessage::Reject(reject)) if reject.code.as_deref() == Some("pending"))
}

fn decode_hold_invoice(invoice: &str) -> Result<DecodedHoldInvoice, PubkySwapError> {
    let invoice = lightning_invoice::Bolt11Invoice::from_str(invoice)
        .map_err(|e| PubkySwapError::validation(format!("hold invoice: {e}")))?;
    let amount_msat = invoice.amount_milli_satoshis();
    let expires_at = invoice
        .expires_at()
        .map(|d| d.as_secs())
        .unwrap_or_default();
    Ok(DecodedHoldInvoice {
        payment_hash: *invoice.payment_hash().as_ref(),
        amount_msat: amount_msat.unwrap_or_default(),
        amount_is_explicit: amount_msat.is_some(),
        expires_at_unix: expires_at,
    })
}

fn chain_tip(electrum_url: &str) -> Result<u32, PubkySwapError> {
    let client = ElectrumClient::new(electrum_url)?;
    Ok(client.block_headers_subscribe()?.height as u32)
}

/// Commit to a routed quote. Returns the hold invoice the wallet pays over Lightning; nothing
/// is funded before that payment.
pub async fn commit_route(
    quote_id: String,
    electrum_url: String,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<PubkySwapRoutePayment, PubkySwapError> {
    let cached = QUOTES
        .lock()
        .unwrap()
        .get(&quote_id)
        .cloned()
        .ok_or_else(|| PubkySwapError::NotFound {
            error_details: format!("quote {quote_id}"),
        })?;
    let CachedQuote {
        route,
        quote,
        network,
    } = cached;
    if network == bitcoin::Network::Bitcoin {
        return Err(PubkySwapError::invalid(
            "Swap routes are not available on mainnet yet.",
        ));
    }
    if quote.is_expired(now_unix()) {
        return Err(PubkySwapError::Refused {
            code: "quote_expired".into(),
            reason: "The quote expired. Route the payment again.".into(),
        });
    }
    let passphrase = bip39_passphrase.as_deref();
    let secret = keys::identity_secret(&mnemonic, passphrase)?;
    let index = store::reserve_index(now_unix())?;
    let swap_keys = keys::swap_keys(&mnemonic, passphrase, index)?;
    let request = SwapRequest {
        script_type: SwapScript::P2wsh,
        quote_id: quote.quote_id,
        client_pkarr: identity_pubky(&secret),
        direction: SwapDirection::Reverse,
        payment_hash_hex: hex::encode(swap_keys.payment_hash),
        client_claim_pubkey_hex: Some(swap_keys.claim_pubkey.to_string()),
        client_refund_pubkey_hex: None,
        invoice: None,
    };

    let client = DirectClient::bind(secret).await?;
    let mut reply = client
        .request(&route.provider_pubky, &SwapMessage::SwapRequest(request))
        .await;
    // `pending` means the provider is still admitting it, never a decline: keep asking until it
    // accepts or refuses. Past the deadline the wallet gives up without paying anything.
    let deadline = tokio::time::Instant::now() + PENDING_TIMEOUT;
    while is_pending(&reply) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(PENDING_POLL).await;
        let status = SwapMessage::SwapStatusRequest(SwapStatusRequest {
            request_id: Some(Uuid::new_v4()),
            swap_id: None,
            quote_id: Some(quote.quote_id),
        });
        reply = client.request(&route.provider_pubky, &status).await;
    }
    client.close().await;
    let accept = match reply? {
        SwapMessage::SwapAccept(accept) => accept,
        SwapMessage::SwapStatusSnapshot(snapshot) => snapshot.accept,
        SwapMessage::Reject(reject) => return Err(refused(reject)),
        other => return Err(unexpected("SwapAccept", &other)),
    };
    // `advance` watches and spends only the P2WSH contract the request asked for.
    if accept.script_type != SwapScript::P2wsh {
        return Err(PubkySwapError::validation(format!(
            "the provider answered with a {:?} contract, not P2WSH",
            accept.script_type
        )));
    }

    let tip = {
        let url = electrum_url.clone();
        tokio::task::spawn_blocking(move || chain_tip(&url))
            .await
            .map_err(PubkySwapError::chain)??
    };
    let policy = policy(network);
    validate_accept(&accept, &quote, Some(tip), &policy).map_err(PubkySwapError::validation)?;
    let htlc_spk = verify_contract(
        &accept,
        SwapDirection::Reverse,
        &swap_keys.payment_hash,
        &swap_keys.claim_pubkey,
        network,
    )
    .map_err(PubkySwapError::validation)?;
    let invoice = accept
        .invoice
        .clone()
        .ok_or_else(|| PubkySwapError::validation("the contract has no hold invoice"))?;
    validate_hold_invoice(
        &decode_hold_invoice(&invoice)?,
        &quote,
        &swap_keys.payment_hash,
        now_unix(),
        &policy,
    )
    .map_err(PubkySwapError::validation)?;
    let htlc_address = Address::from_script(&htlc_spk, network)
        .map_err(PubkySwapError::validation)?
        .to_string();

    let record = SwapRecord {
        swap_id: accept.swap_id.to_string(),
        key_index: index,
        provider_pubky: route.provider_pubky.clone(),
        network,
        electrum_url,
        quote,
        accept: accept.clone(),
        destination_address: route.endpoint.value.clone(),
        delivered_sat: route.delivered_sat,
        claim_fee_sat: route.claim_fee_sat,
        claim_txid: None,
        claim_tx_hex: None,
        created_at_unix: now_unix(),
    };
    store::save(&record)?;
    QUOTES.lock().unwrap().remove(&quote_id);

    Ok(PubkySwapRoutePayment {
        swap_id: record.swap_id,
        invoice,
        debit_sat: route.debit_sat,
        delivered_sat: route.delivered_sat,
        destination_address: route.endpoint.value,
        htlc_address,
        timeout_block_height: accept.timeout_block_height,
    })
}

fn status_of(record: &SwapRecord, state: PubkySwapRouteState) -> PubkySwapRouteStatus {
    PubkySwapRouteStatus {
        swap_id: record.swap_id.clone(),
        state,
        provider_pubky: record.provider_pubky.clone(),
        delivered_sat: record.delivered_sat,
        destination_address: record.destination_address.clone(),
    }
}

/// Find the provider's lockup and, once it has its confirmations, claim it straight to the
/// contact's address. Call again until it reports `Claimed`: until the claim confirms, each call
/// checks it is still in the mempool and broadcasts it again if it dropped out.
fn advance_blocking(
    mut record: SwapRecord,
    mnemonic: &str,
    passphrase: Option<&str>,
) -> Result<PubkySwapRouteStatus, PubkySwapError> {
    let keys = keys::swap_keys(mnemonic, passphrase, record.key_index)?;
    let provider = record
        .accept
        .provider_pubkey_hex
        .parse()
        .map_err(PubkySwapError::validation)?;
    let timeout = record.accept.timeout_block_height;
    let redeem_script =
        build_htlc_script(&keys.payment_hash, &keys.claim_pubkey, &provider, timeout);
    let htlc_spk = ScriptBuf::new_p2wsh(&redeem_script.wscript_hash());
    let destination = parse_address(&record.destination_address, record.network)?.script_pubkey();
    let contract = claim::Contract {
        htlc_spk: &htlc_spk,
        lock_sat: record.accept.onchain_amount_sat,
        destination: &destination,
        timeout,
        required_confirmations: policy(record.network)
            .effective_confirmations(record.quote.required_confirmations),
    };

    let electrum = ElectrumClient::new(&record.electrum_url)?;
    let tip = electrum.block_headers_subscribe()?.height as u32;
    let mut history = vec![];
    for item in electrum.script_get_history(&htlc_spk)? {
        history.push((item.height, electrum.transaction_get(&item.tx_hash)?));
    }

    let state = match claim::next_step(&contract, record.claim_txid.as_deref(), tip, &history) {
        claim::Step::Report(state) => state,
        claim::Step::Claimed { txid, state } => {
            if record.claim_txid.as_deref() != Some(txid.as_str()) {
                record.claim_txid = Some(txid);
                store::save(&record)?;
            }
            state
        }
        claim::Step::Rebroadcast => {
            let txid = record.claim_txid.clone().unwrap_or_default();
            match &record.claim_tx_hex {
                Some(claim) => broadcast_claim(&electrum, claim),
                None => log::warn!("pubky swaps: no saved claim to broadcast again for {txid}"),
            }
            PubkySwapRouteState::ClaimBroadcast { txid }
        }
        claim::Step::Claim(outpoint) => {
            let claim = build_claim_tx(
                outpoint,
                record.accept.onchain_amount_sat,
                &redeem_script,
                destination.clone(),
                record.claim_fee_sat,
                keys.preimage,
                &keys.claim_key,
            )
            .map_err(PubkySwapError::chain)?;
            let txid = claim.compute_txid().to_string();
            let claim = bitcoin::consensus::encode::serialize_hex(&claim);
            // Saved before the broadcast, so a restart in between finds and sends the same claim.
            record.claim_txid = Some(txid.clone());
            record.claim_tx_hex = Some(claim.clone());
            store::save(&record)?;
            broadcast_claim(&electrum, &claim);
            PubkySwapRouteState::ClaimBroadcast { txid }
        }
    };
    Ok(status_of(&record, state))
}

/// Send a saved claim. A failure is only logged: the claim is saved, and the next call finds it
/// missing from the history and sends it again, so the app keeps polling instead of stopping.
fn broadcast_claim(electrum: &ElectrumClient, claim: &str) {
    let sent = bitcoin::consensus::encode::deserialize_hex::<bitcoin::Transaction>(claim)
        .map_err(PubkySwapError::storage)
        .and_then(|claim| Ok(electrum.transaction_broadcast(&claim)?));
    if let Err(e) = sent {
        log::warn!("pubky swaps: claim broadcast failed, retrying on the next advance: {e}");
    }
}

pub async fn advance_swap(
    swap_id: String,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<PubkySwapRouteStatus, PubkySwapError> {
    let record = store::load(&swap_id)?;
    tokio::task::spawn_blocking(move || {
        advance_blocking(record, &mnemonic, bip39_passphrase.as_deref())
    })
    .await
    .map_err(PubkySwapError::chain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    const MNEMONIC: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn onchain_contact() -> PaykitEndpoint {
        PaykitEndpoint {
            method_id: "btc-regtest-p2wpkh".into(),
            value: "bcrt1qdp8zwu9xk08p3dvkz4qyavmfaadkr4lnvwfah2".into(),
        }
    }

    fn request(service_legs: Vec<ServiceRouteLeg>) -> PaykitRouteRequest {
        PaykitRouteRequest {
            amount_sat: 20_000,
            contact_endpoints: vec![onchain_contact()],
            lightning_spendable_sat: 0,
            onchain_spendable_sat: 0,
            providers: vec![],
            fee_rate_sat_vb: 2,
            service_legs,
        }
    }

    async fn route(request: PaykitRouteRequest) -> PaykitRoute {
        route_paykit_payment(request, bitcoin::Network::Regtest, MNEMONIC.into(), None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn routes_an_onchain_contact_through_a_service_without_providers() {
        let leg = ServiceRouteLeg {
            service: "arkade".into(),
            endpoint: onchain_contact(),
            delivered_sat: 20_000,
            debit_sat: 20_300,
            atomicity: "operator".into(),
            valid_until_unix: 0,
        };
        let PaykitRoute::Service { leg: picked } = route(request(vec![leg.clone()])).await else {
            panic!("expected a service route");
        };
        assert_eq!(picked, leg);
    }

    #[tokio::test]
    async fn routes_through_a_service_on_mainnet_where_swaps_are_refused() {
        let leg = ServiceRouteLeg {
            service: "spark".into(),
            endpoint: onchain_contact(),
            delivered_sat: 20_000,
            debit_sat: 20_300,
            atomicity: "trusted".into(),
            valid_until_unix: 0,
        };
        let mut request = request(vec![leg.clone()]);
        request.providers = vec!["pubky-provider".into()];
        let route =
            route_paykit_payment(request, bitcoin::Network::Bitcoin, MNEMONIC.into(), None).await;
        assert!(matches!(route, Ok(PaykitRoute::Service { leg: picked }) if picked == leg));
    }

    #[tokio::test]
    async fn explains_why_nothing_routes_without_providers_or_services() {
        let PaykitRoute::None { reason } = route(request(vec![])).await else {
            panic!("expected no route");
        };
        assert_eq!(reason, "No swap provider is set up.");
    }
}

#[cfg(test)]
mod live_tests {
    //! Against a running regtest provider:
    //! `PUBKY_SWAPS_PROVIDER=<z32>[,<contact z32>…] PUBKY_SWAPS_ELECTRUM=tcp://127.0.0.1:60001
    //! PUBKY_SWAPS_DESTINATION=<bcrt1…> cargo test --lib pubky_swaps::client::live_tests -- --ignored`
    use super::*;

    const MNEMONIC: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a running regtest Pubky Swaps provider"]
    async fn routes_an_onchain_contact_through_the_provider_and_commits() {
        let provider = std::env::var("PUBKY_SWAPS_PROVIDER").unwrap();
        let electrum = std::env::var("PUBKY_SWAPS_ELECTRUM").unwrap();
        let destination = std::env::var("PUBKY_SWAPS_DESTINATION").unwrap();
        let dir = std::env::temp_dir().join(format!("pubky-swaps-{}", Uuid::new_v4()));
        store::init(dir.to_str().unwrap()).unwrap();

        let mut providers: Vec<String> = provider.split(',').map(str::to_string).collect();
        let provider = providers[0].clone();
        let edges = provider_edges(provider.clone(), MNEMONIC.into(), None)
            .await
            .unwrap();
        assert!(edges.iter().any(|e| e.direction == "reverse"), "{edges:?}");

        let request = PaykitRouteRequest {
            amount_sat: 20_000,
            contact_endpoints: vec![PaykitEndpoint {
                method_id: "btc-regtest-p2wpkh".into(),
                value: destination.clone(),
            }],
            lightning_spendable_sat: 500_000,
            onchain_spendable_sat: 0,
            providers: std::mem::take(&mut providers),
            fee_rate_sat_vb: 2,
            service_legs: vec![],
        };
        let route = route_paykit_payment(request, bitcoin::Network::Regtest, MNEMONIC.into(), None)
            .await
            .unwrap();
        let PaykitRoute::Swap { quote } = route else {
            panic!("expected a swap route, got {route:?}");
        };
        assert_eq!(quote.delivered_sat, 20_000);
        assert_eq!(quote.lock_sat, 20_000 + quote.claim_fee_sat);
        assert!(quote.debit_sat > quote.lock_sat);

        let payment = commit_route(quote.quote_id.clone(), electrum, MNEMONIC.into(), None)
            .await
            .unwrap();
        assert_eq!(payment.destination_address, destination);
        assert!(payment.invoice.starts_with("lnbcrt"), "{}", payment.invoice);
        let status = advance_swap(payment.swap_id.clone(), MNEMONIC.into(), None)
            .await
            .unwrap();
        assert!(matches!(
            status.state,
            PubkySwapRouteState::AwaitingLockup { .. }
        ));
        log::info!("{quote:?} {payment:?}");
    }
}
