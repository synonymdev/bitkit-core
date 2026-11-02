//! The lockup each corridor compiles, derived from the quote's binding fields plus the wallet's
//! own data, and the checks a quote must pass before an invoice is published or a lockup funded.
//! Mirrors `rfq.ts` (`deriveLightningReceive`, `deriveLightningSend`, `assertReceivable`,
//! `assertFundable`) and `client/verify.ts`.

use super::bolt11::InvoiceFacts;
use super::error::Error;
use super::error::Result;
use super::vhtlc::seconds_sequence;
use super::vhtlc::NonInteractiveParameters;
use super::vhtlc::VhtlcOptions;
use super::vhtlc::VhtlcV2;
use super::wire::RfqQuote;
use ark_core::ArkAddress;
use bitcoin::hashes::ripemd160;
use bitcoin::hashes::Hash;
use bitcoin::Network;
use bitcoin::ScriptBuf;
use bitcoin::XOnlyPublicKey;
use serde::Deserialize;
use serde::Serialize;

const SEQUENCE_GRANULARITY_SECONDS: u32 = 512;
/// How long the sender's solo refund opens after the receiver's unilateral claim.
pub const SOLO_REFUND_HEADROOM_SECONDS: u32 = 8 * SEQUENCE_GRANULARITY_SECONDS;
/// Receive: the floor between the last moment the hold invoice can be paid and the solver's
/// refund opening.
pub const MIN_CLAIM_WINDOW_SECONDS: u64 = 30 * 60;
/// Send: refuse to fund unless this long remains before the refund path opens (MTP lag).
pub const MIN_HEADROOM_SECONDS: u64 = 90 * 60;
/// Send: the longest refund window the client accepts.
pub const MAX_REFUND_WINDOW_SECONDS: u64 = 30 * 24 * 3600;
/// The solver's CLTV matures on median-time-past, which trails wall clock.
pub const REFUND_MTP_LAG_SECONDS: u64 = 2 * 60 * 60;

/// Covenant co-signer ("emulator") keys, pinned per network as in `@arkade-os/sdk`. Never the
/// one a server reports: it lands in a leaf that decides who can move the funds.
pub fn emulator_pubkey(network: Network) -> Result<XOnlyPublicKey> {
    let hex = match network {
        Network::Bitcoin => "39c196415da47b26456a101daaa12ba9e445bfe153197f1e2b750bf40e52092e",
        Network::Regtest => "999413c46fa10ada5cbc4bcc79a1d09160c2ba3cfc812705d7a13e5e545fb2a9",
        Network::Signet => "f823b9b2febc81f4af967e77aed2f541cbd3397c6d8f5a72e32eb7b471af889a",
        other => return Err(Error::Other(format!("no emulator is pinned for {other}"))),
    };
    Ok(hex.parse().expect("valid pinned key"))
}

/// What both corridors read off the wallet's own Arkade connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terms {
    pub operator: XOnlyPublicKey,
    pub emulator: XOnlyPublicKey,
    /// `unilateralClaimDelay`, seconds: the operator's exit delay rounded up to 512.
    pub claim_delay: u32,
    pub network: Network,
}

impl Terms {
    pub fn from_server_info(info: &ark_core::server::Info) -> Result<Self> {
        let delay = info.unilateral_exit_delay;
        if !delay.is_time_locked() {
            return Err(Error::Other(format!(
                "operator exit delay {delay} is not in seconds"
            )));
        }
        let exit_delay_seconds = (delay.to_consensus_u32() & 0xffff) * SEQUENCE_GRANULARITY_SECONDS;
        Ok(Self {
            operator: info.signer_pk.x_only_public_key().0,
            emulator: emulator_pubkey(info.network)?,
            claim_delay: unilateral_claim_delay(exit_delay_seconds)?,
            network: info.network,
        })
    }
}

