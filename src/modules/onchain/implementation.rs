use std::collections::HashMap;
use std::str::FromStr;

use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::absolute::LockTime;
use bdk::bitcoin::bip32::ExtendedPubKey;
use bdk::bitcoin::consensus::deserialize;
use bdk::bitcoin::psbt::PartiallySignedTransaction as Psbt;
use bdk::bitcoin::{
    Address as BdkAddress, Network as BdkNetwork, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxOut, Txid, Witness,
};
use bdk::blockchain::ElectrumBlockchain;
use bdk::database::MemoryDatabase;
use bdk::electrum_client::ElectrumApi;
use bdk::keys::bip39::Mnemonic;
use bdk::template::{Bip44, Bip49, Bip86};
use bdk::wallet::signer::SignOptions;
use bdk::wallet::{AddressIndex as BdkAddressIndex, SyncOptions, Wallet};
use bdk::KeychainKind;
use bitcoin::address::{Address, NetworkUnchecked};
use bitcoin::Network;
use bitcoin_address_generator;

use super::errors::AccountInfoError;
use super::types::{
    classify_tx, AccountAddresses, AccountInfoResult, AccountType, AccountUtxo, AddressInfo,
    AddressType, ComposeAccount, HistoryTransaction, Network as OnchainNetwork,
    SingleAddressInfoResult, TransactionDetail, TransactionHistoryResult, TxDetailInput,
    TxDetailOutput, ValidationResult, WalletBalance,
};
use crate::modules::scanner::NetworkType;
use crate::onchain::types::{
    GetAddressResponse, GetAddressesResponse, SweepResult, SweepTransactionPreview,
    SweepableBalances, WordCount,
};
use crate::onchain::{AddressError, BroadcastError, SweepError};

struct SweepWallets {
    legacy_wallet: Wallet<MemoryDatabase>,
    p2sh_wallet: Wallet<MemoryDatabase>,
    taproot_wallet: Wallet<MemoryDatabase>,
}

pub struct BitcoinAddressValidator;

impl BitcoinAddressValidator {
    pub fn validate_address(address: &str) -> Result<ValidationResult, AddressError> {
        println!("\nValidating address: {}", address);

        let unchecked_addr = match parse_address(address) {
            Ok(addr) => addr,
            Err(e) => return Err(e),
        };
        let expected_network = match determine_network(address) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        match verify_network(unchecked_addr, expected_network.into()) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        let address_type = get_address_type(address)?;

        println!("✓ Validation successful!");

        Ok(ValidationResult {
            address: address.to_string(),
            network: NetworkType::from(expected_network),
            address_type,
        })
    }

    pub fn genenerate_mnemonic(word_count: Option<WordCount>) -> Result<String, AddressError> {
        let external_word_count = word_count.map(|wc| wc.into());
        let mnemonic = bitcoin_address_generator::generate_mnemonic(external_word_count, None);
        match mnemonic {
            Ok(mnemonic) => {
                println!("✓ Generated mnemonic: {}", mnemonic);
                Ok(mnemonic)
            }
            Err(e) => {
                println!("✗ Failed to generate mnemonic: {:?}", e);
                Err(AddressError::MnemonicGenerationFailed)
            }
        }
    }

    pub fn validate_mnemonic(mnemonic_phrase: &str) -> Result<(), AddressError> {
        bitcoin_address_generator::validate_mnemonic(mnemonic_phrase)
            .map_err(|_| AddressError::InvalidMnemonic)
    }

    pub fn is_valid_bip39_word(word: &str) -> bool {
        bitcoin_address_generator::is_valid_bip39_word(word, None)
    }

    pub fn get_bip39_suggestions(partial_word: &str, limit: usize) -> Vec<String> {
        bitcoin_address_generator::get_bip39_suggestions(partial_word, limit, None)
    }

    pub fn get_bip39_wordlist() -> Vec<String> {
        bitcoin_address_generator::get_bip39_wordlist(None)
    }

    pub fn mnemonic_to_entropy(mnemonic_phrase: &str) -> Result<Vec<u8>, AddressError> {
        bitcoin_address_generator::mnemonic_to_entropy(mnemonic_phrase)
            .map_err(|_| AddressError::InvalidMnemonic)
    }

    pub fn entropy_to_mnemonic(entropy: &[u8]) -> Result<String, AddressError> {
        bitcoin_address_generator::entropy_to_mnemonic(entropy, None)
            .map_err(|_| AddressError::InvalidEntropy)
    }

    pub fn mnemonic_to_seed(
        mnemonic_phrase: &str,
        passphrase: Option<&str>,
    ) -> Result<Vec<u8>, AddressError> {
        bitcoin_address_generator::mnemonic_to_seed(mnemonic_phrase, passphrase)
            .map_err(|_| AddressError::InvalidMnemonic)
    }

    pub fn derive_bitcoin_address(
        mnemonic_phrase: &str,
        derivation_path_str: Option<&str>,
        network: Option<Network>,
        bip39_passphrase: Option<&str>,
    ) -> Result<GetAddressResponse, AddressError> {
        let address = bitcoin_address_generator::derive_bitcoin_address(
            mnemonic_phrase,
            derivation_path_str,
            network.into(),
            bip39_passphrase,
        )
        .map_err(|e| {
            println!("✗ Failed to derive address: {:?}", e);
            AddressError::AddressDerivationFailed
        })?;

        Ok(address.into())
    }

    pub fn derive_bitcoin_addresses(
        mnemonic_phrase: &str,
        derivation_path_str: Option<&str>,
        network: Option<Network>,
        bip39_passphrase: Option<&str>,
        is_change: Option<bool>,
        start_index: Option<u32>,
        count: Option<u32>,
    ) -> Result<GetAddressesResponse, AddressError> {
        let addresses = bitcoin_address_generator::derive_bitcoin_addresses(
            mnemonic_phrase,
            derivation_path_str,
            network.into(),
            bip39_passphrase,
            is_change,
            start_index,
            count,
        )
        .map_err(|e| {
            println!("✗ Failed to derive addresses: {:?}", e);
            AddressError::AddressDerivationFailed
        })?;

        Ok(addresses.into())
    }

