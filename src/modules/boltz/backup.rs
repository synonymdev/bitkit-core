//! Versioned local recovery snapshots. Callers choose their encrypted destination.
use super::{
    db::{insert_record, row_to_record},
    models::{BoltzDB, CreationIntent, SwapRecord},
    BoltzError, BoltzNetwork, BoltzSwapType,
};
use bitcoin::hashes::{sha256, Hash};
use boltz_client::swaps::boltz::{CreateReverseResponse, CreateSubmarineResponse};
use pubky_swap_boltz::{
    model::CreateRequest,
    store::{Store, StoreSnapshot},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
};

const VERSION: u32 = 1;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 10_000;
const MAX_IDENTITIES: usize = 100;
const COLUMNS: &str = "id,swap_type,status,network,electrum_url,swap_index,invoice,lockup_address,onchain_address,amount_sat,onchain_amount_sat,timeout_block_height,create_response_json,claim_tx_id,refund_tx_id,created_at,backend_binding,recipient_amount_sat";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    core: CoreSnapshot,
    identities: Vec<IdentitySnapshot>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentitySnapshot {
    identity: String,
    store: StoreSnapshot,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreSnapshot {
    swaps: Vec<SwapRecord>,
    intents: Vec<CreationIntent>,
    next_swap_index: u64,
    pending_spends: Vec<PendingSpend>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingSpend {
    swap_id: String,
    txid: String,
    refund_address: Option<String>,
}

fn invalid(message: &str) -> BoltzError {
    BoltzError::InvalidInput {
        error_details: format!("Invalid swap recovery snapshot: {message}"),
    }
}

/// Return a logical snapshot without reading private keys or copying live SQLite files.
pub async fn export_backup(db: &BoltzDB, pubky_data_root: String) -> Result<String, BoltzError> {
    // Do not wait behind a provider RPC or queue a writer ahead of nested read guards.
    // Holding this briefly also excludes a new intent appearing only in the later
    // wrapper snapshot without its Core recovery handle and reserved key counter.
    let _operation = db
        .recovery_gate
        .try_write()
        .map_err(|_| invalid("a swap operation is active; retry after it finishes"))?;
    db.ensure_recovery_complete().await?;
    let core = db.core_snapshot().await?;
    validate_core(&core)?;
    let root = checked_root(&pubky_data_root, false)?;
    let active = super::pubky::backup_context().await;
    if let Some((active_path, _)) = &active {
        let active_path = Path::new(active_path)
            .canonicalize()
            .map_err(|e| invalid(&e.to_string()))?;
        if active_path.parent() != Some(root.as_path()) {
            return Err(invalid(
                "configured identity is outside the supplied recovery root",
            ));
        }
    }
    let mut identities = Vec::new();
    if root.exists() {
        for entry in std::fs::read_dir(&root).map_err(|e| invalid(&e.to_string()))? {
            let entry = entry.map_err(|e| invalid(&e.to_string()))?;
            if identities.len() >= MAX_IDENTITIES {
                return Err(invalid("too many identities"));
            }
            let identity = entry
                .file_name()
                .into_string()
                .map_err(|_| invalid("non-UTF8 identity directory"))?;
            validate_identity(&identity)?;
            let path = checked_identity_path(&root, &identity, false)?;
            let store = if let Some((active_path, bridge)) = &active {
                if Path::new(active_path)
                    .canonicalize()
                    .map_err(|e| invalid(&e.to_string()))?
                    == path
                {
                    bridge.export_snapshot().map_err(super::api::bridge_error)?
                } else {
                    Store::snapshot_directory(&path).map_err(super::api::bridge_error)?
                }
            } else {
                Store::snapshot_directory(&path).map_err(super::api::bridge_error)?
            };
            identities.push(IdentitySnapshot { identity, store });
        }
    }
    identities.sort_by(|a, b| a.identity.cmp(&b.identity));
    let snapshot = Snapshot {
        version: VERSION,
        core,
        identities,
    };
    validate_snapshot(&snapshot)?;
    let encoded = serde_json::to_string(&snapshot)?;
    if encoded.len() > MAX_BYTES {
        return Err(invalid("snapshot exceeds 16 MiB"));
    }
    Ok(encoded)
}

/// Validate and merge a logical snapshot. No network connection is needed.
/// The caller must stop updates and disconnect Pubky before restoration.
pub async fn restore_backup(
    db: &BoltzDB,
    snapshot_json: String,
    pubky_data_root: String,
) -> Result<(), BoltzError> {
    if snapshot_json.len() > MAX_BYTES {
        return Err(invalid("snapshot exceeds 16 MiB"));
    }
    let value: serde_json::Value = serde_json::from_str(&snapshot_json)?;
    let snapshot: Snapshot = serde_json::from_value(value.clone())?;
    if serde_json::to_value(&snapshot)? != value {
        return Err(invalid("unknown or noncanonical snapshot fields"));
    }
    validate_snapshot(&snapshot)?;
    let _configuration = super::pubky::restore_guard().await?;
    let _operation = db
        .recovery_gate
        .try_write()
        .map_err(|_| invalid("a swap operation is active; retry after it finishes"))?;
    let root = checked_root(&pubky_data_root, false)?;
    let fingerprint = sha256::Hash::hash(&serde_json::to_vec(&snapshot)?).to_string();
    let local = db.core_snapshot().await?;
    let mut completion_evidence = snapshot.identities.clone();
    // Preflight every store and the core merge before writing any imported record.
    for identity in &snapshot.identities {
        let path = checked_identity_path(&root, &identity.identity, true)?;
        Store::validate_directory_import(&path, &identity.store)
            .map_err(super::api::bridge_error)?;
        if path.join("swaps.sqlite3").exists() {
            completion_evidence.push(IdentitySnapshot {
                identity: identity.identity.clone(),
                store: Store::snapshot_directory(&path).map_err(super::api::bridge_error)?,
            });
        }
    }
    let merged = merge_core(local, snapshot.core.clone(), &completion_evidence)?;
    let root = checked_root(&pubky_data_root, true)?;
    // A later filesystem or Core commit failure must never make keys from a
    // partially imported wrapper contract available to the next new swap.
    db.begin_recovery_import(merged.next_swap_index, &fingerprint, &root)
        .await?;
    for identity in &snapshot.identities {
        let path = checked_identity_path(&root, &identity.identity, true)?;
        Store::import_directory(&path, &identity.store).map_err(super::api::bridge_error)?;
    }
    // Accepted wrapper records must exist before their wallet recovery handles.
    // Store merges are idempotent, so an I/O failure can safely be retried.
    db.write_core_snapshot(&merged).await
}

fn checked_root(value: &str, create: bool) -> Result<PathBuf, BoltzError> {
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid(
            "identity root must be an absolute path without parent traversal",
        ));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(invalid("identity root must not be a symbolic link"));
        }
    }
    if create {
        std::fs::create_dir_all(path).map_err(|e| invalid(&e.to_string()))?;
    }
    if path.exists() {
        if !path.is_dir() {
            return Err(invalid("identity root is not a directory"));
        }
        path.canonicalize().map_err(|e| invalid(&e.to_string()))
    } else {
        Ok(path.to_path_buf())
    }
}

