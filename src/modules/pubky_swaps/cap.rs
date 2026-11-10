//! Signed `swap/1` negotiation (`paykit-cap.*`, Paykit Capabilities §10) with one provider.
//!
//! swap-common's `CapClient` checks every reply: the quote's `terms_hash` and signature against
//! the RFQ and both parties, the contract's `commit_hash` against the commit, its own hash and
//! signature, and the invoice blob. This session adds the signer rule: a reply must be signed by
//! the provider's own key or by the capability signing key its records delegate to (§5), and
//! anything unsigned is refused.

use std::time::Duration;

use swap_common::messages::{QuoteRequest, SwapMessage};
use swap_common::paykit_cap::{
    reply_signer, CapClient, CapError, CapMessage, Parties, SignedQuote,
};

use super::delegation;
use super::errors::PubkySwapError;
use super::transport::{bare_pubky, provider_endpoint_id, DirectClient};

/// How long a provider's published signing key may take to read.
const DELEGATION_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) struct CapSession<'a> {
    client: &'a DirectClient,
    provider: String,
    cap: CapClient,
    /// The delegated key this session accepted, if the provider signs with one.
    delegated_signer: Option<String>,
    /// The commit request of this session, sent again when the provider has the swap but not its
    /// signed contract.
    commit: Option<SwapMessage>,
}

fn cap_error(error: CapError) -> PubkySwapError {
    PubkySwapError::validation(format!("provider reply: {error}"))
}

/// The checker for replies to `owner` from `provider`: the parties every signed quote must name,
/// and the provider's own key as the only signer until a delegation is followed. Never built
/// without an expected signer, which would accept any.
fn checker(owner: &str, provider: &str) -> Result<CapClient, PubkySwapError> {
    let provider = bare_pubky(provider).to_string();
    let root = *provider_endpoint_id(&provider)?.as_bytes();
    let parties = Parties {
        owner: bare_pubky(owner).to_string(),
        provider,
    };
    Ok(CapClient::new(Some(root), parties))
}

impl<'a> CapSession<'a> {
    /// A session as `owner` (the wallet's bare swap identity) with `provider`, accepting replies
    /// signed by the provider's own key.
    pub(crate) fn new(
        client: &'a DirectClient,
        owner: &str,
        provider: &str,
    ) -> Result<Self, PubkySwapError> {
        Ok(Self {
            client,
            provider: bare_pubky(provider).to_string(),
            cap: checker(owner, provider)?,
            delegated_signer: None,
            commit: None,
        })
    }

    /// The delegated key this session accepted, to carry to a later session.
    pub(crate) fn delegated_signer(&self) -> Option<String> {
        self.delegated_signer.clone()
    }

    /// Accept a delegated key a previous session already followed from the provider's records.
    pub(crate) fn allow_delegated_signer(&mut self, signer: Option<String>) {
        if let Some(signer) = signer {
            self.cap.allow_signer(signer.clone());
            self.delegated_signer = Some(signer);
        }
    }

    /// Check a quote an earlier session received again, so this session can commit to it.
    pub(crate) fn remember_quote(
        &mut self,
        request: &QuoteRequest,
        signed: SignedQuote,
    ) -> Result<(), PubkySwapError> {
        self.cap
            .reply(
                &SwapMessage::QuoteRequest(request.clone()),
                CapMessage::Quote(signed),
            )
            .map(|_| ())
            .map_err(cap_error)
    }

    /// Send `request` in its signed form and return the checked reply in plain form, with the
    /// signed reply it came from. Offers and lists have no signed form and are refused here.
    pub(crate) async fn ask(
        &mut self,
        request: &SwapMessage,
    ) -> Result<(SwapMessage, CapMessage), PubkySwapError> {
        let message = self.cap.request(request).ok_or_else(|| {
            PubkySwapError::validation("this request has no signed form, or no signed quote")
        })?;
        if matches!(request, SwapMessage::SwapRequest(_)) {
            self.commit = Some(request.clone());
        }
        let reply = self.client.request_cap(&self.provider, &message).await?;
        self.accept_signer(&reply).await;
        match self.cap.reply(request, reply.clone()) {
            Ok(plain) => Ok((plain, reply)),
            // The provider has the swap but no signed contract for it: sending the same commit
            // again has it sign and store one.
            Err(CapError::MissingContract(commit)) => {
                let commit_request = self
                    .commit
                    .clone()
                    .ok_or_else(|| cap_error(CapError::MissingContract(commit.clone())))?;
                let reply = self
                    .client
                    .request_cap(&self.provider, &CapMessage::Commit(*commit))
                    .await?;
                self.accept_signer(&reply).await;
                let plain = self
                    .cap
                    .reply(&commit_request, reply.clone())
                    .map_err(cap_error)?;
                Ok((plain, reply))
            }
            Err(e) => Err(cap_error(e)),
        }
    }

