use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum UsdtError {
    #[error("Enter a valid USDT amount with at most six decimal places")]
    InvalidAmount,
    #[error("Enter a valid address for the selected network")]
    InvalidAddress,
    #[error("The payment request is for a different network or token")]
    WrongNetwork,
    #[error("Wallet credentials do not match this USDT account")]
    InvalidCredentials,
    #[error("Set your device date and time automatically, then try again")]
    ClockSkew,
    #[error("This account uses another wallet's smart account. Restore its delegation before sending with Bitkit")]
    UnsupportedDelegation,
    #[error("The USDT balance does not cover the amount and maximum fee")]
    InsufficientBalance,
    #[error("The fee quote has expired or changed. Review a new quote")]
    QuoteExpired,
    #[error("A USDT transaction is pending. Wait for confirmation before sending again")]
    PendingTransfer,
    #[error("The selected USDT payment route is unavailable")]
    UnsupportedRoute,
    #[error("This deposit needs provider assistance. Check its recovery status")]
    DepositNeedsAttention,
    #[error("Deposit details changed. Refresh the deposit history and select it again")]
    DepositNotFound,
    #[error("The deposit service could not verify this request. Try again")]
    DepositAuthorizationRejected,
    #[error("The amount is outside this deposit route's limits. Review the minimum and maximum")]
    DepositAmountOutOfRange {
        min_usd_cents: Option<String>,
        max_usd_cents: Option<String>,
    },
    #[error("USDT payments are not configured for this app build")]
    NotConfigured,
    #[error("The network could not be reached. Try again")]
    NetworkUnavailable,
    #[error("Too many requests. Try again shortly")]
    RateLimited,
    #[error("The requested log range exceeds the provider limit")]
    LogRangeTooLarge,
    #[error("The network rejected the transaction: {reason}")]
    TransactionRejected { reason: String },
    #[error("USDT wallet storage failed: {reason}")]
    Storage { reason: String },
    #[error("Invalid response from the USDT network")]
    InvalidResponse,
}

impl From<rusqlite::Error> for UsdtError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage {
            reason: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for UsdtError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidResponse
    }
}
