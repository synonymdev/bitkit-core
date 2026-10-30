//! `IntentsClient`: receive and pay over Lightning through an Arkade Intents solver, persist every
//! swap before it is handed out or funded, and drive claims and refunds in the background.

use super::bolt11;
use super::contract::derive_receive;
use super::contract::derive_send;
use super::contract::hex_32;
use super::contract::verify_receive;
use super::contract::verify_send;
use super::contract::Terms;
use super::contract::REFUND_MTP_LAG_SECONDS;
use super::error::Error;
use super::error::Result;
use super::preimage::payment_hash;
use super::preimage::salted_preimage;
use super::spend::live;
use super::spend::preimage_in;
use super::spend::spend_lockup;
use super::spend::Leaf;
use super::spend::MAX_LOCKUP_SPEND_INPUTS;
use super::store::Direction;
use super::store::Outcome;
use super::store::SwapRecord;
use super::store::SwapStore;
use super::transport::NostrTransport;
use super::transport::SolverCard;
use super::wallet::ArkWallet;
use super::wire;
use ark_core::server::Info;
use ark_core::server::VirtualTxOutPoint;
use ark_core::ArkAddress;
use bitcoin::Amount;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

pub type UpdateCallback = Arc<dyn Fn(&SwapRecord) + Send + Sync>;

/// What paying an invoice would cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayQuote {
    /// The lockup amount minus the invoice amount.
    pub fee_sat: u64,
    /// What the wallet funds.
    pub give_sat: u64,
    /// The invoice amount.
    pub take_sat: u64,
    pub expires_at: u64,
}

pub struct IntentsClient<W: ArkWallet, S: SwapStore> {
    wallet: Arc<W>,
    store: Arc<S>,
    transport: NostrTransport,
    listeners: RwLock<Vec<UpdateCallback>>,
    driving: tokio::sync::Mutex<()>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

enum Spend {
    Claim([u8; 32]),
    Refund,
    Unexplained,
}

struct SendPrep {
    quote: wire::RfqQuote,
    invoice: bolt11::InvoiceFacts,
    lockup: super::contract::SendLockup,
}

impl<W: ArkWallet + 'static, S: SwapStore + 'static> IntentsClient<W, S> {
    pub fn new(wallet: Arc<W>, store: Arc<S>, solver: SolverCard) -> Self {
        Self {
            wallet,
            store,
            transport: NostrTransport::new(solver),
            listeners: RwLock::new(Vec::new()),
            driving: tokio::sync::Mutex::new(()),
        }
    }

    /// Called with the record each time a swap's outcome (or blocked reason) changes.
    pub fn on_update(&self, callback: UpdateCallback) {
        self.listeners.write().expect("listeners").push(callback);
    }

    pub fn swaps(&self) -> Result<Vec<SwapRecord>> {
        self.store.all()
    }

    fn emit(&self, record: &SwapRecord) {
        for listener in self.listeners.read().expect("listeners").iter() {
            listener(record);
        }
    }

    fn save(
        &self,
        record: &mut SwapRecord,
        outcome: Outcome,
        blocked: Option<String>,
    ) -> Result<()> {
        if record.outcome == outcome && record.blocked_reason == blocked {
            return Ok(());
        }
        record.outcome = outcome;
        record.blocked_reason = blocked;
        record.updated_at = now();
        self.store.put(record)?;
        self.emit(record);
        Ok(())
    }

    async fn terms(&self) -> Result<(Info, Terms)> {
        let info = self.wallet.server_info().await?;
        let terms = Terms::from_server_info(&info)?;
        Ok((info, terms))
    }

