use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::str::FromStr;

use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::absolute::LockTime;
use bdk::bitcoin::bip32::{
    DerivationPath as BdkDerivationPath, ExtendedPrivKey as BdkExtendedPrivKey, ExtendedPubKey,
};
use bdk::bitcoin::consensus::{deserialize, serialize};
use bdk::bitcoin::psbt::PartiallySignedTransaction as Psbt;
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::{
    Address as BdkAddress, Network as BdkNetwork, OutPoint, PrivateKey as BdkPrivateKey,
    PublicKey as BdkPublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness,
};
use bdk::blockchain::ElectrumBlockchain;
use bdk::database::MemoryDatabase;
use bdk::electrum_client::ElectrumApi;
use bdk::keys::bip39::Mnemonic as BdkMnemonic;
use bdk::template::{Bip44, Bip49, Bip86, P2Wpkh};
use bdk::wallet::signer::SignOptions;
use bdk::wallet::{AddressIndex as BdkAddressIndex, SyncOptions, Wallet};
use bdk::KeychainKind;
use bip39::Mnemonic as Bip39Mnemonic;
use bitcoin::address::{Address, NetworkUnchecked};
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::{Network, NetworkKind};
use bitcoin_address_generator;

use super::errors::AccountInfoError;
use super::types::{
    classify_tx, AccountAddresses, AccountInfoResult, AccountType, AccountUtxo, AddressInfo,
    AddressType, ComposeAccount, HistoryTransaction, LegacyRnCloseRecoveryScanResult,
    LegacyRnCloseRecoverySweepPreview, Network as OnchainNetwork, SingleAddressInfoResult,
    TransactionDetail, TransactionHistoryResult, TxDetailInput, TxDetailOutput, TxDirection,
    ValidationResult, WalletBalance, DEFAULT_GAP_LIMIT,
};
use crate::modules::activity::{
    Activity, OnchainActivity, PaymentType, TransactionDetails, TxInput, TxOutput,
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
            Ok(mnemonic) => Ok(mnemonic),
            Err(e) => {
                log::error!("Failed to generate mnemonic: {:?}", e);
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
            log::error!("Failed to derive address: {:?}", e);
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
            log::error!("Failed to derive addresses: {:?}", e);
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
            log::error!("Failed to derive private key: {:?}", e);
            AddressError::AddressDerivationFailed
        })?;

        Ok(private_key)
    }

    pub fn derive_onchain_descriptor(
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
        account_type: AccountType,
        account_index: u32,
    ) -> Result<String, AddressError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let derivation_path = derive_base_path(account_type, bdk_network, account_index);

        let mnemonic =
            Bip39Mnemonic::parse(mnemonic_phrase).map_err(|_| AddressError::InvalidMnemonic)?;
        let seed = mnemonic.to_seed(bip39_passphrase.unwrap_or(""));
        let path = DerivationPath::from_str(&derivation_path)
            .map_err(|_| AddressError::AddressDerivationFailed)?;

        let secp = bitcoin::secp256k1::Secp256k1::new();
        let root =
            Xpriv::new_master(network, &seed).map_err(|_| AddressError::AddressDerivationFailed)?;
        let account = root
            .derive_priv(&secp, &path)
            .map_err(|_| AddressError::AddressDerivationFailed)?;

        let master_fingerprint = root.fingerprint(&secp).to_string();
        let mut account_xpub = Xpub::from_priv(&secp, &account);
        // Export standard xpub descriptors; the key origin path still carries
        // the selected network's coin type.
        account_xpub.network = NetworkKind::Main;
        let account_xpub = account_xpub.to_string();
        let key_origin_path = derivation_path
            .strip_prefix("m/")
            .unwrap_or(&derivation_path);
        let (external_descriptor, _) = build_descriptors(
            &account_xpub,
            account_type,
            Some((&master_fingerprint, key_origin_path)),
        );

        Ok(external_descriptor)
    }

    fn create_sweep_wallets(
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
    ) -> Result<SweepWallets, SweepError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let mnemonic =
            BdkMnemonic::from_str(mnemonic_phrase).map_err(|_| SweepError::InvalidMnemonic)?;
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
}

// ------------------------------------------------------------------------
// Legacy RN P2WPKH-from-legacy-or-nested-key close recovery
// ------------------------------------------------------------------------

