use super::vhtlc::VhtlcError;

/// A wallet-side failure, raised by the binding's [`super::ArkWallet`] implementation.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct WalletError(pub String);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The solver answered `rfq_refusal`.
    #[error("solver refused: {reason}{}", error_code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default())]
    Refused {
        reason: String,
        error_code: Option<String>,
        /// The rest of the refusal (`field`, `actual`, `expected`, `limit`, `unit`).
        detail: serde_json::Value,
    },
    /// A quote failed one of the client's checks; never fund or publish it.
    #[error("quote failed the {check} check: expected {expected}, got {actual}")]
    Verification {
        check: &'static str,
        expected: String,
        actual: String,
    },
    #[error("quote expired")]
    QuoteExpired,
    #[error("fee {fee} sat exceeds the {max} sat maximum")]
    FeeTooHigh { fee: u64, max: u64 },
    #[error("invalid invoice: {0}")]
    Invoice(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("no solver reply within {0} seconds")]
    Timeout(u64),
    #[error("wallet: {0}")]
    Wallet(#[from] WalletError),
    #[error("store: {0}")]
    Store(String),
    #[error("lockup script: {0}")]
    Vhtlc(#[from] VhtlcError),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub(crate) fn verification(
        check: &'static str,
        expected: impl ToString,
        actual: impl ToString,
    ) -> Self {
        Self::Verification {
            check,
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