    /// When a reply is signed by a key other than the provider's own, follow the provider's
    /// published records (§5) and accept that key only if they delegate to it. Failing that, the
    /// key stays refused and the reply with it.
    async fn accept_signer(&mut self, reply: &CapMessage) {
        let Some(signer) = reply_signer(reply).map(str::to_string) else {
            return;
        };
        if self.cap.allows(&signer) {
            return;
        }
        match tokio::time::timeout(DELEGATION_TIMEOUT, delegation::resolve(&self.provider)).await {
            Ok(Ok(Some(delegated))) if delegated == signer => {
                self.cap.allow_signer(delegated.clone());
                self.delegated_signer = Some(delegated);
            }
            Ok(Ok(Some(_))) => log::warn!(
                "pubky swaps: {} signed with a key its records do not delegate to",
                self.provider
            ),
            Ok(Ok(None)) => log::warn!(
                "pubky swaps: {} signed with another key and publishes no delegation",
                self.provider
            ),
            Ok(Err(e)) => log::warn!("pubky swaps: {}'s signing key: {e}", self.provider),
            Err(_) => log::warn!("pubky swaps: {}'s signing key took too long", self.provider),
        }
    }
}

#[cfg(test)]
mod tests {
    use swap_common::messages::{Quote, SwapAccept, SwapRequest, SwapScript, SwapStatusRequest};
    use swap_common::paykit_cap::{
        sign_contract, sign_quote, CommitBody, Ed25519Signer, RfqConstraints, Status,
    };
    use swap_common::{NetworkSpec, SwapDirection, SwapState, SwapStatusSnapshot};
    use uuid::Uuid;

    use super::*;

    const PROVIDER_SECRET: [u8; 32] = [1; 32];
    const OWNER_SECRET: [u8; 32] = [2; 32];

    fn z32(secret: &[u8; 32]) -> String {
        pubky::Keypair::from_secret(secret).public_key().to_z32()
    }

    fn parties() -> Parties {
        Parties {
            owner: z32(&OWNER_SECRET),
            provider: z32(&PROVIDER_SECRET),
        }
    }

    fn quote_request() -> QuoteRequest {
        QuoteRequest {
            request_id: Some(Uuid::from_bytes([3; 16])),
            offer_id: Uuid::from_bytes([4; 16]),
            client_pkarr: z32(&OWNER_SECRET),
            direction: SwapDirection::Reverse,
            amount_sat: 20_300,
            protocol_version: 0,
            features: vec![],
        }
    }

    fn quote() -> Quote {
        Quote {
            request_id: None,
            quote_id: Uuid::from_bytes([5; 16]),
            offer_id: Uuid::from_bytes([4; 16]),
            direction: SwapDirection::Reverse,
            amount_sat: 20_300,
            fee_sat: 300,
            service_fee_sat: 150,
            onchain_fee_sat: 150,
            fee_rate_sat_vb: 2,
            total_sat: 20_600,
            htlc_timeout_blocks: 144,
            required_confirmations: 1,
            valid_until_unix: 4_000_000_000,
            fund_within_secs: 60,
            protocol_version: 0,
        }
    }

    fn constraints() -> RfqConstraints {
        let request = quote_request();
        RfqConstraints {
            offer_id: request.offer_id,
            direction: request.direction,
            amount_sat: request.amount_sat,
        }
    }