// One-time recovery path for legacy React Native channel close funds that were
// paid to P2WPKH scripts derived from legacy or nested-SegWit selected keys.
pub(super) struct LegacyRnNativeSegwitRecoverySpendable {
    pub(super) derivation_path: String,
    pub(super) txid: String,
    pub(super) vout: u32,
    pub(super) output: TxOut,
}

impl BitcoinAddressValidator {
    pub(super) fn legacy_rn_p2wpkh_from_selected_purpose_script_map(
        mnemonic_phrase: &str,
        index_limit: u32,
        network: Network,
        bip39_passphrase: Option<&str>,
    ) -> Result<HashMap<Vec<u8>, String>, SweepError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let mnemonic =
            Bip39Mnemonic::from_str(mnemonic_phrase).map_err(|_| SweepError::InvalidMnemonic)?;
        let seed = mnemonic.to_seed(bip39_passphrase.unwrap_or(""));
        let secp = Secp256k1::new();
        let master = BdkExtendedPrivKey::new_master(bdk_network, &seed).map_err(|e| {
            SweepError::SweepFailed(format!("Failed to derive legacy RN master key: {}", e))
        })?;
        let mut scripts = HashMap::new();
        let coin_type = if network == Network::Bitcoin { 0 } else { 1 };

        for purpose in [44, 49] {
            for index in 0..index_limit {
                for chain in 0..=1 {
                    let derivation_path =
                        format!("m/{}'/{}'/0'/{}/{}", purpose, coin_type, chain, index);
                    let child_path =
                        BdkDerivationPath::from_str(&derivation_path).map_err(|e| {
                            SweepError::SweepFailed(format!(
                                "Invalid legacy RN derivation path {}: {}",
                                derivation_path, e
                            ))
                        })?;
                    let child = master.derive_priv(&secp, &child_path).map_err(|e| {
                        SweepError::SweepFailed(format!(
                            "Failed to derive legacy RN private key {}: {}",
                            derivation_path, e
                        ))
                    })?;
                    let public_key =
                        BdkPublicKey::new(bdk::bitcoin::secp256k1::PublicKey::from_secret_key(
                            &secp,
                            &child.private_key,
                        ));
                    let script =
                        ScriptBuf::new_v0_p2wpkh(&public_key.wpubkey_hash().ok_or_else(|| {
                            SweepError::SweepFailed(format!(
                                "Legacy RN public key {} is not compressed",
                                derivation_path
                            ))
                        })?);
                    scripts.insert(script.to_bytes(), derivation_path);
                }
            }
        }

