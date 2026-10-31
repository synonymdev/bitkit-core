//! Keys from the Bitkit seed: BIP86 account 1 (`m/86'/0'/1'` on mainnet, `m/86'/1'/1'` elsewhere), receive keys
//! under `/0/*`, as the bridge's `tr([fp/86'/c'/1']xpub/0/*)` descriptor gives the TypeScript `SeedIdentity`.
//! Savings' Taproot keys use account 0, so the two wallets never share a key.

use super::errors::ArkadeError;
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::key::{Keypair, Secp256k1};
use bitcoin::Network;

/// The BIP86 account Arkade uses; Savings uses account 0.
pub const ARK_ACCOUNT: u32 = 1;

pub struct Keys {
    pub master: Xpriv,
    pub network: Network,
    /// `m/86'/c'/1'`.
    pub account_path: DerivationPath,
    /// `m/86'/c'/1'/0`: the base the HD key provider appends receive indices to.
    pub receive_path: DerivationPath,
    /// `m/86'/c'/1'/0/0`, the TypeScript identity's own key: it signs the LNURL session token and is the
    /// registered claim key, whatever receive index the wallet has rotated to.
    pub identity: Keypair,
}

impl Keys {
    pub fn from_mnemonic(
        mnemonic: &str,
        passphrase: Option<&str>,
        network: Network,
    ) -> Result<Self, ArkadeError> {
        let mnemonic = bip39::Mnemonic::parse_normalized(mnemonic.trim())
            .map_err(|e| ArkadeError::invalid(format!("invalid mnemonic: {e}")))?;
        let seed = mnemonic.to_seed(passphrase.unwrap_or(""));
        let master = Xpriv::new_master(network, &seed).map_err(ArkadeError::internal)?;
        let coin = if network == Network::Bitcoin { 0 } else { 1 };
        let account_path: DerivationPath = vec![
            ChildNumber::from_hardened_idx(86).map_err(ArkadeError::internal)?,
            ChildNumber::from_hardened_idx(coin).map_err(ArkadeError::internal)?,
            ChildNumber::from_hardened_idx(ARK_ACCOUNT).map_err(ArkadeError::internal)?,
        ]
        .into();
        let receive_path =
            account_path.child(ChildNumber::from_normal_idx(0).map_err(ArkadeError::internal)?);
        let identity_path =
            receive_path.child(ChildNumber::from_normal_idx(0).map_err(ArkadeError::internal)?);
        let secp = Secp256k1::new();
        let identity = master
            .derive_priv(&secp, &identity_path)
            .map_err(ArkadeError::internal)?
            .to_keypair(&secp);
        Ok(Self {
            master,
            network,
            account_path,
            receive_path,
            identity,
        })
    }

    /// The account-level extended key. The on-chain wallet ark-client requires is built from it, so its BDK
    /// BIP86 template lands under Arkade's account instead of on Savings' `m/86'/c'/0'`.
    pub fn account_xpriv(&self) -> Result<Xpriv, ArkadeError> {
        self.master
            .derive_priv(&Secp256k1::new(), &self.account_path)
            .map_err(ArkadeError::internal)
    }
}