    fn signed_by(secret: &[u8; 32]) -> SignedQuote {
        sign_quote(
            &Ed25519Signer::from_secret(secret),
            &parties(),
            &constraints(),
            &quote(),
        )
        .unwrap()
    }

    fn swap_request() -> SwapRequest {
        SwapRequest {
            script_type: SwapScript::P2wsh,
            quote_id: quote().quote_id,
            client_pkarr: z32(&OWNER_SECRET),
            direction: SwapDirection::Reverse,
            payment_hash_hex: "aa".repeat(32),
            client_claim_pubkey_hex: Some("02".to_string() + &"bb".repeat(32)),
            client_refund_pubkey_hex: None,
            invoice: None,
            owner_ref: Some("ref".into()),
        }
    }

    fn accept() -> SwapAccept {
        SwapAccept {
            script_type: SwapScript::P2wsh,
            swap_tree: None,
            quote_id: quote().quote_id,
            swap_id: Uuid::from_bytes([6; 16]),
            direction: SwapDirection::Reverse,
            htlc_script_hex: String::new(),
            htlc_address: String::new(),
            onchain_amount_sat: 20_300,
            timeout_block_height: 1_144,
            provider_pubkey_hex: "03".to_string() + &"cc".repeat(32),
            invoice: Some("lnbcrt-hold-invoice".into()),
        }
    }

    /// A checker that holds the signed quote, and the commit it sends for it.
    fn committed() -> (CapClient, SwapMessage) {
        let mut cap = checker(&z32(&OWNER_SECRET), &z32(&PROVIDER_SECRET)).unwrap();
        cap.reply(
            &SwapMessage::QuoteRequest(quote_request()),
            CapMessage::Quote(signed_by(&PROVIDER_SECRET)),
        )
        .unwrap();
        let request = SwapMessage::SwapRequest(swap_request());
        assert!(matches!(cap.request(&request), Some(CapMessage::Commit(_))));
        (cap, request)
    }

    fn contract_over(terms_hash: &str, commit: &CommitBody) -> ContractReplyFixture {
        sign_contract(
            &Ed25519Signer::from_secret(&PROVIDER_SECRET),
            terms_hash,
            commit,
            &accept(),
        )
        .unwrap()
    }

    type ContractReplyFixture = swap_common::paykit_cap::ContractReply;

    fn our_contract() -> ContractReplyFixture {
        contract_over(
            &signed_by(&PROVIDER_SECRET).terms_hash,
            &CommitBody::from_request(&swap_request()),
        )
    }

    #[test]
    fn takes_a_quote_signed_by_the_provider() {
        let mut cap = checker(&z32(&OWNER_SECRET), &z32(&PROVIDER_SECRET)).unwrap();
        let reply = cap.reply(
            &SwapMessage::QuoteRequest(quote_request()),
            CapMessage::Quote(signed_by(&PROVIDER_SECRET)),
        );
        assert!(matches!(reply, Ok(SwapMessage::Quote(q)) if q.quote_id == quote().quote_id));
    }

    #[test]
    fn refuses_a_quote_signed_by_another_key_until_it_is_delegated() {
        let other = [9; 32];
        let request = SwapMessage::QuoteRequest(quote_request());
        let mut cap = checker(&z32(&OWNER_SECRET), &z32(&PROVIDER_SECRET)).unwrap();
        assert!(matches!(
            cap.reply(&request, CapMessage::Quote(signed_by(&other))),
            Err(CapError::UnexpectedSigner)
        ));
        // Only a key the provider's records delegate to is added.
        cap.allow_signer(hex::encode(
            ed25519_dalek::SigningKey::from_bytes(&other)
                .verifying_key()
                .to_bytes(),
        ));
        assert!(cap
            .reply(&request, CapMessage::Quote(signed_by(&other)))
            .is_ok());
    }

