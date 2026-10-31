//! The chain source ark-client reads boarding outputs and transaction status from, after `ark-client-sample`'s.

use ark_client::{Blockchain, Error, SpendStatus, TxStatus};
use ark_core::ExplorerUtxo;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Transaction, Txid};
use esplora_client::Tx as EsploraTx;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::{Duration, Instant};

/// Public esplora servers drop kept-alive connections now and then (`IncompleteMessage`) and rate-limit
/// bursts; reads retry.
async fn retry<T, F, Fut>(f: F) -> Result<T, esplora_client::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, esplora_client::Error>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Err(
                e @ (esplora_client::Error::Reqwest(_)
                | esplora_client::Error::HttpResponse { status: 429, .. }),
            ) if attempt < 3 => {
                attempt += 1;
                log::debug!("esplora read failed, retrying: {e}");
                tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
            }
            other => return other,
        }
    }
}

/// A request that gets no answer fails after this long instead of hanging the wallet.
const TIMEOUT_SECS: u64 = 30;
/// How long a read address history stays good.
const HISTORY_TTL: Duration = Duration::from_secs(10);
/// How long a read chain tip stays good: a scan reads it once per address otherwise.
const TIP_TTL: Duration = Duration::from_secs(30);

pub struct Esplora {
    client: esplora_client::AsyncClient,
    tip: std::sync::Mutex<Option<(u32, Instant)>>,
    /// Recent address histories: balance, history and the chores read the same few boarding addresses within
    /// seconds of each other, and mempool.space throttles an address's bursts by refusing connections.
    histories: std::sync::Mutex<HashMap<ScriptBuf, (Vec<EsploraTx>, Instant)>>,
}

impl Esplora {
    pub fn new(url: &str) -> Result<Self, super::errors::ArkadeError> {
        let client = esplora_client::Builder::new(url)
            .timeout(TIMEOUT_SECS)
            .build_async()
            .map_err(super::errors::ArkadeError::internal)?;
        Ok(Self {
            client,
            tip: std::sync::Mutex::new(None),
            histories: std::sync::Mutex::new(HashMap::new()),
        })
    }

    async fn tip(&self) -> Result<u32, esplora_client::Error> {
        if let Some((height, at)) = self.tip.lock().ok().and_then(|t| *t) {
            if at.elapsed() < TIP_TTL {
                return Ok(height);
            }
        }
        let height = retry(|| self.client.get_height()).await?;
        if let Ok(mut tip) = self.tip.lock() {
            *tip = Some((height, Instant::now()));
        }
        Ok(height)
    }
}

impl Esplora {
    async fn history(&self, script: &ScriptBuf) -> Result<Vec<EsploraTx>, esplora_client::Error> {
        if let Some((txs, at)) = self
            .histories
            .lock()
            .ok()
            .and_then(|h| h.get(script).cloned())
        {
            if at.elapsed() < HISTORY_TTL {
                return Ok(txs);
            }
        }
        let txs = retry(|| self.client.scripthash_txs(script, None)).await?;
        if let Ok(mut histories) = self.histories.lock() {
            histories.insert(script.clone(), (txs.clone(), Instant::now()));
        }
        Ok(txs)
    }
}

impl Blockchain for Esplora {
    async fn find_outpoints(&self, address: &Address) -> Result<Vec<ExplorerUtxo>, Error> {
        let tip = self.tip().await.map_err(Error::consumer)?;
        let script_pubkey = address.script_pubkey();
        let txs = self
            .history(&script_pubkey)
            .await
            .map_err(Error::consumer)?;

        let spent: HashSet<OutPoint> = txs
            .iter()
            .flat_map(|tx| {
                tx.vin
                    .iter()
                    .filter(|input| {
                        input
                            .prevout
                            .as_ref()
                            .is_some_and(|p| p.scriptpubkey == script_pubkey)
                    })
                    .map(|input| OutPoint {
                        txid: input.txid,
                        vout: input.vout,
                    })
            })
            .collect();

        Ok(txs
            .into_iter()
            .flat_map(|tx| {
                let txid = tx.txid;
                let confirmations = tx
                    .status
                    .block_height
                    .and_then(|h| tip.checked_sub(h))
                    .map_or(0, |d| d + 1);
                let blocktime = tx.status.block_time;
                tx.vout
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.scriptpubkey == script_pubkey)
                    .map(|(i, v)| {
                        let outpoint = OutPoint {
                            txid,
                            vout: i as u32,
                        };
                        ExplorerUtxo {
                            outpoint,
                            amount: Amount::from_sat(v.value),
                            confirmation_blocktime: blocktime,
                            confirmations: confirmations as u64,
                            is_spent: spent.contains(&outpoint),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    async fn find_tx(&self, txid: &Txid) -> Result<Option<Transaction>, Error> {
        retry(|| self.client.get_tx(txid))
            .await
            .map_err(Error::consumer)
    }

    async fn get_tx_status(&self, txid: &Txid) -> Result<TxStatus, Error> {
        let info = retry(|| self.client.get_tx_info(txid))
            .await
            .map_err(Error::consumer)?;
        Ok(TxStatus {
            confirmed_at: info.and_then(|s| s.status.block_time.map(|t| t as i64)),
        })
    }

    async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<SpendStatus, Error> {
        let status = retry(|| self.client.get_output_status(txid, vout as u64))
            .await
            .map_err(Error::consumer)?;
        Ok(SpendStatus {
            spend_txid: status.and_then(|s| s.txid),
        })
    }

    async fn broadcast(&self, tx: &Transaction) -> Result<(), Error> {
        self.client.broadcast(tx).await.map_err(Error::consumer)
    }

    async fn get_fee_rate(&self) -> Result<f64, Error> {
        let estimates = retry(|| self.client.get_fee_estimates())
            .await
            .map_err(Error::consumer)?;
        Ok(estimates.get(&6).copied().unwrap_or(1.0))
    }

    async fn broadcast_package(&self, txs: &[&Transaction]) -> Result<(), Error> {
        for tx in txs {
            self.broadcast(tx).await?;
        }
        Ok(())
    }
}