    pub fn derive_private_key(
        mnemonic_phrase: &str,
        derivation_path_str: Option<&str>,
        network: Option<Network>,
        bip39_passphrase: Option<&str>,
    ) -> Result<String, AddressError> {
        let private_key = bitcoin_address_generator::derive_private_key(
            mnemonic_phrase,
            derivation_path_str,
            network.into(),
            bip39_passphrase,
        )
        .map_err(|e| {
            println!("✗ Failed to derive private key: {:?}", e);
            AddressError::AddressDerivationFailed
        })?;

        Ok(private_key)
    }

    fn create_sweep_wallets(
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
    ) -> Result<SweepWallets, SweepError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let mnemonic =
            Mnemonic::from_str(mnemonic_phrase).map_err(|_| SweepError::InvalidMnemonic)?;
        let key = (mnemonic.clone(), bip39_passphrase.map(String::from));

        let legacy_wallet = Wallet::new(
            Bip44(key.clone(), KeychainKind::External),
            Some(Bip44(key.clone(), KeychainKind::Internal)),
            bdk_network,
            MemoryDatabase::new(),
        )
        .map_err(|e| SweepError::SweepFailed(format!("Failed to create legacy wallet: {}", e)))?;

        let p2sh_wallet = Wallet::new(
            Bip49(key.clone(), KeychainKind::External),
            Some(Bip49(key.clone(), KeychainKind::Internal)),
            bdk_network,
            MemoryDatabase::new(),
        )
        .map_err(|e| SweepError::SweepFailed(format!("Failed to create P2SH wallet: {}", e)))?;

        let taproot_wallet = Wallet::new(
            Bip86(key.clone(), KeychainKind::External),
            Some(Bip86(key, KeychainKind::Internal)),
            bdk_network,
            MemoryDatabase::new(),
        )
        .map_err(|e| SweepError::SweepFailed(format!("Failed to create Taproot wallet: {}", e)))?;

