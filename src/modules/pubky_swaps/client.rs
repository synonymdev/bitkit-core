//! The `swap/1` flow for a Paykit payment: route, commit, claim.
//!
//! Message mapping (Paykit Capabilities §13.1): `rfq` is pubky-swap's `QuoteRequest`, `quote`
//! is `Quote`, `commit` is `SwapRequest`, `contract` is `SwapAccept`, `refusal` is `Reject`.
//! Here they travel over iroh `direct/1`, which §13.1 allows for the same payloads.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::{Address, OutPoint, ScriptBuf};
use electrum_client::{Client as ElectrumClient, ConfigBuilder, ElectrumApi};
use once_cell::sync::Lazy;
use swap_common::htlc::build_htlc_script;
use swap_common::messages::{
    OfferRequest, Quote, QuoteRequest, Reject, SwapListRequest, SwapMessage, SwapOffer,
    SwapRequest, SwapScript, SwapStatusRequest, SwapStatusSnapshot, PROTOCOL_VERSION,
    SWAP_LIST_FEATURE,
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
use super::router;
use super::store::{self, SwapRecord};
use super::transport::{bare_pubky, DirectClient};
use super::types::*;
use crate::modules::paykit_route;

/// How long a candidate provider has to answer an offer request.
const OFFER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Most candidates asked for an offer per payment: each is a connection and an address lookup
/// through iroh's n0 discovery, which sees which keys the wallet looks up.
const MAX_CANDIDATES: usize = 30;
const ELECTRUM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// How long the wallet keeps asking about a swap request the provider is still admitting.
const PENDING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const PENDING_POLL: std::time::Duration = std::time::Duration::from_secs(2);
/// Most pages read from one provider's swap list on restore.
const MAX_LIST_PAGES: usize = 100;

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
        &[0; 32],
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
    let service = match paykit_route::route_payment(&request, network, now_unix())
        .map_err(PubkySwapError::invalid)?
    {
        PaykitRoute::Service { leg } => Some(leg),
        PaykitRoute::None { .. } => None,
        route => return Ok(route),
    };
    let onchain = paykit_route::onchain_endpoint(&request.contact_endpoints, network).cloned();
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

/// An Electrum client whose reads time out, so a connection that dies silently cannot hold a swap.
fn electrum(url: &str) -> Result<ElectrumClient, PubkySwapError> {
    electrum_with_retries(url, 1)
}

fn electrum_with_retries(url: &str, retries: u8) -> Result<ElectrumClient, PubkySwapError> {
    let config = ConfigBuilder::new()
        .timeout(Some(ELECTRUM_TIMEOUT))
        .retry(retries)
        .build();
    Ok(ElectrumClient::from_config(url, config)?)
}

fn chain_tip(electrum_url: &str) -> Result<u32, PubkySwapError> {
    let client = electrum(electrum_url)?;
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
    let index = keys::new_index();
    let swap_keys = keys::swap_keys(&mnemonic, passphrase, &index)?;
    let request = SwapRequest {
        script_type: SwapScript::P2wsh,
        quote_id: quote.quote_id,
        client_pkarr: identity_pubky(&secret),
        direction: SwapDirection::Reverse,
        payment_hash_hex: hex::encode(swap_keys.payment_hash),
        client_claim_pubkey_hex: Some(swap_keys.claim_pubkey.to_string()),
        client_refund_pubkey_hex: None,
        invoice: None,
        owner_ref: Some(keys::owner_ref(&index)),
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
        key_index: hex::encode(index),
        provider_pubky: route.provider_pubky.clone(),
        network,
        electrum_url,
        required_confirmations: quote.required_confirmations,
        quote: Some(quote),
        accept: accept.clone(),
        destination_address: route.endpoint.value.clone(),
        delivered_sat: route.delivered_sat,
        claim_fee_sat: route.claim_fee_sat,
        claim_txid: None,
        claim_tx_hex: None,
        claim_published: false,
        claim_seen: false,
        claim_listed: false,
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
    let index: [u8; 32] = hex::decode(&record.key_index)
        .ok()
        .and_then(|index| index.try_into().ok())
        .ok_or_else(|| PubkySwapError::storage("invalid key index"))?;
    let keys = keys::swap_keys(mnemonic, passphrase, &index)?;
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
        payment_hash: &keys.payment_hash,
        timeout,
        required_confirmations: policy(record.network)
            .effective_confirmations(record.required_confirmations),
    };

    let electrum = electrum(&record.electrum_url)?;
    let tip = electrum.block_headers_subscribe()?.height as u32;
    let mut history = vec![];
    for item in electrum.script_get_history(&htlc_spk)? {
        history.push((item.height, electrum.transaction_get(&item.tx_hash)?));
    }

    let saved = record.claim_txid.as_ref().map(|_| claim::SavedClaim {
        published: record.claim_published,
    });
    let required = contract.required_confirmations;
    let state = match claim::next_step(&contract, saved, record.claim_listed, tip, &history) {
        claim::Step::Report(state) => state,
        claim::Step::Claimed { txid, state } => {
            if record.claim_txid.as_deref() != Some(txid.as_str()) || !record.claim_seen {
                record.claim_txid = Some(txid);
                record.claim_published = true;
                record.claim_seen = true;
                store::save(&record)?;
            }
            state
        }
        claim::Step::Rebroadcast {
            outpoint,
            confirmations,
        } => {
            match record.claim_tx_hex.clone() {
                Some(claim) => publish_claim(&mut record, &claim)?,
                // A restored swap whose claim the provider listed holds no claim of its own. Its
                // preimage may be public, so a claim of our own goes out whatever the window.
                None => claim_lockup(&mut record, outpoint, &redeem_script, &destination, &keys)?,
            }
            claim_state(&record, confirmations, required)
        }
        claim::Step::Claim {
            outpoint,
            confirmations,
        } => {
            claim_lockup(&mut record, outpoint, &redeem_script, &destination, &keys)?;
            claim_state(&record, confirmations, required)
        }
    };
    Ok(status_of(&record, state))
}

/// `ClaimBroadcast` once a server took the claim; until then the lockup is still waiting on it.
fn claim_state(record: &SwapRecord, confirmations: u32, required: u32) -> PubkySwapRouteState {
    match (&record.claim_txid, record.claim_seen) {
        (Some(txid), true) => PubkySwapRouteState::ClaimBroadcast { txid: txid.clone() },
        _ => PubkySwapRouteState::AwaitingLockup {
            confirmations,
            required,
        },
    }
}

/// Build the claim of `outpoint` to the contact, save it, then broadcast it.
fn claim_lockup(
    record: &mut SwapRecord,
    outpoint: OutPoint,
    redeem_script: &ScriptBuf,
    destination: &ScriptBuf,
    keys: &keys::SwapKeys,
) -> Result<(), PubkySwapError> {
    let claim = build_claim_tx(
        outpoint,
        record.accept.onchain_amount_sat,
        redeem_script,
        destination.clone(),
        record.claim_fee_sat,
        keys.preimage,
        &keys.claim_key,
    )
    .map_err(PubkySwapError::chain)?;
    let txid = claim.compute_txid().to_string();
    let claim = bitcoin::consensus::encode::serialize_hex(&claim);
    // Saved before the broadcast, so a restart in between finds and sends the same claim.
    record.claim_txid = Some(txid);
    record.claim_tx_hex = Some(claim.clone());
    store::save(record)?;
    publish_claim(record, &claim)
}

/// Send a saved claim. It is recorded as published before it goes out: once a broadcast was
/// attempted its preimage may have left the wallet, even if the reply is lost. Only an explicit
/// refusal of the first attempt marks it private again, and a failure is only logged, so the app
/// keeps polling and the next call sends it again.
fn publish_claim(record: &mut SwapRecord, claim: &str) -> Result<(), PubkySwapError> {
    let claim = bitcoin::consensus::encode::deserialize_hex::<bitcoin::Transaction>(claim)
        .map_err(PubkySwapError::storage)?;
    // No automatic resend: a refusal must answer the one attempt this call made, not a retry that
    // hides an earlier attempt whose reply was lost.
    let sender = match electrum_with_retries(&record.electrum_url, 0) {
        Ok(sender) => sender,
        Err(e) => {
            log::warn!("pubky swaps: no Electrum connection for the claim, retrying: {e}");
            return Ok(());
        }
    };
    // Only a refusal of the very first attempt proves the preimage never left: an earlier attempt
    // may have reached the server even if its reply was lost.
    let first_attempt = !record.claim_published;
    if first_attempt {
        record.claim_published = true;
        store::save(record)?;
    }
    match sender.transaction_broadcast(&claim) {
        Ok(_) if !record.claim_seen => {
            record.claim_seen = true;
            store::save(record)
        }
        Ok(_) => Ok(()),
        Err(e) => {
            log::warn!("pubky swaps: claim broadcast failed, retrying on the next advance: {e}");
            if first_attempt && is_refusal(&e) {
                record.claim_published = false;
                store::save(record)?;
            }
            Ok(())
        }
    }
}

/// Whether the server answered and refused, rather than the reply being lost. Only a bare server
/// error counts, from a client that sends once.
fn is_refusal(error: &electrum_client::Error) -> bool {
    matches!(error, electrum_client::Error::Protocol(_))
}

/// One advance at a time per swap, so two overlapping calls cannot save a stale record over the
/// claim, while a slow call for one swap does not hold up the others.
static ADVANCING: Lazy<Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>> =
    Lazy::new(Default::default);

/// Rebuild the records of reverse swaps this wallet started, for a wallet restored from its seed:
/// each provider that advertises `swap-list-v1` lists the swaps it holds for the wallet's swap
/// identity, the wallet derives each swap's keys from the index in its `owner_ref`, and checks
/// them against the listed contract before writing a record.
///
/// The listing is the provider's word, so a record is written only for a swap whose hash the
/// wallet's own Lightning node paid (`sent_payments`), whose lockup is no smaller than that payment
/// less the policy's most fees, and whose hash no other record holds. Finished swaps, other
/// wallets' swaps and contracts that do not match get no record. The contact's address is not in
/// the listing, so a restored swap claims to `payout_address`, the wallet's own.
#[allow(clippy::too_many_arguments)]
pub async fn restore_swaps(
    providers: Vec<String>,
    sent_payments: Vec<PubkySwapSentPayment>,
    electrum_url: String,
    payout_address: String,
    fee_rate_sat_vb: u64,
    network: bitcoin::Network,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<PubkySwapRestoreReport, PubkySwapError> {
    let payout = parse_address(&payout_address, network)?.script_pubkey();
    let claim_fee_sat = claim_fee(&payout, fee_rate_sat_vb)?;
    let secret = keys::identity_secret(&mnemonic, bip39_passphrase.as_deref())?;
    let client = DirectClient::bind(secret).await?;

    // Providers are asked at once, as an older one never answers a list request and each costs a
    // request timeout.
    let mut seen = std::collections::HashSet::new();
    let mut lists = tokio::task::JoinSet::new();
    for provider in providers
        .iter()
        .map(|p| bare_pubky(p).to_string())
        .filter(|p| seen.insert(p.clone()))
        .take(MAX_CANDIDATES)
    {
        let client = client.clone();
        lists.spawn(async move {
            let listing = provider_swaps(&client, &provider).await;
            (provider, listing)
        });
    }
    let mut report = PubkySwapRestoreReport::default();
    let mut listings = vec![];
    while let Some(list) = lists.join_next().await {
        let Ok((provider, listing)) = list else {
            continue;
        };
        if !listing.complete {
            report.incomplete_providers.push(provider.clone());
        }
        listings.push((provider, listing.snapshots));
    }
    client.close().await;

    let paid: HashMap<String, u64> = sent_payments
        .into_iter()
        .map(|p| (p.payment_hash_hex.to_lowercase(), p.amount_sat))
        .collect();
    // One record per hash: a second listing of the same swap under a new id gets none.
    let mut hashes = std::collections::HashSet::new();
    for record in store::list()? {
        if let Ok(hash) = record_payment_hash(&record, &mnemonic, bip39_passphrase.as_deref()) {
            hashes.insert(hash);
        }
    }
    let wallet = Restoring {
        electrum_url: &electrum_url,
        payout_address: &payout_address,
        claim_fee_sat,
        network,
        mnemonic: &mnemonic,
        passphrase: bip39_passphrase.as_deref(),
        paid: &paid,
    };
    for (provider, snapshots) in listings {
        for snapshot in snapshots {
            let swap_id = snapshot.accept.swap_id.to_string();
            match store::load(&swap_id) {
                Ok(_) => {
                    report.known.push(swap_id);
                    continue;
                }
                Err(PubkySwapError::NotFound { .. }) => {}
                Err(e) => {
                    log::warn!("pubky swaps: could not read swap {swap_id}, not restoring it: {e}");
                    continue;
                }
            }
            if snapshot.state.is_terminal() {
                report.finished.push(swap_id);
                continue;
            }
            let Some(record) = restored_record(&snapshot, &provider, &wallet) else {
                continue;
            };
            if !hashes.insert(snapshot.payment_hash_hex.to_lowercase()) {
                log::warn!("pubky swaps: {provider} listed hash of swap {swap_id} twice");
                continue;
            }
            // One swap that cannot be saved must not cost the user the others.
            if let Err(e) = store::save(&record) {
                log::warn!("pubky swaps: could not save restored swap {swap_id}: {e}");
                continue;
            }
            report.restored.push(swap_id);
        }
    }
    Ok(report)
}

fn record_payment_hash(
    record: &SwapRecord,
    mnemonic: &str,
    passphrase: Option<&str>,
) -> Result<String, PubkySwapError> {
    let index: [u8; 32] = hex::decode(&record.key_index)
        .ok()
        .and_then(|index| index.try_into().ok())
        .ok_or_else(|| PubkySwapError::storage("invalid key index"))?;
    Ok(hex::encode(
        keys::swap_keys(mnemonic, passphrase, &index)?.payment_hash,
    ))
}

/// A provider's list of this wallet's swaps, and whether it was read to the end.
struct Listing {
    snapshots: Vec<SwapStatusSnapshot>,
    complete: bool,
}

/// The swaps a provider that advertises `swap-list-v1` lists for this wallet's identity.
async fn provider_swaps(client: &DirectClient, provider: &str) -> Listing {
    let offer = tokio::time::timeout(OFFER_TIMEOUT, fetch_offer(client, provider)).await;
    match offer {
        Ok(Ok(offer)) if offer.features.iter().any(|f| f == SWAP_LIST_FEATURE) => {
            list_swaps(client, provider).await
        }
        Ok(Ok(_)) => {
            log::debug!("pubky swaps: {provider} cannot list swaps");
            Listing {
                snapshots: vec![],
                complete: true,
            }
        }
        Ok(Err(e)) => {
            log::warn!("pubky swaps: no offer from {provider}: {e}");
            Listing {
                snapshots: vec![],
                complete: false,
            }
        }
        Err(_) => {
            log::warn!("pubky swaps: {provider} did not answer in time");
            Listing {
                snapshots: vec![],
                complete: false,
            }
        }
    }
}

/// Every swap a provider lists for this wallet's identity, page by page. A provider that keeps
/// sending cursors stops at [`MAX_LIST_PAGES`] or at a cursor it sent before, so it cannot hold up
/// the restore of the other providers. A page that fails keeps the pages read before it.
async fn list_swaps(client: &DirectClient, provider: &str) -> Listing {
    let mut snapshots = vec![];
    let mut cursor = None;
    let mut cursors_seen = std::collections::HashSet::new();
    for _ in 0..MAX_LIST_PAGES {
        let request = SwapMessage::SwapListRequest(SwapListRequest {
            request_id: Some(Uuid::new_v4()),
            cursor: cursor.take(),
            limit: None,
            active_only: false,
        });
        let page = match client.request(provider, &request).await {
            Ok(SwapMessage::SwapList(page)) => page,
            reply => {
                let error = match reply {
                    Ok(SwapMessage::Reject(reject)) => refused(reject),
                    Ok(other) => unexpected("SwapList", &other),
                    Err(e) => e,
                };
                log::warn!("pubky swaps: swap list from {provider} stopped: {error}");
                return Listing {
                    snapshots,
                    complete: false,
                };
            }
        };
        snapshots.extend(page.snapshots);
        match page.next_cursor {
            Some(next) if cursors_seen.insert(next.clone()) => cursor = Some(next),
            Some(next) => {
                log::warn!("pubky swaps: {provider} sent cursor {next} again; ending its list");
                return Listing {
                    snapshots,
                    complete: false,
                };
            }
            None => {
                return Listing {
                    snapshots,
                    complete: true,
                }
            }
        }
    }
    log::warn!("pubky swaps: {provider} listed more than {MAX_LIST_PAGES} pages; ending its list");
    Listing {
        snapshots,
        complete: false,
    }
}

/// What a restore checks listed swaps against: the wallet's seed, network and payments.
struct Restoring<'a> {
    electrum_url: &'a str,
    payout_address: &'a str,
    claim_fee_sat: u64,
    network: bitcoin::Network,
    mnemonic: &'a str,
    passphrase: Option<&'a str>,
    /// What the wallet's Lightning node paid, by payment hash in lowercase hex.
    paid: &'a HashMap<String, u64>,
}

/// The record for a listed reverse swap on the wallet's network whose keys this wallet derives
/// from its `owner_ref` and whose hash its node paid for at least the lockup plus the most fees
/// the policy allows, or `None`.
fn restored_record(
    snapshot: &SwapStatusSnapshot,
    provider: &str,
    wallet: &Restoring,
) -> Option<SwapRecord> {
    let accept = &snapshot.accept;
    let swap_id = accept.swap_id;
    if accept.direction != SwapDirection::Reverse
        || accept.script_type != SwapScript::P2wsh
        || snapshot.network.to_bitcoin_network() != wallet.network
    {
        return None;
    }
    let index = keys::index_from_owner_ref(snapshot.owner_ref.as_deref()?)?;
    let keys = keys::swap_keys(wallet.mnemonic, wallet.passphrase, &index).ok()?;
    let hash = hex::encode(keys.payment_hash);
    if hash != snapshot.payment_hash_hex.to_lowercase() {
        return None;
    }
    verify_contract(
        accept,
        SwapDirection::Reverse,
        &keys.payment_hash,
        &keys.claim_pubkey,
        wallet.network,
    )
    .ok()?;
    // The listing names the lockup amount, so it is checked against the payment the wallet's own
    // node sent: a smaller lockup would let the provider keep the difference.
    let Some(&paid_sat) = wallet.paid.get(&hash) else {
        log::warn!("pubky swaps: no payment for swap {swap_id} from {provider}; not restoring it");
        return None;
    };
    let max_fee_sat = paid_sat.saturating_mul(policy(wallet.network).max_fee_bps as u64) / 10_000;
    if accept.onchain_amount_sat.saturating_add(max_fee_sat) < paid_sat {
        log::warn!(
            "pubky swaps: swap {swap_id} from {provider} locks {} sat for a {paid_sat} sat \
             payment; not restoring it",
            accept.onchain_amount_sat
        );
        return None;
    }
    let delivered_sat = accept
        .onchain_amount_sat
        .checked_sub(wallet.claim_fee_sat)?;
    Some(SwapRecord {
        swap_id: swap_id.to_string(),
        key_index: hex::encode(index),
        provider_pubky: provider.to_string(),
        network: wallet.network,
        electrum_url: wallet.electrum_url.to_string(),
        quote: None,
        required_confirmations: snapshot.required_confirmations,
        accept: accept.clone(),
        destination_address: wallet.payout_address.to_string(),
        delivered_sat,
        claim_fee_sat: wallet.claim_fee_sat,
        // The provider's word that a claim went out is no proof: `advance` finds it in the chain's
        // history by its preimage, or claims again once the lockup has its confirmations.
        claim_txid: None,
        claim_tx_hex: None,
        claim_published: false,
        claim_seen: false,
        claim_listed: snapshot.claim_observed_txid_hex.is_some(),
        created_at_unix: snapshot.created_at_unix,
    })
}

pub async fn advance_swap(
    swap_id: String,
    mnemonic: String,
    bip39_passphrase: Option<String>,
) -> Result<PubkySwapRouteStatus, PubkySwapError> {
    tokio::task::spawn_blocking(move || {
        let lock = ADVANCING
            .lock()
            .unwrap()
            .entry(swap_id.clone())
            .or_default()
            .clone();
        let _advancing = lock.lock().unwrap();
        let record = store::load(&swap_id)?;
        advance_blocking(record, &mnemonic, bip39_passphrase.as_deref())
    })
    .await
    .map_err(PubkySwapError::chain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const OTHER_SEED: &str =
        "legal winner thank year wave sausage worth useful legal winner thank yellow";
    const PAYOUT: &str = "bcrt1qdp8zwu9xk08p3dvkz4qyavmfaadkr4lnvwfah2";

    fn listed_swap(mnemonic: &str, owner_ref: Option<String>) -> SwapStatusSnapshot {
        let index = owner_ref
            .as_deref()
            .and_then(keys::index_from_owner_ref)
            .unwrap_or([7; 32]);
        let keys = keys::swap_keys(mnemonic, None, &index).unwrap();
        let provider_secret = bitcoin::secp256k1::SecretKey::from_slice(&[9; 32]).unwrap();
        let provider = bitcoin::PublicKey::new(
            provider_secret.public_key(&bitcoin::secp256k1::Secp256k1::new()),
        );
        SwapStatusSnapshot {
            request_id: None,
            accept: swap_common::messages::SwapAccept {
                script_type: SwapScript::P2wsh,
                swap_tree: None,
                quote_id: Uuid::new_v4(),
                swap_id: Uuid::new_v4(),
                direction: SwapDirection::Reverse,
                htlc_script_hex: String::new(),
                htlc_address: String::new(),
                onchain_amount_sat: 20_300,
                timeout_block_height: 1_144,
                provider_pubkey_hex: provider.to_string(),
                invoice: None,
            },
            network: swap_common::NetworkSpec::Regtest,
            state: swap_common::SwapState::LockupConfirmed,
            funding_txid_hex: None,
            funding_vout: None,
            spend_txid_hex: None,
            claim_observed_txid_hex: None,
            required_confirmations: 1,
            updated_at_unix: 0,
            observed_at_unix: 0,
            created_at_unix: 1_000,
            payment_hash_hex: hex::encode(keys.payment_hash),
            owner_ref,
        }
    }

    /// Restore as a wallet whose node paid `paid_sat` for the snapshot's hash.
    fn restore_paid(snapshot: &SwapStatusSnapshot, paid_sat: u64) -> Option<SwapRecord> {
        let paid = HashMap::from([(snapshot.payment_hash_hex.clone(), paid_sat)]);
        let wallet = Restoring {
            electrum_url: "tcp://127.0.0.1:60001",
            payout_address: PAYOUT,
            claim_fee_sat: 300,
            network: bitcoin::Network::Regtest,
            mnemonic: SEED,
            passphrase: None,
            paid: &paid,
        };
        restored_record(snapshot, "provider", &wallet)
    }

    fn restore(snapshot: &SwapStatusSnapshot) -> Option<SwapRecord> {
        restore_paid(snapshot, 20_500)
    }

    #[test]
    fn restores_a_listed_swap_from_the_index_in_its_owner_ref() {
        let index = keys::new_index();
        let snapshot = listed_swap(SEED, Some(keys::owner_ref(&index)));
        let record = restore(&snapshot).expect("restored");
        assert_eq!(record.key_index, hex::encode(index));
        assert_eq!(record.destination_address, PAYOUT);
        assert_eq!(record.delivered_sat, 20_000);
        assert_eq!(record.required_confirmations, 1);
        assert!(record.quote.is_none() && record.claim_txid.is_none());
    }

    #[test]
    fn skips_swaps_this_wallet_cannot_rebuild() {
        let other_wallets = listed_swap(OTHER_SEED, Some(keys::owner_ref(&keys::new_index())));
        assert!(restore(&other_wallets).is_none());
        assert!(restore(&listed_swap(SEED, None)).is_none());
        assert!(restore(&listed_swap(SEED, Some("someone-else:1".into()))).is_none());
        let mut other_network = listed_swap(SEED, Some(keys::owner_ref(&keys::new_index())));
        other_network.network = swap_common::NetworkSpec::Signet;
        assert!(restore(&other_network).is_none());
    }

    #[test]
    fn restores_only_a_lockup_the_wallet_paid_for() {
        let snapshot = listed_swap(SEED, Some(keys::owner_ref(&keys::new_index())));
        // Locks 20,300 sat; the policy allows 5% of the payment in fees.
        assert!(restore_paid(&snapshot, 21_368).is_some());
        assert!(restore_paid(&snapshot, 21_369).is_none());
        let paid = HashMap::new();
        let wallet = Restoring {
            electrum_url: "",
            payout_address: PAYOUT,
            claim_fee_sat: 300,
            network: bitcoin::Network::Regtest,
            mnemonic: SEED,
            passphrase: None,
            paid: &paid,
        };
        assert!(restored_record(&snapshot, "provider", &wallet).is_none());
    }

    #[test]
    fn does_not_take_a_listed_claim_as_published() {
        let mut snapshot = listed_swap(SEED, Some(keys::owner_ref(&keys::new_index())));
        snapshot.claim_observed_txid_hex = Some("ab".repeat(32));
        let record = restore(&snapshot).expect("restored");
        assert!(record.claim_listed);
        assert!(record.claim_txid.is_none() && !record.claim_published && !record.claim_seen);
    }

    #[test]
    fn only_an_answer_from_the_server_counts_as_a_refusal() {
        use electrum_client::Error;
        let refused = || Error::Protocol(serde_json::json!({"message": "min relay fee not met"}));
        let lost = || Error::IOError(std::io::Error::from(std::io::ErrorKind::TimedOut));
        assert!(is_refusal(&refused()));
        assert!(!is_refusal(&lost()));
        // A transport error after the single attempt, or retries that hid an earlier one.
        assert!(!is_refusal(&Error::AllAttemptsErrored(vec![])));
        assert!(!is_refusal(&Error::AllAttemptsErrored(vec![
            lost(),
            refused()
        ])));
    }

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
            service: "spark".into(),
            endpoint: onchain_contact(),
            delivered_sat: 20_000,
            debit_sat: 20_300,
            atomicity: "trusted".into(),
            valid_until_unix: 0,
        };
        let PaykitRoute::Service { leg: picked } = route(request(vec![leg.clone()])).await else {
            panic!("expected a service route");
        };
        assert_eq!(picked, leg);
    }

    #[tokio::test]
    async fn routes_through_a_service_on_mainnet_where_swaps_are_refused() {
        let mainnet_contact = PaykitEndpoint {
            method_id: "btc-bitcoin-p2wpkh".into(),
            value: "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu".into(),
        };
        let leg = ServiceRouteLeg {
            service: "spark".into(),
            endpoint: mainnet_contact.clone(),
            delivered_sat: 20_000,
            debit_sat: 20_300,
            atomicity: "trusted".into(),
            valid_until_unix: 0,
        };
        let mut request = request(vec![leg.clone()]);
        request.contact_endpoints = vec![mainnet_contact];
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
