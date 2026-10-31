//! The started wallet: an ark-client `Client` on the HD key provider, esplora, BDK and sqlite, with the
//! bridge's methods on top.

use super::errors::ArkadeError;
use super::esplora::Esplora;
use super::intents::{ArkWallet, WalletError};
use super::keys::Keys;
use super::lnurl::{ArkadeLightningAddress, Destinations, LnurlService};
use super::store::{self, SqliteContracts};
use super::types::*;
use ark_client::vtxo_watcher::{VtxoWatcherConfig, VtxoWatcherHandle};
use ark_client::{
    Blockchain, GetVtxosRequestFilter, InMemorySwapStorage, OfflineClient, OfflineClientConfig,
};
use ark_core::history::Transaction;
use ark_core::send::SendReceiver;
use ark_core::server::{Info, VirtualTxOutPoint};
use ark_core::ArkAddress;
use ark_delegator::DelegatorClient;
use bitcoin::{Address, Amount, OutPoint, Psbt, Sequence, Txid, XOnlyPublicKey};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Swaps arrive with the Arkade Intents client (stage B); until then the swap store ark-client requires is the
/// in-memory one.
pub type Ark = ark_client::Client<Esplora, ark_bdk_wallet::Wallet, InMemorySwapStorage>;

/// Outputs this close to their batch expiry are renewed (the TypeScript `VtxoManager`'s threshold).
pub const RENEW_WITHIN: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// How many unused keys the restore scan walks past before it stops.
pub const RESTORE_GAP_LIMIT: u32 = ark_client::DEFAULT_GAP_LIMIT;

pub struct Wallet {
    pub client: Arc<Ark>,
    pub esplora: Arc<Esplora>,
    pub network: &'static str,
    pub delegate: bool,
    pub lnurl: Option<LnurlService>,
    /// One batch at a time: the chores and the app's own settle, renew and offboard calls would otherwise
    /// register competing intents for the same outputs.
    pub batch: tokio::sync::Mutex<()>,
    pub watcher: std::sync::Mutex<Option<VtxoWatcherHandle>>,
    /// `m/86'/c'/1'/0/0`, the TypeScript wallet's static key: the Lightning swaps' claim and refund key.
    identity: bitcoin::secp256k1::Keypair,
}

pub struct BoardingUtxo {
    pub outpoint: OutPoint,
    pub amount: Amount,
    pub confirmed: bool,
    pub spent: bool,
}

impl Wallet {
    pub async fn start(params: &ArkadeStartParams) -> Result<Arc<Self>, ArkadeError> {
        let data_dir = Path::new(&params.data_dir);
        let config = params.network.config();
        let keys = Keys::from_mnemonic(
            &params.mnemonic,
            params.passphrase.as_deref(),
            config.network,
        )?;
        std::fs::create_dir_all(data_dir).map_err(ArkadeError::storage)?;
        if let Some(backup) = params.backup.as_deref() {
            match store::import(data_dir, backup) {
                Ok(rows) => log::info!("Imported '{rows}' Arkade contracts from backup"),
                Err(e) => log::warn!("importing Arkade backup failed: {e}"),
            }
        }

        // The TypeScript wallet builds the delegate (3-leaf) address only when the delegate answered, and the
        // plain one otherwise; so does this.
        let delegator = config.delegate_url.clone().map(DelegatorClient::new);
        let delegator_pk = match &delegator {
            Some(delegator) => match delegator.info().await {
                Ok(info) => Some(parse_xonly(&info.pubkey)?),
                Err(e) => {
                    log::warn!("delegate info failed, starting without a delegate: {e}");
                    None
                }
            },
            None => None,
        };

        let esplora = Arc::new(Esplora::new(&config.esplora_url)?);
        let onchain = ark_bdk_wallet::Wallet::new_from_xpriv(
            keys.account_xpriv()?,
            config.network,
            &config.esplora_url,
        )
        .map_err(ArkadeError::internal)?;
        let swaps = InMemorySwapStorage::new();
        let contracts = SqliteContracts::open(data_dir)?;
        let client_config = OfflineClientConfig {
            ark_server_url: config.ark_server_url.clone(),
            delegator_pk,
            ..Default::default()
        };
        let client = OfflineClient::with_bip32(
            client_config,
            keys.master,
            Some(keys.receive_path.clone()),
            esplora.clone(),
            Arc::new(onchain),
            Arc::new(swaps),
        )
        .with_contract_store(Box::new(contracts))
        .connect()
        .await?;
        let info = client.server_info().await?;
        if info.network != config.network {
            return Err(ArkadeError::invalid(format!(
                "operator runs on {}, expected {}",
                info.network, config.name
            )));
        }

        if params.restore {
            let started = std::time::Instant::now();
            match client.restore_contracts(RESTORE_GAP_LIMIT).await {
                Ok(report) => log::info!(
                    "Restored '{}' Arkade keys and '{}' contracts in '{}' ms",
                    report.discovered_keys(),
                    report.discovered_contracts(),
                    started.elapsed().as_millis()
                ),
                Err(e) => log::warn!("restore scan failed: {e}"),
            }
        }

        let client = Arc::new(client);
        let watcher = delegator
            .filter(|_| delegator_pk.is_some())
            .map(|d| client.start_vtxo_watcher(Arc::new(d), VtxoWatcherConfig::default()));
        let lnurl = config
            .lnurl_url
            .as_deref()
            .map(|url| LnurlService::new(url, &keys.identity))
            .transpose()?;
        Ok(Arc::new(Self {
            client,
            esplora,
            network: config.name,
            delegate: delegator_pk.is_some(),
            lnurl,
            batch: tokio::sync::Mutex::new(()),
            watcher: std::sync::Mutex::new(watcher),
            identity: keys.identity,
        }))
    }

