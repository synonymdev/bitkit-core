use crate::onchain::AccountType;

/// A completely decoded UR payload.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum UrPayload {
    /// The byte string wrapped by a `bytes` registry item.
    Bytes { data: Vec<u8> },
    /// A `crypto-psbt` registry item, returned in Bitkit's usual base64 form.
    CryptoPsbt { psbt: String },
    /// An uninterpreted UR registry item.
    Cbor { ur_type: String, cbor: Vec<u8> },
}

/// Current state after accepting a scanned UR frame.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UrDecoderStatus {
    /// Estimated completion from 0.0 through 1.0.
    pub progress: f64,
    /// Fountain source-fragment count, or 1 for a single-part UR.
    pub fragment_count: u32,
    /// Present once the complete message has been decoded.
    pub payload: Option<UrPayload>,
}

/// One single-signature account in Passport's generic JSON export.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PassportAccount {
    pub account_type: AccountType,
    /// Standard xpub/tpub encoding used by Passport's export.
    pub xpub: String,
    /// Account-level BIP32 path, such as `m/84'/0'/0'`.
    pub derivation_path: String,
}

/// The single-signature accounts exported by Passport for one account index.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PassportAccountExport {
    /// Root fingerprint used in descriptors and PSBT key origins.
    pub master_fingerprint: String,
    pub account_index: u32,
    pub accounts: Vec<PassportAccount>,
}