        Ok(SweepWallets {
            legacy_wallet,
            p2sh_wallet,
            taproot_wallet,
        })
    }

    fn create_electrum_client(
        electrum_url: &str,
    ) -> Result<bdk::electrum_client::Client, SweepError> {
        bdk::electrum_client::Client::new(electrum_url)
            .map_err(|e| SweepError::SweepFailed(format!("Failed to connect to Electrum: {}", e)))
    }

    fn create_electrum_backend(electrum_url: &str) -> Result<ElectrumBlockchain, SweepError> {
        let client = Self::create_electrum_client(electrum_url)?;
        Ok(ElectrumBlockchain::from(client))
    }

    fn sync_wallets(
        wallets: &mut SweepWallets,
        backend: &ElectrumBlockchain,
    ) -> Result<(), SweepError> {
        wallets
            .legacy_wallet
            .sync(backend, SyncOptions::default())
            .map_err(|e| SweepError::SweepFailed(format!("Failed to sync legacy wallet: {}", e)))?;

        wallets
            .p2sh_wallet
            .sync(backend, SyncOptions::default())
            .map_err(|e| SweepError::SweepFailed(format!("Failed to sync P2SH wallet: {}", e)))?;

        wallets
            .taproot_wallet
            .sync(backend, SyncOptions::default())
            .map_err(|e| {
                SweepError::SweepFailed(format!("Failed to sync Taproot wallet: {}", e))
            })?;

        Ok(())
    }

    fn sign_psbt(wallets: &SweepWallets, psbt: &mut Psbt) -> Result<(), SweepError> {
        let sign_options = SignOptions {
            trust_witness_utxo: true,
            allow_all_sighashes: true,
            ..Default::default()
        };

        wallets
            .legacy_wallet
            .sign(psbt, sign_options.clone())
            .map_err(|e| {
                SweepError::SweepFailed(format!("Failed to sign with legacy wallet: {}", e))
            })?;

        wallets
            .p2sh_wallet
            .sign(psbt, sign_options.clone())
            .map_err(|e| {
                SweepError::SweepFailed(format!("Failed to sign with P2SH wallet: {}", e))
            })?;

        wallets
            .taproot_wallet
            .sign(psbt, sign_options)
            .map_err(|e| {
                SweepError::SweepFailed(format!("Failed to sign with Taproot wallet: {}", e))
            })?;

        Ok(())
    }

    pub async fn check_sweepable_balances(
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
        electrum_url: &str,
    ) -> Result<SweepableBalances, SweepError> {
        let wallets = Self::create_sweep_wallets(mnemonic_phrase, network, bip39_passphrase)?;

        let electrum_url = electrum_url.to_string();
        let wallets = tokio::task::spawn_blocking(move || {
            let backend = Self::create_electrum_backend(&electrum_url)?;
            let mut wallets = wallets;
            Self::sync_wallets(&mut wallets, &backend)?;
            Ok::<_, SweepError>(wallets)
        })
        .await
        .map_err(|e| SweepError::SweepFailed(format!("Sync task failed: {}", e)))??;

        let legacy_utxos = wallets
            .legacy_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list legacy UTXOs: {}", e)))?;

        let p2sh_utxos = wallets
            .p2sh_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list P2SH UTXOs: {}", e)))?;

        let taproot_utxos = wallets
            .taproot_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list Taproot UTXOs: {}", e)))?;

        let legacy_balance: u64 = legacy_utxos.iter().map(|u| u.txout.value).sum();
        let p2sh_balance: u64 = p2sh_utxos.iter().map(|u| u.txout.value).sum();
        let taproot_balance: u64 = taproot_utxos.iter().map(|u| u.txout.value).sum();

        Ok(SweepableBalances {
            legacy_balance,
            p2sh_balance,
            taproot_balance,
            total_balance: legacy_balance + p2sh_balance + taproot_balance,
            legacy_utxos_count: legacy_utxos.len() as u32,
            p2sh_utxos_count: p2sh_utxos.len() as u32,
            taproot_utxos_count: taproot_utxos.len() as u32,
            total_utxos_count: (legacy_utxos.len() + p2sh_utxos.len() + taproot_utxos.len()) as u32,
        })
    }

    pub async fn prepare_sweep_transaction(
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
        electrum_url: &str,
        destination_address: &str,
        fee_rate_sats_per_vbyte: Option<u32>,
    ) -> Result<SweepTransactionPreview, SweepError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let wallets = Self::create_sweep_wallets(mnemonic_phrase, network, bip39_passphrase)?;

        let dest_addr = BdkAddress::from_str(destination_address)
            .map_err(|e| SweepError::SweepFailed(format!("Invalid destination address: {}", e)))?
            .require_network(bdk_network)
            .map_err(|e| {
                SweepError::SweepFailed(format!("Network mismatch for destination address: {}", e))
            })?;

        let electrum_url_owned = electrum_url.to_string();
        let (wallets, electrum_client) = tokio::task::spawn_blocking(move || {
            let backend = Self::create_electrum_backend(&electrum_url_owned)?;
            let mut wallets = wallets;
            Self::sync_wallets(&mut wallets, &backend)?;

            let tx_client = Self::create_electrum_client(&electrum_url_owned)?;
            Ok::<_, SweepError>((wallets, tx_client))
        })
        .await
        .map_err(|e| SweepError::SweepFailed(format!("Sync task failed: {}", e)))??;

        let legacy_utxos: Vec<_> = wallets
            .legacy_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list legacy UTXOs: {}", e)))?;

        let p2sh_utxos: Vec<_> = wallets
            .p2sh_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list P2SH UTXOs: {}", e)))?;

        let taproot_utxos: Vec<_> = wallets
            .taproot_wallet
            .list_unspent()
            .map_err(|e| SweepError::SweepFailed(format!("Failed to list Taproot UTXOs: {}", e)))?;

        let mut all_utxos: Vec<_> = legacy_utxos.iter().collect();
        all_utxos.extend(p2sh_utxos.iter());
        all_utxos.extend(taproot_utxos.iter());

        if all_utxos.is_empty() {
            return Err(SweepError::NoUtxosFound);
        }

        let total_amount: u64 = all_utxos.iter().map(|u| u.txout.value).sum();
        let legacy_count = legacy_utxos.len();
        let p2sh_count = p2sh_utxos.len();

        let build_psbt = |output_value: u64| -> Result<Psbt, SweepError> {
            let inputs: Vec<TxIn> = all_utxos
                .iter()
                .map(|utxo| TxIn {
                    previous_output: OutPoint {
                        txid: utxo.outpoint.txid,
                        vout: utxo.outpoint.vout,
                    },
                    script_sig: ScriptBuf::new(),
                    sequence: Sequence::MAX,
                    witness: Witness::new(),
                })
                .collect();

            let tx = Transaction {
                version: 2,
                lock_time: LockTime::from_consensus(0),
                input: inputs,
                output: vec![TxOut {
                    value: output_value,
                    script_pubkey: dest_addr.script_pubkey(),
                }],
            };

            let mut psbt = Psbt::from_unsigned_tx(tx)
                .map_err(|e| SweepError::SweepFailed(format!("Failed to create PSBT: {}", e)))?;

            for (i, utxo) in all_utxos.iter().enumerate() {
                psbt.inputs[i].witness_utxo = Some(utxo.txout.clone());

                if i < legacy_count + p2sh_count {
                    let tx_bytes = electrum_client
                        .transaction_get_raw(&utxo.outpoint.txid)
                        .map_err(|e| {
                            SweepError::SweepFailed(format!(
                                "Failed to fetch tx {}: {}",
                                utxo.outpoint.txid, e
                            ))
                        })?;

                    let tx: Transaction = deserialize(&tx_bytes).map_err(|e| {
                        SweepError::SweepFailed(format!(
                            "Failed to deserialize tx {}: {}",
                            utxo.outpoint.txid, e
                        ))
                    })?;

                    psbt.inputs[i].non_witness_utxo = Some(tx);
                }
            }

            Ok(psbt)
        };

        let mut probe_psbt = build_psbt(total_amount)?;
        Self::sign_psbt(&wallets, &mut probe_psbt)?;
        let actual_vsize = probe_psbt.extract_tx().weight().to_vbytes_ceil();

        let fee_rate_sats = fee_rate_sats_per_vbyte.unwrap_or(1) as u64;
        let estimated_fee = actual_vsize * fee_rate_sats;
        let amount_after_fees = total_amount.saturating_sub(estimated_fee);

        let final_psbt = build_psbt(amount_after_fees)?;
        let psbt_base64 = general_purpose::STANDARD.encode(final_psbt.serialize());

        Ok(SweepTransactionPreview {
            psbt: psbt_base64,
            total_amount,
            estimated_fee,
            estimated_vsize: actual_vsize,
            utxos_count: all_utxos.len() as u32,
            destination_address: dest_addr.to_string(),
            amount_after_fees,
        })
    }

    pub async fn broadcast_sweep_transaction(
        psbt_base64: &str,
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
        electrum_url: &str,
    ) -> Result<SweepResult, SweepError> {
        let psbt_bytes = general_purpose::STANDARD
            .decode(psbt_base64)
            .map_err(|e| SweepError::SweepFailed(format!("Failed to decode PSBT: {}", e)))?;

        let psbt = Psbt::deserialize(&psbt_bytes)
            .map_err(|e| SweepError::SweepFailed(format!("Failed to deserialize PSBT: {}", e)))?;

        if psbt.unsigned_tx.output.len() != 1 {
            return Err(SweepError::SweepFailed(format!(
                "PSBT must have exactly 1 output, found {}",
                psbt.unsigned_tx.output.len()
            )));
        }

        let total_input: u64 = psbt
            .inputs
            .iter()
            .filter_map(|i| i.witness_utxo.as_ref())
            .map(|u| u.value)
            .sum();

        let output_amount = psbt.unsigned_tx.output[0].value;
        let fee_amount = total_input.saturating_sub(output_amount);
        let utxos_count = psbt.inputs.len() as u32;

        let wallets = Self::create_sweep_wallets(mnemonic_phrase, network, bip39_passphrase)?;

        let electrum_url_owned = electrum_url.to_string();
        let wallets = tokio::task::spawn_blocking(move || {
            let backend = Self::create_electrum_backend(&electrum_url_owned)?;
            let mut wallets = wallets;
            Self::sync_wallets(&mut wallets, &backend)?;
            Ok::<_, SweepError>(wallets)
        })
        .await
        .map_err(|e| SweepError::SweepFailed(format!("Sync task failed: {}", e)))??;

        let mut signing_psbt = psbt;
        Self::sign_psbt(&wallets, &mut signing_psbt)?;

        for input in &signing_psbt.inputs {
            if input.final_script_sig.is_none() && input.final_script_witness.is_none() {
                return Err(SweepError::SweepFailed(
                    "Transaction signing incomplete - some inputs not finalized".to_string(),
                ));
            }
        }

        let final_tx = signing_psbt.extract_tx();
        let txid = final_tx.txid();

        let electrum_url_owned = electrum_url.to_string();
        tokio::task::spawn_blocking(move || {
            use bdk::blockchain::Blockchain;
            let backend = Self::create_electrum_backend(&electrum_url_owned)?;
            backend
                .broadcast(&final_tx)
                .map_err(|e| SweepError::SweepFailed(format!("Broadcast failed: {}", e)))
        })
        .await
        .map_err(|e| SweepError::SweepFailed(format!("Broadcast task panicked: {}", e)))??;

        Ok(SweepResult {
            txid: txid.to_string(),
            amount_swept: output_amount,
            fee_paid: fee_amount,
            utxos_swept: utxos_count,
        })
    }
}