    pub fn stop(&self) {
        if let Some(watcher) = self.watcher.lock().ok().and_then(|mut w| w.take()) {
            watcher.stop();
        }
    }

    pub async fn info(&self) -> Result<ArkadeInfo, ArkadeError> {
        let info = self.client.server_info().await?;
        let (ark_address, _) = self.client.get_offchain_address().await?;
        let boarding_address = self.client.get_boarding_address().await?;
        Ok(ArkadeInfo {
            ark_address: ark_address.encode(),
            boarding_address: boarding_address.to_string(),
            network: self.network.to_string(),
            dust: info.dust.to_sat(),
            unilateral_exit_delay: sequence_value(info.unilateral_exit_delay),
            boarding_exit_delay: sequence_value(info.boarding_exit_delay),
            fees: fees_of(&info),
            delegate: self.delegate,
        })
    }

    pub async fn fees(&self) -> Result<ArkadeFees, ArkadeError> {
        Ok(fees_of(&self.client.server_info().await?))
    }

    /// Every output on every boarding address this wallet watches, with whether a batch has taken it.
    pub async fn boarding_utxos(&self) -> Result<Vec<BoardingUtxo>, ArkadeError> {
        let mut seen = HashSet::new();
        let mut utxos = Vec::new();
        for address in self.client.get_boarding_addresses().await? {
            for utxo in self.esplora.find_outpoints(&address).await? {
                if seen.insert(utxo.outpoint) {
                    utxos.push(BoardingUtxo {
                        outpoint: utxo.outpoint,
                        amount: utxo.amount,
                        confirmed: utxo.confirmation_blocktime.is_some(),
                        spent: utxo.is_spent,
                    });
                }
            }
        }
        Ok(utxos)
    }

    pub async fn balance(&self) -> Result<ArkadeBalance, ArkadeError> {
        let offchain = self.client.offchain_balance().await?;
        let boarding = self.boarding_utxos().await?;
        let sum = |confirmed: bool| {
            boarding
                .iter()
                .filter(|u| !u.spent && u.confirmed == confirmed)
                .map(|u| u.amount.to_sat())
                .sum::<u64>()
        };
        let settled = offchain.confirmed().to_sat();
        let preconfirmed = offchain.pre_confirmed().to_sat();
        let recoverable = (offchain.recoverable() + offchain.pending_recovery()).to_sat();
        let (boarding_confirmed, boarding_unconfirmed) = (sum(true), sum(false));
        Ok(ArkadeBalance {
            available: settled + preconfirmed,
            settled,
            preconfirmed,
            recoverable,
            boarding_confirmed,
            boarding_unconfirmed,
            total: settled + preconfirmed + recoverable + boarding_confirmed + boarding_unconfirmed,
        })
    }

