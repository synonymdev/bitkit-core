//! The whole module against a local regtest stack (ArkLabsHQ/arkade-regtest, solver in `relay` mode), with test
//! money only: receive into an empty wallet, send, an offline receive to the Lightning address, and a restore.
//!
//! The stack's ports are reached on localhost (7070 arkd, 3000 esplora, 7777 relay, 9090 lnurl-server, 7012
//! delegate). Two shell commands stand in for the other Lightning node; `{}` is replaced:
//!
//!     ARKADE_REGTEST_SOLVER=<discovery pubkey from the stack's solver cards> \
//!     ARKADE_REGTEST_PAY='docker exec ark-lnd-peer lncli --network=regtest payinvoice --force {}' \
//!     ARKADE_REGTEST_INVOICE='docker exec ark-lnd-peer lncli --network=regtest addinvoice --amt {}' \
//!     cargo test --lib modules::arkade::regtest_tests -- --ignored --nocapture --test-threads 1

use super::*;
use rand::RngCore;
use serde_json::Value;
use serial_test::serial;
use std::process::{Command, Stdio};
use std::time::Duration;

const RECEIVE_SAT: u64 = 20_000;
const SEND_SAT: u64 = 5_000;
const OFFLINE_SAT: u64 = 3_000;
const WAIT: Duration = Duration::from_secs(180);

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("set {key}"))
}

fn network() -> ArkadeNetwork {
    ArkadeNetwork::Regtest {
        endpoints: ArkadeRegtestEndpoints {
            ark_server_url: "http://localhost:7070".into(),
            esplora_url: "http://localhost:3000/api".into(),
            lnurl_url: Some("http://localhost:9090".into()),
            delegate_url: Some("http://localhost:7012".into()),
            solver_pubkey: Some(env("ARKADE_REGTEST_SOLVER")),
            solver_relays: vec!["ws://localhost:7777".into()],
        },
    }
}

fn params(
    dir: &tempfile::TempDir,
    mnemonic: &str,
    restore: bool,
    backup: Option<String>,
) -> ArkadeStartParams {
    ArkadeStartParams {
        data_dir: dir.path().to_string_lossy().into_owned(),
        mnemonic: mnemonic.into(),
        passphrase: None,
        network: network(),
        restore,
        backup,
    }
}

/// Runs a template command with `{}` replaced; `wait` false leaves it running (a payment blocks until it settles).
fn shell(template_key: &str, arg: &str, wait: bool) -> String {
    let cmd = env(template_key).replace("{}", arg);
    let mut command = Command::new("sh");
    command.arg("-c").arg(&cmd);
    if !wait {
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        std::thread::spawn(move || child.wait());
        return String::new();
    }
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

async fn wait_for<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = std::time::Instant::now();
    while started.elapsed() < WAIT {
        if check().await {
            println!("{what} after {} s", started.elapsed().as_secs());
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    panic!("{what}: not within {} s", WAIT.as_secs());
}

async fn swap(id: &str) -> ArkadeSwap {
    swaps()
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
#[ignore]
async fn receive_send_offline_receive_and_restore() {
    let mut entropy = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = bip39::Mnemonic::from_entropy(&entropy).unwrap().to_string();

    // Receive into an empty wallet: the payer pays the solver's hold invoice and the wallet claims the lockup.
    let first = tempfile::tempdir().unwrap();
    let info = start(params(&first, &mnemonic, false, None)).await.unwrap();
    assert_eq!(info.network, "regtest");
    assert!(info.ark_address.starts_with("tark1"));
    assert_eq!(balance().await.unwrap().total, 0);
    let received = receive(RECEIVE_SAT).await.unwrap();
    println!(
        "receive: invoice for {} sats, fee {}",
        received.give_sat, received.fee_sat
    );
    shell("ARKADE_REGTEST_PAY", &received.bolt11, false);
    wait_for("receive claimed", || async {
        swap(&received.id).await.outcome == "claimed"
    })
    .await;
    wait_for("balance credited", || async {
        balance().await.unwrap().available > 0
    })
    .await;
    let after_receive = balance().await.unwrap().available;
    assert_eq!(after_receive, received.take_sat);

    // Send: the wallet locks funds for the solver, which pays the invoice and claims with its preimage.
    let invoice: Value = serde_json::from_str(&shell(
        "ARKADE_REGTEST_INVOICE",
        &SEND_SAT.to_string(),
        true,
    ))
    .unwrap();
    let bolt11 = invoice["payment_request"].as_str().unwrap().to_string();
    let quote = quote_pay(bolt11.clone()).await.unwrap();
    println!(
        "send: {} sats for a {} sat invoice",
        quote.give_sat, quote.take_sat
    );
    let sent = pay(bolt11, Some(quote.fee_sat)).await.unwrap();
    wait_for("send paid", || async {
        swap(&sent.id).await.outcome == "paid"
    })
    .await;
    let after_send = balance().await.unwrap().available;
    assert_eq!(after_send, after_receive - quote.give_sat);

    // Offline receive: with the wallet stopped, a payer pays the Lightning address; lnurl-server claims it for the
    // wallet, which finds the output when it starts again.
    let address = lightning_address().await.unwrap();
    println!("lightning address: {:?}", address.lightning_address);
    let before_stop = super::info().await.unwrap();
    let backup = export_state(first.path().to_str().unwrap()).unwrap();
    stop().await;
    let pay_request: Value = reqwest::get(format!(
        "http://localhost:9090/.well-known/lnurlp/{}",
        address.handle
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    // The stack's lnurl-server names its callback by its bare domain, `localhost`, without the published port.
    let callback = pay_request["callback"].as_str().unwrap().replacen(
        "://localhost/",
        "://localhost:9090/",
        1,
    );
    let separator = if callback.contains('?') { '&' } else { '?' };
    let invoice: Value = reqwest::get(format!(
        "{callback}{separator}amount={}",
        OFFLINE_SAT * 1000
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let offline_bolt11 = invoice["pr"].as_str().unwrap().to_string();
    shell("ARKADE_REGTEST_PAY", &offline_bolt11, false);

    // Restore from the phrase and the backup in a fresh directory: same addresses, the funds, and the offline
    // receive.
    let second = tempfile::tempdir().unwrap();
    let restored = start(params(&second, &mnemonic, true, Some(backup)))
        .await
        .unwrap();
    // The wallet hands out a fresh key per receive; a restore finds the used ones and resumes after them.
    assert_eq!(restored.ark_address, before_stop.ark_address);
    assert_eq!(restored.boarding_address, before_stop.boarding_address);
    assert_eq!(lightning_address().await.unwrap(), address);
    wait_for("offline receive found", || async {
        balance().await.unwrap().available > after_send
    })
    .await;
    let after_offline = balance().await.unwrap().available;
    println!("offline receive: {} sats", after_offline - after_send);
    assert!(swaps()
        .await
        .unwrap()
        .iter()
        .any(|s| s.id == sent.id && s.outcome == "paid"));
    wipe(second.path().to_str().unwrap()).await.unwrap();
}
