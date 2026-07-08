//! Wallet-scoped backup migration for Core-owned activity data.
//!
//! Android and iOS embed Core-owned records (activities, tags, pre-activity
//! metadata, transaction details) inside their own VSS backup envelopes. Those
//! records gained a required `wallet_id`/`walletId` field, so a backup written
//! before the field existed no longer decodes into the current binding types:
//! the platform decoder (kotlinx.serialization on Android, `Codable` on iOS)
//! rejects the missing field before Core is ever consulted.
//!
//! Two boundaries are provided so the apps never hand-edit Core model JSON:
//!
//! * **Format-tolerant normalizers** (`migrate_backup_*_json`) take the raw
//!   Core-owned slice exactly as the app serialized it, inject the default
//!   `walletId` wherever a record is missing it (or has it empty), and return
//!   JSON in the same shape. The app then decodes it with its normal platform
//!   decoder. These handle every wrapper the apps produce for the `Activity`
//!   union without Core needing to model each platform's encoding:
//!     - Android kotlinx: `{"type": "...", "v1": {record}}`
//!     - iOS `Codable`:   `{"onchain": {"_0": {record}}}`
//!     - Core canonical:  `{"onchain": {record}}`
//!     - unwrapped record: `{ ...fields... }` (tags/metadata/details/raw)
//!
//! * **Canonical serialization** (`*_to_json` / `*_from_json`) is Core's own
//!   stable JSON for these slices. Apps can move their backups onto it so the
//!   platform-specific divergence stops mattering. The `*_from_json` side also
//!   normalizes a missing/empty `wallet_id` to [`DEFAULT_WALLET_ID`] so it can
//!   read legacy canonical data too.
//!
//! Both boundaries follow the same migration rule: a missing **or empty**
//! `wallet_id` on legacy input becomes [`DEFAULT_WALLET_ID`], while any existing
//! non-default id (e.g. `trezor:{hash}`) is preserved untouched. Normal writes
//! still reject an empty `wallet_id` (see `normalize_wallet_id`); the empty ->
//! default relaxation applies to legacy migration input only.

use super::{
    Activity, ActivityError, ActivityTags, ClosedChannelDetails, PreActivityMetadata,
    TransactionDetails, DEFAULT_WALLET_ID,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};

/// Both key spellings a record may use for its wallet id: Core serde emits
/// snake_case, the platform binding decoders emit camelCase.
const WALLET_ID_KEYS: [&str; 2] = ["wallet_id", "walletId"];

fn to_json<T: Serialize>(value: &T) -> Result<String, ActivityError> {
    serde_json::to_string(value).map_err(|e| ActivityError::SerializationError {
        error_details: e.to_string(),
    })
}

fn parse_json(json: &str) -> Result<Value, ActivityError> {
    serde_json::from_str(json).map_err(|e| ActivityError::SerializationError {
        error_details: e.to_string(),
    })
}

fn from_json<T: DeserializeOwned>(json: &str) -> Result<T, ActivityError> {
    serde_json::from_str(json).map_err(|e| ActivityError::SerializationError {
        error_details: e.to_string(),
    })
}

/// True when `record` already carries a non-empty wallet id under either spelling.
fn has_wallet_id(record: &Map<String, Value>) -> bool {
    WALLET_ID_KEYS
        .iter()
        .any(|key| matches!(record.get(*key), Some(Value::String(id)) if !id.trim().is_empty()))
}

/// Ensure a record object carries a non-empty wallet id, defaulting a
/// missing/empty one to [`DEFAULT_WALLET_ID`]. An existing non-default id is
/// left untouched. When only an empty snake_case key is present it is filled in
/// place so the record keeps its original spelling; otherwise the camelCase key
/// (what the platform backups use) is added.
fn ensure_wallet_id(record: &mut Map<String, Value>) {
    if has_wallet_id(record) {
        return;
    }

    let target = if record.contains_key("wallet_id") && !record.contains_key("walletId") {
        "wallet_id"
    } else {
        "walletId"
    };
    record.insert(
        target.to_string(),
        Value::String(DEFAULT_WALLET_ID.to_string()),
    );
}