/// Broadcast a signed raw transaction via Electrum.
///
/// Takes a hex-encoded serialized transaction and an Electrum server URL.
/// Returns the transaction ID on success.
pub async fn broadcast_raw_tx(
    serialized_tx: String,
    electrum_url: &str,
) -> Result<String, BroadcastError> {
    let tx_bytes = hex::decode(&serialized_tx).map_err(|e| BroadcastError::InvalidHex {
        error_details: format!("Invalid transaction hex: {}", e),
    })?;

    // Validate that the bytes are a valid transaction
    let _tx: Transaction =
        deserialize(&tx_bytes).map_err(|e| BroadcastError::InvalidTransaction {
            error_details: format!("Invalid transaction data: {}", e),
        })?;

    let electrum_url_owned = electrum_url.to_string();

    let txid = tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            BroadcastError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        client
            .transaction_broadcast_raw(&tx_bytes)
            .map_err(|e| BroadcastError::ElectrumError {
                error_details: format!("Broadcast failed: {}", e),
            })
    })
    .await
    .map_err(|e| BroadcastError::TaskError {
        error_details: format!("Broadcast task failed: {}", e),
    })??;

    Ok(txid.to_string())
}

// ============================================================================
// Account info: key/account type detection helpers
// ============================================================================

/// Detect the account type from an extended public key prefix.
/// `xpub`/`tpub` default to `Legacy`; use `account_type_override` for other script types.
pub fn detect_account_type(extended_key: &str) -> Result<AccountType, AccountInfoError> {
    let prefix = extended_key
        .get(..4)
        .ok_or(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        })?;
    match prefix {
        "xpub" | "tpub" => Ok(AccountType::Legacy),
        "ypub" | "upub" => Ok(AccountType::WrappedSegwit),
        "zpub" | "vpub" => Ok(AccountType::NativeSegwit),
        prefix => Err(AccountInfoError::UnsupportedKeyType {
            error_details: format!("Unsupported key prefix: {}", prefix),
        }),
    }
}

/// Detect network from an extended public key prefix.
pub fn detect_network_from_key(extended_key: &str) -> Result<BdkNetwork, AccountInfoError> {
    let prefix = extended_key
        .get(..4)
        .ok_or(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        })?;
    match prefix {
        "xpub" | "ypub" | "zpub" => Ok(BdkNetwork::Bitcoin),
        "tpub" | "upub" | "vpub" => Ok(BdkNetwork::Testnet),
        prefix => Err(AccountInfoError::UnsupportedKeyType {
            error_details: format!("Cannot determine network from prefix: {}", prefix),
        }),
    }
}

/// Convert ypub/zpub/upub/vpub to xpub/tpub by swapping the version bytes.
/// BDK only understands standard xpub/tpub format.
pub fn normalize_extended_key(extended_key: &str) -> Result<String, AccountInfoError> {
    let prefix = extended_key
        .get(..4)
        .ok_or(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        })?;
    let target_version: Option<[u8; 4]> = match prefix {
        "xpub" | "tpub" => None,                           // Already standard format
        "ypub" | "zpub" => Some([0x04, 0x88, 0xB2, 0x1E]), // Convert to xpub
        "upub" | "vpub" => Some([0x04, 0x35, 0x87, 0xCF]), // Convert to tpub
        _ => {
            return Err(AccountInfoError::UnsupportedKeyType {
                error_details: format!("Unknown key prefix: {}", prefix),
            })
        }
    };

    match target_version {
        None => Ok(extended_key.to_string()),
        Some(version) => {
            let mut decoded = bdk::bitcoin::base58::decode_check(extended_key).map_err(|e| {
                AccountInfoError::InvalidExtendedKey {
                    error_details: format!("Base58 decode failed: {:?}", e),
                }
            })?;

            if decoded.len() < 4 {
                return Err(AccountInfoError::InvalidExtendedKey {
                    error_details: "Decoded key too short".to_string(),
                });
            }

            decoded[0..4].copy_from_slice(&version);
            Ok(bdk::bitcoin::base58::encode_check(&decoded))
        }
    }
}

/// Build BDK descriptor strings for external and internal keychains.
///
/// When `key_origin` is provided as `(fingerprint_hex, derivation_path)`,
/// the descriptors include key origin info needed for PSBT BIP32 derivation
/// paths, e.g. `wpkh([73c5da0a/84'/0'/0']xpub.../0/*)`.
pub fn build_descriptors(
    normalized_xpub: &str,
    account_type: AccountType,
    key_origin: Option<(&str, &str)>,
) -> (String, String) {
    let key_expr = match key_origin {
        Some((fingerprint, path)) => format!("[{}/{}]{}", fingerprint, path, normalized_xpub),
        None => normalized_xpub.to_string(),
    };
    let (external, internal) = match account_type {
        AccountType::Legacy => (
            format!("pkh({}/0/*)", key_expr),
            format!("pkh({}/1/*)", key_expr),
        ),
        AccountType::WrappedSegwit => (
            format!("sh(wpkh({}/0/*))", key_expr),
            format!("sh(wpkh({}/1/*))", key_expr),
        ),
        AccountType::NativeSegwit => (
            format!("wpkh({}/0/*)", key_expr),
            format!("wpkh({}/1/*)", key_expr),
        ),
        AccountType::Taproot => (
            format!("tr({}/0/*)", key_expr),
            format!("tr({}/1/*)", key_expr),
        ),
    };
    (external, internal)
}

