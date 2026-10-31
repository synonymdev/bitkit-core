//! Swap records and where they live. A record holds everything needed to finish a swap after a
//! restart or a restore (the lockup's parameters and, on a receive, the public preimage salt);
//! never a private key or a receive preimage. `export`/`import` carry the whole set as one JSON
//! document, for the app's backup.

use super::contract::LockupParams;
use super::error::Error;
use super::error::Result;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Receive,
    Send,
}

/// The TS client's `Outcome` vocabulary, trader-centric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Persisted, and its funding has not been broadcast (send).
    Accepted,
    /// Waiting on the counterparty: the hold invoice is out and unpaid (receive).
    Open,
    /// The trader's lockup is funded (send), or the solver's is and a claim is pending (receive).
    Funded,
    /// Receive: the trader's claim landed.
    Claimed,
    /// Send: the solver claimed with the preimage, so the invoice was paid.
    Paid,
    /// Send: the lockup came back to the trader.
    Refunded,
    /// Receive: never claimed; the solver took the lockup back and the payment never arrived.
    Lapsed,
    /// Surfaced, never terminal; `blocked_reason` says why.
    NeedsRecovery,
    /// An action failed and its window closed.
    Failed,
}

impl Outcome {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Outcome::Claimed
                | Outcome::Paid
                | Outcome::Refunded
                | Outcome::Lapsed
                | Outcome::Failed
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Accepted => "accepted",
            Outcome::Open => "open",
            Outcome::Funded => "funded",
            Outcome::Claimed => "claimed",
            Outcome::Paid => "paid",
            Outcome::Refunded => "refunded",
            Outcome::Lapsed => "lapsed",
            Outcome::NeedsRecovery => "needs_recovery",
            Outcome::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapRecord {
    // The app's `ArkadeSwap` shape.
    pub id: String,
    pub direction: Direction,
    pub outcome: Outcome,
    pub give_sat: u64,
    pub take_sat: u64,
    pub fee_sat: u64,
    pub bolt11: String,
    pub payment_hash: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    /// Receive: the last moment the invoice can be paid. Send: the quote's `valid_until`.
    pub expires_at: u64,

    // What finishing the swap needs.
    pub solver: String,
    pub rfq_id: String,
    pub quote: serde_json::Value,
    pub lockup: LockupParams,
    pub lockup_address: String,
    /// Receive: the public salt `P` re-derives from with the identity key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preimage_salt: Option<String>,
    /// Send: the preimage read off the solver's claim, once observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preimage: Option<String>,
    /// Receive: where the claim pays.
    pub payout_address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding_txid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_txid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refund_txid: Option<String>,
}

pub trait SwapStore: Send + Sync {
    fn put(&self, record: &SwapRecord) -> Result<()>;
    fn get(&self, id: &str) -> Result<Option<SwapRecord>>;
    fn all(&self) -> Result<Vec<SwapRecord>>;

    /// Every record as one JSON document.
    fn export(&self) -> Result<String> {
        let all = self.all()?;
        serde_json::to_string(&Backup {
            version: 1,
            swaps: all,
        })
        .map_err(|e| Error::Store(e.to_string()))
    }

    /// Merge a backup in: unknown records are added, known ones kept unless the backup's copy is
    /// newer. Returns how many were written.
    fn import(&self, json: &str) -> Result<usize> {
        let backup: Backup = serde_json::from_str(json).map_err(|e| Error::Store(e.to_string()))?;
        let mut written = 0;
        for record in backup.swaps {
            let newer = match self.get(&record.id)? {
                Some(held) => record.updated_at > held.updated_at,
                None => true,
            };
            if newer {
                self.put(&record)?;
                written += 1;
            }
        }
        Ok(written)
    }
}

#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    swaps: Vec<SwapRecord>,
}

/// In-memory, optionally mirrored to a JSON file written atomically on every change.
pub struct JsonStore {
    records: Mutex<BTreeMap<String, SwapRecord>>,
    path: Option<PathBuf>,
}

impl JsonStore {
    pub fn memory() -> Self {
        Self {
            records: Mutex::new(BTreeMap::new()),
            path: None,
        }
    }

    /// Open (or create) the file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let records = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str::<Backup>(&json)
                .map_err(|e| Error::Store(format!("{}: {e}", path.display())))?
                .swaps
                .into_iter()
                .map(|r| (r.id.clone(), r))
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(Error::Store(e.to_string())),
        };
        Ok(Self {
            records: Mutex::new(records),
            path: Some(path),
        })
    }

    fn flush(&self, records: &BTreeMap<String, SwapRecord>) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_vec(&Backup {
            version: 1,
            swaps: records.values().cloned().collect(),
        })
        .map_err(|e| Error::Store(e.to_string()))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| Error::Store(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::Store(e.to_string()))
    }
}

impl SwapStore for JsonStore {
    fn put(&self, record: &SwapRecord) -> Result<()> {
        let mut records = self.records.lock().expect("store lock");
        let previous = records.insert(record.id.clone(), record.clone());
        if let Err(e) = self.flush(&records) {
            // Keep memory and disk in step: a record that is not durable was not stored.
            match previous {
                Some(p) => records.insert(p.id.clone(), p),
                None => records.remove(&record.id),
            };
            return Err(e);
        }
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<SwapRecord>> {
        Ok(self.records.lock().expect("store lock").get(id).cloned())
    }

    fn all(&self) -> Result<Vec<SwapRecord>> {
        let mut all: Vec<_> = self
            .records
            .lock()
            .expect("store lock")
            .values()
            .cloned()
            .collect();
        all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        Ok(all)
    }
}