fn validate_identity(identity: &str) -> Result<(), BoltzError> {
    if identity.len() != 52
        || pubky_swap_boltz::canonical_pubky(identity).map_err(super::api::bridge_error)?
            != identity
    {
        return Err(invalid("invalid identity directory"));
    }
    Ok(())
}

fn checked_identity_path(
    root: &Path,
    identity: &str,
    missing_allowed: bool,
) -> Result<PathBuf, BoltzError> {
    validate_identity(identity)?;
    let path = root.join(identity);
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid("identity entry is not a real directory"));
        }
        for name in [
            "swaps.sqlite3",
            "swaps.sqlite3-wal",
            "swaps.sqlite3-shm",
            "process.lock",
        ] {
            if let Ok(metadata) = std::fs::symlink_metadata(path.join(name)) {
                if metadata.file_type().is_symlink() {
                    return Err(invalid("identity database must not be a symbolic link"));
                }
            }
        }
        path.canonicalize().map_err(|e| invalid(&e.to_string()))
    } else if missing_allowed {
        Ok(path)
    } else {
        Err(invalid("identity directory disappeared"))
    }
}

fn binding_parts(binding: &str) -> Result<(&str, BoltzNetwork), BoltzError> {
    let parts: Vec<_> = binding.split('|').collect();
    if parts.len() != 3 {
        return Err(invalid("invalid identity binding"));
    }
    validate_identity(parts[0])?;
    validate_identity(parts[1])?;
    let network =
        BoltzNetwork::from_str(parts[2]).ok_or_else(|| invalid("invalid binding network"))?;
    Ok((parts[0], network))
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), BoltzError> {
    if snapshot.version != VERSION || snapshot.identities.len() > MAX_IDENTITIES {
        return Err(invalid("unsupported version or identity count"));
    }
    validate_core(&snapshot.core)?;
    let mut stores = BTreeMap::new();
    let mut directories = HashSet::new();
    let mut wrapper_records = 0usize;
    for identity in &snapshot.identities {
        validate_identity(&identity.identity)?;
        wrapper_records = wrapper_records
            .checked_add(identity.store.swaps.len())
            .filter(|count| *count <= MAX_RECORDS)
            .ok_or_else(|| invalid("too many wrapper records"))?;
        if !directories.insert(&identity.identity) {
            return Err(invalid("duplicate identity directory"));
        }
        identity
            .store
            .validate()
            .map_err(super::api::bridge_error)?;
        if binding_parts(&identity.store.identity_binding)?.0 != identity.identity
            || stores
                .insert(identity.store.identity_binding.clone(), &identity.store)
                .is_some()
        {
            return Err(invalid("duplicate or mismatched identity store"));
        }
    }
    for record in &snapshot.core.swaps {
        if let Some(binding) = &record.backend_binding {
            let store = stores
                .get(binding)
                .ok_or_else(|| invalid("accepted swap is missing its identity store"))?;
            let stored = store
                .swaps
                .iter()
                .find(|swap| swap.record.id.to_string() == record.id)
                .ok_or_else(|| invalid("accepted swap is missing its wrapper record"))?;
            let response: serde_json::Value = serde_json::from_str(&record.create_response_json)?;
            if stored.record.response.as_ref() != Some(&response) {
                return Err(invalid("wallet and wrapper acceptance disagree"));
            }
        }
    }
    for intent in &snapshot.core.intents {
        let store = stores
            .get(&intent.backend_binding)
            .ok_or_else(|| invalid("creation intent is missing its identity store"))?;
        // A pre-admission intent can legitimately precede the wrapper reservation.
        // Once a retry key exists, both databases must agree on the exact request.
        if let Some(binding) = store.idempotency.iter().find(|b| b.key == intent.id) {
            if binding.fingerprint
                != normalized_request(&intent.request)?
                    .fingerprint()
                    .map_err(super::api::bridge_error)?
            {
                return Err(invalid(
                    "creation intent and wrapper retry binding disagree",
                ));
            }
        }
    }
    Ok(())
}

