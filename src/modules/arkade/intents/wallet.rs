//! The wallet behind the client: Arkade network I/O and the identity key. The binding implements
//! it on its ark-client `Client`; every spend is built and signed here, so the trait carries no
//! swap logic.

use super::error::WalletError;
use ark_core::server::Info;
use ark_core::server::VirtualTxOutPoint;
use ark_core::ArkAddress;
use async_trait::async_trait;
use bitcoin::secp256k1::Keypair;
use bitcoin::Amount;
use bitcoin::Psbt;
use bitcoin::Txid;

#[async_trait]
pub trait ArkWallet: Send + Sync {
    /// The operator's terms (`ark_client::Client::server_info`).
    async fn server_info(&self) -> Result<Info, WalletError>;

    /// The wallet's offchain receive address: receive payouts and send refunds land here.
    async fn address(&self) -> Result<ArkAddress, WalletError>;

    /// The identity key, `m/86'/c'/1'/0/0`: the TS wallet's static key. It is the receive claim
    /// key (its preimages derive from it) and the send refund key.
    fn identity_keypair(&self) -> Result<Keypair, WalletError>;

    /// Every VTXO, spent or not, at an address (`get_virtual_tx_outpoints`).
    async fn vtxos(&self, address: &ArkAddress) -> Result<Vec<VirtualTxOutPoint>, WalletError>;

    /// A virtual transaction by id (the indexer's `GetVirtualTxs`), `None` when unknown. Used to
    /// read the preimage out of the solver's claim of a send lockup.
    async fn virtual_tx(&self, txid: Txid) -> Result<Option<Psbt>, WalletError>;

    /// `submit_offchain_transaction_request`; returns the server-signed checkpoints.
    async fn submit_offchain(
        &self,
        ark_tx: Psbt,
        checkpoints: Vec<Psbt>,
    ) -> Result<Vec<Psbt>, WalletError>;

    /// `finalize_offchain_transaction`.
    async fn finalize_offchain(
        &self,
        ark_txid: Txid,
        checkpoints: Vec<Psbt>,
    ) -> Result<(), WalletError>;

    /// Pay `amount` from the wallet's own VTXOs to `address` (funds a send lockup).
    async fn send(&self, address: &ArkAddress, amount: Amount) -> Result<Txid, WalletError>;
}
