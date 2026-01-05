use std::str::FromStr;

use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::absolute::LockTime;
use bdk::bitcoin::consensus::deserialize;
use bdk::bitcoin::psbt::PartiallySignedTransaction as Psbt;
use bdk::bitcoin::{
    Address as BdkAddress, Network as BdkNetwork, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxOut, Witness,
};
use bdk::blockchain::ElectrumBlockchain;
use bdk::database::MemoryDatabase;
use bdk::electrum_client::ElectrumApi;
use bdk::keys::bip39::Mnemonic;
use bdk::template::{Bip44, Bip49, Bip86};
use bdk::wallet::signer::SignOptions;
use bdk::wallet::{SyncOptions, Wallet};
use bdk::{FeeRate, KeychainKind};
use bitcoin::address::{Address, NetworkUnchecked};
use bitcoin::Network;
use bitcoin_address_generator;

use super::types::{AddressType, ValidationResult};
use crate::modules::scanner::NetworkType;
use crate::onchain::types::{
    GetAddressResponse, GetAddressesResponse, SweepResult, SweepTransactionPreview,
    SweepableBalances, WordCount,
};
use crate::onchain::{AddressError, SweepError};

const P2SH_P2WPKH_WITNESS_WU: u64 = 107;
const P2TR_WITNESS_WU: u64 = 65;
const P2PKH_SCRIPTSIG_WU: u64 = 107 * 4;
const P2SH_P2WPKH_SCRIPTSIG_WU: u64 = 23 * 4;