        Ok(scripts)
    }

    fn legacy_rn_native_segwit_recovery_spendables(
        mnemonic_phrase: &str,
        network: Network,
        electrum_client: &bdk::electrum_client::Client,
        index_limit: u32,
        bip39_passphrase: Option<&str>,
    ) -> Result<Vec<LegacyRnNativeSegwitRecoverySpendable>, SweepError> {
        let scripts = Self::legacy_rn_p2wpkh_from_selected_purpose_script_map(
            mnemonic_phrase,
            index_limit,
            network,
            bip39_passphrase,
        )?;
        let mut spendables = Vec::new();
        let mut seen_outpoints = HashSet::new();

        let script_entries = scripts.into_iter().collect::<Vec<_>>();
        for chunk in script_entries.chunks(100) {
            let electrum_scripts = chunk
                .iter()
                .map(|(script_pubkey, _)| ScriptBuf::from_bytes(script_pubkey.clone()))
                .collect::<Vec<_>>();
            let electrum_script_refs = electrum_scripts
                .iter()
                .map(|script| script.as_script())
                .collect::<Vec<_>>();
            let unspent_batches = electrum_client
                .batch_script_list_unspent(electrum_script_refs)
                .map_err(|e| {
                    SweepError::SweepFailed(format!(
                        "Failed to scan legacy RN recovery addresses: {}",
                        e
                    ))
                })?;

            for ((script_pubkey, derivation_path), unspent_outputs) in
                chunk.iter().zip(unspent_batches.into_iter())
            {
                for utxo in unspent_outputs {
                    let vout_u32 = u32::try_from(utxo.tx_pos).map_err(|_| {
                        SweepError::SweepFailed(format!(
                            "Legacy RN recovery output index {} is invalid",
                            utxo.tx_pos
                        ))
                    })?;
                    let outpoint_key = format!("{}:{}", utxo.tx_hash, utxo.tx_pos);

                    if !seen_outpoints.insert(outpoint_key) {
                        continue;
                    }

                    spendables.push(LegacyRnNativeSegwitRecoverySpendable {
                        derivation_path: derivation_path.clone(),
                        txid: utxo.tx_hash.to_string(),
                        vout: vout_u32,
                        output: TxOut {
                            value: utxo.value,
                            script_pubkey: ScriptBuf::from_bytes(script_pubkey.clone()),
                        },
                    });
                }
            }
        }

        Ok(spendables)
    }

    fn sign_legacy_rn_native_segwit_recovery_psbt(
        psbt: &mut Psbt,
        spendables: &[LegacyRnNativeSegwitRecoverySpendable],
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
    ) -> Result<(), SweepError> {
        let bdk_network = onchain_to_bdk_network(network.into());
        let mnemonic =
            Bip39Mnemonic::from_str(mnemonic_phrase).map_err(|_| SweepError::InvalidMnemonic)?;
        let seed = mnemonic.to_seed(bip39_passphrase.unwrap_or(""));
        let secp = Secp256k1::new();
        let master = BdkExtendedPrivKey::new_master(bdk_network, &seed).map_err(|e| {
            SweepError::SweepFailed(format!("Failed to derive legacy RN master key: {}", e))
        })?;

        let sign_options = SignOptions {
            trust_witness_utxo: true,
            allow_all_sighashes: true,
            ..Default::default()
        };

        for item in spendables {
            let derivation_path =
                BdkDerivationPath::from_str(&item.derivation_path).map_err(|e| {
                    SweepError::SweepFailed(format!(
                        "Invalid legacy RN derivation path {}: {}",
                        item.derivation_path, e
                    ))
                })?;
            let child = master.derive_priv(&secp, &derivation_path).map_err(|e| {
                SweepError::SweepFailed(format!(
                    "Failed to derive legacy RN private key {}: {}",
                    item.derivation_path, e
                ))
            })?;
            let public_key = BdkPublicKey::new(
                bdk::bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &child.private_key),
            );
            let expected_script =
                ScriptBuf::new_v0_p2wpkh(&public_key.wpubkey_hash().ok_or_else(|| {
                    SweepError::SweepFailed(format!(
                        "Legacy RN public key {} is not compressed",
                        item.derivation_path
                    ))
                })?);
            if expected_script != item.output.script_pubkey {
                return Err(SweepError::SweepFailed(format!(
                    "Derived script for {} does not match recovery output {}:{}",
                    item.derivation_path, item.txid, item.vout
                )));
            }

            let wallet = Wallet::new(
                P2Wpkh(BdkPrivateKey::new(child.private_key, bdk_network)),
                None,
                bdk_network,
                MemoryDatabase::new(),
            )
            .map_err(|e| {
                SweepError::SweepFailed(format!(
                    "Failed to create recovery signer for {}: {}",
                    item.derivation_path, e
                ))
            })?;
            wallet.ensure_addresses_cached(1).map_err(|e| {
                SweepError::SweepFailed(format!(
                    "Failed to cache recovery signer address {}: {}",
                    item.derivation_path, e
                ))
            })?;
            wallet.sign(psbt, sign_options.clone()).map_err(|e| {
                SweepError::SweepFailed(format!(
                    "Failed to sign recovery output {}:{}: {}",
                    item.txid, item.vout, e
                ))
            })?;
        }

        for input in &psbt.inputs {
            if input.final_script_sig.is_none() && input.final_script_witness.is_none() {
                return Err(SweepError::SweepFailed(
                    "Recovery transaction signing incomplete - some inputs not finalized"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    pub(super) fn build_legacy_rn_native_segwit_recovery_sweep_tx(
        mnemonic_phrase: &str,
        spendables: &[LegacyRnNativeSegwitRecoverySpendable],
        network: Network,
        destination_address: &str,
        fee_rate_sats_per_vbyte: Option<u32>,
        bip39_passphrase: Option<&str>,
    ) -> Result<LegacyRnCloseRecoverySweepPreview, SweepError> {
        if spendables.is_empty() {
            return Err(SweepError::NoUtxosFound);
        }

        let bdk_network = onchain_to_bdk_network(network.into());
        let dest_addr = BdkAddress::from_str(destination_address)
            .map_err(|e| SweepError::SweepFailed(format!("Invalid destination address: {}", e)))?
            .require_network(bdk_network)
            .map_err(|e| {
                SweepError::SweepFailed(format!("Network mismatch for destination address: {}", e))
            })?;
        let total_amount = spendables.iter().map(|item| item.output.value).sum::<u64>();
        let fee_rate_sats = fee_rate_sats_per_vbyte.unwrap_or(1) as u64;

        let build_psbt = |output_value: u64| -> Result<Psbt, SweepError> {
            let inputs = spendables
                .iter()
                .map(|item| {
                    let txid = Txid::from_str(&item.txid).map_err(|e| {
                        SweepError::SweepFailed(format!(
                            "Invalid legacy RN recovery txid {}: {}",
                            item.txid, e
                        ))
                    })?;
                    Ok(TxIn {
                        previous_output: OutPoint {
                            txid,
                            vout: item.vout,
                        },
                        script_sig: ScriptBuf::new(),
                        sequence: Sequence::MAX,
                        witness: Witness::new(),
                    })
                })
                .collect::<Result<Vec<_>, SweepError>>()?;

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
            for (input, item) in psbt.inputs.iter_mut().zip(spendables.iter()) {
                input.witness_utxo = Some(item.output.clone());
            }
            Ok(psbt)
        };

        let mut probe_psbt = build_psbt(total_amount)?;
        Self::sign_legacy_rn_native_segwit_recovery_psbt(
            &mut probe_psbt,
            spendables,
            mnemonic_phrase,
            network,
            bip39_passphrase,
        )?;
        let estimated_vsize = probe_psbt.extract_tx().weight().to_vbytes_ceil();
        let estimated_fee = estimated_vsize.saturating_mul(fee_rate_sats);

        if estimated_fee >= total_amount {
            return Err(SweepError::SweepFailed(format!(
                "Recovery amount {} sats is too small to sweep at {} sat/vB",
                total_amount, fee_rate_sats
            )));
        }

        let amount_after_fees = total_amount - estimated_fee;
        let mut final_psbt = build_psbt(amount_after_fees)?;
        Self::sign_legacy_rn_native_segwit_recovery_psbt(
            &mut final_psbt,
            spendables,
            mnemonic_phrase,
            network,
            bip39_passphrase,
        )?;
        let tx = final_psbt.extract_tx();

        Ok(LegacyRnCloseRecoverySweepPreview {
            tx_hex: hex::encode(serialize(&tx)),
            txid: tx.txid().to_string(),
            total_amount,
            estimated_fee,
            estimated_vsize,
            outputs_count: u32::try_from(spendables.len()).unwrap_or(u32::MAX),
            destination_address: destination_address.to_string(),
            amount_after_fees,
        })
    }

    pub async fn scan_legacy_rn_native_segwit_recovery_funds(
        mnemonic_phrase: &str,
        network: Network,
        electrum_url: &str,
        index_limit: u32,
        bip39_passphrase: Option<&str>,
    ) -> Result<LegacyRnCloseRecoveryScanResult, SweepError> {
        let mnemonic_phrase = mnemonic_phrase.to_string();
        let electrum_url = electrum_url.to_string();
        let bip39_passphrase = bip39_passphrase.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let electrum_client = Self::create_electrum_client(&electrum_url)?;
            let spendables = Self::legacy_rn_native_segwit_recovery_spendables(
                &mnemonic_phrase,
                network,
                &electrum_client,
                index_limit,
                bip39_passphrase.as_deref(),
            )?;
            Ok::<_, SweepError>(LegacyRnCloseRecoveryScanResult {
                total_amount: spendables.iter().map(|item| item.output.value).sum(),
                outputs_count: u32::try_from(spendables.len()).unwrap_or(u32::MAX),
            })
        })
        .await
        .map_err(|e| {
            SweepError::SweepFailed(format!("Legacy RN recovery scan task failed: {}", e))
        })?
    }

    pub async fn prepare_legacy_rn_native_segwit_recovery_sweep(
        mnemonic_phrase: &str,
        network: Network,
        electrum_url: &str,
        destination_address: &str,
        fee_rate_sats_per_vbyte: Option<u32>,
        index_limit: u32,
        bip39_passphrase: Option<&str>,
    ) -> Result<LegacyRnCloseRecoverySweepPreview, SweepError> {
        let mnemonic_phrase = mnemonic_phrase.to_string();
        let electrum_url = electrum_url.to_string();
        let destination_address = destination_address.to_string();
        let bip39_passphrase = bip39_passphrase.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let electrum_client = Self::create_electrum_client(&electrum_url)?;
            let spendables = Self::legacy_rn_native_segwit_recovery_spendables(
                &mnemonic_phrase,
                network,
                &electrum_client,
                index_limit,
                bip39_passphrase.as_deref(),
            )?;
            Self::build_legacy_rn_native_segwit_recovery_sweep_tx(
                &mnemonic_phrase,
                &spendables,
                network,
                &destination_address,
                fee_rate_sats_per_vbyte,
                bip39_passphrase.as_deref(),
            )
        })
        .await
        .map_err(|e| {
            SweepError::SweepFailed(format!("Legacy RN recovery sweep task failed: {}", e))
        })?
    }

    // ------------------------------------------------------------------------
    // Standard wallet sweep
    // ------------------------------------------------------------------------

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

