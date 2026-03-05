//! Account information retrieval for Trezor compose.
//!
//! These functions query the blockchain via Electrum to build account
//! structures compatible with Trezor's transaction compose flow.
//! They do not require a connected Trezor device.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use bdk::bitcoin::bip32::ExtendedPubKey;
use bdk::bitcoin::consensus::deserialize;
use bdk::bitcoin::{Network as BdkNetwork, Transaction, Txid};
use bdk::bitcoin::Address as BdkAddress;
use bdk::blockchain::ElectrumBlockchain;
use bdk::electrum_client::ElectrumApi;
use bdk::wallet::{AddressIndex as BdkAddressIndex, SyncOptions, Wallet};
use bitcoin::address::Address;

use super::errors::AccountInfoError;
use super::types::{
    AccountAddresses, AccountInfoResult, AccountType, AccountUtxo,
    AddressInfo, ComposeAccount, SingleAddressInfoResult, TrezorCoinType,
    TrezorPrevTx, TrezorPrevTxInput, TrezorPrevTxOutput, TrezorScriptType,
};

// ============================================================================
// Network conversion helper
// ============================================================================

/// Convert TrezorCoinType to BDK's Network type.
fn coin_type_to_bdk_network(coin: TrezorCoinType) -> BdkNetwork {
    match coin {
        TrezorCoinType::Bitcoin => BdkNetwork::Bitcoin,
        TrezorCoinType::Testnet => BdkNetwork::Testnet,
        TrezorCoinType::Signet => BdkNetwork::Signet,
        TrezorCoinType::Regtest => BdkNetwork::Regtest,
    }
}

// ============================================================================
// Key/account type detection helpers
// ============================================================================