pub fn unilateral_claim_delay(operator_exit_delay_seconds: u32) -> Result<u32> {
    let max = 0xffff * SEQUENCE_GRANULARITY_SECONDS - SOLO_REFUND_HEADROOM_SECONDS;
    if !(SEQUENCE_GRANULARITY_SECONDS..=max).contains(&operator_exit_delay_seconds) {
        return Err(Error::Other(format!(
            "operator exit delay {operator_exit_delay_seconds}s is out of range"
        )));
    }
    Ok(
        operator_exit_delay_seconds.div_ceil(SEQUENCE_GRANULARITY_SECONDS)
            * SEQUENCE_GRANULARITY_SECONDS,
    )
}

/// Everything a lockup is rebuilt from; persisted with the swap record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockupParams {
    pub sender: XOnlyPublicKey,
    pub receiver: XOnlyPublicKey,
    pub server: XOnlyPublicKey,
    pub emulator: XOnlyPublicKey,
    /// `sha256(P)`, hex.
    pub payment_hash: String,
    pub refund_locktime: u32,
    pub claim_delay: u32,
    pub refund_without_receiver_delay: u32,
    pub sender_pk_script: ScriptBuf,
    pub receiver_pk_script: ScriptBuf,
    pub legacy_pre_timelocked_refund: bool,
}

impl LockupParams {
    pub fn build(&self) -> Result<VhtlcV2> {
        let hash = hex_32(&self.payment_hash, "payment_hash")?;
        Ok(VhtlcV2::new(
            VhtlcOptions {
                sender: self.sender,
                receiver: self.receiver,
                server: self.server,
                preimage_hash: ripemd160::Hash::hash(&hash),
                refund_locktime: self.refund_locktime,
                unilateral_claim_delay: seconds_sequence(self.claim_delay)?,
                // Level with the claim: a two-signature leaf neither party spends alone.
                unilateral_refund_delay: seconds_sequence(self.claim_delay)?,
                unilateral_refund_without_receiver_delay: seconds_sequence(
                    self.refund_without_receiver_delay,
                )?,
            },
            NonInteractiveParameters {
                emulator_pubkey: self.emulator,
                receiver_pk_script: self.receiver_pk_script.clone(),
                sender_pk_script: self.sender_pk_script.clone(),
                legacy_pre_timelocked_refund: self.legacy_pre_timelocked_refund,
            },
        )?)
    }

    /// Build both shapes the quote may use (full suite first) and keep the one whose address
    /// the solver quoted. The quoted address is compare-only: a mismatch refuses, never adopts.
    fn matching(
        mut self,
        quote: &RfqQuote,
        network: Network,
    ) -> Result<(Self, VhtlcV2, ArkAddress)> {
        let quoted = quote.profile_str("lockup_address")?;
        let mut derived = Vec::new();
        for legacy in [false, true] {
            self.legacy_pre_timelocked_refund = legacy;
            let script = self.build()?;
            let address = script.address(network);
            if address.encode() == quoted {
                return Ok((self, script, address));
            }
            derived.push(address.encode());
        }
        Err(Error::verification(
            "lockup_address",
            derived.join(" or "),
            quoted,
        ))
    }
}

pub fn hex_32(value: &str, field: &'static str) -> Result<[u8; 32]> {
    hex::decode(value)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| Error::verification(field, "32 bytes of hex", value))
}

/// `toXOnly`: a 32-byte x-only key, or a 33-byte compressed one.
fn solver_key(quote: &RfqQuote) -> Result<XOnlyPublicKey> {
    let bytes = hex::decode(&quote.solver_pubkey)
        .map_err(|_| Error::verification("solver_pubkey", "hex", &quote.solver_pubkey))?;
    let key = match bytes.len() {
        32 => XOnlyPublicKey::from_slice(&bytes).ok(),
        33 => bitcoin::secp256k1::PublicKey::from_slice(&bytes)
            .ok()
            .map(|k| k.x_only_public_key().0),
        _ => None,
    };
    key.ok_or_else(|| Error::verification("solver_pubkey", "a public key", &quote.solver_pubkey))
}

fn p2tr_script(value: &str, field: &'static str) -> Result<ScriptBuf> {
    let bytes = hex::decode(value).map_err(|_| Error::verification(field, "hex", value))?;
    if bytes.len() != 34 || bytes[0] != 0x51 || bytes[1] != 0x20 {
        return Err(Error::verification(field, "a P2TR pkScript", value));
    }
    Ok(ScriptBuf::from_bytes(bytes))
}