/// Returns true when an Electrum broadcast error indicates the transaction was
/// already accepted (already in a block, in the mempool, or otherwise known)
/// rather than a real failure, meaning a retry of a previously successful
/// broadcast.
///
/// Matching is case-insensitive. It must NOT classify relay-policy or
/// invalid-transaction rejections (e.g. `bad-txns-inputs-missingorspent`,
/// `min relay fee not met`) as already-known.
pub(crate) fn is_already_known_broadcast_error(message: &str) -> bool {
    const ALREADY_KNOWN_MARKERS: [&str; 7] = [
        "already in block chain",
        "already in blockchain",
        "already in mempool",
        "already-in-block-chain",
        "already-in-mempool",
        "txn-already-known",
        "transaction already exists",
    ];
    let lowered = message.to_lowercase();
    ALREADY_KNOWN_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// Decode a hex-encoded transaction, validate that it deserializes, and compute
/// its canonical txid. Returns the raw bytes (for broadcast) alongside the txid.
///
/// Note: within this module the transaction is a `bdk::bitcoin::Transaction`
/// (bitcoin 0.30), whose canonical-txid method is `.txid()` (the equivalent of
/// `compute_txid()` in newer bitcoin releases). It returns the txid, never the
/// witness txid (wtxid).
pub(crate) fn decode_and_compute_txid(
    serialized_tx: &str,
) -> Result<(Vec<u8>, Txid), BroadcastError> {
    let tx_bytes = hex::decode(serialized_tx).map_err(|e| BroadcastError::InvalidHex {
        error_details: format!("Invalid transaction hex: {}", e),
    })?;

    let tx: Transaction =
        deserialize(&tx_bytes).map_err(|e| BroadcastError::InvalidTransaction {
            error_details: format!("Invalid transaction data: {}", e),
        })?;

    let txid = tx.txid();
    Ok((tx_bytes, txid))
}

/// Broadcast a signed raw transaction via Electrum.
///
/// Takes a hex-encoded serialized transaction and an Electrum server URL.
/// Returns the transaction's canonical txid (computed locally) on success.
///
/// If Electrum reports that the transaction is already known (already in a block,
/// in the mempool, or otherwise accepted), this is treated as success and the same
/// locally computed txid is returned, so retrying a broadcast after an ambiguous
/// network failure completes cleanly. Genuine connectivity failures and unrelated
/// broadcast rejections are preserved as typed errors.
pub async fn broadcast_raw_tx(
    serialized_tx: String,
    electrum_url: &str,
) -> Result<String, BroadcastError> {
    let (tx_bytes, local_txid) = decode_and_compute_txid(&serialized_tx)?;
    let electrum_url_owned = electrum_url.to_string();

    tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            BroadcastError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        match client.transaction_broadcast_raw(&tx_bytes) {
            Ok(_) => Ok(()),
            Err(e) => {
                let message = e.to_string();
                if is_already_known_broadcast_error(&message) {
                    Ok(())
                } else {
                    Err(BroadcastError::ElectrumError {
                        error_details: format!("Broadcast failed: {}", message),
                    })
                }
            }
        }
    })
    .await
    .map_err(|e| BroadcastError::TaskError {
        error_details: format!("Broadcast task failed: {}", e),
    })??;

    Ok(local_txid.to_string())
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

