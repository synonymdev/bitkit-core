use thiserror::Error;

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum AddressError {
    #[error("Invalid Bitcoin address format")]
    InvalidAddress,
    #[error("Invalid network type")]
    InvalidNetwork,
    #[error("Mnemonic generation failed")]
    MnemonicGenerationFailed,
    #[error("Invalid mnemonic format")]
    InvalidMnemonic,
    #[error("Invalid entropy")]
    InvalidEntropy,
    #[error("Address derivation failed")]
    AddressDerivationFailed,
}

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum SweepError {
    #[error("Sweep operation failed: {0}")]
    SweepFailed(String),
    #[error("No UTXOs found to sweep")]
    NoUtxosFound,
    #[error("Invalid mnemonic format")]
    InvalidMnemonic,
}

#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum BroadcastError {
    #[error("Invalid transaction hex: {error_details}")]
    InvalidHex { error_details: String },
    #[error("Invalid transaction data: {error_details}")]
    InvalidTransaction { error_details: String },
    #[error("Electrum error: {error_details}")]
    ElectrumError { error_details: String },
    #[error("Task error: {error_details}")]
    TaskError { error_details: String },
}

/// Errors specific to account info operations (BDK/Electrum-based).
#[derive(uniffi::Error, Debug, Error)]
#[non_exhaustive]
pub enum AccountInfoError {
    /// The provided extended public key is invalid or cannot be parsed
    #[error("Invalid extended public key: {error_details}")]
    InvalidExtendedKey { error_details: String },

    /// The provided address is invalid
    #[error("Invalid address: {error_details}")]
    InvalidAddress { error_details: String },

    /// Electrum connection or query failed
    #[error("Electrum connection failed: {error_details}")]
    ElectrumError { error_details: String },

    /// BDK wallet creation or operation error
    #[error("Wallet error: {error_details}")]
    WalletError { error_details: String },

    /// Wallet sync with Electrum failed
    #[error("Sync failed: {error_details}")]
    SyncError { error_details: String },

    /// The key type/prefix is not recognized
    #[error("Unsupported key type: {error_details}")]
    UnsupportedKeyType { error_details: String },

    /// Network mismatch between key prefix and specified network
    #[error("Network mismatch: {error_details}")]
    NetworkMismatch { error_details: String },

    /// Invalid transaction ID provided
    #[error("Invalid transaction ID: {error_details}")]
    InvalidTxid { error_details: String },

    /// A valid transaction ID was not found in the wallet
    #[error("Transaction not found: {error_details}")]
    TransactionNotFound { error_details: String },
}