    /// An invoice for exactly `amount_sat`: the payer pays it, the wallet gets it less the
    /// solver's fee. The record is persisted before the invoice is returned.
    pub async fn receive(&self, amount_sat: u64) -> Result<SwapRecord> {
        let (_, terms) = self.terms().await?;
        let keypair = self.wallet.identity_keypair()?;
        let payout_pubkey = keypair.x_only_public_key().0;
        let payout = self.wallet.address().await?;

        // `quote_conflict` means the solver already holds this hash; one retry with a fresh salt.
        let mut attempt = 0;
        let (salt, hash, rfq_id, quote) = loop {
            let salt: [u8; 32] = rand::random();
            let hash = hex::encode(payment_hash(&salted_preimage(&keypair, &salt)));
            let rfq_id = wire::new_rfq_id();
            let request = wire::receive_request(
                &rfq_id,
                amount_sat,
                wire::ReceiveProfile {
                    payment_hash: hash.clone(),
                    payout_address: payout.encode(),
                    payout_pubkey: payout_pubkey.to_string(),
                },
            );
            match self.transport.request_quote(&request).await {
                Ok(quote) => break (salt, hash, rfq_id, quote),
                Err(Error::Refused { ref reason, .. })
                    if reason == "quote_conflict" && attempt == 0 =>
                {
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        };

        let lockup = derive_receive(&quote, &hash, payout_pubkey, &payout, &terms)?;
        let invoice = bolt11::decode(&lockup.invoice)?;
        let time = now();
        let pay_deadline = verify_receive(&quote, amount_sat, &hash, &invoice, time)?;
        let (give, take) = (quote.give()?, quote.take()?);
        let record = SwapRecord {
            id: rfq_id.clone(),
            direction: Direction::Receive,
            // As TS reports a just-accepted receive; the first drive pass makes it `open`.
            outcome: Outcome::Accepted,
            give_sat: give,
            take_sat: take,
            fee_sat: give - take,
            bolt11: invoice.raw.clone(),
            payment_hash: hash,
            created_at: time,
            updated_at: time,
            blocked_reason: None,
            expires_at: pay_deadline,
            solver: self.transport.card.name.clone(),
            rfq_id,
            quote: serde_json::to_value(&quote).map_err(|e| Error::Other(e.to_string()))?,
            lockup: lockup.params,
            lockup_address: lockup.address.encode(),
            preimage_salt: Some(hex::encode(salt)),
            preimage: None,
            payout_address: payout.encode(),
            funding_txid: None,
            claim_txid: None,
            refund_txid: None,
        };
        self.store.put(&record)?;
        self.emit(&record);
        Ok(record)
    }

    async fn prepare_send(&self, bolt11: &str) -> Result<SendPrep> {
        let invoice = bolt11::decode(bolt11)?;
        if invoice.amount_sats == 0 {
            return Err(Error::Invoice(
                "amountless invoices cannot be paid by swap".into(),
            ));
        }
        let time = now();
        if time >= invoice.expires_at {
            return Err(Error::Invoice("invoice expired".into()));
        }
        let (_, terms) = self.terms().await?;
        let keypair = self.wallet.identity_keypair()?;
        let refund = self.wallet.address().await?;
        let rfq_id = wire::new_rfq_id();
        let request = wire::send_request(
            &rfq_id,
            wire::SendProfile {
                invoice: invoice.raw.clone(),
                refund_address: refund.encode(),
                client_refund_pubkey: keypair.x_only_public_key().0.to_string(),
            },
        );
        let quote = self.transport.request_quote(&request).await?;
        let time = now();
        verify_send(&quote, &invoice, time)?;
        let lockup = derive_send(
            &quote,
            &invoice.payment_hash,
            keypair.x_only_public_key().0,
            &refund,
            &terms,
            time,
        )?;
        Ok(SendPrep {
            quote,
            invoice,
            lockup,
        })
    }

    /// What paying `bolt11` would cost. Discloses the invoice to the solver; funds nothing.
    pub async fn quote_pay(&self, bolt11: &str) -> Result<PayQuote> {
        let prep = self.prepare_send(bolt11).await?;
        let (give, take) = (prep.quote.give()?, prep.quote.take()?);
        Ok(PayQuote {
            fee_sat: give - take,
            give_sat: give,
            take_sat: take,
            expires_at: prep.quote.valid_until,
        })
    }

    /// Pay `bolt11` by funding a lockup the solver claims once it has paid the invoice. Refuses a
    /// fee above `max_fee_sat`. The record is persisted before funding.
    pub async fn pay(&self, bolt11: &str, max_fee_sat: Option<u64>) -> Result<SwapRecord> {
        let prep = self.prepare_send(bolt11).await?;
        let (give, take) = (prep.quote.give()?, prep.quote.take()?);
        let fee = give - take;
        if let Some(max) = max_fee_sat {
            if fee > max {
                return Err(Error::FeeTooHigh { fee, max });
            }
        }
        let time = now();
        let mut record = SwapRecord {
            id: prep.quote.rfq_id.clone(),
            direction: Direction::Send,
            outcome: Outcome::Accepted,
            give_sat: give,
            take_sat: take,
            fee_sat: fee,
            bolt11: prep.invoice.raw.clone(),
            payment_hash: prep.invoice.payment_hash.clone(),
            created_at: time,
            updated_at: time,
            blocked_reason: None,
            expires_at: prep.quote.valid_until,
            solver: self.transport.card.name.clone(),
            rfq_id: prep.quote.rfq_id.clone(),
            quote: serde_json::to_value(&prep.quote).map_err(|e| Error::Other(e.to_string()))?,
            lockup: prep.lockup.params.clone(),
            lockup_address: prep.lockup.address.encode(),
            preimage_salt: None,
            preimage: None,
            payout_address: String::new(),
            funding_txid: None,
            claim_txid: None,
            refund_txid: None,
        };
        self.store.put(&record)?;
        self.emit(&record);
        if now() >= prep.quote.valid_until {
            self.save(
                &mut record,
                Outcome::Failed,
                Some("quote expired before funding".into()),
            )?;
            return Err(Error::QuoteExpired);
        }
        let txid = self
            .wallet
            .send(&prep.lockup.address, Amount::from_sat(give))
            .await
            .map_err(Error::from)?;
        record.funding_txid = Some(txid.to_string());
        self.save(&mut record, Outcome::Funded, None)?;
        Ok(record)
    }

    /// Run [`Self::drive_once`] every `every` until the handle is aborted.
    pub fn drive(self: &Arc<Self>, every: Duration) -> tokio::task::JoinHandle<()> {
        let client = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if let Err(e) = client.drive_once().await {
                    log::warn!("arkade intents drive pass failed: {e}");
                }
                tokio::time::sleep(every).await;
            }
        })
    }