/// The receive lockup the solver must fund, derived and matched against the quote.
#[derive(Debug, Clone)]
pub struct ReceiveLockup {
    pub params: LockupParams,
    pub script: VhtlcV2,
    pub address: ArkAddress,
    /// The solver's hold invoice on our hash.
    pub invoice: String,
}

/// `deriveLightningReceive`: the trader is `receiver` (it holds `P`), the solver `sender`.
pub fn derive_receive(
    quote: &RfqQuote,
    payment_hash: &str,
    payout_pubkey: XOnlyPublicKey,
    payout_address: &ArkAddress,
    terms: &Terms,
) -> Result<ReceiveLockup> {
    let refund_locktime = quote
        .refund_locktime
        .ok_or_else(|| Error::verification("refund_window", "refund_locktime", "missing"))?;
    let invoice = quote.profile_str("invoice")?.to_string();
    let params = LockupParams {
        sender: solver_key(quote)?,
        receiver: payout_pubkey,
        server: terms.operator,
        emulator: terms.emulator,
        payment_hash: payment_hash.to_string(),
        refund_locktime: u32::try_from(refund_locktime)
            .map_err(|_| Error::verification("refund_window", "a u32 locktime", refund_locktime))?,
        claim_delay: terms.claim_delay,
        refund_without_receiver_delay: terms.claim_delay + SOLO_REFUND_HEADROOM_SECONDS,
        sender_pk_script: p2tr_script(
            quote.profile_str("solver_refund_pk_script")?,
            "solver_refund_pk_script",
        )?,
        receiver_pk_script: payout_address.to_p2tr_script_pubkey(),
        legacy_pre_timelocked_refund: false,
    };
    let (params, script, address) = params.matching(quote, terms.network)?;
    Ok(ReceiveLockup {
        params,
        script,
        address,
        invoice,
    })
}

/// The receive gates (`quoteLightningReceive`): the pinned give side, the hold invoice bound to
/// our hash and the quoted amount, and a claim window. Returns the pay deadline.
pub fn verify_receive(
    quote: &RfqQuote,
    amount_sat: u64,
    payment_hash: &str,
    invoice: &InvoiceFacts,
    now: u64,
) -> Result<u64> {
    let (give, take) = (quote.give()?, quote.take()?);
    if give != amount_sat {
        return Err(Error::verification("amount", amount_sat, give));
    }
    if take > give {
        return Err(Error::verification(
            "amount",
            "take <= give",
            format!("{give} -> {take}"),
        ));
    }
    if take == 0 {
        return Err(Error::verification("amount", "a positive take", take));
    }
    if invoice.payment_hash != payment_hash {
        return Err(Error::verification(
            "invoice",
            payment_hash,
            &invoice.payment_hash,
        ));
    }
    if invoice.amount_sats == 0 {
        return Err(Error::verification(
            "invoice",
            give,
            "the invoice names no amount",
        ));
    }
    if invoice.amount_sats != give {
        return Err(Error::verification("invoice", give, invoice.amount_sats));
    }
    let pay_deadline = invoice.expires_at.min(quote.valid_until);
    if now >= pay_deadline {
        return Err(Error::QuoteExpired);
    }
    let refund_locktime = quote
        .refund_locktime
        .ok_or_else(|| Error::verification("refund_window", "refund_locktime", "missing"))?;
    if refund_locktime < pay_deadline + MIN_CLAIM_WINDOW_SECONDS {
        return Err(Error::verification(
            "refund_window",
            format!("at least {MIN_CLAIM_WINDOW_SECONDS}s to claim after the pay deadline"),
            refund_locktime.saturating_sub(pay_deadline),
        ));
    }
    Ok(pay_deadline)
}

/// The send lockup we fund, derived and matched against the quote.
#[derive(Debug, Clone)]
pub struct SendLockup {
    pub params: LockupParams,
    pub script: VhtlcV2,
    pub address: ArkAddress,
}

