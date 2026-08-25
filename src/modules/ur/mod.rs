//! Uniform Resources used by QR-based hardware wallets.
//!
//! This module owns UR framing and registry payloads. Camera scanning and QR
//! rendering remain application concerns.

mod decoder;
mod encoding;
mod error;
mod passport;
mod types;

const MAX_FRAGMENT_COUNT: usize = 1_000;

pub use decoder::UrDecoder;
pub use encoding::ur_encode_crypto_psbt;
pub use error::UrError;
pub use passport::passport_parse_account_export;
pub use types::{PassportAccount, PassportAccountExport, UrDecoderStatus, UrPayload};