/// Determine the BIP derivation base path.
pub fn derive_base_path(
    account_type: AccountType,
    network: BdkNetwork,
    account_index: u32,
) -> String {
    let purpose = match account_type {
        AccountType::Legacy => 44,
        AccountType::WrappedSegwit => 49,
        AccountType::NativeSegwit => 84,
        AccountType::Taproot => 86,
    };
    let coin_type = match network {
        BdkNetwork::Bitcoin => 0,
        _ => 1, // testnet/signet/regtest all use coin_type 1
    };
    format!("m/{}'/{}'/{}'", purpose, coin_type, account_index)
}

// ============================================================================
// Shared wallet setup helper
// ============================================================================

pub(crate) struct WalletSetup {
    pub external_desc: String,
    pub internal_desc: String,
    pub network: BdkNetwork,
    pub base_path: String,
    pub account_type: AccountType,
}

/// Resolve an extended key into descriptors, network, and derivation path.
pub(crate) fn resolve_wallet_setup(
    extended_key: &str,
    network: Option<OnchainNetwork>,
    account_type_override: Option<AccountType>,
    fingerprint: Option<&str>,
) -> Result<WalletSetup, AccountInfoError> {
    let account_type = match account_type_override {
        Some(st) => match extended_key.get(..4).unwrap_or("") {
            "xpub" | "tpub" => st,
            _ => detect_account_type(extended_key)?,
        },
        _ => detect_account_type(extended_key)?,
    };

    let detected_network = detect_network_from_key(extended_key)?;

    if let Some(net) = network {
        let specified = onchain_to_bdk_network(net);
        // Regtest uses the same key prefixes (tpub/upub/vpub) as Testnet,
        // so treat them as compatible for key validation purposes.
        let networks_compatible = specified == detected_network
            || (specified == BdkNetwork::Regtest && detected_network == BdkNetwork::Testnet);
        if !networks_compatible {
            return Err(AccountInfoError::NetworkMismatch {
                error_details: format!(
                    "Key prefix suggests {:?} but {:?} was specified",
                    detected_network, specified
                ),
            });
        }
    }

    let normalized_key = normalize_extended_key(extended_key)?;

    let xpub = ExtendedPubKey::from_str(&normalized_key).map_err(|e| {
        AccountInfoError::InvalidExtendedKey {
            error_details: format!("Failed to parse extended key: {}", e),
        }
    })?;
    let account_index = match xpub.child_number {
        bdk::bitcoin::bip32::ChildNumber::Hardened { index } => index,
        bdk::bitcoin::bip32::ChildNumber::Normal { index } => index,
    };
    let base_path = derive_base_path(account_type, detected_network, account_index);

    let normalized_fp = if let Some(fp) = fingerprint {
        if fp.len() != 8 || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AccountInfoError::InvalidExtendedKey {
                error_details: format!(
                    "Fingerprint must be 8 hex characters (e.g. \"73c5da0a\"), got \"{}\"",
                    fp
                ),
            });
        }
        Some(fp.to_ascii_lowercase())
    } else {
        None
    };

    let derivation = base_path.strip_prefix("m/").unwrap_or(&base_path);
    let key_origin: Option<(&str, &str)> = normalized_fp.as_deref().map(|fp| (fp, derivation));
    let (external_desc, internal_desc) =
        build_descriptors(&normalized_key, account_type, key_origin);

    // Use the specified network when provided and compatible, so that
    // regtest addresses (bcrt1q) are validated correctly even though the
    // key prefix (tpub) is detected as testnet.
    let effective_network = network
        .map(onchain_to_bdk_network)
        .unwrap_or(detected_network);

    Ok(WalletSetup {
        external_desc,
        internal_desc,
        network: effective_network,
        base_path,
        account_type,
    })
}

/// Convert our Network enum to BDK's Network.
pub(crate) fn onchain_to_bdk_network(network: OnchainNetwork) -> BdkNetwork {
    match network {
        OnchainNetwork::Bitcoin => BdkNetwork::Bitcoin,
        OnchainNetwork::Testnet | OnchainNetwork::Testnet4 => BdkNetwork::Testnet,
        OnchainNetwork::Signet => BdkNetwork::Signet,
        OnchainNetwork::Regtest => BdkNetwork::Regtest,
    }
}

/// Connect to Electrum and return the raw client.
pub(crate) fn connect_electrum(
    electrum_url: &str,
) -> Result<bdk::electrum_client::Client, AccountInfoError> {
    bdk::electrum_client::Client::new(electrum_url).map_err(|e| AccountInfoError::ElectrumError {
        error_details: format!("Failed to connect to Electrum: {}", e),
    })
}

/// Connect to Electrum and fetch the current blockchain tip height.
///
/// Returns the raw client (not yet consumed by `create_and_sync_wallet`)
/// and the tip height as u32.
pub(crate) fn connect_and_get_tip(
    electrum_url: &str,
) -> Result<(bdk::electrum_client::Client, u32), AccountInfoError> {
    let client = connect_electrum(electrum_url)?;
    let header = client
        .block_headers_subscribe()
        .map_err(|e| AccountInfoError::ElectrumError {
            error_details: format!("Failed to get block height: {}", e),
        })?;
    let tip_height = u32::try_from(header.height).map_err(|_| AccountInfoError::ElectrumError {
        error_details: format!("Invalid block height: {}", header.height),
    })?;
    Ok((client, tip_height))
}