    pub async fn vtxos(&self) -> Result<Vec<ArkadeVtxo>, ArkadeError> {
        let list = self
            .client
            .list_vtxos_filtered(GetVtxosRequestFilter::Renewable)
            .await?;
        Ok(list
            .all_unspent()
            .map(|entry| {
                let v = entry.vtxo();
                let state = if v.is_swept {
                    "swept"
                } else if v.is_spent {
                    "spent"
                } else if v.is_preconfirmed {
                    "preconfirmed"
                } else {
                    "settled"
                };
                ArkadeVtxo {
                    txid: v.outpoint.txid.to_string(),
                    vout: v.outpoint.vout,
                    value: v.amount.to_sat(),
                    state: state.to_string(),
                    batch_expiry: u64::try_from(v.expires_at).ok().filter(|e| *e > 0),
                    created_at: u64::try_from(v.created_at).ok().map(|s| s * 1000),
                    is_preconfirmed: v.is_preconfirmed,
                    is_swept: v.is_swept,
                }
            })
            .collect())
    }

    pub async fn history(&self) -> Result<Vec<ArkadeHistoryEntry>, ArkadeError> {
        let boarded: HashMap<Txid, bool> = self
            .boarding_utxos()
            .await?
            .into_iter()
            .map(|u| (u.outpoint.txid, u.spent))
            .collect();
        let now_ms = now_secs() * 1000;
        let at = |secs: Option<i64>| {
            secs.and_then(|s| u64::try_from(s).ok())
                .map_or(now_ms, |s| s * 1000)
        };
        let txs = self.client.transaction_history().await?;
        Ok(txs
            .into_iter()
            .map(|tx| match tx {
                Transaction::Boarding {
                    txid,
                    amount,
                    confirmed_at,
                } => entry(
                    (Some(txid), None, None),
                    RECEIVED,
                    "boarding",
                    amount.to_sat(),
                    boarded.get(&txid).copied().unwrap_or(false),
                    at(confirmed_at),
                ),
                Transaction::Commitment {
                    txid,
                    amount,
                    created_at,
                } => entry(
                    (None, Some(txid), None),
                    if amount.is_negative() { SENT } else { RECEIVED },
                    "batch",
                    amount.unsigned_abs().to_sat(),
                    true,
                    at(Some(created_at)),
                ),
                Transaction::Ark {
                    txid,
                    amount,
                    is_settled,
                    created_at,
                } => entry(
                    (None, None, Some(txid)),
                    if amount.is_negative() { SENT } else { RECEIVED },
                    "offchain",
                    amount.unsigned_abs().to_sat(),
                    is_settled,
                    at(Some(created_at)),
                ),
                Transaction::Offboard {
                    commitment_txid,
                    amount,
                    confirmed_at,
                } => entry(
                    (None, Some(commitment_txid), None),
                    SENT,
                    "batch",
                    amount.to_sat(),
                    true,
                    at(confirmed_at),
                ),
            })
            .collect())
    }

    /// Boards confirmed boarding outputs now (the bridge's `settle`, `Ramps.onboard`).
    pub async fn settle(&self) -> Result<Option<String>, ArkadeError> {
        let boarding: Vec<OutPoint> = self
            .boarding_utxos()
            .await?
            .into_iter()
            .filter(|u| u.confirmed && !u.spent)
            .map(|u| u.outpoint)
            .collect();
        if boarding.is_empty() {
            return Ok(None);
        }
        let _batch = self.batch.lock().await;
        let txid = self.client.settle_vtxos(&mut rng(), &[], &boarding).await?;
        Ok(txid.map(|t| t.to_string()))
    }

    /// Renews outputs within [`RENEW_WITHIN`] of their batch expiry, and recoverable ones.
    pub async fn renew(&self) -> Result<Option<String>, ArkadeError> {
        let info = self.client.server_info().await?;
        let cutoff = now_secs() as i64 + RENEW_WITHIN.as_secs() as i64;
        let list = self
            .client
            .list_vtxos_filtered(GetVtxosRequestFilter::Renewable)
            .await?;
        let due: Vec<OutPoint> = list
            .all_unspent()
            .filter(|e| {
                e.vtxo().is_recoverable(info.dust)
                    || (e.vtxo().expires_at > 0 && e.vtxo().expires_at < cutoff)
            })
            .map(|e| e.vtxo().outpoint)
            .collect();
        if due.is_empty() {
            return Ok(None);
        }
        let _batch = self.batch.lock().await;
        let txid = self.client.settle_vtxos(&mut rng(), &due, &[]).await?;
        Ok(txid.map(|t| t.to_string()))
    }