fn panic_payload_to_string(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

pub(super) async fn run_account_info_blocking<T, F>(
    task_name: &'static str,
    task: F,
) -> Result<T, AccountInfoError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AccountInfoError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        catch_unwind(AssertUnwindSafe(task)).map_err(|payload| AccountInfoError::SyncError {
            error_details: format!(
                "{} task panicked: {}",
                task_name,
                panic_payload_to_string(payload.as_ref())
            ),
        })?
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("{} task failed: {}", task_name, e),
    })?
}

/// Create a BDK wallet (in-memory, unsynced) from the resolved setup.
///
/// Addresses can be derived from the returned wallet without syncing; call
/// [`sync_wallet`] to populate balances and transaction history.
pub(crate) fn create_wallet(
    setup: &WalletSetup,
) -> Result<Wallet<MemoryDatabase>, AccountInfoError> {
    Wallet::new(
        &setup.external_desc,
        Some(&setup.internal_desc),
        setup.network,
        MemoryDatabase::new(),
    )
    .map_err(|e| AccountInfoError::WalletError {
        error_details: format!("Failed to create wallet: {}", e),
    })
}

/// Sync a wallet against an existing `ElectrumBlockchain`.
///
/// Unlike [`create_and_sync_wallet`], the blockchain is borrowed (not consumed),
/// so a single connection can be reused across repeated syncs.
pub(crate) fn sync_wallet(
    wallet: &Wallet<MemoryDatabase>,
    blockchain: &ElectrumBlockchain,
) -> Result<(), AccountInfoError> {
    wallet
        .sync(blockchain, SyncOptions::default())
        .map_err(|e| AccountInfoError::SyncError {
            error_details: format!("Failed to sync wallet: {}", e),
        })
}