fn normalized_request(request: &CreateRequest) -> Result<CreateRequest, BoltzError> {
    let mut request = request.clone();
    let key = bitcoin::PublicKey::from_str(request.client_key())
        .map_err(|_| invalid("invalid client public key"))?
        .to_string();
    match &mut request {
        CreateRequest::Submarine(r) => {
            r.refund_public_key = key;
            r.invoice = boltz_client::Bolt11Invoice::from_str(&r.invoice)
                .map_err(|_| invalid("invalid invoice"))?
                .to_string();
        }
        CreateRequest::Reverse(r) => {
            r.claim_public_key = key;
            r.preimage_hash = sha256::Hash::from_str(&r.preimage_hash)
                .map_err(|_| invalid("invalid payment hash"))?
                .to_string();
        }
    }
    Ok(request)
}

fn validate_core(core: &CoreSnapshot) -> Result<(), BoltzError> {
    if core.swaps.len().saturating_add(core.intents.len()) > MAX_RECORDS
        || core.pending_spends.len() > MAX_RECORDS * 10
        || core.next_swap_index > super::models::SWAP_INDEX_LIMIT
    {
        return Err(invalid("too many records or invalid derivation counter"));
    }
    let mut ids = HashSet::new();
    let mut indices = HashSet::new();
    for record in &core.swaps {
        if record.id.is_empty()
            || record.id.len() > 128
            || !ids.insert(record.id.clone())
            || !indices.insert(record.swap_index)
            || record.swap_index >= core.next_swap_index
            || record.amount_sat > i64::MAX as u64
            || record
                .onchain_amount_sat
                .is_some_and(|a| a > i64::MAX as u64)
            || record.created_at > i64::MAX as u64
            || record.timeout_block_height > u64::from(u32::MAX)
            || record.status.len() > 128
            || record.electrum_url.len() > 2048
            || record.create_response_json.len() > 100_000
        {
            return Err(invalid("invalid or duplicate swap record"));
        }
        if let Some(binding) = &record.backend_binding {
            if binding_parts(binding)?.1 != record.network {
                return Err(invalid("swap binding network mismatch"));
            }
            uuid::Uuid::parse_str(&record.id)
                .map_err(|_| invalid("invalid Pubky swap identifier"))?;
        }
        super::send::validate_recipient(record)?;
        let original_response: serde_json::Value =
            serde_json::from_str(&record.create_response_json)?;
        match record.swap_type {
            BoltzSwapType::Submarine => {
                let response: CreateSubmarineResponse =
                    serde_json::from_str(&record.create_response_json)?;
                if serde_json::to_value(&response)? != original_response {
                    return Err(invalid("unsupported submarine response fields"));
                }
                if response.id != record.id
                    || Some(&response.address) != record.lockup_address.as_ref()
                    || response.expected_amount != record.amount_sat
                    || response.timeout_block_height != record.timeout_block_height
                    || response.blinding_key.is_some()
                    || record.claim_tx_id.is_some()
                {
                    return Err(invalid("submarine response does not match wallet record"));
                }
            }
            BoltzSwapType::Reverse => {
                let response: CreateReverseResponse =
                    serde_json::from_str(&record.create_response_json)?;
                if serde_json::to_value(&response)? != original_response {
                    return Err(invalid("unsupported reverse response fields"));
                }
                if response.id != record.id
                    || Some(&response.lockup_address) != record.lockup_address.as_ref()
                    || Some(response.onchain_amount) != record.onchain_amount_sat
                    || u64::from(response.timeout_block_height) != record.timeout_block_height
                    || response.invoice != record.invoice
                    || response.blinding_key.is_some()
                    || record.refund_tx_id.is_some()
                {
                    return Err(invalid("reverse response does not match wallet record"));
                }
            }
        }
        for address in [&record.lockup_address, &record.onchain_address]
            .into_iter()
            .flatten()
        {
            super::validation::validate_onchain_address(address, record.network)?;
        }
        for txid in [&record.claim_tx_id, &record.refund_tx_id]
            .into_iter()
            .flatten()
        {
            validate_txid(txid)?;
        }
        let invoice = record
            .invoice
            .as_ref()
            .ok_or_else(|| invalid("missing invoice"))?;
        if invoice.len() > 20_000 {
            return Err(invalid("invoice too large"));
        }
        let parsed = boltz_client::Bolt11Invoice::from_str(invoice)
            .map_err(|_| invalid("invalid invoice"))?;
        parsed
            .check_signature()
            .map_err(|_| invalid("invalid invoice signature"))?;
        if record.swap_type == BoltzSwapType::Reverse
            && parsed.amount_milli_satoshis() != record.amount_sat.checked_mul(1000)
        {
            return Err(invalid("invoice amount does not match reverse swap"));
        }
        let expected_currency = match record.network {
            BoltzNetwork::Mainnet => "bc",
            BoltzNetwork::Testnet => "tb",
            BoltzNetwork::Regtest => "bcrt",
        };
        if parsed.currency().to_string() != expected_currency {
            return Err(invalid("invoice network mismatch"));
        }
    }
    let mut intent_ids = HashSet::new();
    let mut request_keys = HashSet::new();
    for intent in &core.intents {
        uuid::Uuid::parse_str(&intent.id).map_err(|_| invalid("invalid creation identifier"))?;
        if !intent_ids.insert(&intent.id)
            || !request_keys.insert(&intent.request_key)
            || !indices.insert(intent.swap_index)
            || intent.swap_index >= core.next_swap_index
            || binding_parts(&intent.backend_binding)?.1 != intent.network
            || intent.electrum_url.len() > 2048
            || intent.created_at > i64::MAX as u64
        {
            return Err(invalid("invalid creation intent"));
        }
        sha256::Hash::from_str(&intent.wallet_fingerprint)
            .map_err(|_| invalid("invalid wallet fingerprint"))?;
        let key = bitcoin::PublicKey::from_str(intent.request.client_key())
            .map_err(|_| invalid("invalid client public key"))?;
        if !key.compressed {
            return Err(invalid("client key must be compressed"));
        }
        match &intent.request {
            CreateRequest::Submarine(r)
                if r.from == "BTC"
                    && r.to == "BTC"
                    && r.referral_id.is_empty()
                    && r.error.is_empty()
                    && r.invoice.len() <= 20_000
                    && intent.recipient_amount_sat.is_none() =>
            {
                boltz_client::Bolt11Invoice::from_str(&r.invoice)
                    .map_err(|_| invalid("invalid intent invoice"))?;
            }
            CreateRequest::Reverse(r)
                if r.from == "BTC"
                    && r.to == "BTC"
                    && r.referral_id.is_empty()
                    && r.error.is_empty()
                    && r.invoice_amount > 0
                    && r.onchain_amount == 0
                    && intent
                        .recipient_amount_sat
                        .is_none_or(|v| v > 0 && v < r.invoice_amount) =>
            {
                sha256::Hash::from_str(&r.preimage_hash)
                    .map_err(|_| invalid("invalid intent payment hash"))?;
                let address = intent
                    .claim_address
                    .as_ref()
                    .ok_or_else(|| invalid("missing claim destination"))?;
                super::validation::validate_onchain_address(address, intent.network)?;
            }
            _ => return Err(invalid("unsupported creation request")),
        }
    }
    let mut journal = HashSet::new();
    for spend in &core.pending_spends {
        let record = core
            .swaps
            .iter()
            .find(|r| r.id == spend.swap_id)
            .ok_or_else(|| invalid("spend journal refers to unknown swap"))?;
        validate_txid(&spend.txid)?;
        if !journal.insert((&spend.swap_id, &spend.txid))
            || record.backend_binding.is_none()
            || spend.refund_address.is_some() != (record.swap_type == BoltzSwapType::Submarine)
        {
            return Err(invalid("invalid spend journal entry"));
        }
        if let Some(address) = &spend.refund_address {
            super::validation::validate_onchain_address(address, record.network)?;
        }
    }
    Ok(())
}