    #[test]
    fn refuses_a_tampered_quote() {
        let request = SwapMessage::QuoteRequest(quote_request());
        let mut cheaper = signed_by(&PROVIDER_SECRET);
        cheaper.quote.total_sat -= 1;
        let mut renamed = signed_by(&PROVIDER_SECRET);
        renamed.terms_hash = "00".repeat(32);
        for tampered in [cheaper, renamed] {
            let mut cap = checker(&z32(&OWNER_SECRET), &z32(&PROVIDER_SECRET)).unwrap();
            assert!(cap.reply(&request, CapMessage::Quote(tampered)).is_err());
        }
        // A quote issued to another wallet does not name this one.
        let mut cap = checker(&z32(&[8; 32]), &z32(&PROVIDER_SECRET)).unwrap();
        assert!(cap
            .reply(&request, CapMessage::Quote(signed_by(&PROVIDER_SECRET)))
            .is_err());
    }

    #[test]
    fn takes_a_contract_signed_over_this_commit() {
        let (mut cap, request) = committed();
        let reply = cap.reply(&request, CapMessage::Contract(our_contract()));
        assert!(matches!(
            reply,
            Ok(SwapMessage::SwapAccept(a)) if a.invoice.as_deref() == Some("lnbcrt-hold-invoice")
        ));
    }

    #[test]
    fn refuses_a_contract_over_another_commit_or_tampered() {
        let terms_hash = signed_by(&PROVIDER_SECRET).terms_hash;
        let mut other_commit = CommitBody::from_request(&swap_request());
        other_commit.payment_hash_hex = "dd".repeat(32);
        let mut tampered = our_contract();
        tampered.signed.contract.accept.onchain_amount_sat -= 1;
        let mut other_blob = our_contract();
        other_blob.invoice_blob = contract_over(&terms_hash, &other_commit).invoice_blob;
        other_blob.invoice_blob.as_mut().unwrap().ciphertext = "00".repeat(40);
        for contract in [
            contract_over(&terms_hash, &other_commit),
            tampered,
            other_blob,
        ] {
            let (mut cap, request) = committed();
            assert!(cap.reply(&request, CapMessage::Contract(contract)).is_err());
        }
    }

    #[test]
    fn refuses_a_contract_before_a_signed_quote() {
        let mut cap = checker(&z32(&OWNER_SECRET), &z32(&PROVIDER_SECRET)).unwrap();
        let request = SwapMessage::SwapRequest(swap_request());
        // No signed quote, so no signed form to send and nothing to check a contract against.
        assert!(cap.request(&request).is_none());
        assert!(cap
            .reply(&request, CapMessage::Contract(our_contract()))
            .is_err());
    }

    fn status(snapshot_accept: SwapAccept, contract: Option<ContractReplyFixture>) -> CapMessage {
        CapMessage::Status(Box::new(Status {
            request_id: None,
            swap_id: Some(snapshot_accept.swap_id),
            quote_id: Some(snapshot_accept.quote_id),
            state: "lockup_confirmed".into(),
            reference: None,
            snapshot: Some(SwapStatusSnapshot {
                request_id: None,
                accept: SwapAccept {
                    invoice: None,
                    ..snapshot_accept
                },
                network: NetworkSpec::Regtest,
                state: SwapState::LockupConfirmed,
                funding_txid_hex: None,
                funding_vout: None,
                spend_txid_hex: None,
                claim_observed_txid_hex: None,
                required_confirmations: 1,
                updated_at_unix: 0,
                observed_at_unix: 0,
                created_at_unix: 0,
                payment_hash_hex: "aa".repeat(32),
                owner_ref: None,
                claimable: false,
            }),
            contract,
        }))
    }

    #[test]
    fn checks_a_status_against_the_contract_over_this_commit() {
        let request = SwapMessage::SwapStatusRequest(SwapStatusRequest {
            request_id: None,
            swap_id: None,
            quote_id: Some(quote().quote_id),
        });
        let (mut cap, _) = committed();
        assert!(cap
            .reply(&request, status(accept(), Some(our_contract())))
            .is_ok());
        // A snapshot that differs from what the contract signed.
        let mut moved = accept();
        moved.onchain_amount_sat -= 1;
        let (mut cap, _) = committed();
        assert!(cap
            .reply(&request, status(moved, Some(our_contract())))
            .is_err());
        // No contract for a committed swap: the session sends the commit again.
        let (mut cap, _) = committed();
        assert!(matches!(
            cap.reply(&request, status(accept(), None)),
            Err(CapError::MissingContract(_))
        ));
    }
}