/// Normalize the wallet id on a single serialized activity element, descending
/// through whichever `Activity`-union wrapper the app produced to reach the
/// inner record.
fn ensure_wallet_id_in_activity_element(element: &mut Value) {
    let Some(object) = element.as_object_mut() else {
        return;
    };

    // Android kotlinx sealed-class shape: {"type": "...", "v1": {record}}.
    if let Some(record) = object.get_mut("v1").and_then(Value::as_object_mut) {
        ensure_wallet_id(record);
        return;
    }

    // iOS Codable enum shape: {"onchain": {"_0": {record}}} / {"lightning": ...}.
    // Core canonical shape drops the "_0" wrapper: {"onchain": {record}}.
    for variant in ["onchain", "lightning"] {
        if let Some(inner) = object.get_mut(variant).and_then(Value::as_object_mut) {
            if let Some(record) = inner.get_mut("_0").and_then(Value::as_object_mut) {
                ensure_wallet_id(record);
            } else {
                ensure_wallet_id(inner);
            }
            return;
        }
    }

    // Unwrapped record (a bare activity object).
    if object.contains_key("id") {
        ensure_wallet_id(object);
    }
}

fn parse_array(json: &str) -> Result<Vec<Value>, ActivityError> {
    match parse_json(json)? {
        Value::Array(items) => Ok(items),
        _ => Err(ActivityError::SerializationError {
            error_details: "expected a JSON array of records".to_string(),
        }),
    }
}

/// Normalize a JSON array of flat records (tags, pre-activity metadata,
/// transaction details) by defaulting each record's missing/empty wallet id.
fn migrate_flat_records_json(json: &str) -> Result<String, ActivityError> {
    let mut items = parse_array(json)?;
    for item in &mut items {
        if let Some(record) = item.as_object_mut() {
            ensure_wallet_id(record);
        }
    }
    to_json(&items)
}

// ---------------------------------------------------------------------------
// Format-tolerant normalizers (operate on the app's own backup JSON)
// ---------------------------------------------------------------------------

/// Inject the default wallet id into a serialized `activities` slice from an
/// app backup envelope, preserving the app's original JSON shape. Handles the
/// Android, iOS and canonical `Activity`-union encodings.
#[uniffi::export]
pub fn migrate_backup_activities_json(json: String) -> Result<String, ActivityError> {
    let mut items = parse_array(&json)?;
    for item in &mut items {
        ensure_wallet_id_in_activity_element(item);
    }
    to_json(&items)
}

/// Inject the default wallet id into a serialized `activityTags` slice.
#[uniffi::export]
pub fn migrate_backup_activity_tags_json(json: String) -> Result<String, ActivityError> {
    migrate_flat_records_json(&json)
}

/// Inject the default wallet id into a serialized pre-activity metadata slice.
#[uniffi::export]
pub fn migrate_backup_pre_activity_metadata_json(json: String) -> Result<String, ActivityError> {
    migrate_flat_records_json(&json)
}

/// Inject the default wallet id into a serialized transaction-details slice.
/// Transaction details are not in the current backup envelopes, but the helper
/// is provided for parity if they are ever added.
#[uniffi::export]
pub fn migrate_backup_transaction_details_json(json: String) -> Result<String, ActivityError> {
    migrate_flat_records_json(&json)
}

// ---------------------------------------------------------------------------
// Canonical Core serialization (stable, cross-platform JSON)
// ---------------------------------------------------------------------------

fn normalize_str_wallet_id(wallet_id: &mut String) {
    if wallet_id.trim().is_empty() {
        *wallet_id = DEFAULT_WALLET_ID.to_string();
    }
}

fn normalize_activity_wallet_id(activity: &mut Activity) {
    match activity {
        Activity::Onchain(a) => normalize_str_wallet_id(&mut a.wallet_id),
        Activity::Lightning(a) => normalize_str_wallet_id(&mut a.wallet_id),
    }
}

/// Serialize activities to Core's canonical backup JSON.
#[uniffi::export]
pub fn activities_to_json(activities: Vec<Activity>) -> Result<String, ActivityError> {
    to_json(&activities)
}

/// Decode activities from Core's canonical backup JSON, defaulting a
/// missing/empty wallet id to [`DEFAULT_WALLET_ID`].
#[uniffi::export]
pub fn activities_from_json(json: String) -> Result<Vec<Activity>, ActivityError> {
    let mut activities: Vec<Activity> = from_json(&json)?;
    for activity in &mut activities {
        normalize_activity_wallet_id(activity);
    }
    Ok(activities)
}

/// Serialize activity tags to Core's canonical backup JSON.
#[uniffi::export]
pub fn activity_tags_to_json(tags: Vec<ActivityTags>) -> Result<String, ActivityError> {
    to_json(&tags)
}