fn network_to_bdk(network: Network) -> BdkNetwork {
    match network {
        Network::Bitcoin => BdkNetwork::Bitcoin,
        Network::Testnet | Network::Testnet4 => BdkNetwork::Testnet,
        Network::Signet => BdkNetwork::Signet,
        Network::Regtest => BdkNetwork::Regtest,
    }
}

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
            Ok(_) => {},
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

    pub fn genenerate_mnemonic(
        word_count: Option<WordCount>,
    ) -> Result<String, AddressError> {
        let external_word_count = word_count.map(|wc| wc.into());
        let mnemonic = bitcoin_address_generator::generate_mnemonic(external_word_count, None);
        match mnemonic {
            Ok(mnemonic) => {
                println!("✓ Generated mnemonic: {}", mnemonic);
                Ok(mnemonic)
            },
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
        let bdk_network = network_to_bdk(network);
        let mnemonic =
            Mnemonic::from_str(mnemonic_phrase).map_err(|_| SweepError::InvalidMnemonic)?;
        let key = (mnemonic.clone(), bip39_passphrase.map(String::from));

        let legacy_wallet = Wallet::new(
            Bip44(key.clone(), KeychainKind::External),
            None,
            bdk_network,
            MemoryDatabase::new(),
        )
        .map_err(|e| SweepError::SweepFailed(format!("Failed to create legacy wallet: {}", e)))?;

        let p2sh_wallet = Wallet::new(
            Bip49(key.clone(), KeychainKind::External),
            None,
            bdk_network,
            MemoryDatabase::new(),
        )
        .map_err(|e| SweepError::SweepFailed(format!("Failed to create P2SH wallet: {}", e)))?;

        let taproot_wallet = Wallet::new(
            Bip86(key, KeychainKind::External),
            None,
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

    fn estimate_sweep_vsize(
        base_vsize: u64,
        legacy_count: usize,
        p2sh_count: usize,
        taproot_count: usize,
    ) -> u64 {
        let witness_weight =
            (p2sh_count as u64 * P2SH_P2WPKH_WITNESS_WU) + (taproot_count as u64 * P2TR_WITNESS_WU);

        let scriptsig_weight = (legacy_count as u64 * P2PKH_SCRIPTSIG_WU)
            + (p2sh_count as u64 * P2SH_P2WPKH_SCRIPTSIG_WU);

        let additional_weight = witness_weight + scriptsig_weight;
        let additional_vsize = (additional_weight + 3) / 4; // Round up

        base_vsize + additional_vsize
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
        let bdk_network = network_to_bdk(network);
        let wallets = Self::create_sweep_wallets(mnemonic_phrase, network, bip39_passphrase)?;

        let dest_addr = BdkAddress::from_str(destination_address)
            .map_err(|e| SweepError::SweepFailed(format!("Invalid destination address: {}", e)))?
            .require_network(bdk_network)
            .map_err(|e| {
                SweepError::SweepFailed(format!("Network mismatch for destination address: {}", e))
            })?;

        // Sync wallets and get electrum client for fetching transactions
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

        // Collect UTXOs from all wallet types
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

        // Build transaction inputs
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

        // Create placeholder transaction for size estimation
        let placeholder_tx = Transaction {
            version: 2,
            lock_time: LockTime::from_consensus(0),
            input: inputs.clone(),
            output: vec![TxOut {
                value: total_amount,
                script_pubkey: dest_addr.script_pubkey(),
            }],
        };

        // Estimate actual vsize with witness data
        let base_vsize = placeholder_tx.weight().to_vbytes_ceil();
        let actual_vsize = Self::estimate_sweep_vsize(
            base_vsize,
            legacy_utxos.len(),
            p2sh_utxos.len(),
            taproot_utxos.len(),
        );

        // Calculate fee and output amount
        let fee_rate = FeeRate::from_sat_per_vb(fee_rate_sats_per_vbyte.unwrap_or(1) as f32);
        let estimated_fee = fee_rate.fee_vb(actual_vsize as usize);
        let amount_after_fees = total_amount.saturating_sub(estimated_fee);

        // Build final transaction
        let final_tx = Transaction {
            version: 2,
            lock_time: LockTime::from_consensus(0),
            input: inputs,
            output: vec![TxOut {
                value: amount_after_fees,
                script_pubkey: dest_addr.script_pubkey(),
            }],
        };

        let mut psbt = Psbt::from_unsigned_tx(final_tx)
            .map_err(|e| SweepError::SweepFailed(format!("Failed to create PSBT: {}", e)))?;

        // Populate PSBT inputs with UTXO data
        let legacy_count = legacy_utxos.len();
        let p2sh_count = p2sh_utxos.len();

        for (i, utxo) in all_utxos.iter().enumerate() {
            psbt.inputs[i].witness_utxo = Some(utxo.txout.clone());

            // Legacy and P2SH inputs require full previous transaction
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

        let psbt_base64 = general_purpose::STANDARD.encode(psbt.serialize());

        Ok(SweepTransactionPreview {
            psbt: psbt_base64,
            total_amount,
            estimated_fee,
            utxos_count: all_utxos.len() as u32,
            destination_address: dest_addr.to_string(),
            amount_after_fees,
        })
    }

    pub async fn broadcast_sweep_transaction(
        psbt_base64: &str,
        fee_rate_sats_per_vbyte: u32,
        mnemonic_phrase: &str,
        network: Network,
        bip39_passphrase: Option<&str>,
        electrum_url: &str,
    ) -> Result<SweepResult, SweepError> {
        // Decode and validate PSBT
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

        // Calculate total input and count UTXO types for fee estimation
        let mut total_input: u64 = 0;
        let mut legacy_count = 0usize;
        let mut p2sh_count = 0usize;
        let mut taproot_count = 0usize;

        for input in &psbt.inputs {
            if let Some(utxo) = &input.witness_utxo {
                total_input += utxo.value;

                // Detect UTXO type from scriptPubKey
                let script = &utxo.script_pubkey;
                if script.is_p2pkh() {
                    legacy_count += 1;
                } else if script.is_p2sh() {
                    p2sh_count += 1;
                } else if script.is_v1_p2tr() {
                    taproot_count += 1;
                }
            }
        }

        // Estimate actual vsize with witness data
        let base_vsize = psbt.unsigned_tx.weight().to_vbytes_ceil();
        let actual_vsize =
            Self::estimate_sweep_vsize(base_vsize, legacy_count, p2sh_count, taproot_count);

        let fee_rate = FeeRate::from_sat_per_vb(fee_rate_sats_per_vbyte as f32);
        let fee_amount = fee_rate.fee_vb(actual_vsize as usize);
        let output_amount = total_input.saturating_sub(fee_amount);

        if output_amount == 0 || output_amount > total_input {
            return Err(SweepError::InsufficientFunds);
        }

        // Rebuild PSBT with updated output amount
        let mut new_outputs = psbt.unsigned_tx.output.clone();
        new_outputs[0].value = output_amount;

        let updated_tx = Transaction {
            version: psbt.unsigned_tx.version,
            lock_time: psbt.unsigned_tx.lock_time,
            input: psbt.unsigned_tx.input.clone(),
            output: new_outputs,
        };

        let mut updated_psbt = Psbt::from_unsigned_tx(updated_tx)
            .map_err(|e| SweepError::SweepFailed(format!("Failed to recreate PSBT: {}", e)))?;

        // Copy UTXO data from original PSBT
        for (i, input) in psbt.inputs.iter().enumerate() {
            if i < updated_psbt.inputs.len() {
                updated_psbt.inputs[i].witness_utxo = input.witness_utxo.clone();
                updated_psbt.inputs[i].non_witness_utxo = input.non_witness_utxo.clone();
            }
        }

        // Create wallets and sync before signing
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

        // Sign the PSBT
        Self::sign_psbt(&wallets, &mut updated_psbt)?;

        // Verify all inputs are finalized
        let utxos_count = updated_psbt.inputs.len() as u32;
        for input in &updated_psbt.inputs {
            if input.final_script_sig.is_none() && input.final_script_witness.is_none() {
                return Err(SweepError::SweepFailed(
                    "Transaction signing incomplete - some inputs not finalized".to_string(),
                ));
            }
        }

        // Extract and broadcast
        let final_tx = updated_psbt.extract_tx();
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
            amount_swept: total_input,
            fee_paid: fee_amount,
            utxos_swept: utxos_count,
        })
    }
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
        },
        s if s.starts_with("2") || s.starts_with("tb1") || s.starts_with("m") || s.starts_with("n") => {
            println!("✓ Determined network: Testnet");
            Ok(Network::Testnet)
        },
        s if s.starts_with("bcrt1") => {
            println!("✓ Determined network: Regtest");
            Ok(Network::Regtest)
        },
        _ => {
            println!("✗ Could not determine network");
            Err(AddressError::InvalidNetwork)
        }
    }
}