fn validate_txid(value: &str) -> Result<(), BoltzError> {
    bitcoin::Txid::from_str(value)
        .map(|_| ())
        .map_err(|_| invalid("invalid transaction id"))
}

fn same_swap(a: &SwapRecord, b: &SwapRecord) -> Result<bool, BoltzError> {
    Ok(a.id == b.id
        && a.backend_binding == b.backend_binding
        && a.swap_type == b.swap_type
        && a.network == b.network
        && a.swap_index == b.swap_index
        && a.invoice == b.invoice
        && a.lockup_address == b.lockup_address
        && a.amount_sat == b.amount_sat
        && a.onchain_amount_sat == b.onchain_amount_sat
        && a.recipient_amount_sat == b.recipient_amount_sat
        && a.timeout_block_height == b.timeout_block_height
        && (a.swap_type == BoltzSwapType::Submarine || a.onchain_address == b.onchain_address)
        && serde_json::from_str::<serde_json::Value>(&a.create_response_json)?
            == serde_json::from_str::<serde_json::Value>(&b.create_response_json)?)
}

fn completed_intent(
    intent: &CreationIntent,
    record: &SwapRecord,
    identities: &[IdentitySnapshot],
) -> Result<bool, BoltzError> {
    if intent.recipient_amount_sat != record.recipient_amount_sat
        || (record.swap_type == BoltzSwapType::Reverse
            && intent.claim_address != record.onchain_address)
    {
        return Ok(false);
    }
    let fingerprint = normalized_request(&intent.request)?
        .fingerprint()
        .map_err(super::api::bridge_error)?;
    Ok(identities
        .iter()
        .filter(|identity| identity.store.identity_binding == intent.backend_binding)
        .any(|identity| {
            let store = &identity.store;
            store
                .idempotency
                .iter()
                .any(|binding| binding.key == intent.id && binding.fingerprint == fingerprint)
                && store.swaps.iter().any(|swap| {
                    swap.fingerprint == fingerprint && swap.record.id.to_string() == record.id
                })
        }))
}