/// `deriveLightningSend`: the trader is `sender` (it holds the refund), the solver `receiver`.
pub fn derive_send(
    quote: &RfqQuote,
    payment_hash: &str,
    sender_pubkey: XOnlyPublicKey,
    refund_address: &ArkAddress,
    terms: &Terms,
    now: u64,
) -> Result<SendLockup> {
    let refund_locktime = quote
        .refund_locktime
        .ok_or_else(|| Error::verification("refund_window", "refund_locktime", "missing"))?;
    let solo = quote.profile_u64("refund_without_receiver_delay")?;
    let valid_solo = solo >= u64::from(terms.claim_delay)
        && solo % u64::from(SEQUENCE_GRANULARITY_SECONDS) == 0
        && solo <= 0xffff * u64::from(SEQUENCE_GRANULARITY_SECONDS);
    if !valid_solo {
        return Err(Error::verification(
            "refund_window",
            "a valid refund_without_receiver_delay",
            solo,
        ));
    }
    if solo < refund_locktime.saturating_sub(now) {
        return Err(Error::verification(
            "refund_window",
            "the solo refund opening after refund_locktime",
            solo,
        ));
    }
    let params = LockupParams {
        sender: sender_pubkey,
        receiver: solver_key(quote)?,
        server: terms.operator,
        emulator: terms.emulator,
        payment_hash: payment_hash.to_string(),
        refund_locktime: u32::try_from(refund_locktime)
            .map_err(|_| Error::verification("refund_window", "a u32 locktime", refund_locktime))?,
        claim_delay: terms.claim_delay,
        refund_without_receiver_delay: solo as u32,
        sender_pk_script: refund_address.to_p2tr_script_pubkey(),
        receiver_pk_script: p2tr_script(
            quote.profile_str("receiver_pk_script")?,
            "receiver_pk_script",
        )?,
        legacy_pre_timelocked_refund: false,
    };
    let (params, script, address) = params.matching(quote, terms.network)?;
    Ok(SendLockup {
        params,
        script,
        address,
    })
}

/// The send gates (`verifySendInvoice`, `assertFundable`, `verifyRefundWindow`): the take side is
/// the invoice verbatim, the spread is not negative, and the clocks leave room to refund.
pub fn verify_send(quote: &RfqQuote, invoice: &InvoiceFacts, now: u64) -> Result<()> {
    let (give, take) = (quote.give()?, quote.take()?);
    if take != invoice.amount_sats {
        return Err(Error::verification("invoice", invoice.amount_sats, take));
    }
    if give < take {
        return Err(Error::verification(
            "invoice",
            format!("at least {take}"),
            give,
        ));
    }
    if quote
        .profile
        .get("payment_hash")
        .and_then(|v| v.as_str())
        .is_some_and(|h| h != invoice.payment_hash)
    {
        return Err(Error::verification(
            "invoice",
            &invoice.payment_hash,
            quote.profile_str("payment_hash")?,
        ));
    }
    if now >= invoice.expires_at {
        return Err(Error::Invoice("invoice expired".into()));
    }
    if now >= quote.valid_until {
        return Err(Error::QuoteExpired);
    }
    let refund_locktime = quote
        .refund_locktime
        .ok_or_else(|| Error::verification("refund_window", "refund_locktime", "missing"))?;
    if refund_locktime < now + MIN_HEADROOM_SECONDS {
        return Err(Error::verification(
            "refund_window",
            format!("at least {MIN_HEADROOM_SECONDS}s before the refund opens"),
            refund_locktime.saturating_sub(now),
        ));
    }
    if refund_locktime - now > MAX_REFUND_WINDOW_SECONDS {
        return Err(Error::verification(
            "refund_window",
            format!("at most {MAX_REFUND_WINDOW_SECONDS}s"),
            refund_locktime - now,
        ));
    }
    let solo = quote.profile_u64("refund_without_receiver_delay")?;
    if solo >= MAX_REFUND_WINDOW_SECONDS + u64::from(SEQUENCE_GRANULARITY_SECONDS) {
        return Err(Error::verification(
            "refund_window",
            format!("a solo-refund delay of at most {MAX_REFUND_WINDOW_SECONDS}s"),
            solo,
        ));
    }
    Ok(())
}