    /// One pass over every open swap: claim funded receives, refund expired sends, and record what
    /// the lockups show.
    pub async fn drive_once(&self) -> Result<()> {
        let _guard = self.driving.lock().await;
        let open: Vec<_> = self
            .store
            .all()?
            .into_iter()
            .filter(|r| !r.outcome.is_terminal())
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        let info = self.wallet.server_info().await?;
        for mut record in open {
            let result = match record.direction {
                Direction::Receive => self.drive_receive(&info, &mut record).await,
                Direction::Send => self.drive_send(&info, &mut record).await,
            };
            if let Err(e) = result {
                log::warn!("arkade intents swap {}: {e}", record.id);
            }
        }
        Ok(())
    }

    async fn lockup_vtxos(
        &self,
        record: &SwapRecord,
    ) -> Result<(ArkAddress, Vec<VirtualTxOutPoint>)> {
        let address = ArkAddress::decode(&record.lockup_address)
            .map_err(|e| Error::Store(format!("lockup address: {e}")))?;
        let vtxos = self.wallet.vtxos(&address).await?;
        Ok((address, vtxos))
    }

    /// Read the transactions that spent a lockup: the preimage if any carries it, else whether
    /// one pays the lockup's `sender` destination (a refund). `None` when nothing could be read.
    async fn spend_evidence(
        &self,
        spent: &[VirtualTxOutPoint],
        hash: &[u8; 32],
        refund_script: &bitcoin::ScriptBuf,
    ) -> Result<Option<Spend>> {
        let mut seen_any = false;
        let mut refunded = false;
        for vtxo in spent {
            for txid in [vtxo.spent_by, vtxo.ark_txid].into_iter().flatten() {
                if let Some(tx) = self.wallet.virtual_tx(txid).await? {
                    seen_any = true;
                    if let Some(p) = preimage_in(&tx, hash) {
                        return Ok(Some(Spend::Claim(p)));
                    }
                    refunded |= tx
                        .unsigned_tx
                        .output
                        .iter()
                        .any(|o| o.script_pubkey == *refund_script);
                }
            }
        }
        Ok(seen_any.then_some(if refunded {
            Spend::Refund
        } else {
            Spend::Unexplained
        }))
    }