    /// Collaborative exit to `address`, paying the operator's on-chain output fee out of the amount, as
    /// `Ramps.offboard` does: the address gets `amount` (or everything) less that fee.
    pub async fn offboard(
        &self,
        address: &str,
        amount: Option<u64>,
    ) -> Result<String, ArkadeError> {
        let info = self.client.server_info().await?;
        let address = Address::from_str(address)
            .and_then(|a| a.require_network(info.network))
            .map_err(|e| ArkadeError::invalid(format!("invalid address: {e}")))?;
        let estimator = fee_estimator(&info)?;
        let output_fee = |sats: u64| -> Result<u64, ArkadeError> {
            let output = ark_fees::Output {
                amount: sats,
                script: address.script_pubkey().to_string(),
            };
            Ok(estimator
                .eval_onchain_output(output)
                .map_err(ArkadeError::ark)?
                .to_satoshis())
        };
        let mut rng = rng();
        let _batch = self.batch.lock().await;
        let txid = match amount {
            Some(amount) => {
                let send = amount
                    .checked_sub(output_fee(amount)?)
                    .filter(|s| *s > 0)
                    .ok_or_else(|| {
                        ArkadeError::invalid(format!("{amount} sats does not cover the exit fee"))
                    })?;
                self.client
                    .collaborative_redeem(&mut rng, address, Amount::from_sat(send))
                    .await?
            }
            None => {
                let now = now_secs() as i64;
                let list = self
                    .client
                    .list_vtxos_filtered(GetVtxosRequestFilter::Renewable)
                    .await?;
                let inputs: Vec<(OutPoint, u64)> = list
                    .batch_settleable_at(&info, now)
                    .map(|e| (e.vtxo().outpoint, e.vtxo().amount.to_sat()))
                    .collect();
                let total: u64 = inputs.iter().map(|(_, sats)| sats).sum();
                // A fee program may price the output on its own amount: settle on the largest output whose
                // amount plus fee fits the inputs.
                let mut send = total.saturating_sub(output_fee(total)?);
                while send > 0 && send + output_fee(send)? > total {
                    send -= 1;
                }
                if send == 0 {
                    return Err(ArkadeError::invalid("nothing to offboard after fees"));
                }
                let outpoints = inputs.into_iter().map(|(o, _)| o).collect::<Vec<_>>();
                self.client
                    .collaborative_redeem_vtxo_selection(
                        &mut rng,
                        outpoints.into_iter(),
                        address,
                        Amount::from_sat(send),
                    )
                    .await?
            }
        };
        Ok(txid.to_string())
    }

    pub async fn send_ark(&self, address: &str, amount: u64) -> Result<String, ArkadeError> {
        let address = ArkAddress::decode(address)
            .map_err(|e| ArkadeError::invalid(format!("invalid Arkade address: {e}")))?;
        let txid = self
            .client
            .send(vec![SendReceiver::bitcoin(
                address,
                Amount::from_sat(amount),
            )])
            .await?;
        Ok(txid.to_string())
    }

    pub async fn lightning_address(&self) -> Result<ArkadeLightningAddress, ArkadeError> {
        let lnurl = self
            .lnurl
            .as_ref()
            .ok_or_else(|| ArkadeError::lnurl("not configured on this network"))?;
        let info = self.info().await?;
        lnurl
            .lightning_address(&Destinations {
                arkade_address: info.ark_address,
                boarding_address: info.boarding_address,
            })
            .await
    }

    /// One pass of the background chores: board confirmed deposits, renew outputs near expiry.
    pub async fn chores(&self) {
        if let Err(e) = self.settle().await {
            log::warn!("boarding failed: {e}");
        }
        if let Err(e) = self.renew().await {
            log::warn!("renewal failed: {e}");
        }
    }
}

fn entry(
    (boarding, commitment, ark): (Option<Txid>, Option<Txid>, Option<Txid>),
    tx_type: &str,
    tag: &str,
    amount: u64,
    settled: bool,
    created_at: u64,
) -> ArkadeHistoryEntry {
    let [boarding_txid, commitment_txid, ark_txid] =
        [boarding, commitment, ark].map(|t| t.map(|t| t.to_string()));
    let key = [&boarding_txid, &commitment_txid, &ark_txid]
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(":");
    ArkadeHistoryEntry {
        key,
        boarding_txid,
        commitment_txid,
        ark_txid,
        tx_type: tx_type.to_string(),
        tag: tag.to_string(),
        amount,
        settled,
        created_at,
    }
}

fn parse_xonly(pk: &str) -> Result<XOnlyPublicKey, ArkadeError> {
    XOnlyPublicKey::from_str(pk)
        .or_else(|_| bitcoin::PublicKey::from_str(pk).map(XOnlyPublicKey::from))
        .map_err(|e| ArkadeError::ark(format!("invalid delegate key '{pk}': {e}")))
}