/// Create a BDK wallet and sync it via the provided Electrum client.
///
/// The client is consumed; make any pre-sync calls (e.g. `block_headers_subscribe`)
/// before passing it here.
pub(crate) fn create_and_sync_wallet(
    setup: &WalletSetup,
    client: bdk::electrum_client::Client,
) -> Result<Wallet<MemoryDatabase>, AccountInfoError> {
    let wallet = Wallet::new(
        &setup.external_desc,
        Some(&setup.internal_desc),
        setup.network,
        MemoryDatabase::new(),
    )
    .map_err(|e| AccountInfoError::WalletError {
        error_details: format!("Failed to create wallet: {}", e),
    })?;

    let blockchain = ElectrumBlockchain::from(client);
    wallet
        .sync(&blockchain, SyncOptions::default())
        .map_err(|e| AccountInfoError::SyncError {
            error_details: format!("Failed to sync wallet: {}", e),
        })?;

    Ok(wallet)
}

// ============================================================================
// Account info: main async functions
// ============================================================================

/// Query account information for an extended public key (xpub/ypub/zpub) via Electrum.
pub async fn get_account_info(
    extended_key: &str,
    electrum_url: &str,
    network: Option<OnchainNetwork>,
    gap_limit: Option<u32>,
    script_type: Option<AccountType>,
) -> Result<AccountInfoResult, AccountInfoError> {
    let setup = resolve_wallet_setup(extended_key, network, script_type, None)?;
    let gap = gap_limit.unwrap_or(20);
    let base_path = setup.base_path.clone();
    let account_type = setup.account_type;

    let electrum_url_owned = electrum_url.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let base_path = &setup.base_path;

        // Single Electrum connection: get tip height first, then sync wallet
        let (client, tip_height) = connect_and_get_tip(&electrum_url_owned)?;

        let wallet = create_and_sync_wallet(&setup, client)?;

        // Get the next unused external address index
        let next_external = wallet
            .get_address(BdkAddressIndex::LastUnused)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get address: {}", e),
            })?;
        let max_external = next_external.index.saturating_add(gap);

        // Get the next unused change address index
        let next_change = wallet
            .get_internal_address(BdkAddressIndex::LastUnused)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get change address: {}", e),
            })?;
        let max_change = next_change.index.saturating_add(gap);

        // Build address lists using BDK's LastUnused boundary (no extra Electrum calls)
        let mut address_paths: HashMap<ScriptBuf, String> = HashMap::new();

        // External addresses
        let mut used_addresses: Vec<AddressInfo> = Vec::new();
        let mut unused_addresses: Vec<AddressInfo> = Vec::new();

        for index in 0..max_external {
            let addr = wallet
                .get_address(BdkAddressIndex::Peek(index))
                .map_err(|e| AccountInfoError::WalletError {
                    error_details: format!("Failed to peek address at index {}: {}", index, e),
                })?;

            let addr_str = addr.address.to_string();
            let path = format!("{}/0/{}", base_path, index);
            let script = addr.address.script_pubkey();

            address_paths.insert(script, path.clone());

            let is_used = index < next_external.index;
            let info = AddressInfo {
                address: addr_str,
                path,
                transfers: if is_used { 1 } else { 0 },
            };

            if is_used {
                used_addresses.push(info);
            } else {
                unused_addresses.push(info);
            }
        }

        // Change addresses
        let mut change_addresses: Vec<AddressInfo> = Vec::new();

        for index in 0..max_change {
            let addr = wallet
                .get_internal_address(BdkAddressIndex::Peek(index))
                .map_err(|e| AccountInfoError::WalletError {
                    error_details: format!(
                        "Failed to peek change address at index {}: {}",
                        index, e
                    ),
                })?;

            let addr_str = addr.address.to_string();
            let path = format!("{}/1/{}", base_path, index);
            let script = addr.address.script_pubkey();

            address_paths.insert(script, path.clone());

            let is_used = index < next_change.index;
            change_addresses.push(AddressInfo {
                address: addr_str,
                path,
                transfers: if is_used { 1 } else { 0 },
            });
        }

        // Extract UTXOs
        let utxos = wallet
            .list_unspent()
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to list UTXOs: {}", e),
            })?;

        // Get transaction details for confirmation info and coinbase detection
        let transactions =
            wallet
                .list_transactions(true)
                .map_err(|e| AccountInfoError::WalletError {
                    error_details: format!("Failed to list transactions: {}", e),
                })?;

        // Map UTXOs to AccountUtxo
        let mut account_utxos: Vec<AccountUtxo> = Vec::new();
        for utxo in &utxos {
            let utxo_script = &utxo.txout.script_pubkey;

            let utxo_path = match address_paths.get(utxo_script) {
                Some(path) => path.clone(),
                None => {
                    log::warn!(
                        "No derivation path found for UTXO {}:{}",
                        utxo.outpoint.txid,
                        utxo.outpoint.vout,
                    );
                    String::new()
                }
            };

            let addr_str = BdkAddress::from_script(utxo_script, setup.network)
                .map(|a| a.to_string())
                .unwrap_or_default();

            // Get confirmation info and coinbase status from transaction details
            let tx_detail = transactions.iter().find(|tx| tx.txid == utxo.outpoint.txid);

            let (block_height, confirmations) = tx_detail
                .and_then(|tx| tx.confirmation_time.as_ref())
                .map(|conf| {
                    let height = conf.height;
                    let confs = tip_height.saturating_sub(height) + 1;
                    (height, confs)
                })
                .unwrap_or((0, 0));

            let coinbase = tx_detail
                .and_then(|tx| tx.transaction.as_ref())
                .map_or(false, |t| t.is_coin_base());

            account_utxos.push(AccountUtxo {
                txid: utxo.outpoint.txid.to_string(),
                vout: utxo.outpoint.vout,
                amount: utxo.txout.value,
                block_height,
                address: addr_str,
                path: utxo_path,
                confirmations,
                coinbase,
                own: true,
                required: None,
            });
        }

        let balance: u64 = utxos.iter().map(|u| u.txout.value).sum();

        Ok((
            used_addresses,
            unused_addresses,
            change_addresses,
            account_utxos,
            balance,
            u32::try_from(utxos.len()).unwrap_or(u32::MAX),
            tip_height,
        ))
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    let (
        used_addresses,
        unused_addresses,
        change_addresses,
        account_utxos,
        balance,
        utxo_count,
        block_height,
    ) = result;

    let account = ComposeAccount {
        path: base_path,
        addresses: AccountAddresses {
            used: used_addresses,
            unused: unused_addresses,
            change: change_addresses,
        },
        utxo: account_utxos,
    };

    Ok(AccountInfoResult {
        account,
        balance,
        utxo_count,
        account_type,
        block_height,
    })
}

