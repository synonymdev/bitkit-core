use crate::modules::boltz::errors::BoltzError;
use crate::modules::boltz::types::{BoltzNetwork, BoltzSwap, BoltzSwapStatus, BoltzSwapType};
use boltz_client::swaps::boltz::{CreateReverseResponse, CreateSubmarineResponse};
use boltz_client::util::secrets::{Preimage, SwapMasterKey};
use boltz_client::Keypair;
use rusqlite::Connection;
use tokio::sync::Mutex;

/// SQLite-backed store for Boltz swaps.
///
/// Wraps a single connection behind an async mutex (mirroring the blocktank
/// module). No swap secrets are persisted: each swap's key and (for reverse
/// swaps) preimage are *re-derived on demand* from the wallet mnemonic and the
/// swap's deterministic [`SwapRecord::swap_index`] via BIP85
/// ([`derive_swap_keypair`]). The database therefore holds no key material —
/// only the index needed to reconstruct it given the seed.
pub struct BoltzDB {
    pub(crate) conn: Mutex<Connection>,
}

/// Re-derive a swap's secp256k1 keypair from the wallet mnemonic.
///
/// Keys are derived via Boltz's BIP85 scheme: the wallet mnemonic yields a
/// per-wallet swap master key, and each swap uses a unique `index` under it
/// (`m/26589'/0'/0'/{index}`). The same `(mnemonic, passphrase, index)` always
/// reproduces the same key, so swaps are recoverable from the seed alone — the
/// derived swap mnemonic can also be registered with Boltz's rescue API.
///
/// The key value is independent of `network`; the parameter is accepted for
/// symmetry with the rest of the API and to match how the swap was created.
pub(crate) fn derive_swap_keypair(
    mnemonic: &str,
    passphrase: Option<&str>,
    network: BoltzNetwork,
    index: u64,
) -> Result<Keypair, BoltzError> {
    let master =
        SwapMasterKey::new(mnemonic, passphrase, network.as_client_network()).map_err(|e| {
            BoltzError::InvalidInput {
                error_details: format!("Invalid mnemonic or key derivation failed: {}", e),
            }
        })?;
    master.derive_swapkey(index).map_err(BoltzError::from)
}

pub const CREATE_SWAPS_TABLE: &str = "CREATE TABLE IF NOT EXISTS swaps (
    id TEXT PRIMARY KEY,
    swap_type TEXT NOT NULL,
    status TEXT NOT NULL,
    network TEXT NOT NULL,
    electrum_url TEXT NOT NULL,
    swap_index INTEGER NOT NULL,
    invoice TEXT,
    lockup_address TEXT,
    onchain_address TEXT,
    amount_sat INTEGER NOT NULL,
    onchain_amount_sat INTEGER,
    timeout_block_height INTEGER NOT NULL,
    create_response_json TEXT NOT NULL,
    claim_tx_id TEXT,
    refund_tx_id TEXT,
    created_at INTEGER NOT NULL
)";

/// Single-row counter table backing monotonic [`BoltzDB::reserve_swap_index`]
/// allocation. Keeping the counter in its own table (rather than `MAX(index)+1`)
/// guarantees an index is never reused even if a later swap creation fails after
/// the index was reserved.
pub const CREATE_META_TABLE: &str = "CREATE TABLE IF NOT EXISTS swap_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
)";

/// Current `boltz.db` schema version, written to `PRAGMA user_version` so future
/// changes have a migration anchor.
pub const SCHEMA_VERSION: i64 = 1;

/// Internal, fully-detailed representation of a persisted swap, including
/// secrets. This is never exposed across the FFI boundary — use
/// [`SwapRecord::to_boltz_swap`] to produce the public [`BoltzSwap`].
#[derive(Debug, Clone)]
pub struct SwapRecord {
    pub id: String,
    pub swap_type: BoltzSwapType,
    /// Raw Boltz status string (mapped to [`BoltzSwapStatus`] on the way out).
    pub status: String,
    pub network: BoltzNetwork,
    pub electrum_url: String,
    /// Deterministic BIP85 derivation index for this swap's key (and, for
    /// reverse swaps, its preimage). The secrets themselves are never stored;
    /// they are re-derived from the wallet mnemonic and this index.
    pub swap_index: u64,
    pub invoice: Option<String>,
    pub lockup_address: Option<String>,
    pub onchain_address: Option<String>,
    pub amount_sat: u64,
    pub onchain_amount_sat: Option<u64>,
    pub timeout_block_height: u64,
    /// Serialized `CreateSubmarineResponse` or `CreateReverseResponse`, used to
    /// reconstruct the swap script for claims/refunds.
    pub create_response_json: String,
    pub claim_tx_id: Option<String>,
    pub refund_tx_id: Option<String>,
    pub created_at: u64,
}

impl SwapRecord {
    /// Re-derive the client keypair from the wallet mnemonic and this swap's
    /// [`SwapRecord::swap_index`].
    pub fn keypair(&self, mnemonic: &str, passphrase: Option<&str>) -> Result<Keypair, BoltzError> {
        derive_swap_keypair(mnemonic, passphrase, self.network, self.swap_index)
    }

    /// Re-derive the preimage (reverse swaps only) from the swap key. The
    /// preimage is `sha256(swap_private_key)`, so it is reproducible from the
    /// seed without ever being stored.
    pub fn preimage(
        &self,
        mnemonic: &str,
        passphrase: Option<&str>,
    ) -> Result<Preimage, BoltzError> {
        let keypair = self.keypair(mnemonic, passphrase)?;
        Ok(Preimage::from_swap_key(&keypair))
    }

    /// Deserialize the stored submarine swap creation response.
    pub fn submarine_response(&self) -> Result<CreateSubmarineResponse, BoltzError> {
        serde_json::from_str(&self.create_response_json).map_err(BoltzError::from)
    }

    /// Deserialize the stored reverse swap creation response.
    pub fn reverse_response(&self) -> Result<CreateReverseResponse, BoltzError> {
        serde_json::from_str(&self.create_response_json).map_err(BoltzError::from)
    }

    /// Whether the swap is complete from this wallet's perspective, i.e. no
    /// further local action is possible or required.
    ///
    /// A terminal server status alone does not prove that: a reverse swap's
    /// cooperative claim discloses the preimage to Boltz *before* the claim
    /// transaction is broadcast, so Boltz can report `invoice.settled` while
    /// our claim never made it onchain. Such a swap still holds claimable
    /// funds and must stay recoverable until a claim txid is recorded locally.
    pub fn is_locally_complete(&self) -> bool {
        let status = BoltzSwapStatus::from_raw(&self.status);
        if !status.is_terminal() {
            return false;
        }
        if self.swap_type == BoltzSwapType::Reverse
            && status == BoltzSwapStatus::InvoiceSettled
            && self.claim_tx_id.is_none()
        {
            return false;
        }
        true
    }

    /// Project to the public FFI type.
    pub fn to_boltz_swap(&self) -> BoltzSwap {
        BoltzSwap {
            id: self.id.clone(),
            swap_type: self.swap_type,
            status: BoltzSwapStatus::from_raw(&self.status),
            network: self.network,
            swap_index: self.swap_index,
            amount_sat: self.amount_sat,
            onchain_amount_sat: self.onchain_amount_sat,
            invoice: self.invoice.clone(),
            lockup_address: self.lockup_address.clone(),
            onchain_address: self.onchain_address.clone(),
            timeout_block_height: self.timeout_block_height,
            created_at: self.created_at,
            claim_tx_id: self.claim_tx_id.clone(),
            refund_tx_id: self.refund_tx_id.clone(),
        }
    }
}