fn verify_network(unchecked_addr: Address<NetworkUnchecked>, expected_network: Network)
                  -> Result<Address, AddressError> {
    println!("Attempting to verify address for network: {:?}", expected_network);
    unchecked_addr.require_network(expected_network)
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
        s if s.starts_with("1") || s.starts_with("m") || s.starts_with("n") => Some(AddressType::P2PKH),
        // SegWit addresses (P2SH)
        s if s.starts_with("3") || s.starts_with("2") => Some(AddressType::P2SH),
        // Taproot addresses (P2TR)
        s if s.starts_with("bc1p") || s.starts_with("tb1p") => Some(AddressType::P2TR),
        // Native SegWit addresses (P2WPKH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 42 => Some(AddressType::P2WPKH),
        // Native SegWit Script addresses (P2WSH)
        s if (s.starts_with("bc1q") || s.starts_with("tb1q")) && s.len() == 62 => Some(AddressType::P2WSH),
        // Regtest addresses
        s if s.starts_with("bcrt1") => {
            if s.len() == 42 {
                Some(AddressType::P2WPKH)
            } else if s.len() == 62 {
                Some(AddressType::P2WSH)
            } else {
                Some(AddressType::Unknown)
            }
        },
        _ => Some(AddressType::Unknown)
    };

    address_type.map(|t| {
        println!("✓ Determined address type: {:?}", t);
        t
    }).ok_or_else(|| {
        println!("✗ Could not determine address type");
        AddressError::InvalidAddress
    })
}