/// Query transaction history and balance for an extended public key via Electrum.
pub async fn get_transaction_history(
    extended_key: &str,
    electrum_url: &str,
    network: Option<OnchainNetwork>,
    script_type: Option<AccountType>,
) -> Result<TransactionHistoryResult, AccountInfoError> {
    let setup = resolve_wallet_setup(extended_key, network, script_type, None)?;
    let account_type = setup.account_type;

    let electrum_url = electrum_url.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let (client, tip_height) = connect_and_get_tip(&electrum_url)?;

        let wallet = create_and_sync_wallet(&setup, client)?;

        // Balance
        let bdk_balance = wallet
            .get_balance()
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get balance: {}", e),
            })?;
        let balance: WalletBalance = bdk_balance.into();

        // Transaction history
        let txs = wallet
            .list_transactions(false)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to list transactions: {}", e),
            })?;

        let mut history: Vec<HistoryTransaction> = txs
            .iter()
            .map(|tx| {
                let (direction, amount, net) = classify_tx(tx.sent, tx.received, tx.fee);

                let (block_height, timestamp, confirmations) = match tx.confirmation_time.as_ref() {
                    Some(conf) => {
                        let confs = tip_height.saturating_sub(conf.height) + 1;
                        (Some(conf.height), Some(conf.timestamp), confs)
                    }
                    None => (None, None, 0),
                };

                HistoryTransaction {
                    txid: tx.txid.to_string(),
                    received: tx.received,
                    sent: tx.sent,
                    net,
                    fee: tx.fee,
                    amount,
                    direction,
                    block_height,
                    timestamp,
                    confirmations,
                }
            })
            .collect();

        // Sort: unconfirmed first, then by timestamp descending
        history.sort_by(|a, b| match (a.timestamp, b.timestamp) {
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
            (Some(a_ts), Some(b_ts)) => b_ts.cmp(&a_ts),
        });

        let tx_count = u32::try_from(history.len()).unwrap_or(u32::MAX);

        Ok((history, balance, tx_count, tip_height))
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    let (transactions, balance, tx_count, block_height) = result;

    Ok(TransactionHistoryResult {
        transactions,
        balance,
        tx_count,
        block_height,
        account_type,
    })
}

/// Get full details for a single transaction by txid.
///
/// Requires the extended public key because BDK needs to create a wallet to determine
/// which outputs belong to the wallet and to compute sent/received amounts.
pub async fn get_transaction_detail(
    extended_key: &str,
    electrum_url: &str,
    txid: &str,
    network: Option<OnchainNetwork>,
    script_type: Option<AccountType>,
) -> Result<TransactionDetail, AccountInfoError> {
    let target_txid = Txid::from_str(txid).map_err(|e| AccountInfoError::InvalidTxid {
        error_details: format!("Invalid txid '{}': {}", txid, e),
    })?;

    let setup = resolve_wallet_setup(extended_key, network, script_type, None)?;
    let wallet_network = setup.network;

    let electrum_url = electrum_url.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let (client, tip_height) = connect_and_get_tip(&electrum_url)?;

        let wallet = create_and_sync_wallet(&setup, client)?;

        // Include raw transaction data
        let txs = wallet
            .list_transactions(true)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to list transactions: {}", e),
            })?;

        let tx = txs.iter().find(|t| t.txid == target_txid).ok_or_else(|| {
            AccountInfoError::TransactionNotFound {
                error_details: format!("Transaction {} not found in wallet", target_txid),
            }
        })?;

        // Summary fields
        let (direction, amount, net) = classify_tx(tx.sent, tx.received, tx.fee);

        let (block_height, timestamp, confirmations) = match tx.confirmation_time.as_ref() {
            Some(conf) => {
                let confs = tip_height.saturating_sub(conf.height) + 1;
                (Some(conf.height), Some(conf.timestamp), confs)
            }
            None => (None, None, 0),
        };

        // Raw transaction details
        let raw_tx = tx
            .transaction
            .as_ref()
            .ok_or_else(|| AccountInfoError::WalletError {
                error_details: format!("Raw transaction data not available for {}", target_txid),
            })?;

        let inputs: Vec<TxDetailInput> = raw_tx
            .input
            .iter()
            .map(|inp| TxDetailInput {
                txid: inp.previous_output.txid.to_string(),
                vout: inp.previous_output.vout,
                sequence: inp.sequence.0,
                script_sig: hex::encode(inp.script_sig.as_bytes()),
                witness: inp.witness.iter().map(|w| hex::encode(w)).collect(),
            })
            .collect();

        let outputs: Vec<TxDetailOutput> = raw_tx
            .output
            .iter()
            .map(|out| {
                let address = BdkAddress::from_script(&out.script_pubkey, wallet_network)
                    .ok()
                    .map(|a| a.to_string());
                let is_mine = wallet.is_mine(&out.script_pubkey).map_err(|e| {
                    AccountInfoError::WalletError {
                        error_details: format!("Failed to check script ownership: {}", e),
                    }
                })?;
                Ok(TxDetailOutput {
                    value: out.value,
                    script_pubkey: hex::encode(out.script_pubkey.as_bytes()),
                    address,
                    is_mine,
                })
            })
            .collect::<Result<Vec<_>, AccountInfoError>>()?;

        let size = u32::try_from(raw_tx.size()).unwrap_or(u32::MAX);
        let vsize = u32::try_from(raw_tx.vsize()).unwrap_or(u32::MAX);
        let weight = u32::try_from(raw_tx.weight().to_wu()).unwrap_or(u32::MAX);
        let fee_rate = match tx.fee {
            Some(f) if vsize > 0 => Some(f as f64 / vsize as f64),
            _ => None,
        };

        Ok(TransactionDetail {
            txid: tx.txid.to_string(),
            received: tx.received,
            sent: tx.sent,
            net,
            amount,
            fee: tx.fee,
            direction,
            block_height,
            timestamp,
            confirmations,
            inputs,
            outputs,
            size,
            vsize,
            weight,
            fee_rate,
        })
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    Ok(result)
}