fn merge_core(
    mut local: CoreSnapshot,
    incoming: CoreSnapshot,
    identities: &[IdentitySnapshot],
) -> Result<CoreSnapshot, BoltzError> {
    validate_core(&local)?;
    validate_core(&incoming)?;
    local.next_swap_index = local.next_swap_index.max(incoming.next_swap_index);
    for incoming in incoming.swaps {
        if let Some(existing) = local.swaps.iter_mut().find(|r| r.id == incoming.id) {
            if !same_swap(existing, &incoming)? {
                return Err(invalid("conflicting swap recovery terms"));
            }
            if existing.claim_tx_id.is_none() {
                existing.claim_tx_id = incoming.claim_tx_id;
            }
            if existing.refund_tx_id.is_none() && incoming.refund_tx_id.is_some() {
                existing.refund_tx_id = incoming.refund_tx_id;
                existing.onchain_address = incoming.onchain_address;
            }
            if existing.claim_tx_id.is_some() {
                existing.status = "transaction.claimed".into();
            }
            if existing.refund_tx_id.is_some() {
                existing.status = "transaction.refunded".into();
            }
        } else {
            local.swaps.push(incoming);
        }
    }
    for incoming in incoming.intents {
        if let Some(record) = local
            .swaps
            .iter()
            .find(|r| r.swap_index == incoming.swap_index)
        {
            if record.backend_binding.as_deref() != Some(&incoming.backend_binding)
                || !completed_intent(&incoming, record, identities)?
            {
                return Err(invalid(
                    "a different swap occupies the creation intent's derivation index",
                ));
            }
            continue;
        }
        if let Some(existing) = local
            .intents
            .iter()
            .find(|i| i.id == incoming.id || i.request_key == incoming.request_key)
        {
            if existing.swap_index != incoming.swap_index
                || existing
                    .request
                    .fingerprint()
                    .map_err(super::api::bridge_error)?
                    != incoming
                        .request
                        .fingerprint()
                        .map_err(super::api::bridge_error)?
                || !existing.same_operation(&incoming)
            {
                return Err(invalid("conflicting creation recovery terms"));
            }
        } else {
            local.intents.push(incoming);
        }
    }
    let mut unfinished = Vec::new();
    for intent in local.intents {
        if let Some(record) = local
            .swaps
            .iter()
            .find(|r| r.swap_index == intent.swap_index)
        {
            if record.backend_binding.as_deref() != Some(&intent.backend_binding)
                || !completed_intent(&intent, record, identities)?
            {
                return Err(invalid(
                    "a different swap occupies the creation intent's derivation index",
                ));
            }
        } else {
            unfinished.push(intent);
        }
    }
    local.intents = unfinished;
    for spend in incoming.pending_spends {
        if !local
            .pending_spends
            .iter()
            .any(|s| s.swap_id == spend.swap_id && s.txid == spend.txid)
        {
            local.pending_spends.push(spend);
        }
    }
    local.pending_spends.retain(|spend| {
        local
            .swaps
            .iter()
            .any(|r| r.id == spend.swap_id && r.claim_tx_id.is_none() && r.refund_tx_id.is_none())
    });
    validate_core(&local)?;
    Ok(local)
}

