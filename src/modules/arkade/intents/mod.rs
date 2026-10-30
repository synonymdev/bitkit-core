//! Arkade Intents Lightning client: quotes Lightning swaps from an Arkade solver over Nostr RFQ,
//! verifies them against locally derived VHTLC v2 lockups, and drives claims and refunds.
//!
//! Every module refers to its siblings through `super::`. `ArkWallet` is implemented on the Arkade wallet in
//! the Arkade wallet (`src/modules/arkade/wallet.rs`); swap records live in the Arkade database
//! (`src/modules/arkade/store.rs`), so `arkade_export_state` carries them.

pub mod bolt11;
pub mod client;
pub mod contract;
pub mod error;

pub mod preimage;
pub mod spend;
pub mod store;
pub mod transport;
pub mod vhtlc;
pub mod wallet;
pub mod wire;

#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod mainnet_tests;
#[cfg(test)]
mod oracle_tests;

pub use client::IntentsClient;
pub use client::PayQuote;
pub use error::Error;
pub use error::Result;
pub use error::WalletError;
pub use store::Direction;
pub use store::JsonStore;
pub use store::Outcome;
pub use store::SwapRecord;
pub use store::SwapStore;
pub use transport::SolverCard;
pub use wallet::ArkWallet;