    async fn drive_receive(&self, info: &Info, record: &mut SwapRecord) -> Result<()> {
        let (_, vtxos) = self.lockup_vtxos(record).await?;
        let time = now();
        let refund_locktime = u64::from(record.lockup.refund_locktime);
        let live = live(&vtxos, info.dust);
        let hash = hex_32(&record.payment_hash, "payment_hash")?;

        if live.is_empty() && !vtxos.is_empty() {
            if vtxos
                .iter()
                .any(|v| v.is_unrolled || (!v.is_spent && v.is_swept))
            {
                return self.save(record, Outcome::NeedsRecovery, Some(
                    "the lockup left the offchain path (swept or unrolled); only an onchain spend can move it".into(),
                ));
            }
            // Fully spent: ours (or any claim revealing our P) is a claim; anything else is the
            // solver taking it back.
            let ours = record.claim_txid.as_deref().is_some_and(|c| {
                vtxos
                    .iter()
                    .any(|v| v.ark_txid.map(|t| t.to_string()).as_deref() == Some(c))
            });
            if ours {
                return self.save(record, Outcome::Claimed, None);
            }
            let solver_refund = record.lockup.sender_pk_script.clone();
            return match self.spend_evidence(&vtxos, &hash, &solver_refund).await? {
                Some(Spend::Claim(_)) => self.save(record, Outcome::Claimed, None),
                Some(Spend::Refund) => self.save(record, Outcome::Lapsed, None),
                Some(Spend::Unexplained) => self.save(
                    record,
                    Outcome::NeedsRecovery,
                    Some(
                        "the lockup was spent without our preimage or a refund to the solver"
                            .into(),
                    ),
                ),
                None => Ok(()),
            };
        }

        if time >= refund_locktime {
            // The window closed. Publishing P into the solver's live refund window risks a race.
            if record.claim_txid.is_some() {
                return Ok(());
            }
            if time < refund_locktime + REFUND_MTP_LAG_SECONDS {
                return self.save(record, Outcome::NeedsRecovery, Some(
                    "the claim window closed with the lockup unclaimed; only the solver can act now".into(),
                ));
            }
            return self.save(record, Outcome::Lapsed, None);
        }

        if live.is_empty() {
            return self.save(record, Outcome::Open, None);
        }
        let funded: u64 = live.iter().map(|v| v.amount.to_sat()).sum();
        // The anti-dust gate: claiming publishes P, which settles the payer's HTLC in full.
        if record.claim_txid.is_none() && funded < record.take_sat {
            return self.save(
                record,
                Outcome::NeedsRecovery,
                Some(format!(
                "the lockup holds {funded} sat, below the agreed {}; not revealing the preimage",
                record.take_sat
            )),
            );
        }
        let salt = hex_32(
            record
                .preimage_salt
                .as_deref()
                .ok_or_else(|| Error::Store("receive record has no salt".into()))?,
            "preimage_salt",
        )?;
        let keypair = self.wallet.identity_keypair()?;
        let preimage = salted_preimage(&keypair, &salt);
        if payment_hash(&preimage) != hash {
            return self.save(
                record,
                Outcome::NeedsRecovery,
                Some("this wallet's key does not re-derive the swap's preimage".into()),
            );
        }
        let mut batch = live.clone();
        batch.sort_by_key(|v| std::cmp::Reverse(v.amount));
        batch.truncate(MAX_LOCKUP_SPEND_INPUTS);
        let lockup = record.lockup.build()?;
        let payout = ArkAddress::decode(&record.payout_address)
            .map_err(|e| Error::Store(format!("payout address: {e}")))?;
        let txid = spend_lockup(
            self.wallet.as_ref(),
            info,
            &lockup,
            &batch,
            &keypair,
            Leaf::Claim { preimage },
            &payout,
        )
        .await?;
        record.claim_txid = Some(txid.to_string());
        let outcome = if batch.len() == live.len() {
            Outcome::Claimed
        } else {
            Outcome::Funded
        };
        record.updated_at = now();
        self.store.put(record)?;
        self.save(record, outcome, None)
    }