/// Create a BDK wallet and sync it via the provided Electrum client.
///
/// The client is consumed; make any pre-sync calls (e.g. `block_headers_subscribe`)
/// before passing it here.
pub(crate) fn create_and_sync_wallet(
    setup: &WalletSetup,
    client: bdk::electrum_client::Client,
) -> Result<Wallet<MemoryDatabase>, AccountInfoError> {
    let wallet = create_wallet(setup)?;
    let blockchain = ElectrumBlockchain::from(client);
    sync_wallet(&wallet, &blockchain)?;
    Ok(wallet)
}

// ============================================================================
// Shared tx-mapping helpers
// ============================================================================

/// Map a single BDK TransactionDetails to a HistoryTransaction.
pub(crate) fn map_bdk_tx_to_history(
    tx: &bdk::TransactionDetails,
    tip_height: u32,
) -> HistoryTransaction {
    let (direction, amount, net) = classify_tx(tx.sent, tx.received, tx.fee);

    let (block_height, timestamp, confirmations) = match tx.confirmation_time.as_ref() {
        Some(conf) => {
            let confs = tip_height.saturating_sub(conf.height) + 1;
            (Some(conf.height), Some(conf.timestamp), confs)
        }
        None => (None, None, 0),
    };

    // Fee rate (sat/vB) requires both the fee and the raw transaction (for vsize).
    let fee_rate = match (tx.fee, tx.transaction.as_ref()) {
        (Some(fee), Some(raw_tx)) => {
            let vsize = raw_tx.vsize();
            if vsize > 0 {
                Some(fee as f64 / vsize as f64)
            } else {
                None
            }
        }
        _ => None,
    };

    HistoryTransaction {
        txid: tx.txid.to_string(),
        received: tx.received,
        sent: tx.sent,
        net,
        fee: tx.fee,
        fee_rate,
        amount,
        direction,
        block_height,
        timestamp,
        confirmations,
    }
}

/// Sort history transactions: unconfirmed first, then by timestamp descending.
pub(crate) fn sort_history_transactions(history: &mut [HistoryTransaction]) {
    history.sort_by(|a, b| match (a.timestamp, b.timestamp) {
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
        (Some(a_ts), Some(b_ts)) => b_ts.cmp(&a_ts),
    });
}

