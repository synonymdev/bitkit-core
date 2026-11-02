//! The chain source ark-client reads boarding outputs and transaction status from, after `ark-client-sample`'s.

use ark_client::{Blockchain, Error, SpendStatus, TxStatus};
use ark_core::ExplorerUtxo;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Transaction, Txid};
use esplora_client::Tx as EsploraTx;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Public esplora servers drop kept-alive connections now and then (`IncompleteMessage`); reads retry. A
/// server that times out, refuses the connection or rate-limits isn't retried: the read moves to the next
/// server instead.
async fn retry<T, F, Fut>(f: F) -> Result<T, esplora_client::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, esplora_client::Error>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Err(esplora_client::Error::Reqwest(e)) if e.is_timeout() || e.is_connect() => {
                return Err(esplora_client::Error::Reqwest(e))
            }
            Err(e @ esplora_client::Error::Reqwest(_)) if attempt < 3 => {
                attempt += 1;
                log::debug!("esplora read failed, retrying: {e}");
                tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
            }
            other => return other,
        }
    }
}

/// Whether the server failed rather than the request: another server may answer it.
fn server_failed(e: &esplora_client::Error) -> bool {
    matches!(
        e,
        esplora_client::Error::Reqwest(_)
            | esplora_client::Error::HttpResponse {
                status: 429 | 500..=599,
                ..
            }
    )
}

/// A request that gets no answer fails after this long instead of hanging the wallet.
const TIMEOUT_SECS: u64 = 10;
/// How long a read address history stays good. Public explorers allow a few hundred reads an hour per IP
/// (blockstream.info: 700), and the history watch and chores read the same boarding addresses each pass.
const HISTORY_TTL: Duration = Duration::from_secs(60);
/// How long a read chain tip stays good: a scan reads it once per address otherwise.
const TIP_TTL: Duration = Duration::from_secs(60);

pub struct Esplora {
    /// The servers in order of preference, with their URLs for the log.
    clients: Vec<(String, esplora_client::AsyncClient)>,
    /// The server reads go to first: the last one that answered, so a server that is down costs one timeout,
    /// not one per read.
    current: AtomicUsize,
    tip: std::sync::Mutex<Option<(u32, Instant)>>,
    /// Recent address histories: balance, history and the chores read the same few boarding addresses within
    /// seconds of each other, and mempool.space throttles an address's bursts by refusing connections.
    histories: std::sync::Mutex<HashMap<ScriptBuf, (Vec<EsploraTx>, Instant)>>,
    /// Outputs found spent, and transactions found confirmed: final, so never read again.
    spent: std::sync::Mutex<HashMap<OutPoint, Txid>>,
    confirmed: std::sync::Mutex<HashMap<Txid, i64>>,
}

impl Esplora {
    pub fn new(urls: &[String]) -> Result<Self, super::errors::ArkadeError> {
        let clients = urls
            .iter()
            .map(|url| {
                esplora_client::Builder::new(url)
                    .timeout(TIMEOUT_SECS)
                    .build_async()
                    .map(|client| (url.clone(), client))
                    .map_err(super::errors::ArkadeError::internal)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if clients.is_empty() {
            return Err(super::errors::ArkadeError::internal("no esplora server"));
        }
        Ok(Self {
            clients,
            current: AtomicUsize::new(0),
            tip: std::sync::Mutex::new(None),
            histories: std::sync::Mutex::new(HashMap::new()),
            spent: std::sync::Mutex::new(HashMap::new()),
            confirmed: std::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Runs a read on the current server, then on each other server while they fail; the one that answers
    /// becomes current.
    async fn read<'a, T, F, Fut>(&'a self, f: F) -> Result<T, esplora_client::Error>
    where
        F: Fn(&'a esplora_client::AsyncClient) -> Fut,
        Fut: Future<Output = Result<T, esplora_client::Error>>,
    {
        let start = self.current.load(Ordering::Relaxed);
        let mut failed = None;
        for step in 0..self.clients.len() {
            let i = (start + step) % self.clients.len();
            let (url, client) = &self.clients[i];
            match retry(|| f(client)).await {
                Err(e) if server_failed(&e) => {
                    log::warn!("esplora '{url}' failed: {e}");
                    failed = Some(e);
                }
                other => {
                    if i != start && self.current.swap(i, Ordering::Relaxed) != i {
                        log::info!("esplora reads now go to '{url}'");
                    }
                    return other;
                }
            }
        }
        Err(failed.expect("at least one server"))
    }

    async fn tip(&self) -> Result<u32, esplora_client::Error> {
        if let Some((height, at)) = self.tip.lock().ok().and_then(|t| *t) {
            if at.elapsed() < TIP_TTL {
                return Ok(height);
            }
        }
        let height = self.read(|c| c.get_height()).await?;
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
        let txs = self.read(|c| c.scripthash_txs(script, None)).await?;
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
        self.read(|c| c.get_tx(txid)).await.map_err(Error::consumer)
    }

    async fn get_tx_status(&self, txid: &Txid) -> Result<TxStatus, Error> {
        if let Some(at) = self
            .confirmed
            .lock()
            .ok()
            .and_then(|c| c.get(txid).copied())
        {
            return Ok(TxStatus {
                confirmed_at: Some(at),
            });
        }
        let info = self
            .read(|c| c.get_tx_info(txid))
            .await
            .map_err(Error::consumer)?;
        let confirmed_at = info.and_then(|s| s.status.block_time.map(|t| t as i64));
        if let (Some(at), Ok(mut confirmed)) = (confirmed_at, self.confirmed.lock()) {
            confirmed.insert(*txid, at);
        }
        Ok(TxStatus { confirmed_at })
    }

    async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<SpendStatus, Error> {
        let outpoint = OutPoint { txid: *txid, vout };
        if let Some(spend) = self
            .spent
            .lock()
            .ok()
            .and_then(|s| s.get(&outpoint).copied())
        {
            return Ok(SpendStatus {
                spend_txid: Some(spend),
            });
        }
        let status = self
            .read(|c| c.get_output_status(txid, vout as u64))
            .await
            .map_err(Error::consumer)?;
        let spend_txid = status.and_then(|s| s.txid);
        if let (Some(spend), Ok(mut spent)) = (spend_txid, self.spent.lock()) {
            spent.insert(outpoint, spend);
        }
        Ok(SpendStatus { spend_txid })
    }

    async fn broadcast(&self, tx: &Transaction) -> Result<(), Error> {
        self.read(|c| c.broadcast(tx))
            .await
            .map_err(Error::consumer)
    }

    async fn get_fee_rate(&self) -> Result<f64, Error> {
        let estimates = self
            .read(|c| c.get_fee_estimates())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// An esplora that answers every request with `height`.
    async fn serve_height(height: u32) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 1024];
                let _ = socket.read(&mut request).await;
                let body = height.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        url
    }

    /// A port nothing listens on: connections are refused.
    async fn dead_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    }

    #[tokio::test]
    async fn reads_move_to_the_next_server_when_one_is_down() {
        let esplora = Esplora::new(&[dead_url().await, serve_height(974_269).await]).unwrap();

        assert_eq!(esplora.tip().await.unwrap(), 974_269);
        assert_eq!(esplora.current.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_read_fails_when_every_server_is_down() {
        let esplora = Esplora::new(&[dead_url().await, dead_url().await]).unwrap();

        assert!(esplora.tip().await.is_err());
        assert_eq!(esplora.current.load(Ordering::Relaxed), 0);
    }
}
