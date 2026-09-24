use super::UsdtError;
use alloy_primitives::Address;
use bip39::Mnemonic;
use bitcoin::{
    bip32::{DerivationPath, Xpriv},
    secp256k1::{Secp256k1, SecretKey},
};
use std::str::FromStr;
use zeroize::Zeroizing;

pub(super) struct SigningKey(SecretKey);
impl std::ops::Deref for SigningKey {
    type Target = SecretKey;
    fn deref(&self) -> &SecretKey {
        &self.0
    }
}
impl Drop for SigningKey {
    fn drop(&mut self) {
        self.0.non_secure_erase();
    }
}

pub(super) fn derive_key(
    mnemonic: Zeroizing<String>,
    passphrase: Option<Zeroizing<String>>,
) -> Result<SigningKey, UsdtError> {
    let passphrase = passphrase.unwrap_or_default();
    let mnemonic = Mnemonic::parse(&*mnemonic).map_err(|_| UsdtError::InvalidCredentials)?;
    let seed = Zeroizing::new(mnemonic.to_seed(passphrase.as_str()));
    let mut root = Xpriv::new_master(bitcoin::Network::Bitcoin, seed.as_ref())
        .map_err(|_| UsdtError::InvalidCredentials)?;
    let path =
        DerivationPath::from_str("m/44'/60'/0'/0/0").map_err(|_| UsdtError::InvalidCredentials)?;
    let derived = root.derive_priv(&Secp256k1::new(), &path);
    root.private_key.non_secure_erase();
    let mut derived = derived.map_err(|_| UsdtError::InvalidCredentials)?;
    let key = SigningKey(derived.private_key);
    derived.private_key.non_secure_erase();
    Ok(key)
}

pub(super) fn key_address(key: &SecretKey) -> Address {
    let public = key.public_key(&Secp256k1::new()).serialize_uncompressed();
    Address::from_raw_public_key(&public[1..])
}

#[uniffi::export]
pub fn usdt_address(mnemonic: String, passphrase: Option<String>) -> Result<String, UsdtError> {
    Ok(key_address(&*derive_key(mnemonic.into(), passphrase.map(Into::into))?).to_checksum(None))
}

pub(super) fn parse_address(value: &str) -> Result<Address, UsdtError> {
    let address = Address::from_str(value).map_err(|_| UsdtError::InvalidAddress)?;
    if value.len() != 42 || !value.starts_with("0x") || address == Address::ZERO {
        return Err(UsdtError::InvalidAddress);
    }
    let body = &value[2..];
    if body.bytes().any(|c| c.is_ascii_lowercase())
        && body.bytes().any(|c| c.is_ascii_uppercase())
        && address.to_checksum(None) != value
    {
        return Err(UsdtError::InvalidAddress);
    }
    Ok(address)
}