/// Decode a single BDK transaction into a rich [`TransactionDetail`] (addresses,
/// per-output ownership, fee rate, vsize). Requires `tx.transaction` to be present,
/// i.e. the tx must come from `list_transactions(true)`.
pub(crate) fn map_bdk_tx_to_detail(
    wallet: &Wallet<MemoryDatabase>,
    tx: &bdk::TransactionDetails,
    tip_height: u32,
    wallet_network: BdkNetwork,
) -> Result<TransactionDetail, AccountInfoError> {
    let (direction, amount, net) = classify_tx(tx.sent, tx.received, tx.fee);

    let (block_height, timestamp, confirmations) = match tx.confirmation_time.as_ref() {
        Some(conf) => {
            let confs = tip_height.saturating_sub(conf.height) + 1;
            (Some(conf.height), Some(conf.timestamp), confs)
        }
        None => (None, None, 0),
    };

    let raw_tx = tx
        .transaction
        .as_ref()
        .ok_or_else(|| AccountInfoError::WalletError {
            error_details: format!("Raw transaction data not available for {}", tx.txid),
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
            let is_mine =
                wallet
                    .is_mine(&out.script_pubkey)
                    .map_err(|e| AccountInfoError::WalletError {
                        error_details: format!("Failed to check script ownership: {}", e),
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
}

/// Convert a decoded [`TransactionDetail`] into persistence-ready Core activity
/// data scoped to `wallet_id`, from the *watched wallet's* perspective.
///
/// Semantics chosen so these render identically to any other onchain activity:
/// - `Received`: `value` = amount received, `fee`/`fee_rate` = 0 (the fee was paid
///   by the sender, not this wallet — even when BDK attaches one to the row).
/// - `Sent`: `value` = amount that left the wallet (sent − received − fee),
///   `fee`/`fee_rate` = the paid fee. Bitkit shows `value + fee` as the total.
/// - `SelfTransfer`: `value` = 0, `fee`/`fee_rate` = the paid fee (total = fee).
/// - `address`: the owned output for receives, the destination (non-owned) output
///   for sends, falling back to any decodable output address.
/// - `timestamp`: block time when confirmed, else `now` (always > 0 so the row is
///   DB-valid and sorts to the top while pending).
pub(crate) fn watch_only_activity_from_detail(
    wallet_id: &str,
    detail: &TransactionDetail,
    now: u64,
) -> (Activity, TransactionDetails) {
    // Did the watched wallet actually spend inputs? If not, the fee isn't ours.
    let spent = detail.sent > 0;
    let fee = if spent { detail.fee.unwrap_or(0) } else { 0 };
    let fee_rate = if spent {
        detail.fee_rate.map(|r| r.round() as u64).unwrap_or(0)
    } else {
        0
    };

    let (tx_type, value) = match detail.direction {
        TxDirection::Received => (PaymentType::Received, detail.received),
        TxDirection::Sent => (
            PaymentType::Sent,
            detail
                .sent
                .saturating_sub(detail.received)
                .saturating_sub(fee),
        ),
        // Pure self-transfer: nothing leaves the wallet but the fee.
        TxDirection::SelfTransfer => (PaymentType::Sent, 0),
    };

    // Pick the most meaningful address for this direction.
    let pick_address = || -> String {
        let owned = detail
            .outputs
            .iter()
            .find(|o| o.is_mine)
            .and_then(|o| o.address.clone());
        let external = detail
            .outputs
            .iter()
            .find(|o| !o.is_mine)
            .and_then(|o| o.address.clone());
        let any = detail.outputs.iter().find_map(|o| o.address.clone());
        let chosen = match detail.direction {
            TxDirection::Received => owned.or(any),
            TxDirection::Sent => external.or(any),
            TxDirection::SelfTransfer => owned.or(any),
        };
        chosen.unwrap_or_default()
    };

    let confirmed = detail.confirmations > 0;
    let timestamp = detail.timestamp.unwrap_or(now);

    let activity = Activity::Onchain(OnchainActivity {
        wallet_id: wallet_id.to_string(),
        id: detail.txid.clone(),
        tx_type,
        tx_id: detail.txid.clone(),
        value,
        fee,
        fee_rate,
        address: pick_address(),
        confirmed,
        timestamp,
        is_boosted: false,
        boost_tx_ids: Vec::new(),
        is_transfer: false,
        does_exist: true,
        confirm_timestamp: if confirmed { detail.timestamp } else { None },
        channel_id: None,
        transfer_tx_id: None,
        contact: None,
        created_at: None,
        updated_at: None,
        seen_at: None,
    });

    let details = TransactionDetails {
        wallet_id: wallet_id.to_string(),
        tx_id: detail.txid.clone(),
        // `amount_sats` is documented as fee-excluded. BDK's `sent` includes the
        // fee, so `net` (received - sent) is fee-inclusive for spends; add our fee
        // back to exclude it. For receive-only rows `fee` is 0 (sender paid it).
        amount_sats: detail.net + fee as i64,
        inputs: detail
            .inputs
            .iter()
            .map(|i| TxInput {
                txid: i.txid.clone(),
                vout: i.vout,
                scriptsig: i.script_sig.clone(),
                witness: i.witness.clone(),
                sequence: i.sequence,
            })
            .collect(),
        outputs: detail
            .outputs
            .iter()
            .enumerate()
            .map(|(n, o)| TxOutput {
                scriptpubkey: o.script_pubkey.clone(),
                scriptpubkey_type: None,
                scriptpubkey_address: o.address.clone(),
                value: o.value as i64,
                n: n as u32,
            })
            .collect(),
    };

    (activity, details)
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
    let gap = gap_limit.unwrap_or(DEFAULT_GAP_LIMIT);
    let base_path = setup.base_path.clone();
    let account_type = setup.account_type;

    let electrum_url_owned = electrum_url.to_string();

    let result = run_account_info_blocking("account info", move || {
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
    .await?;

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

    let result = run_account_info_blocking("transaction history", move || {
        let (client, tip_height) = connect_and_get_tip(&electrum_url)?;

        let wallet = create_and_sync_wallet(&setup, client)?;

        // Balance
        let bdk_balance = wallet
            .get_balance()
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get balance: {}", e),
            })?;
        let balance: WalletBalance = bdk_balance.into();

        // Transaction history. `true` includes the raw transaction so
        // map_bdk_tx_to_history can derive fee_rate from the tx vsize.
        let txs = wallet
            .list_transactions(true)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to list transactions: {}", e),
            })?;

        let mut history: Vec<HistoryTransaction> = txs
            .iter()
            .map(|tx| map_bdk_tx_to_history(tx, tip_height))
            .collect();

        sort_history_transactions(&mut history);

        let tx_count = u32::try_from(history.len()).unwrap_or(u32::MAX);

        Ok((history, balance, tx_count, tip_height))
    })
    .await?;

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

    let result = run_account_info_blocking("transaction detail", move || {
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

        map_bdk_tx_to_detail(&wallet, tx, tip_height, wallet_network)
    })
    .await?;

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

    let result = run_account_info_blocking("address info", move || {
        let (client, tip_height) = connect_and_get_tip(&electrum_url_owned)?;

        let script = bdk_addr.script_pubkey();

        // Get UTXOs for this address
        let utxos =
            client
                .script_list_unspent(&script)
                .map_err(|e| AccountInfoError::ElectrumError {
                    error_details: format!("Failed to list UTXOs: {}", e),
                })?;

        // Get history for transfer count
        let history =
            client
                .script_get_history(&script)
                .map_err(|e| AccountInfoError::ElectrumError {
                    error_details: format!("Failed to get history: {}", e),
                })?;

        let account_utxos: Vec<AccountUtxo> = utxos
            .iter()
            .map(|utxo| {
                let height =
                    u32::try_from(utxo.height).map_err(|_| AccountInfoError::ElectrumError {
                        error_details: format!("UTXO height {} exceeds u32", utxo.height),
                    })?;
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

        let balance = utxos.iter().try_fold(0u64, |balance, utxo| {
            balance
                .checked_add(utxo.value)
                .ok_or_else(|| AccountInfoError::ElectrumError {
                    error_details: "Address UTXO balance overflow".to_string(),
                })
        })?;

        Ok::<_, AccountInfoError>(SingleAddressInfoResult {
            address: addr_str,
            balance,
            utxos: account_utxos,
            transfers: u32::try_from(history.len()).unwrap_or(u32::MAX),
            block_height: tip_height,
        })
    })
    .await?;

    Ok(result)
}

fn parse_address(address: &str) -> Result<Address<NetworkUnchecked>, AddressError> {
    Address::from_str(address).map_err(|_| AddressError::InvalidAddress)
}

fn determine_network(address: &str) -> Result<Network, AddressError> {
    match address {
        s if s.starts_with("1") || s.starts_with("3") || s.starts_with("bc1") => {
            Ok(Network::Bitcoin)
        }
        s if s.starts_with("2")
            || s.starts_with("tb1")
            || s.starts_with("m")
            || s.starts_with("n") =>
        {
            Ok(Network::Testnet)
        }
        s if s.starts_with("bcrt1") => Ok(Network::Regtest),
        _ => Err(AddressError::InvalidNetwork),
    }
}

fn verify_network(
    unchecked_addr: Address<NetworkUnchecked>,
    expected_network: Network,
) -> Result<Address, AddressError> {
    unchecked_addr
        .require_network(expected_network)
        .map_err(|_| AddressError::InvalidNetwork)
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

    address_type.ok_or(AddressError::InvalidAddress)
}