/// Query balance and UTXOs for a single Bitcoin address via Electrum.
///
/// When `network` is `None`, the address is accepted without network validation
/// (`assume_checked`). In that case, if the Electrum server is on a different
/// network the query will silently return empty or incorrect results.
pub async fn get_address_info(
    address: &str,
    electrum_url: &str,
    network: Option<OnchainNetwork>,
) -> Result<SingleAddressInfoResult, AccountInfoError> {
    // Parse with BDK's bitcoin crate for script_pubkey generation
    let bdk_addr = BdkAddress::from_str(address).map_err(|e| AccountInfoError::InvalidAddress {
        error_details: format!("Invalid address: {}", e),
    })?;
    let bdk_addr = match network {
        Some(net) => {
            let bdk_network = onchain_to_bdk_network(net);
            bdk_addr.require_network(bdk_network).map_err(|e| {
                AccountInfoError::NetworkMismatch {
                    error_details: format!("Address network mismatch: {}", e),
                }
            })?
        }
        None => bdk_addr.assume_checked(),
    };

    let electrum_url_owned = electrum_url.to_string();
    let addr_str = address.to_string();

    let result =
        tokio::task::spawn_blocking(move || {
            let (client, tip_height) = connect_and_get_tip(&electrum_url_owned)?;

            let script = bdk_addr.script_pubkey();

            // Get UTXOs for this address
            let utxos = client.script_list_unspent(&script).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to list UTXOs: {}", e),
                }
            })?;

            // Get history for transfer count
            let history = client.script_get_history(&script).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to get history: {}", e),
                }
            })?;

            let account_utxos: Vec<AccountUtxo> = utxos
                .iter()
                .map(|utxo| {
                    let height = u32::try_from(utxo.height).unwrap_or(0);
                    let confirmations = if height > 0 {
                        tip_height.saturating_sub(height) + 1
                    } else {
                        0
                    };

                    let vout =
                        u32::try_from(utxo.tx_pos).map_err(|_| AccountInfoError::WalletError {
                            error_details: format!("Output index {} exceeds u32", utxo.tx_pos),
                        })?;

                    Ok(AccountUtxo {
                        txid: utxo.tx_hash.to_string(),
                        vout,
                        amount: utxo.value,
                        block_height: height,
                        address: addr_str.clone(),
                        path: String::new(), // No derivation path for single address
                        confirmations,
                        coinbase: false,
                        own: true,
                        required: None,
                    })
                })
                .collect::<Result<Vec<_>, AccountInfoError>>()?;

            let balance: u64 = utxos.iter().map(|u| u.value).sum();

            Ok::<_, AccountInfoError>(SingleAddressInfoResult {
                address: addr_str,
                balance,
                utxos: account_utxos,
                transfers: u32::try_from(history.len()).unwrap_or(u32::MAX),
                block_height: tip_height,
            })
        })
        .await
        .map_err(|e| AccountInfoError::SyncError {
            error_details: format!("Task failed: {}", e),
        })??;

    Ok(result)
}

fn parse_address(address: &str) -> Result<Address<NetworkUnchecked>, AddressError> {
    Address::from_str(address)
        .map_err(|e| {
            println!("✗ Failed to parse address: {:?}", e);
            AddressError::InvalidAddress
        })
        .map(|addr| {
            println!("✓ Successfully parsed address");
            addr
        })
}

fn determine_network(address: &str) -> Result<Network, AddressError> {
    match address {
        s if s.starts_with("1") || s.starts_with("3") || s.starts_with("bc1") => {
            println!("✓ Determined network: Bitcoin");
            Ok(Network::Bitcoin)
        }
        s if s.starts_with("2")
            || s.starts_with("tb1")
            || s.starts_with("m")
            || s.starts_with("n") =>
        {
            println!("✓ Determined network: Testnet");
            Ok(Network::Testnet)
        }
        s if s.starts_with("bcrt1") => {
            println!("✓ Determined network: Regtest");
            Ok(Network::Regtest)
        }
        _ => {
            println!("✗ Could not determine network");
            Err(AddressError::InvalidNetwork)
        }
    }
}

fn verify_network(
    unchecked_addr: Address<NetworkUnchecked>,
    expected_network: Network,
) -> Result<Address, AddressError> {
    println!(
        "Attempting to verify address for network: {:?}",
        expected_network
    );
    unchecked_addr
        .require_network(expected_network)
        .map_err(|e| {
            println!("✗ Network verification failed: {:?}", e);
            AddressError::InvalidNetwork
        })
        .map(|addr| {
            println!("✓ Address verified for network");
            addr
        })
}

fn get_address_type(address: &str) -> Result<AddressType, AddressError> {
    let address_type = match address {
        // Legacy addresses (P2PKH)
        s if s.starts_with("1") || s.starts_with("m") || s.starts_with("n") => {
            Some(AddressType::P2PKH)
        }
        // SegWit addresses (P2SH)
        s if s.starts_with("3") || s.starts_with("2") => Some(AddressType::P2SH),
        // Taproot addresses (P2TR)
        s if s.starts_with("bc1p") || s.starts_with("tb1p") => Some(AddressType::P2TR),
        // Native SegWit addresses (P2WPKH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 42 => {
            Some(AddressType::P2WPKH)
        }
        // Native SegWit Script addresses (P2WSH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 62 => {
            Some(AddressType::P2WSH)
        }
        // Regtest addresses
        s if s.starts_with("bcrt1") => {
            if s.len() == 42 {
                Some(AddressType::P2WPKH)
            } else if s.len() == 62 {
                Some(AddressType::P2WSH)
            } else {
                Some(AddressType::Unknown)
            }
        }
        _ => Some(AddressType::Unknown),
    };

    address_type
        .map(|t| {
            println!("✓ Determined address type: {:?}", t);
            t
        })
        .ok_or_else(|| {
            println!("✗ Could not determine address type");
            AddressError::InvalidAddress
        })
}