/// The operator's delay as `/v1/info` gives it: seconds for time-based delays, blocks otherwise.
pub fn sequence_value(sequence: Sequence) -> u64 {
    match sequence.to_relative_lock_time() {
        Some(bitcoin::relative::LockTime::Time(t)) => u64::from(t.value()) * 512,
        Some(bitcoin::relative::LockTime::Blocks(b)) => u64::from(b.value()),
        None => 0,
    }
}

fn fees_of(info: &Info) -> ArkadeFees {
    let fees = info
        .fees
        .clone()
        .unwrap_or_else(|| ark_core::server::FeeInfo {
            intent_fee: Default::default(),
            tx_fee_rate: String::new(),
        });
    ArkadeFees {
        intent_offchain_input: fees.intent_fee.offchain_input.unwrap_or_default(),
        intent_offchain_output: fees.intent_fee.offchain_output.unwrap_or_default(),
        intent_onchain_input: fees.intent_fee.onchain_input.unwrap_or_default(),
        intent_onchain_output: fees.intent_fee.onchain_output.unwrap_or_default(),
        tx_fee_rate: fees.tx_fee_rate,
    }
}

/// ark-client builds the same estimator internally but keeps it private.
fn fee_estimator(info: &Info) -> Result<ark_fees::Estimator, ArkadeError> {
    let fees = fees_of(info);
    ark_fees::Estimator::new(ark_fees::Config {
        intent_offchain_input_program: fees.intent_offchain_input,
        intent_onchain_input_program: fees.intent_onchain_input,
        intent_offchain_output_program: fees.intent_offchain_output,
        intent_onchain_output_program: fees.intent_onchain_output,
    })
    .map_err(ArkadeError::ark)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A `Send` RNG: the batch futures hold it across awaits on a multi-threaded runtime.
fn rng() -> rand::rngs::StdRng {
    rand::SeedableRng::from_entropy()
}

/// The Arkade Intents client's wallet: network I/O on the ark-client `Client` and the identity key.
#[async_trait::async_trait]
impl ArkWallet for Wallet {
    async fn server_info(&self) -> Result<Info, WalletError> {
        self.client.server_info().await.map_err(wallet_error)
    }

    async fn address(&self) -> Result<ArkAddress, WalletError> {
        Ok(self
            .client
            .get_offchain_address()
            .await
            .map_err(wallet_error)?
            .0)
    }

    fn identity_keypair(&self) -> Result<bitcoin::secp256k1::Keypair, WalletError> {
        Ok(self.identity)
    }

    async fn vtxos(&self, address: &ArkAddress) -> Result<Vec<VirtualTxOutPoint>, WalletError> {
        self.client
            .get_virtual_tx_outpoints(std::iter::once(*address))
            .await
            .map_err(wallet_error)
    }

    async fn virtual_tx(&self, txid: Txid) -> Result<Option<Psbt>, WalletError> {
        let response = self
            .client
            .network_client()
            .get_virtual_txs(vec![txid.to_string()], None, None)
            .await
            .map_err(wallet_error)?;
        Ok(response
            .txs
            .into_iter()
            .find(|tx| tx.unsigned_tx.compute_txid() == txid))
    }

    async fn submit_offchain(
        &self,
        ark_tx: Psbt,
        checkpoints: Vec<Psbt>,
    ) -> Result<Vec<Psbt>, WalletError> {
        let response = self
            .client
            .network_client()
            .submit_offchain_transaction_request(ark_tx, checkpoints)
            .await
            .map_err(wallet_error)?;
        Ok(response.signed_checkpoint_txs)
    }

    async fn finalize_offchain(
        &self,
        ark_txid: Txid,
        checkpoints: Vec<Psbt>,
    ) -> Result<(), WalletError> {
        self.client
            .network_client()
            .finalize_offchain_transaction(ark_txid, checkpoints)
            .await
            .map(|_| ())
            .map_err(wallet_error)
    }

    async fn send(&self, address: &ArkAddress, amount: Amount) -> Result<Txid, WalletError> {
        self.client
            .send(vec![SendReceiver::bitcoin(*address, amount)])
            .await
            .map_err(wallet_error)
    }
}

fn wallet_error(e: impl std::fmt::Display) -> WalletError {
    WalletError(format!("{e:#}"))
}