impl BoltzDB {
    async fn ensure_recovery_complete(&self) -> Result<(), BoltzError> {
        let conn = self.conn.lock().await;
        if conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM swap_recovery_import)",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(invalid(
                "an interrupted restore must be retried with the same snapshot before export",
            ));
        }
        Ok(())
    }

    async fn begin_recovery_import(
        &self,
        next_swap_index: u64,
        fingerprint: &str,
        root: &Path,
    ) -> Result<(), BoltzError> {
        let root = root
            .to_str()
            .ok_or_else(|| invalid("non-UTF8 recovery root"))?;
        let mut locked = self.conn.lock().await;
        let conn = locked.transaction()?;
        let pending: Option<(String, String)> = conn
            .query_row(
                "SELECT fingerprint,target_root FROM swap_recovery_import WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if pending.is_some_and(|(previous, target)| previous != fingerprint || target != root) {
            return Err(invalid(
                "retry the interrupted restore using the same snapshot and directory",
            ));
        }
        conn.execute(
            "INSERT INTO swap_meta VALUES ('next_swap_index',?1) ON CONFLICT(key) DO UPDATE SET value=MAX(value,excluded.value)",
            [next_swap_index as i64],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO swap_recovery_import VALUES (1,?1,?2)",
            params![fingerprint, root],
        )?;
        conn.commit()?;
        Ok(())
    }

    async fn core_snapshot(&self) -> Result<CoreSnapshot, BoltzError> {
        let mut locked = self.conn.lock().await;
        let conn = locked.transaction()?;
        let snapshot = read_core(&conn)?;
        conn.commit()?;
        Ok(snapshot)
    }

    async fn write_core_snapshot(&self, snapshot: &CoreSnapshot) -> Result<(), BoltzError> {
        let mut locked = self.conn.lock().await;
        let conn = locked.transaction()?;
        conn.execute("DELETE FROM pending_spends", [])?;
        conn.execute("DELETE FROM creation_intents", [])?;
        conn.execute("DELETE FROM swaps", [])?;
        for record in &snapshot.swaps {
            insert_record(&conn, record)?;
        }
        for intent in &snapshot.intents {
            conn.execute(
                "INSERT INTO creation_intents VALUES (?1,?2,?3)",
                params![
                    intent.request_key,
                    intent.id,
                    serde_json::to_string(intent)?
                ],
            )?;
        }
        for spend in &snapshot.pending_spends {
            conn.execute(
                "INSERT INTO pending_spends VALUES (?1,?2,?3)",
                params![spend.swap_id, spend.txid, spend.refund_address],
            )?;
        }
        conn.execute("INSERT INTO swap_meta VALUES ('next_swap_index',?1) ON CONFLICT(key) DO UPDATE SET value=MAX(value,excluded.value)", [snapshot.next_swap_index as i64])?;
        conn.execute("DELETE FROM swap_recovery_import", [])?;
        conn.commit()?;
        Ok(())
    }
}

fn read_core(conn: &Connection) -> Result<CoreSnapshot, BoltzError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM swaps ORDER BY created_at,id"
    ))?;
    let swaps = statement
        .query_map([], row_to_record)?
        .map(|r| r?)
        .collect::<Result<Vec<_>, BoltzError>>()?;
    let mut statement = conn.prepare("SELECT intent_json FROM creation_intents ORDER BY rowid")?;
    let intents = statement
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str(&r?)?))
        .collect::<Result<Vec<_>, BoltzError>>()?;
    let next: Option<i64> = conn
        .query_row(
            "SELECT value FROM swap_meta WHERE key='next_swap_index'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let next_swap_index =
        u64::try_from(next.unwrap_or(0)).map_err(|_| invalid("invalid derivation counter"))?;
    let mut statement =
        conn.prepare("SELECT swap_id,txid,refund_address FROM pending_spends ORDER BY rowid")?;
    let pending_spends = statement
        .query_map([], |r| {
            Ok(PendingSpend {
                swap_id: r.get(0)?,
                txid: r.get(1)?,
                refund_address: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(CoreSnapshot {
        swaps,
        intents,
        next_swap_index,
        pending_spends,
    })
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