/// Decode activity tags from Core's canonical backup JSON, defaulting a
/// missing/empty wallet id to [`DEFAULT_WALLET_ID`].
#[uniffi::export]
pub fn activity_tags_from_json(json: String) -> Result<Vec<ActivityTags>, ActivityError> {
    let mut tags: Vec<ActivityTags> = from_json(&json)?;
    for tag in &mut tags {
        normalize_str_wallet_id(&mut tag.wallet_id);
    }
    Ok(tags)
}

/// Serialize pre-activity metadata to Core's canonical backup JSON.
#[uniffi::export]
pub fn pre_activity_metadata_to_json(
    metadata: Vec<PreActivityMetadata>,
) -> Result<String, ActivityError> {
    to_json(&metadata)
}

/// Decode pre-activity metadata from Core's canonical backup JSON, defaulting a
/// missing/empty wallet id to [`DEFAULT_WALLET_ID`].
#[uniffi::export]
pub fn pre_activity_metadata_from_json(
    json: String,
) -> Result<Vec<PreActivityMetadata>, ActivityError> {
    let mut metadata: Vec<PreActivityMetadata> = from_json(&json)?;
    for entry in &mut metadata {
        normalize_str_wallet_id(&mut entry.wallet_id);
    }
    Ok(metadata)
}

/// Serialize transaction details to Core's canonical backup JSON.
#[uniffi::export]
pub fn transaction_details_to_json(
    details: Vec<TransactionDetails>,
) -> Result<String, ActivityError> {
    to_json(&details)
}

/// Decode transaction details from Core's canonical backup JSON, defaulting a
/// missing/empty wallet id to [`DEFAULT_WALLET_ID`].
#[uniffi::export]
pub fn transaction_details_from_json(
    json: String,
) -> Result<Vec<TransactionDetails>, ActivityError> {
    let mut details: Vec<TransactionDetails> = from_json(&json)?;
    for entry in &mut details {
        normalize_str_wallet_id(&mut entry.wallet_id);
    }
    Ok(details)
}

/// Serialize closed channels to Core's canonical backup JSON. Closed channels
/// are not wallet-scoped, so no wallet-id normalization is applied.
#[uniffi::export]
pub fn closed_channels_to_json(
    channels: Vec<ClosedChannelDetails>,
) -> Result<String, ActivityError> {
    to_json(&channels)
}