    async fn drive_send(&self, info: &Info, record: &mut SwapRecord) -> Result<()> {
        let (_, vtxos) = self.lockup_vtxos(record).await?;
        let time = now();
        let refund_locktime = u64::from(record.lockup.refund_locktime);

        if record.outcome == Outcome::Accepted {
            if vtxos.is_empty() {
                if time > record.expires_at + 600 {
                    return self.save(
                        record,
                        Outcome::Failed,
                        Some("the lockup was never funded before the quote expired".into()),
                    );
                }
                return Ok(());
            }
            self.save(record, Outcome::Funded, None)?;
        }

        let live = live(&vtxos, info.dust);
        if live.is_empty() && !vtxos.is_empty() {
            if vtxos
                .iter()
                .any(|v| v.is_unrolled || (!v.is_spent && v.is_swept))
            {
                return self.save(record, Outcome::NeedsRecovery, Some(
                    "the lockup left the offchain path (swept or unrolled); only an onchain spend can move it".into(),
                ));
            }
            let ours = record.refund_txid.as_deref().is_some_and(|r| {
                vtxos
                    .iter()
                    .any(|v| v.ark_txid.map(|t| t.to_string()).as_deref() == Some(r))
            });
            if ours {
                return self.save(record, Outcome::Refunded, None);
            }
            let hash = hex_32(&record.payment_hash, "payment_hash")?;
            let our_refund = record.lockup.sender_pk_script.clone();
            return match self.spend_evidence(&vtxos, &hash, &our_refund).await? {
                Some(Spend::Claim(p)) => {
                    record.preimage = Some(hex::encode(p));
                    self.save(record, Outcome::Paid, None)
                }
                // The solver's refund paths, which all pay our refund address.
                Some(Spend::Refund) => self.save(record, Outcome::Refunded, None),
                // Never guess `refunded`: a paid invoice read as refunded invites paying twice.
                Some(Spend::Unexplained) => self.save(
                    record,
                    Outcome::NeedsRecovery,
                    Some(
                        "the lockup was spent without a readable preimage or a refund to us".into(),
                    ),
                ),
                None => Ok(()),
            };
        }

        if !live.is_empty() && time >= refund_locktime {
            let keypair = self.wallet.identity_keypair()?;
            let lockup = record.lockup.build()?;
            let refund = self.wallet.address().await?;
            let mut batch = live.clone();
            batch.truncate(MAX_LOCKUP_SPEND_INPUTS);
            let txid = spend_lockup(
                self.wallet.as_ref(),
                info,
                &lockup,
                &batch,
                &keypair,
                Leaf::RefundWithoutReceiver,
                &refund,
            )
            .await?;
            record.refund_txid = Some(txid.to_string());
            let outcome = if batch.len() == live.len() {
                Outcome::Refunded
            } else {
                Outcome::Funded
            };
            record.updated_at = now();
            self.store.put(record)?;
            return self.save(record, outcome, None);
        }
        Ok(())
    }
}