/// Detect the account type from an extended public key prefix.
pub fn detect_account_type(extended_key: &str) -> Result<AccountType, AccountInfoError> {
    if extended_key.len() < 4 {
        return Err(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        });
    }
    match &extended_key[..4] {
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
    if extended_key.len() < 4 {
        return Err(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        });
    }
    match &extended_key[..4] {
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
    if extended_key.len() < 4 {
        return Err(AccountInfoError::InvalidExtendedKey {
            error_details: "Key too short".to_string(),
        });
    }

    let prefix = &extended_key[..4];
    let target_version: Option<[u8; 4]> = match prefix {
        "xpub" | "tpub" => None, // Already standard format
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
pub fn build_descriptors(
    normalized_xpub: &str,
    account_type: AccountType,
) -> (String, String) {
    let (external, internal) = match account_type {
        AccountType::Legacy => (
            format!("pkh({}/0/*)", normalized_xpub),
            format!("pkh({}/1/*)", normalized_xpub),
        ),
        AccountType::WrappedSegwit => (
            format!("sh(wpkh({}/0/*))", normalized_xpub),
            format!("sh(wpkh({}/1/*))", normalized_xpub),
        ),
        AccountType::NativeSegwit => (
            format!("wpkh({}/0/*)", normalized_xpub),
            format!("wpkh({}/1/*)", normalized_xpub),
        ),
        AccountType::Taproot => (
            format!("tr({}/0/*)", normalized_xpub),
            format!("tr({}/1/*)", normalized_xpub),
        ),
    };
    (external, internal)
}

/// Determine the BIP derivation base path for Trezor.
pub fn derive_base_path(account_type: AccountType, network: BdkNetwork, account_index: u32) -> String {
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

/// Map AccountType to Trezor's ScriptType for transaction inputs.
pub fn account_type_to_script_type(account_type: AccountType) -> TrezorScriptType {
    match account_type {
        AccountType::Legacy => TrezorScriptType::SpendAddress,
        AccountType::WrappedSegwit => TrezorScriptType::SpendP2shWitness,
        AccountType::NativeSegwit => TrezorScriptType::SpendWitness,
        AccountType::Taproot => TrezorScriptType::SpendTaproot,
    }
}

// ============================================================================
// Main async functions
// ============================================================================

/// Query account information for an extended public key (xpub/ypub/zpub) via Electrum.
/// Returns data formatted for Trezor's composeTransaction in precompose mode.
pub async fn get_account_info(
    extended_key: &str,
    electrum_url: &str,
    network: Option<TrezorCoinType>,
    gap_limit: Option<u32>,
) -> Result<AccountInfoResult, AccountInfoError> {
    let account_type = detect_account_type(extended_key)?;
    let detected_network = detect_network_from_key(extended_key)?;

    // Verify network matches if caller specified one
    if let Some(coin) = network {
        let specified_bdk = coin_type_to_bdk_network(coin);
        if specified_bdk != detected_network {
            return Err(AccountInfoError::NetworkMismatch {
                error_details: format!(
                    "Key prefix suggests {:?} but {:?} was specified",
                    detected_network, specified_bdk
                ),
            });
        }
    }

    let normalized_key = normalize_extended_key(extended_key)?;
    let (external_desc, internal_desc) = build_descriptors(&normalized_key, account_type);

    // Parse xpub to get account index from child_number
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
    let gap = gap_limit.unwrap_or(20);
    let bdk_network = detected_network;

    let electrum_url_owned = electrum_url.to_string();
    let base_path_clone = base_path.clone();

    // All BDK + Electrum operations in a blocking task
    let result = tokio::task::spawn_blocking(move || {
        let base_path = base_path_clone;
        // Create BDK wallet from descriptors
        let wallet = Wallet::new(
            &external_desc,
            Some(&internal_desc),
            bdk_network,
            bdk::database::MemoryDatabase::new(),
        )
        .map_err(|e| AccountInfoError::WalletError {
            error_details: format!("Failed to create wallet: {}", e),
        })?;

        // Connect and sync
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;
        let blockchain = ElectrumBlockchain::from(client);

        wallet
            .sync(&blockchain, SyncOptions::default())
            .map_err(|e| AccountInfoError::SyncError {
                error_details: format!("Failed to sync wallet: {}", e),
            })?;

        // Get block tip height
        let electrum_client =
            bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to connect to Electrum: {}", e),
                }
            })?;
        let header = electrum_client.block_headers_subscribe().map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to get block height: {}", e),
            }
        })?;
        let tip_height = header.height as u32;

        // Get the next unused external address index
        let next_external = wallet
            .get_address(BdkAddressIndex::LastUnused)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get address: {}", e),
            })?;
        let max_external = next_external.index + gap;

        // Get the next unused change address index
        let next_change = wallet
            .get_internal_address(BdkAddressIndex::LastUnused)
            .map_err(|e| AccountInfoError::WalletError {
                error_details: format!("Failed to get change address: {}", e),
            })?;
        let max_change = next_change.index + gap;

        // Build address transfer counts via Electrum script_get_history
        let mut address_paths: HashMap<String, String> = HashMap::new();

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
            let history = electrum_client.script_get_history(&script).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!(
                        "Failed to get history for address {}: {}",
                        addr_str, e
                    ),
                }
            })?;

            let transfer_count = history.len() as u32;
            address_paths.insert(addr_str.clone(), path.clone());

            let info = AddressInfo {
                address: addr_str,
                path,
                transfers: transfer_count,
            };

            if transfer_count > 0 {
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
            let history = electrum_client.script_get_history(&script).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!(
                        "Failed to get history for change address {}: {}",
                        addr_str, e
                    ),
                }
            })?;

            let transfer_count = history.len() as u32;
            address_paths.insert(addr_str.clone(), path.clone());

            change_addresses.push(AddressInfo {
                address: addr_str,
                path,
                transfers: transfer_count,
            });
        }

        // Extract UTXOs
        let utxos = wallet.list_unspent().map_err(|e| AccountInfoError::WalletError {
            error_details: format!("Failed to list UTXOs: {}", e),
        })?;

        // Get transaction details for confirmation info
        let transactions =
            wallet
                .list_transactions(false)
                .map_err(|e| AccountInfoError::WalletError {
                    error_details: format!("Failed to list transactions: {}", e),
                })?;

        // Map UTXOs to AccountUtxo
        let mut account_utxos: Vec<AccountUtxo> = Vec::new();
        for utxo in &utxos {
            let addr_str = BdkAddress::from_script(&utxo.txout.script_pubkey, bdk_network)
                .map(|a| a.to_string())
                .unwrap_or_default();

            let utxo_path = address_paths
                .get(&addr_str)
                .cloned()
                .unwrap_or_default();

            // Get confirmation info from transaction details
            let (block_height, confirmations) = transactions
                .iter()
                .find(|tx| tx.txid == utxo.outpoint.txid)
                .and_then(|tx| tx.confirmation_time.as_ref())
                .map(|conf| {
                    let height = conf.height as u32;
                    let confs = tip_height.saturating_sub(height) + 1;
                    (height, confs)
                })
                .unwrap_or((0, 0));

            account_utxos.push(AccountUtxo {
                txid: utxo.outpoint.txid.to_string(),
                vout: utxo.outpoint.vout,
                amount: utxo.txout.value,
                block_height,
                address: addr_str,
                path: utxo_path,
                confirmations,
                coinbase: false,
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
            utxos.len() as u32,
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

/// Query balance and UTXOs for a single Bitcoin address via Electrum.
pub async fn get_address_info(
    address: &str,
    electrum_url: &str,
    _network: Option<TrezorCoinType>,
) -> Result<SingleAddressInfoResult, AccountInfoError> {
    // Validate address parses correctly using the top-level bitcoin crate
    let _parsed = Address::from_str(address).map_err(|e| AccountInfoError::InvalidAddress {
        error_details: format!("Invalid address: {}", e),
    })?;

    // Parse with BDK's bitcoin crate for script_pubkey generation
    let bdk_addr = BdkAddress::from_str(address)
        .map_err(|e| AccountInfoError::InvalidAddress {
            error_details: format!("Invalid address: {}", e),
        })?
        .assume_checked();

    let electrum_url_owned = electrum_url.to_string();
    let addr_str = address.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        let script = bdk_addr.script_pubkey();

        // Get block height
        let header = client.block_headers_subscribe().map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to get block height: {}", e),
            }
        })?;
        let tip_height = header.height as u32;

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
                let height = if utxo.height > 0 {
                    utxo.height as u32
                } else {
                    0
                };
                let confirmations = if utxo.height > 0 {
                    tip_height.saturating_sub(utxo.height as u32) + 1
                } else {
                    0
                };

                AccountUtxo {
                    txid: utxo.tx_hash.to_string(),
                    vout: utxo.tx_pos as u32,
                    amount: utxo.value,
                    block_height: height,
                    address: addr_str.clone(),
                    path: String::new(), // No derivation path for single address
                    confirmations,
                    coinbase: false,
                    own: true,
                    required: None,
                }
            })
            .collect();

        let balance: u64 = utxos.iter().map(|u| u.value).sum();

        Ok::<_, AccountInfoError>(SingleAddressInfoResult {
            address: addr_str,
            balance,
            utxos: account_utxos,
            transfers: history.len() as u32,
            block_height: tip_height,
        })
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    Ok(result)
}