/// Decode closed channels from Core's canonical backup JSON.
#[uniffi::export]
pub fn closed_channels_from_json(json: String) -> Result<Vec<ClosedChannelDetails>, ActivityError> {
    from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wallet_id_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
        value.pointer(pointer).and_then(Value::as_str)
    }

    // -------------------------------------------------------------------
    // Format-tolerant normalizers (B)
    // -------------------------------------------------------------------

    #[test]
    fn android_shaped_activities_missing_wallet_id_default_to_bitkit() {
        // kotlinx sealed-class encoding: {"type": ..., "v1": {record}} with
        // camelCase fields and no walletId.
        let backup = json!([
            {
                "type": "com.synonym.bitkitcore.Activity.Onchain",
                "v1": {
                    "id": "onchain_1",
                    "txType": "SENT",
                    "txId": "tx_1",
                    "value": 1000,
                    "fee": 100,
                    "feeRate": 1,
                    "address": "bc1qexample",
                    "confirmed": true,
                    "timestamp": 111,
                    "isBoosted": false,
                    "boostTxIds": [],
                    "isTransfer": false,
                    "doesExist": true
                }
            },
            {
                "type": "com.synonym.bitkitcore.Activity.Lightning",
                "v1": {
                    "id": "lightning_1",
                    "txType": "RECEIVED",
                    "status": "SUCCEEDED",
                    "value": 2000,
                    "invoice": "lnbc1",
                    "message": "hi",
                    "timestamp": 222
                }
            }
        ])
        .to_string();

        let migrated = migrate_backup_activities_json(backup).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();

        assert_eq!(wallet_id_at(&value, "/0/v1/walletId"), Some(DEFAULT_WALLET_ID));
        assert_eq!(wallet_id_at(&value, "/1/v1/walletId"), Some(DEFAULT_WALLET_ID));
        // Non-wallet fields are preserved untouched.
        assert_eq!(value.pointer("/0/v1/txId").and_then(Value::as_str), Some("tx_1"));
        assert_eq!(value.pointer("/1/v1/invoice").and_then(Value::as_str), Some("lnbc1"));
    }

    #[test]
    fn ios_shaped_activities_missing_wallet_id_default_to_bitkit() {
        // Swift Codable enum encoding: {"onchain": {"_0": {record}}}.
        let backup = json!([
            {
                "onchain": {
                    "_0": {
                        "id": "onchain_1",
                        "txType": { "sent": {} },
                        "txId": "tx_1",
                        "value": 1000,
                        "fee": 100,
                        "feeRate": 1,
                        "address": "bc1qexample",
                        "confirmed": true,
                        "timestamp": 111,
                        "isBoosted": false,
                        "boostTxIds": [],
                        "isTransfer": false,
                        "doesExist": true
                    }
                }
            },
            {
                "lightning": {
                    "_0": {
                        "id": "lightning_1",
                        "txType": { "received": {} },
                        "status": { "succeeded": {} },
                        "value": 2000,
                        "invoice": "lnbc1",
                        "message": "hi",
                        "timestamp": 222
                    }
                }
            }
        ])
        .to_string();

        let migrated = migrate_backup_activities_json(backup).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();

        assert_eq!(
            wallet_id_at(&value, "/0/onchain/_0/walletId"),
            Some(DEFAULT_WALLET_ID)
        );
        assert_eq!(
            wallet_id_at(&value, "/1/lightning/_0/walletId"),
            Some(DEFAULT_WALLET_ID)
        );
        assert_eq!(
            value.pointer("/0/onchain/_0/txId").and_then(Value::as_str),
            Some("tx_1")
        );
    }

    #[test]
    fn canonical_shaped_activities_missing_wallet_id_default_to_bitkit() {
        // Core canonical encoding: {"onchain": {record}} (no "_0" wrapper).
        let backup = json!([
            { "onchain": { "id": "onchain_1", "txId": "tx_1" } },
            { "lightning": { "id": "lightning_1", "invoice": "lnbc1" } }
        ])
        .to_string();

        let migrated = migrate_backup_activities_json(backup).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();

        assert_eq!(
            wallet_id_at(&value, "/0/onchain/walletId"),
            Some(DEFAULT_WALLET_ID)
        );
        assert_eq!(
            wallet_id_at(&value, "/1/lightning/walletId"),
            Some(DEFAULT_WALLET_ID)
        );
    }

    #[test]
    fn non_default_wallet_id_is_preserved() {
        let backup = json!([
            {
                "type": "com.synonym.bitkitcore.Activity.Onchain",
                "v1": { "id": "a1", "walletId": "trezor:abcd1234", "txId": "tx_1" }
            }
        ])
        .to_string();

        let migrated = migrate_backup_activities_json(backup).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();

        assert_eq!(wallet_id_at(&value, "/0/v1/walletId"), Some("trezor:abcd1234"));
    }

    #[test]
    fn empty_wallet_id_is_normalized_to_default() {
        let backup = json!([
            {
                "onchain": { "_0": { "id": "a1", "walletId": "   ", "txId": "tx_1" } }
            }
        ])
        .to_string();

        let migrated = migrate_backup_activities_json(backup).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();

        assert_eq!(
            wallet_id_at(&value, "/0/onchain/_0/walletId"),
            Some(DEFAULT_WALLET_ID)
        );
    }

    #[test]
    fn flat_tags_and_metadata_missing_wallet_id_default_to_bitkit() {
        let tags = json!([
            { "activityId": "a1", "tags": ["food"] },
            { "walletId": "trezor:hash", "activityId": "a2", "tags": ["gift"] }
        ])
        .to_string();
        let migrated = migrate_backup_activity_tags_json(tags).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(wallet_id_at(&value, "/0/walletId"), Some(DEFAULT_WALLET_ID));
        assert_eq!(wallet_id_at(&value, "/1/walletId"), Some("trezor:hash"));

        let metadata = json!([
            { "paymentId": "p1", "tags": ["x"], "isReceive": true, "feeRate": 1, "isTransfer": false, "createdAt": 5 }
        ])
        .to_string();
        let migrated = migrate_backup_pre_activity_metadata_json(metadata).unwrap();
        let value: Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(wallet_id_at(&value, "/0/walletId"), Some(DEFAULT_WALLET_ID));
    }

    #[test]
    fn non_array_backup_json_is_rejected() {
        let err = migrate_backup_activities_json("{}".to_string()).unwrap_err();
        assert!(matches!(err, ActivityError::SerializationError { .. }));
    }

    // -------------------------------------------------------------------
    // Canonical serialization (A)
    // -------------------------------------------------------------------

    #[test]
    fn activities_round_trip_through_canonical_json() {
        // Legacy canonical input: onchain lacks wallet_id, lightning has empty.
        let canonical = json!([
            {
                "onchain": {
                    "id": "onchain_1",
                    "tx_type": "Sent",
                    "tx_id": "tx_1",
                    "value": 1000,
                    "fee": 100,
                    "fee_rate": 1,
                    "address": "bc1qexample",
                    "confirmed": true,
                    "timestamp": 111,
                    "is_boosted": false,
                    "boost_tx_ids": [],
                    "is_transfer": false,
                    "does_exist": true,
                    "confirm_timestamp": null,
                    "channel_id": null,
                    "transfer_tx_id": null
                }
            },
            {
                "lightning": {
                    "wallet_id": "",
                    "id": "lightning_1",
                    "tx_type": "Received",
                    "status": "Succeeded",
                    "value": 2000,
                    "fee": null,
                    "invoice": "lnbc1",
                    "message": "hi",
                    "timestamp": 222,
                    "preimage": null
                }
            }
        ])
        .to_string();

        let activities = activities_from_json(canonical).unwrap();
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].get_wallet_id(), DEFAULT_WALLET_ID);
        assert_eq!(activities[1].get_wallet_id(), DEFAULT_WALLET_ID);

        // Re-serialize and decode again: wallet ids survive and stay default.
        let json = activities_to_json(activities).unwrap();
        assert!(json.contains("\"wallet_id\":\"bitkit\""));
        let reparsed = activities_from_json(json).unwrap();
        assert_eq!(reparsed[0].get_id(), "onchain_1");
        assert_eq!(reparsed[1].get_id(), "lightning_1");
        assert_eq!(reparsed[1].get_wallet_id(), DEFAULT_WALLET_ID);
    }

    #[test]
    fn canonical_preserves_non_default_wallet_id() {
        let canonical = json!([
            {
                "onchain": {
                    "wallet_id": "trezor:deadbeef",
                    "id": "onchain_1",
                    "tx_type": "Sent",
                    "tx_id": "tx_1",
                    "value": 1000,
                    "fee": 100,
                    "fee_rate": 1,
                    "address": "bc1qexample",
                    "confirmed": true,
                    "timestamp": 111,
                    "is_boosted": false,
                    "boost_tx_ids": [],
                    "is_transfer": false,
                    "does_exist": true,
                    "confirm_timestamp": null,
                    "channel_id": null,
                    "transfer_tx_id": null
                }
            }
        ])
        .to_string();

        let activities = activities_from_json(canonical).unwrap();
        assert_eq!(activities[0].get_wallet_id(), "trezor:deadbeef");
    }

    #[test]
    fn tags_metadata_details_from_canonical_default_wallet_id() {
        let tags = activity_tags_from_json(
            json!([{ "activity_id": "a1", "tags": ["food"] }]).to_string(),
        )
        .unwrap();
        assert_eq!(tags[0].wallet_id, DEFAULT_WALLET_ID);
        assert_eq!(tags[0].activity_id, "a1");

        let metadata = pre_activity_metadata_from_json(
            json!([{
                "payment_id": "p1",
                "tags": ["x"],
                "is_receive": true,
                "fee_rate": 1,
                "is_transfer": false,
                "created_at": 5
            }])
            .to_string(),
        )
        .unwrap();
        assert_eq!(metadata[0].wallet_id, DEFAULT_WALLET_ID);

        let details = transaction_details_from_json(
            json!([{
                "tx_id": "tx_1",
                "amount_sats": -100,
                "inputs": [],
                "outputs": [],
                "wallet_id": ""
            }])
            .to_string(),
        )
        .unwrap();
        assert_eq!(details[0].wallet_id, DEFAULT_WALLET_ID);
    }

    #[test]
    fn closed_channels_round_trip_without_wallet_scope() {
        let canonical = json!([{
            "channel_id": "c1",
            "counterparty_node_id": "03abc",
            "funding_txo_txid": "ftx",
            "funding_txo_index": 0,
            "channel_value_sats": 1000,
            "closed_at": 5,
            "outbound_capacity_msat": 1,
            "inbound_capacity_msat": 1,
            "counterparty_unspendable_punishment_reserve": 0,
            "unspendable_punishment_reserve": 0,
            "forwarding_fee_proportional_millionths": 0,
            "forwarding_fee_base_msat": 0,
            "channel_name": "n",
            "channel_closure_reason": "r"
        }])
        .to_string();

        let channels = closed_channels_from_json(canonical).unwrap();
        assert_eq!(channels.len(), 1);
        let json = closed_channels_to_json(channels).unwrap();
        assert!(json.contains("\"channel_id\":\"c1\""));
        assert!(!json.contains("wallet_id"));
    }
}
