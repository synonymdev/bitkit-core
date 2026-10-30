//! The records the apps get back: the shapes of the bridge's results (`bridge.js` and `ArkadeService.kt`).

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ArkadeNetwork {
    /// Mainnet, against Ark Labs' operator.
    Bitcoin,
    /// Arkade's signet, for dev builds.
    Mutinynet,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArkadeStartParams {
    /// Where the wallet keeps its database; the app gives each wallet its own.
    pub data_dir: String,
    pub mnemonic: String,
    pub passphrase: Option<String>,
    pub network: ArkadeNetwork,
    /// Scan the operator and the chain for the wallet's past outputs before returning, as a wallet restored from
    /// its phrase needs (TypeScript `wallet.restore()`).
    pub restore: bool,
    /// An `export_state` dump to import before the wallet opens its storage.
    pub backup: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeFees {
    /// The operator's fee programs (CEL), as `/v1/info` gives them.
    pub intent_offchain_input: String,
    pub intent_offchain_output: String,
    pub intent_onchain_input: String,
    pub intent_onchain_output: String,
    pub tx_fee_rate: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeInfo {
    pub ark_address: String,
    pub boarding_address: String,
    /// `bitcoin` or `mutinynet`.
    pub network: String,
    pub dust: u64,
    /// Seconds (or blocks, below 512), as the operator advertises them.
    pub unilateral_exit_delay: u64,
    pub boarding_exit_delay: u64,
    pub fees: ArkadeFees,
    /// Whether a delegate renews this wallet's outputs while it is offline.
    pub delegate: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeBalance {
    /// Spendable now: settled plus preconfirmed.
    pub available: u64,
    pub settled: u64,
    pub preconfirmed: u64,
    pub recoverable: u64,
    pub boarding_confirmed: u64,
    pub boarding_unconfirmed: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeVtxo {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    /// `preconfirmed`, `settled`, `swept` or `spent`.
    pub state: String,
    /// Unix seconds after which the operator may sweep this output unless it is renewed.
    pub batch_expiry: Option<u64>,
    /// Unix milliseconds, as the bridge reports it.
    pub created_at: Option<u64>,
    pub is_preconfirmed: bool,
    pub is_swept: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeHistoryEntry {
    /// The non-empty ids of `boarding_txid`, `commitment_txid` and `ark_txid`, joined by `:`.
    pub key: String,
    pub boarding_txid: Option<String>,
    pub commitment_txid: Option<String>,
    pub ark_txid: Option<String>,
    /// `SENT` or `RECEIVED` (the bridge's `type`).
    pub tx_type: String,
    /// `boarding`, `batch`, `offchain` or `exit`.
    pub tag: String,
    pub amount: u64,
    pub settled: bool,
    /// Unix milliseconds.
    pub created_at: u64,
}

/// A Lightning swap through Arkade Intents (the app's `ArkadeSwap`): `receive` pays into the wallet, `send` pays
/// an invoice out of it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadeSwap {
    pub id: String,
    /// `receive` or `send`.
    pub direction: String,
    /// `accepted`, `open`, `funded`, `claimed`, `paid`, `refunded`, `lapsed`, `needs_recovery` or `failed`.
    pub outcome: String,
    pub give_sat: u64,
    pub take_sat: u64,
    pub fee_sat: u64,
    pub bolt11: String,
    pub payment_hash: String,
    /// Unix seconds.
    pub created_at: u64,
    pub updated_at: u64,
    pub blocked_reason: Option<String>,
    /// Receive: the last moment the invoice can be paid. Send: when the quote lapses.
    pub expires_at: u64,
}

impl From<&super::intents::SwapRecord> for ArkadeSwap {
    fn from(r: &super::intents::SwapRecord) -> Self {
        Self {
            id: r.id.clone(),
            direction: match r.direction {
                super::intents::Direction::Receive => "receive",
                super::intents::Direction::Send => "send",
            }
            .to_string(),
            outcome: r.outcome.as_str().to_string(),
            give_sat: r.give_sat,
            take_sat: r.take_sat,
            fee_sat: r.fee_sat,
            bolt11: r.bolt11.clone(),
            payment_hash: r.payment_hash.clone(),
            created_at: r.created_at,
            updated_at: r.updated_at,
            blocked_reason: r.blocked_reason.clone(),
            expires_at: r.expires_at,
        }
    }
}

/// What paying an invoice would cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArkadePayQuote {
    /// `give_sat - take_sat`.
    pub fee_sat: u64,
    /// What the wallet funds.
    pub give_sat: u64,
    /// The invoice amount.
    pub take_sat: u64,
    pub expires_at: u64,
}

impl From<super::intents::PayQuote> for ArkadePayQuote {
    fn from(q: super::intents::PayQuote) -> Self {
        Self {
            fee_sat: q.fee_sat,
            give_sat: q.give_sat,
            take_sat: q.take_sat,
            expires_at: q.expires_at,
        }
    }
}

pub const SENT: &str = "SENT";
pub const RECEIVED: &str = "RECEIVED";

pub struct NetworkConfig {
    pub name: &'static str,
    pub network: bitcoin::Network,
    pub ark_server_url: &'static str,
    pub delegate_url: &'static str,
    pub esplora_url: &'static str,
    pub lnurl_url: Option<&'static str>,
}

impl ArkadeNetwork {
    pub fn config(self) -> NetworkConfig {
        match self {
            Self::Bitcoin => NetworkConfig {
                name: "bitcoin",
                network: bitcoin::Network::Bitcoin,
                ark_server_url: "https://arkade.computer",
                delegate_url: "https://delegate.arkade.money",
                esplora_url: "https://mempool.space/api",
                lnurl_url: Some("https://lnurl.arkade.sh"),
            },
            Self::Mutinynet => NetworkConfig {
                name: "mutinynet",
                network: bitcoin::Network::Signet,
                ark_server_url: "https://mutinynet.arkade.sh",
                delegate_url: "https://delegator.mutinynet.arkade.sh",
                esplora_url: "https://mutinynet.com/api",
                lnurl_url: None,
            },
        }
    }
}