// ============================================================================
// Previous transaction fetching
// ============================================================================

/// Fetch previous transactions from Electrum and convert to TrezorPrevTx format.
///
/// Takes a list of transaction ID hex strings, fetches the raw transactions
/// from an Electrum server, and returns them as `TrezorPrevTx` structures
/// suitable for inclusion in `TrezorSignTxParams.prev_txs`.
///
/// Duplicate txids are automatically deduplicated.
pub async fn fetch_prev_txs(
    txids: Vec<String>,
    electrum_url: &str,
) -> Result<Vec<TrezorPrevTx>, AccountInfoError> {
    // Deduplicate txids
    let unique_txids: Vec<String> = txids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Parse all txid strings upfront, fail fast on bad input
    let parsed_txids: Vec<(String, Txid)> = unique_txids
        .into_iter()
        .map(|txid_str| {
            let txid = Txid::from_str(&txid_str).map_err(|e| {
                AccountInfoError::InvalidTxid {
                    error_details: format!("Invalid txid '{}': {}", txid_str, e),
                }
            })?;
            Ok((txid_str, txid))
        })
        .collect::<Result<Vec<_>, AccountInfoError>>()?;

    let electrum_url_owned = electrum_url.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        let mut prev_txs: Vec<TrezorPrevTx> = Vec::with_capacity(parsed_txids.len());

        for (txid_hex, txid) in &parsed_txids {
            let tx_bytes = client.transaction_get_raw(txid).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to fetch tx {}: {}", txid_hex, e),
                }
            })?;

            let tx: Transaction = deserialize(&tx_bytes).map_err(|e| {
                AccountInfoError::ElectrumError {
                    error_details: format!("Failed to deserialize tx {}: {}", txid_hex, e),
                }
            })?;

            prev_txs.push(transaction_to_prev_tx(&tx));
        }

        Ok::<_, AccountInfoError>(prev_txs)
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Task failed: {}", e),
    })??;

    Ok(result)
}

/// Convert a bitcoin::Transaction to TrezorPrevTx.
pub(crate) fn transaction_to_prev_tx(tx: &Transaction) -> TrezorPrevTx {
    let inputs = tx
        .input
        .iter()
        .map(|input| TrezorPrevTxInput {
            prev_hash: input.previous_output.txid.to_string(),
            prev_index: input.previous_output.vout,
            script_sig: hex::encode(input.script_sig.as_bytes()),
            sequence: input.sequence.0,
        })
        .collect();

    let outputs = tx
        .output
        .iter()
        .map(|output| TrezorPrevTxOutput {
            amount: output.value,
            script_pubkey: hex::encode(output.script_pubkey.as_bytes()),
        })
        .collect();

    TrezorPrevTx {
        hash: tx.txid().to_string(),
        version: tx.version as u32,
        lock_time: tx.lock_time.to_consensus_u32(),
        inputs,
        outputs,
    }
}

/// Broadcast a signed raw transaction via Electrum.
///
/// Takes a hex-encoded serialized transaction and an Electrum server URL.
/// Returns the transaction ID on success.
pub async fn broadcast_raw_tx(
    serialized_tx: String,
    electrum_url: &str,
) -> Result<String, AccountInfoError> {
    let tx_bytes = hex::decode(&serialized_tx).map_err(|e| AccountInfoError::ElectrumError {
        error_details: format!("Invalid transaction hex: {}", e),
    })?;

    // Validate that the bytes are a valid transaction
    let _: Transaction = deserialize(&tx_bytes).map_err(|e| AccountInfoError::ElectrumError {
        error_details: format!("Invalid transaction data: {}", e),
    })?;

    let electrum_url_owned = electrum_url.to_string();

    let txid = tokio::task::spawn_blocking(move || {
        let client = bdk::electrum_client::Client::new(&electrum_url_owned).map_err(|e| {
            AccountInfoError::ElectrumError {
                error_details: format!("Failed to connect to Electrum: {}", e),
            }
        })?;

        client
            .transaction_broadcast_raw(&tx_bytes)
            .map_err(|e| AccountInfoError::ElectrumError {
                error_details: format!("Broadcast failed: {}", e),
            })
    })
    .await
    .map_err(|e| AccountInfoError::SyncError {
        error_details: format!("Broadcast task failed: {}", e),
    })??;

    Ok(txid.to_string())
}
