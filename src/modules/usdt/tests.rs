use super::*;

#[test]
fn account_and_signatures_match_entrypoint_v8_reference() {
    use alloy_primitives::{Address, Bytes, B256};
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/eip7702-vectors.json")).unwrap();
    let address: Address = vector["address"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        usdt_address(TEST_PHRASE.into(), None).unwrap(),
        address.to_checksum(None)
    );
    let mut op: user_operation::UserOperation =
        serde_json::from_value(vector["operation"].clone()).unwrap();
    let calls: Vec<(Address, Bytes)> = vector["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| {
            (
                call["to"].as_str().unwrap().parse().unwrap(),
                call["data"].as_str().unwrap().parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(account::batch(&calls), op.call_data);
    let expected_hash: B256 = vector["userOperationHash"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(op.hash(42161).unwrap(), expected_hash);
    assert_eq!(
        op.eip7702_auth.hash().unwrap(),
        vector["authorizationHash"]
            .as_str()
            .unwrap()
            .parse::<B256>()
            .unwrap()
    );
    let key = keys::derive_key(TEST_PHRASE.to_owned().into(), None).unwrap();
    assert_eq!(op.sign(&key, 42161).unwrap(), expected_hash);
    assert_eq!(
        op.signature,
        vector["signature"]
            .as_str()
            .unwrap()
            .parse::<Bytes>()
            .unwrap()
    );
    assert_eq!(
        op.eip7702_auth,
        serde_json::from_value(vector["operation"]["eip7702Auth"].clone()).unwrap()
    );
    assert_ne!(op.hash(42161).unwrap(), op.hash(1).unwrap());
    op.sender = RECIPIENT.parse().unwrap();
    assert!(matches!(
        op.sign(&key, 42161),
        Err(UsdtError::InvalidCredentials)
    ));
}

#[test]
fn amount_round_trips_without_rounding() {
    for (text, amount) in [
        ("0", 0),
        ("0.000001", 1),
        ("12.345678", 12_345_678),
        ("18446744073709.551615", u64::MAX),
    ] {
        assert_eq!(usdt_parse_amount(text.into()).unwrap(), amount);
        assert_eq!(usdt_format_amount(amount), text);
    }
    for invalid in [
        "-1",
        "+1",
        "1e6",
        "1.0000001",
        "18446744073709.551616",
        "1,000",
        ".",
        "",
    ] {
        assert!(usdt_parse_amount(invalid.into()).is_err());
    }
}

#[test]
fn wallet_requires_both_provider_endpoints() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("usdt.sqlite");
    for (rpc, bundler) in [
        ("", "https://provider.example"),
        ("https://provider.example", ""),
        ("not a url", "https://provider.example"),
        (
            "https://provider.example",
            "https://example.com?api-key=secret",
        ),
    ] {
        assert!(matches!(
            UsdtWallet::new(
                RECIPIENT.into(),
                path.to_string_lossy().into(),
                rpc.into(),
                bundler.into()
            ),
            Err(UsdtError::NotConfigured)
        ));
    }
    assert!(!path.exists());
}

#[test]
fn address_matches_standard_ethereum_recovery_path() {
    let phrase = "test test test test test test test test test test test junk";
    assert_eq!(
        usdt_address(phrase.into(), None).unwrap(),
        "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    );
    assert_ne!(
        usdt_address(phrase.into(), Some("secret".into())).unwrap(),
        usdt_address(phrase.into(), None).unwrap()
    );
}

#[test]
fn payment_request_rejects_wrong_chain_and_malformed_checksum() {
    let address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    assert_eq!(
        usdt_parse_payment_request(format!("ethereum:{address}@42161"))
            .unwrap()
            .recipient,
        address
    );
    let request = format!(
        "ethereum:{}@42161/transfer?address={address}",
        types::TOKEN.to_checksum(None)
    );
    assert_eq!(
        usdt_parse_payment_request(request).unwrap().recipient,
        address
    );
    for invalid in [
        format!("ethereum:{address}@42161/transfer?address={address}"),
        format!("ethereum:{address}@1"),
        format!("ethereum:{address}@42161?value=1"),
        "0x0000000000000000000000000000000000000000".into(),
        "0xF39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into(),
    ] {
        assert!(usdt_parse_payment_request(invalid).is_err());
    }
}

#[test]
fn payment_requests_preserve_exact_token_amounts_and_reject_ambiguous_terms() {
    let uri = |query: &str| {
        format!(
            "ethereum:pay-{}@42161/transfer?{query}",
            types::TOKEN.to_checksum(None)
        )
    };
    for (encoded, expected) in [
        ("1", 1),
        ("1500000", 1_500_000),
        ("1.5e6", 1_500_000),
        (".5E6", 500_000),
        ("+1.0000000e6", 1_000_000),
        ("%2B1e6", 1_000_000),
        ("0", 0),
        (".0", 0),
        ("18446744073709551615", u64::MAX),
    ] {
        let value = uri(&format!("uint256={encoded}&address={RECIPIENT}"));
        let request = usdt_parse_payment_request(value).unwrap();
        assert_eq!(request.recipient, RECIPIENT);
        assert_eq!(request.amount, Some(expected));
    }
    for value in [RECIPIENT.into(), uri(&format!("address={RECIPIENT}"))] {
        assert_eq!(usdt_parse_payment_request(value).unwrap().amount, None);
    }
    for amount in [
        "-1",
        "1.01",
        "1e-1",
        "18446744073709551616",
        "1e100000",
        "1e",
        "NaN",
        "",
        "1.2.3",
    ] {
        assert!(
            usdt_parse_payment_request(uri(&format!("address={RECIPIENT}&uint256={amount}")))
                .is_err()
        );
    }
    for query in [
        format!("address={RECIPIENT}&address={RECIPIENT}"),
        format!("address={RECIPIENT}&uint256=1&uint256=2"),
        format!("address={RECIPIENT}&value=1"),
        format!("address={RECIPIENT}&data=0x00"),
        "uint256=1000000".into(),
    ] {
        assert!(usdt_parse_payment_request(uri(&query)).is_err());
    }
    assert!(usdt_parse_payment_request(
        uri(&format!("address={RECIPIENT}&uint256=1")).replace("@42161", "@1")
    )
    .is_err());
}

const TEST_PHRASE: &str = "test test test test test test test test test test test junk";
const RECIPIENT: &str = "0x1111111111111111111111111111111111111111";

struct MockChain {
    url: String,
    state: std::sync::Arc<std::sync::Mutex<ChainState>>,
    task: tokio::task::JoinHandle<()>,
}
struct ChainState {
    chain: u64,
    account_code: alloy_primitives::Bytes,
    authorization_nonce: u64,
    paymaster_valid_until: Option<u64>,
    pre_verification_estimates: std::collections::VecDeque<u64>,
    gas_price: u64,
    external_outgoing: bool,
    nonce: u64,
    balance: alloy_primitives::U256,
    operations: Vec<user_operation::UserOperation>,
    mined: bool,
    tip: u64,
    timestamp: alloy_primitives::U256,
    reject_broadcast: bool,
    delay_gas_estimate: bool,
    paymaster: alloy_primitives::Address,
    history_input: Option<alloy_primitives::Bytes>,
    history_target: Option<alloy_primitives::Address>,
    receipt_logs: Option<Vec<serde_json::Value>>,
    incoming: bool,
    hide_logs: bool,
    hide_receipts: bool,
    receipt_failure: Option<alloy_primitives::B256>,
    oversized_block: Option<u64>,
    receipt_padding: usize,
    log_response: Option<Vec<serde_json::Value>>,
    block_transactions: Option<Vec<alloy_primitives::B256>>,
    replacement_block: Option<u64>,
    log_error: Option<(i64, String)>,
    max_log_range: Option<u64>,
    oversized_logs: bool,
    log_requests: usize,
    incoming_count: u64,
    block_reads: usize,
    fail_block_read_at: Option<usize>,
    authorization_change_on_block_read: bool,
    receipt_reads: usize,
    receipt_response: Option<serde_json::Value>,
}
impl Drop for MockChain {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl MockChain {
    async fn start() -> Self {
        use alloy_primitives::U256;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = std::sync::Arc::new(std::sync::Mutex::new(ChainState {
            chain: 42161,
            account_code: alloy_primitives::Bytes::new(),
            authorization_nonce: 0,
            paymaster_valid_until: None,
            pre_verification_estimates: Default::default(),
            gas_price: 50_000_000,
            external_outgoing: false,
            nonce: 0,
            balance: U256::from(10_000_000),
            operations: vec![],
            mined: false,
            tip: 20000,
            timestamp: U256::from(wallet::now()),
            reject_broadcast: false,
            delay_gas_estimate: false,
            paymaster: paymaster::PAYMASTER,
            history_input: None,
            history_target: Some(account::ENTRY_POINT),
            receipt_logs: None,
            incoming: false,
            hide_logs: false,
            hide_receipts: false,
            receipt_failure: None,
            oversized_block: None,
            receipt_padding: 0,
            log_response: None,
            block_transactions: None,
            replacement_block: None,
            log_error: None,
            max_log_range: None,
            oversized_logs: false,
            log_requests: 0,
            incoming_count: 0,
            block_reads: 0,
            fail_block_read_at: None,
            authorization_change_on_block_read: false,
            receipt_reads: 0,
            receipt_response: None,
        }));
        let server_state = state.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0u8; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break 0;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(p) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                if header_end == 0 {
                    continue;
                }
                let length: usize = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                while request.len() < header_end + length {
                    let mut chunk = [0u8; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                }
                let body: serde_json::Value =
                    serde_json::from_slice(&request[header_end..]).unwrap();
                let path = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let method = body["method"].as_str().unwrap();
                let bundler = matches!(
                    method,
                    "pimlico_getTokenQuotes"
                        | "pimlico_getUserOperationGasPrice"
                        | "pm_getPaymasterData"
                        | "pm_getPaymasterStubData"
                        | "eth_estimateUserOperationGas"
                        | "eth_sendUserOperation"
                );
                if path == "/chain" {
                    assert!(!bundler, "Bundler method on chain endpoint");
                }
                if path == "/bundler" {
                    assert!(
                        bundler || method == "eth_chainId",
                        "Chain method on bundler endpoint"
                    );
                }
                let delay = body["method"] == "eth_estimateUserOperationGas"
                    && std::mem::take(&mut server_state.lock().unwrap().delay_gas_estimate);
                if delay {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                }
                let response = server_state.lock().unwrap().respond(&body).to_string();
                let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len());
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self { url, state, task }
    }
    fn wallet(&self, dir: &tempfile::TempDir) -> std::sync::Arc<UsdtWallet> {
        UsdtWallet::new(
            usdt_address(TEST_PHRASE.into(), None).unwrap(),
            dir.path().join("usdt.sqlite").to_string_lossy().into(),
            format!("{}/chain", self.url),
            format!("{}/bundler", self.url),
        )
        .unwrap()
    }
}
impl ChainState {
    fn respond(&mut self, body: &serde_json::Value) -> serde_json::Value {
        use alloy_primitives::{Bytes, U256};
        use alloy_sol_types::{SolCall, SolValue};
        use serde_json::json;
        if matches!(
            body["method"].as_str(),
            Some("eth_estimateUserOperationGas" | "eth_sendUserOperation")
        ) {
            let op = &body["params"][0];
            let auth = &op["eip7702Auth"];
            // Alto requires authorization with the marker, even for an existing delegation.
            if op["factory"] != "0x7702"
                || auth.is_null()
                || serde_json::from_value::<U256>(auth["nonce"].clone()).ok()
                    != Some(U256::from(self.authorization_nonce))
            {
                return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Invalid EIP-7702 authorization"}});
            }
        }
        let result = match body["method"].as_str().unwrap() {
            "eth_chainId" => json!(U256::from(self.chain)),
            "eth_blockNumber" => json!(U256::from(self.tip)),
            "eth_getBlockByNumber" => {
                self.block_reads += 1;
                if std::mem::take(&mut self.authorization_change_on_block_read) {
                    self.authorization_nonce += 1;
                }
                if self.fail_block_read_at == Some(self.block_reads) {
                    self.fail_block_read_at = None;
                    return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"temporarily unavailable"}});
                }
                json!({"hash":alloy_primitives::B256::repeat_byte(9),"timestamp":self.timestamp,"transactions":self.block_transactions.clone().unwrap_or_else(|| vec![alloy_primitives::B256::repeat_byte(7)])})
            }
            "eth_getCode" => json!(self.account_code),
            "eth_getTransactionCount" => json!(U256::from(self.authorization_nonce)),
            "eth_call" => {
                let data: Bytes =
                    serde_json::from_value(body["params"][0]["data"].clone()).unwrap();
                let encoded = if data.starts_with(&transaction::EntryPoint::getNonceCall::SELECTOR)
                {
                    let block = serde_json::from_value::<U256>(body["params"][1].clone()).ok();
                    let nonce = if self
                        .replacement_block
                        .zip(block)
                        .is_some_and(|(mined, at)| at < U256::from(mined))
                    {
                        0
                    } else {
                        self.nonce
                    };
                    U256::from(nonce).abi_encode()
                } else {
                    self.balance.abi_encode()
                };
                json!(Bytes::from(encoded))
            }
            "pimlico_getTokenQuotes" => {
                json!({"quotes":[{"token":types::TOKEN,"paymaster":self.paymaster,"postOpGas":"0xc350","exchangeRate":U256::from(3_000_000_000u64)}]})
            }
            "pimlico_getUserOperationGasPrice" => {
                json!({"slow":{"maxFeePerGas":U256::from(self.gas_price * 9 / 10),"maxPriorityFeePerGas":U256::from(100000)},"fast":{"maxFeePerGas":U256::from(self.gas_price),"maxPriorityFeePerGas":U256::from(100000)}})
            }
            "pm_getPaymasterData" | "pm_getPaymasterStubData" => {
                let mut data = vec![0; 118];
                data[0] = 3;
                if body["method"] == "pm_getPaymasterData" {
                    data[2..8].copy_from_slice(
                        &self
                            .paymaster_valid_until
                            .unwrap_or(u64::try_from(self.timestamp).unwrap() + 600)
                            .to_be_bytes()[2..],
                    );
                }
                data[14..34].copy_from_slice(types::TOKEN.as_slice());
                data[34..50].copy_from_slice(&50_000u128.to_be_bytes());
                data[50..82].copy_from_slice(&U256::from(3_000_000_000u64).to_be_bytes::<32>());
                data.extend_from_slice(&[1; 65]);
                let mut response =
                    json!({"paymaster":self.paymaster,"paymasterData":Bytes::from(data)});
                if body["method"] == "pm_getPaymasterStubData" {
                    response["paymasterPostOpGasLimit"] = json!("0x186a0");
                }
                response
            }
            "eth_estimateUserOperationGas" => {
                let data: Bytes =
                    serde_json::from_value(body["params"][0]["paymasterData"].clone()).unwrap();
                let verification_gas = if data[2..8].iter().any(|byte| *byte != 0) {
                    "0x13880"
                } else {
                    "0xc350"
                };
                let pre_verification_gas = self
                    .pre_verification_estimates
                    .pop_front()
                    .unwrap_or(80_000);
                json!({"callGasLimit":"0x249f0","verificationGasLimit":"0x3d090","preVerificationGas":U256::from(pre_verification_gas),"paymasterVerificationGasLimit":verification_gas,"paymasterPostOpGasLimit":"0xc350"})
            }
            "eth_sendUserOperation" => {
                let op: user_operation::UserOperation =
                    serde_json::from_value(body["params"][0].clone()).unwrap();
                let hash = op.hash(types::CHAIN_ID).unwrap();
                self.operations.push(op);
                if self.reject_broadcast {
                    return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"temporarily unavailable"}});
                }
                json!(hash)
            }
            "eth_getLogs" => {
                self.log_requests += 1;
                if let Some((code, message)) = &self.log_error {
                    return json!({"jsonrpc":"2.0","id":1,"error":{"code":code,"message":message}});
                }
                let filter = &body["params"][0];
                let from: U256 = serde_json::from_value(filter["fromBlock"].clone()).unwrap();
                let to: U256 = serde_json::from_value(filter["toBlock"].clone()).unwrap();
                if self
                    .oversized_block
                    .is_some_and(|block| from <= U256::from(block) && to >= U256::from(block))
                {
                    return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"Log query limit exceeded"}});
                }
                if let Some(logs) = &self.log_response {
                    return json!({"jsonrpc":"2.0","id":1,"result":logs});
                }
                if self
                    .max_log_range
                    .is_some_and(|limit| to - from + U256::from(1) > U256::from(limit))
                {
                    if self.oversized_logs {
                        return json!({"jsonrpc":"2.0","id":1,"result":[],"padding":" ".repeat(2_097_152)});
                    }
                    return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"Log query limit exceeded"}});
                }
                if let Some(block) = self.replacement_block {
                    if !self.hide_logs
                        && from <= U256::from(block)
                        && to >= U256::from(block)
                        && (filter["address"] == json!(account::ENTRY_POINT)
                            || filter["address"].is_array())
                        && filter["topics"][1].is_null()
                    {
                        use alloy_sol_types::SolEvent;
                        let event = transaction::EntryPoint::UserOperationEvent {
                            userOpHash: alloy_primitives::B256::repeat_byte(0xbb),
                            sender: self.operations[0].sender,
                            paymaster: self.paymaster,
                            nonce: U256::ZERO,
                            success: true,
                            actualGasCost: U256::from(1),
                            actualGasUsed: U256::from(1),
                        }
                        .encode_log_data();
                        return json!({"jsonrpc":"2.0","id":1,"result":[{"address":account::ENTRY_POINT,"topics":event.topics(),"data":event.data,"transactionHash":alloy_primitives::B256::repeat_byte(8),"blockNumber":U256::from(block),"logIndex":"0x1"}]});
                    }
                }
                if self.external_outgoing
                    && filter["address"] == json!(types::TOKEN)
                    && !filter["topics"][1].is_null()
                    && from <= U256::from(20000)
                    && to >= U256::from(20000)
                {
                    return json!({"jsonrpc":"2.0","id":1,"result":[self.event_logs().last().unwrap()]});
                }
                if self.incoming_count > 0
                    && (filter["address"] == json!(types::TOKEN) || filter["address"].is_array())
                    && !filter["topics"][2].is_null()
                {
                    use alloy_sol_types::SolEvent;
                    let event = transaction::Erc20::Transfer {
                        from: alloy_primitives::Address::repeat_byte(8),
                        to: self.operations[0].sender,
                        value: U256::from(42),
                    }
                    .encode_log_data();
                    let logs: Vec<_> = (1..=self.incoming_count).filter(|index| U256::from(20000 + index) >= from && U256::from(20000 + index) <= to).map(|index| {
                        json!({"address":types::TOKEN,"topics":event.topics(),"data":event.data,"transactionHash":format!("0x{index:064x}"),"blockNumber":U256::from(20000+index),"logIndex":"0x0"})
                    }).collect();
                    return json!({"jsonrpc":"2.0","id":1,"result":logs});
                }
                if self.mined
                    && !self.hide_logs
                    && from <= U256::from(20000)
                    && to >= U256::from(20000)
                    && (filter["address"] == json!(account::ENTRY_POINT)
                        || filter["address"].is_array())
                {
                    json!([self.event_logs()[1]])
                } else {
                    json!([])
                }
            }
            "eth_getTransactionReceipt" => {
                self.receipt_reads += 1;
                if let Some(response) = &self.receipt_response {
                    return json!({"jsonrpc":"2.0","id":1,"result":response});
                }
                if self.hide_receipts
                    || self
                        .receipt_failure
                        .is_some_and(|hash| body["params"][0] == json!(hash))
                {
                    return json!({"jsonrpc":"2.0","id":1,"result":null});
                }
                json!({"transactionHash":body["params"][0],"blockHash":alloy_primitives::B256::repeat_byte(9),"blockNumber":"0x4e20","logs":self.receipt_logs.clone().unwrap_or_else(|| self.event_logs()),"padding":" ".repeat(self.receipt_padding)})
            }
            "eth_getTransactionByHash" => {
                let op = &self.operations[0];
                let input = transaction::EntryPoint::handleOpsCall {
                    ops: vec![transaction::PackedOperation {
                        sender: op.sender,
                        nonce: op.nonce,
                        // EntryPoint's onchain marker is right-padded to 20 bytes.
                        initCode: Bytes::from_static(&alloy_primitives::hex!(
                            "7702000000000000000000000000000000000000"
                        )),
                        callData: op.call_data.clone(),
                        accountGasLimits: Default::default(),
                        preVerificationGas: op.pre_verification_gas,
                        gasFees: Default::default(),
                        paymasterAndData: op.paymaster_and_data().unwrap(),
                        signature: op.signature.clone(),
                    }],
                    beneficiary: alloy_primitives::Address::repeat_byte(9),
                }
                .abi_encode();
                json!({"to":self.history_target,"input":self.history_input.clone().unwrap_or_else(|| Bytes::from(input))})
            }
            method => panic!("Unexpected RPC method {method}"),
        };
        json!({"jsonrpc":"2.0","id":1,"result":result})
    }
    fn event_logs(&self) -> Vec<serde_json::Value> {
        use alloy_primitives::{B256, U256};
        use alloy_sol_types::SolEvent;
        use serde_json::json;
        let op = self.operations.first().unwrap();
        let hash = op.hash(types::CHAIN_ID).unwrap();
        let gas = transaction::Paymaster::UserOperationSponsored {
            userOpHash: hash,
            user: op.sender,
            paymasterMode: 1,
            token: types::TOKEN,
            tokenAmountPaid: U256::from(123),
            exchangeRate: U256::from(3_000_000_000u64),
        }
        .encode_log_data();
        let event = transaction::EntryPoint::UserOperationEvent {
            userOpHash: hash,
            sender: op.sender,
            paymaster: paymaster::PAYMASTER,
            nonce: op.nonce,
            success: true,
            actualGasCost: U256::from(1),
            actualGasUsed: U256::from(1),
        }
        .encode_log_data();
        let mut logs = vec![
            json!({"address":paymaster::PAYMASTER,"topics":gas.topics(),"data":gas.data,"transactionHash":B256::repeat_byte(7),"blockNumber":"0x4e20","logIndex":"0x0"}),
            json!({"address":account::ENTRY_POINT.to_checksum(None),"topics":event.topics(),"data":event.data,"transactionHash":B256::repeat_byte(7),"blockNumber":"0x4e20","logIndex":"0x1"}),
        ];
        if !self.mined {
            logs.clear();
        }
        if self.incoming {
            let event = transaction::Erc20::Transfer {
                from: alloy_primitives::Address::repeat_byte(8),
                to: op.sender,
                value: U256::from(42),
            }
            .encode_log_data();
            logs.push(json!({"address":types::TOKEN,"topics":event.topics(),"data":event.data,"logIndex":"0x2"}));
        }
        if self.external_outgoing {
            let event = transaction::Erc20::Transfer {
                from: op.sender,
                to: RECIPIENT.parse().unwrap(),
                value: U256::from(77),
            }
            .encode_log_data();
            logs.push(json!({"address":types::TOKEN,"topics":event.topics(),"data":event.data,"transactionHash":B256::repeat_byte(7),"blockNumber":"0x4e20","logIndex":"0x3"}));
        }
        logs
    }
}

#[tokio::test]
async fn signed_operation_survives_uncertain_broadcast_and_restart() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        wallet.quote_transfer(RECIPIENT.into(), 1_000_000),
    )
    .await
    .expect("An idle wallet must quote without a fixed per-request delay")
    .unwrap();
    let next = wallet
        .quote_transfer(RECIPIENT.into(), 2_000_000)
        .await
        .unwrap();
    chain.state.lock().unwrap().reject_broadcast = true;
    let sent = wallet
        .send(quote.id.clone(), TEST_PHRASE.into(), None)
        .await
        .unwrap();
    assert_eq!(sent.status, UsdtTransferStatus::Pending);
    assert!(sent.tx_hash.is_empty());
    let op = chain.state.lock().unwrap().operations[0].clone();
    assert_eq!(op.sender.to_checksum(None), wallet.receive_address());
    assert_eq!(
        op.paymaster_verification_gas_limit,
        alloy_primitives::U256::from(80_000)
    );
    assert_eq!(
        op.paymaster_post_op_gas_limit,
        alloy_primitives::U256::from(100_000)
    );
    drop(wallet);
    let wallet = chain.wallet(&dir);
    assert_eq!(
        wallet
            .send(quote.id, TEST_PHRASE.into(), None)
            .await
            .unwrap()
            .user_operation_hash,
        sent.user_operation_hash
    );
    // A pending payment is rejected locally even if the provider is now misconfigured.
    chain.state.lock().unwrap().chain = 1;
    assert!(matches!(
        wallet.quote_transfer(RECIPIENT.into(), 2_000_000).await,
        Err(UsdtError::PendingTransfer)
    ));
    assert!(matches!(
        wallet.send(next.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::PendingTransfer)
    ));
    chain.state.lock().unwrap().chain = 42161;
    chain.state.lock().unwrap().tip += 3;
    wallet.refresh_transfers().await.unwrap();
    assert!(chain
        .state
        .lock()
        .unwrap()
        .operations
        .iter()
        .all(|item| serde_json::to_value(item).unwrap() == serde_json::to_value(&op).unwrap()));
    chain.state.lock().unwrap().mined = true;
    chain.state.lock().unwrap().timestamp += alloy_primitives::U256::from(60);
    let history = wallet.refresh_transfers().await.unwrap();
    assert_eq!(
        history[0].timestamp,
        u64::try_from(chain.state.lock().unwrap().timestamp).unwrap()
    );
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    assert_eq!(history[0].fee, Some(123));
    assert!(!history[0].tx_hash.is_empty());
}

#[tokio::test]
async fn fluctuating_gas_estimates_preserve_the_reviewed_operation() {
    use alloy_primitives::U256;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    chain.state.lock().unwrap().pre_verification_estimates =
        [80_000, 80_100, 80_200, 80_300].into();
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let reviewed = wallet.store.quote(&quote.id).unwrap().plan.operation;
    chain.state.lock().unwrap().pre_verification_estimates = [100_000].into();
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), None)
            .await,
        Err(UsdtError::QuoteExpired)
    ));
    assert!(wallet.history().unwrap().is_empty());
    assert!(chain.state.lock().unwrap().operations.is_empty());

    chain.state.lock().unwrap().pre_verification_estimates = [80_400].into();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let signed = chain.state.lock().unwrap().operations[0].clone();
    assert_eq!(
        signed.hash(types::CHAIN_ID).unwrap(),
        reviewed.hash(types::CHAIN_ID).unwrap()
    );
    assert!(signed.pre_verification_gas >= U256::from(80_400));
    assert!(signed.pre_verification_gas < U256::from(100_000));
}

#[tokio::test]
async fn wrong_network_owner_nonce_balance_and_paymaster_cannot_sign() {
    use alloy_primitives::{Address, U256};
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    for amount in ["", "&uint256=2000000"] {
        let request = format!(
            "ethereum:{}@42161/transfer?address={RECIPIENT}{amount}",
            types::TOKEN.to_checksum(None)
        );
        assert!(matches!(
            wallet.quote_transfer(request, 1_000_000).await,
            Err(UsdtError::InvalidAddress)
        ));
    }
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), Some("wrong".into()))
            .await,
        Err(UsdtError::InvalidCredentials)
    ));
    chain.state.lock().unwrap().chain = 1;
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), None)
            .await,
        Err(UsdtError::WrongNetwork)
    ));
    {
        let mut state = chain.state.lock().unwrap();
        state.chain = 42161;
        state.nonce = 1;
    }
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), None)
            .await,
        Err(UsdtError::QuoteExpired)
    ));
    {
        let mut state = chain.state.lock().unwrap();
        state.nonce = 0;
        state.balance = U256::ZERO;
    }
    assert!(matches!(
        wallet.send(quote.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::InsufficientBalance)
    ));
    {
        let mut state = chain.state.lock().unwrap();
        state.balance = U256::from(10_000_000);
        state.paymaster = Address::repeat_byte(1);
    }
    assert!(matches!(
        wallet.quote_transfer(RECIPIENT.into(), 1_000_000).await,
        Err(UsdtError::UnsupportedRoute)
    ));
    assert!(chain.state.lock().unwrap().operations.is_empty());
}

#[tokio::test]
async fn expired_unmined_operation_releases_nonce_for_a_new_approval() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.timestamp += alloy_primitives::U256::from(180);
        state.tip += 3;
        state.max_log_range = Some(1);
    }
    assert_eq!(
        wallet.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert!(matches!(
        wallet.quote_transfer(RECIPIENT.into(), 1_000_000).await,
        Err(UsdtError::PendingTransfer)
    ));
    chain.state.lock().unwrap().timestamp += alloy_primitives::U256::from(421);
    assert_eq!(
        wallet.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Failed
    );
    let next = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    assert_eq!(
        wallet
            .send(next.id, TEST_PHRASE.into(), None)
            .await
            .unwrap()
            .status,
        UsdtTransferStatus::Pending
    );
}

#[tokio::test]
async fn seed_restore_recovers_mined_payments_without_local_submission_data() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
    }
    let restored_dir = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_dir);
    assert_eq!(restored.receive_address(), wallet.receive_address());
    sync_history_to_tip(&restored).await;
    let history = restored.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].user_operation_hash, sent.user_operation_hash);
    assert_eq!(history[0].recipient, RECIPIENT);
    assert_eq!(history[0].amount, 1_000_000);
    assert_eq!(history[0].fee, Some(123));
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    sync_history_to_tip(&restored).await;
    assert_eq!(restored.history().unwrap().len(), 1);
}

#[tokio::test]
async fn bundled_operations_cannot_contribute_another_payments_fee() {
    use alloy_primitives::{B256, U256};
    use alloy_sol_types::SolEvent;
    use serde_json::json;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let mut transfer = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let own_hash = transfer
        .user_operation_hash
        .as_ref()
        .unwrap()
        .parse()
        .unwrap();
    let other_hash = B256::repeat_byte(2);
    let event = |hash| {
        transaction::EntryPoint::UserOperationEvent {
            userOpHash: hash,
            sender: wallet.address,
            paymaster: paymaster::PAYMASTER,
            nonce: U256::ZERO,
            success: true,
            actualGasCost: U256::from(1),
            actualGasUsed: U256::from(1),
        }
        .encode_log_data()
    };
    let log = |address, data: alloy_primitives::LogData| json!({"address":address,"topics":data.topics(),"data":data.data});
    let receipt = json!({"logs":[
        log(paymaster::PAYMASTER, transaction::Paymaster::UserOperationSponsored { userOpHash:other_hash, user:wallet.address, paymasterMode:1, token:types::TOKEN, tokenAmountPaid:U256::from(500), exchangeRate:U256::from(1) }.encode_log_data()),
        log(account::ENTRY_POINT, event(other_hash)),
        log(paymaster::PAYMASTER, transaction::Paymaster::UserOperationSponsored { userOpHash:own_hash, user:wallet.address, paymasterMode:1, token:types::TOKEN, tokenAmountPaid:U256::from(123), exchangeRate:U256::from(1) }.encode_log_data()),
        log(account::ENTRY_POINT, event(own_hash)),
    ]});
    assert_eq!(
        transaction::operation_logs(&receipt, own_hash).unwrap(),
        &receipt["logs"].as_array().unwrap()[2..]
    );
    wallet.settle(&mut transfer, &receipt, false).unwrap();
    assert_eq!(transfer.status, UsdtTransferStatus::Failed);
    assert_eq!(transfer.fee, Some(123));
    wallet.settle(&mut transfer, &receipt, true).unwrap();
    assert_eq!(transfer.status, UsdtTransferStatus::Confirmed);
    assert_eq!(transfer.fee, Some(123));
}

#[tokio::test]
#[ignore = "requires a fresh local Arbitrum fork and tests/usdt-fork/provider.mjs"]
async fn deployed_contracts_collect_usdt_fees_without_account_eth() {
    use alloy_primitives::U256;
    use serde_json::json;
    let rpc = rpc::Rpc::new("http://127.0.0.1:18545".into(), types::CHAIN_ID).unwrap();
    let client: String = rpc.call("web3_clientVersion", json!([])).await.unwrap();
    assert!(client.to_lowercase().contains("anvil"));
    let dir = tempfile::tempdir().unwrap();
    let wallet = UsdtWallet::new(
        usdt_address(TEST_PHRASE.into(), None).unwrap(),
        dir.path().join("usdt.sqlite").to_string_lossy().into(),
        std::env::var("USDT_FORK_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:18545".into()),
        std::env::var("USDT_FORK_BUNDLER_URL").unwrap_or_else(|_| "http://127.0.0.1:18546".into()),
    )
    .unwrap();
    assert_eq!(
        rpc.call::<U256>("eth_getBalance", json!([wallet.address, "pending"]))
            .await
            .unwrap(),
        U256::ZERO
    );
    let initial = wallet.balance().await.unwrap();
    // Only locally mined transactions belong to this fixture's history.
    wallet
        .store
        .complete_history(wallet.block_number().await.unwrap())
        .unwrap();
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let history = wallet.refresh_transfers().await.unwrap();
    let transfer = history.iter().find(|t| t.id == sent.id).unwrap();
    assert_eq!(transfer.status, UsdtTransferStatus::Confirmed);
    let fee = transfer.fee.unwrap();
    assert!(fee > 0 && fee <= quote.maximum_fee);
    assert_eq!(wallet.balance().await.unwrap(), initial - 1_000_000 - fee);
    assert_eq!(
        rpc.call::<U256>("eth_getBalance", json!([wallet.address, "pending"]))
            .await
            .unwrap(),
        U256::ZERO
    );
}

#[tokio::test]
async fn history_preserves_receipts_with_external_account_call_shapes() {
    use alloy_primitives::{Bytes, U256};
    use alloy_sol_types::SolCall;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let original = chain.state.lock().unwrap().operations[0].call_data.clone();
    let oversized = account::batch(&[(
        types::TOKEN,
        transaction::Erc20::transferCall {
            recipient: RECIPIENT.parse().unwrap(),
            amount: U256::MAX,
        }
        .abi_encode()
        .into(),
    )]);
    let single = account::SimpleAccount::executeCall {
        target: types::TOKEN,
        value: U256::ZERO,
        data: transaction::Erc20::transferCall {
            recipient: RECIPIENT.parse().unwrap(),
            amount: U256::from(1_000_000),
        }
        .abi_encode()
        .into(),
    }
    .abi_encode()
    .into();
    for (call_data, outer, expected_outgoing) in [
        (original.clone(), None, true),
        (single, None, true),
        (oversized, None, false),
        (Bytes::from_static(&[1, 2, 3, 4]), None, false),
        (original, Some(Bytes::from_static(&[5, 6, 7, 8])), false),
    ] {
        {
            let mut state = chain.state.lock().unwrap();
            state.mined = true;
            state.incoming = true;
            state.tip = 20003;
            state.operations[0].call_data = call_data;
            state.history_input = outer;
        }
        let restored_dir = tempfile::tempdir().unwrap();
        let restored = chain.wallet(&restored_dir);
        sync_history_to_tip(&restored).await;
        let history = restored.history().unwrap();
        assert_eq!(
            history
                .iter()
                .filter(|t| t.is_incoming && t.amount == 42)
                .count(),
            1
        );
        assert_eq!(
            history.iter().filter(|t| !t.is_incoming).count(),
            usize::from(expected_outgoing)
        );
        sync_history_to_tip(&restored).await;
        assert_eq!(restored.history().unwrap().len(), history.len());
    }
}

#[tokio::test]
async fn wrapped_history_preserves_signed_payments_and_restores_token_transfers() {
    use alloy_primitives::{Address, Bytes, U256};
    use alloy_sol_types::{SolCall, SolEvent};
    use serde_json::json;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let decoy_input = {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
        let tx = state.respond(&json!({"method":"eth_getTransactionByHash"}));
        let input: Bytes = serde_json::from_value(tx["result"]["input"].clone()).unwrap();
        let mut batch = transaction::EntryPoint::handleOpsCall::abi_decode(&input).unwrap();
        batch.ops[0].callData = account::batch(&[(
            types::TOKEN,
            transaction::Erc20::transferCall {
                recipient: Address::repeat_byte(0x22),
                amount: U256::from(9_000_000),
            }
            .abi_encode()
            .into(),
        )]);
        let mut logs = state.event_logs();
        for (recipient, amount) in [
            (RECIPIENT.parse().unwrap(), 1_000_000),
            (paymaster::PAYMASTER, 123),
        ] {
            let transfer = transaction::Erc20::Transfer {
                from: wallet.address,
                to: recipient,
                value: U256::from(amount),
            }
            .encode_log_data();
            logs.insert(
                0,
                json!({"address":types::TOKEN,"topics":transfer.topics(),"data":transfer.data}),
            );
        }
        for (index, log) in logs.iter_mut().enumerate() {
            log["logIndex"] = json!(U256::from(index));
        }
        state.receipt_logs = Some(logs);
        state.history_target = Some(Address::repeat_byte(0x33));
        Bytes::from(batch.abi_encode())
    };
    drop(wallet);
    for input in [decoy_input, Bytes::from_static(&[1, 2, 3, 4])] {
        chain.state.lock().unwrap().history_input = Some(input);
        let reopened = chain.wallet(&dir);
        sync_history_to_tip(&reopened).await;
        let history = reopened.history().unwrap();
        assert_eq!(history.len(), 1);
        let payment = &history[0];
        assert_eq!(payment.id, sent.id);
        assert_eq!(payment.user_operation_hash, sent.user_operation_hash);
        assert_eq!(payment.amount, 1_000_000);
        assert_eq!(payment.received_amount, 1_000_000);
        assert_eq!(payment.recipient, RECIPIENT);
        assert_eq!(payment.status, UsdtTransferStatus::Confirmed);
        assert_eq!(payment.fee, Some(123));
        assert!(!payment.tx_hash.is_empty());
        assert!(reopened.store.pending_plan(&sent.id).unwrap().is_none());

        let restored_dir = tempfile::tempdir().unwrap();
        let restored = chain.wallet(&restored_dir);
        for _ in 0..2 {
            sync_history_to_tip(&restored).await;
            let history = restored.history().unwrap();
            assert_eq!(history.len(), 2);
            for (recipient, amount) in [
                (RECIPIENT.into(), 1_000_000),
                (paymaster::PAYMASTER.to_checksum(None), 123),
            ] {
                let transfer = history.iter().find(|t| t.recipient == recipient).unwrap();
                assert_eq!(transfer.amount, amount);
                assert_eq!(transfer.user_operation_hash, None);
                assert_eq!(transfer.status, UsdtTransferStatus::Confirmed);
                assert!(!transfer.is_incoming);
            }
        }
    }
}

#[test]
fn stored_activity_is_complete_and_sorted_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = store::Store::open(
        dir.path().join("history.sqlite").to_str().unwrap(),
        "account",
    )
    .unwrap();
    let transfers: Vec<_> = (0..501)
        .map(|index| UsdtTransfer {
            id: format!("receipt-{index}"),
            tx_hash: format!("tx-{index}"),
            user_operation_hash: None,
            recipient: RECIPIENT.into(),
            amount: 1,
            received_amount: 1,
            fee: None,
            is_incoming: true,
            status: UsdtTransferStatus::Confirmed,
            timestamp: index,
            explorer_url: String::new(),
        })
        .collect();
    store.save_history_receipt(&transfers, "receipt").unwrap();
    store.complete_history(1000).unwrap();
    let history = store.transfers().unwrap();
    assert_eq!(history.len(), 501);
    assert_eq!(history.first().unwrap().id, "receipt-500");
    assert_eq!(history.last().unwrap().id, "receipt-0");
}

#[tokio::test]
async fn nonce_advance_with_delayed_logs_remains_pending_and_history_reconciles() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    for recipient in [wallet.receive_address(), types::TOKEN.to_checksum(None)] {
        assert!(matches!(
            wallet.quote_transfer(recipient, 1_000_000).await,
            Err(UsdtError::InvalidAddress)
        ));
    }
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id.clone(), TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.hide_logs = true;
        state.hide_receipts = true;
        state.max_log_range = Some(1);
        state.nonce = 1;
        state.tip += 3;
        state.timestamp += alloy_primitives::U256::from(180);
    }
    assert!(wallet.refresh_transfers().await.is_err());
    assert_eq!(
        wallet.history().unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    assert!(matches!(
        wallet.quote_transfer(RECIPIENT.into(), 1_000_000).await,
        Err(UsdtError::PendingTransfer)
    ));
    {
        let mut state = chain.state.lock().unwrap();
        state.hide_logs = false;
        state.hide_receipts = false;
        state.max_log_range = None;
    }
    sync_history_to_tip(&wallet).await;
    let history = wallet.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, sent.id);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    assert_eq!(history[0].fee, Some(123));
}

#[tokio::test]
async fn interrupted_history_resumes_without_repeating_completed_work() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.incoming_count = 100;
        state.tip = 508_000_000;
        state.block_reads = 0;
        state.fail_block_read_at = Some(25);
    }
    let restored_dir = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_dir);
    restored.store.save_history_progress(20_000).unwrap();
    let error = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        loop {
            match restored.sync_history().await {
                Ok(false) => {}
                Ok(true) => panic!("The interrupted scan must report its provider error"),
                Err(error) => break error,
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(error, UsdtError::NetworkUnavailable));
    assert_eq!(restored.history().unwrap().len(), 24);
    assert!(restored.store.history_progress().unwrap().is_some());
    drop(restored);
    let restored = chain.wallet(&restored_dir);
    sync_history_to_tip(&restored).await;
    assert_eq!(restored.history().unwrap().len(), 100);
    assert_eq!(chain.state.lock().unwrap().block_reads, 101);
    sync_history_to_tip(&restored).await;
    assert_eq!(restored.history().unwrap().len(), 100);
    assert_eq!(chain.state.lock().unwrap().block_reads, 101);
    assert!(restored.store.history_progress().unwrap().is_none());
    assert_eq!(restored.store.synced_block().unwrap().unwrap(), 507_999_998);
    assert!(!restored
        .store
        .has_history_receipt(&restored.history().unwrap()[0].tx_hash)
        .unwrap());
}

#[tokio::test]
async fn replacement_after_expiry_recovers_pending_send_after_restart() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    chain.state.lock().unwrap().reject_broadcast = true;
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    drop(wallet);
    {
        let mut state = chain.state.lock().unwrap();
        state.nonce = 1;
        state.timestamp += alloy_primitives::U256::from(600);
        state.tip = 508_000_000;
        state.replacement_block = Some(507_000_000);
        state.hide_logs = true;
        state.hide_receipts = true;
        state.max_log_range = Some(1);
    }
    let wallet = chain.wallet(&dir);
    assert!(wallet.refresh_transfers().await.is_err());
    assert_eq!(
        wallet.history().unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    chain.state.lock().unwrap().hide_logs = false;
    assert_eq!(
        wallet.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Replaced
    );
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_none());
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    assert_eq!(
        wallet
            .send(quote.id, TEST_PHRASE.into(), None)
            .await
            .unwrap()
            .status,
        UsdtTransferStatus::Pending
    );
}

#[tokio::test]
async fn history_distinguishes_rate_limits_from_log_range_limits() {
    for (code, message) in [
        (-32005, "Rate limit exceeded"),
        (-32016, "Provider throttled"),
        (-32000, "Invalid request"),
    ] {
        let chain = MockChain::start().await;
        let dir = tempfile::tempdir().unwrap();
        let wallet = chain.wallet(&dir);
        {
            let mut state = chain.state.lock().unwrap();
            state.log_error = Some((code, message.into()));
        }
        let error = wallet.sync_history().await.unwrap_err();
        if code == -32000 {
            assert!(matches!(error, UsdtError::NetworkUnavailable));
        } else {
            assert!(matches!(error, UsdtError::RateLimited));
        }
        assert!((1..=2).contains(&chain.state.lock().unwrap().log_requests));
    }
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    chain.state.lock().unwrap().log_error = Some((-32005, "Log query limit exceeded".into()));
    assert!(matches!(
        wallet
            .rpc
            .call::<serde_json::Value>(
                "eth_getLogs",
                serde_json::json!([{"fromBlock":"0x0","toBlock":"0x1"}])
            )
            .await,
        Err(UsdtError::LogRangeTooLarge)
    ));
    {
        let mut state = chain.state.lock().unwrap();
        state.log_error = None;
        state.max_log_range = Some(10_000);
        state.oversized_logs = true;
    }
    sync_history_to_tip(&wallet).await;
    assert_eq!(wallet.store.synced_block().unwrap(), Some(19_998));
    chain.state.lock().unwrap().max_log_range = None;
    wallet
        .history_range_limit
        .store(1, std::sync::atomic::Ordering::Relaxed);
    sync_history_to_tip(&wallet).await;
    assert!(
        wallet
            .history_range_limit
            .load(std::sync::atomic::Ordering::Relaxed)
            > 1
    );
}

#[tokio::test]
async fn history_budget_returns_incomplete_and_resumes_to_tip() {
    let chain = MockChain::start().await;
    chain.state.lock().unwrap().tip = 508_000_000;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    assert!(!wallet.sync_history().await.unwrap());
    let next = wallet.store.history_progress().unwrap().unwrap();
    assert!(next > 0 && next < 508_000_000);
    sync_history_to_tip(&wallet).await;
    assert_eq!(wallet.store.synced_block().unwrap(), Some(507_999_998));
}

#[tokio::test]
async fn quote_expiring_during_validation_is_not_signed() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let mut data = wallet.store.quote(&quote.id).unwrap();
    data.quote.id = "expiring-quote".into();
    data.quote.expires_at = wallet::now() + 10;
    data.plan.expires_at = data.quote.expires_at;
    wallet.store.save_quote(&data).unwrap();
    chain.state.lock().unwrap().delay_gas_estimate = true;
    assert!(matches!(
        wallet.send(data.quote.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::QuoteExpired)
    ));
    assert!(!chain.state.lock().unwrap().delay_gas_estimate);
    assert!(wallet.history().unwrap().is_empty());
    assert!(chain.state.lock().unwrap().operations.is_empty());
}

async fn sync_history_to_tip(wallet: &UsdtWallet) {
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        while !wallet.sync_history().await.unwrap() {}
    })
    .await
    .expect("History must make progress within successive bounded scans");
}

#[tokio::test]
async fn invalid_chain_data_and_stored_json_have_distinct_errors() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let db = rusqlite::Connection::open(dir.path().join("usdt.sqlite")).unwrap();
    db.execute("INSERT INTO usdt_quotes VALUES ('invalid','{')", [])
        .unwrap();
    assert!(matches!(
        wallet.store.quote("invalid"),
        Err(UsdtError::Storage { .. })
    ));
    db.execute(
        "INSERT INTO usdt_transfers (id,hash,raw,data) VALUES ('invalid','hash','{','{')",
        [],
    )
    .unwrap();
    assert!(matches!(wallet.history(), Err(UsdtError::Storage { .. })));
    assert!(matches!(
        wallet.store.pending_plan("invalid"),
        Err(UsdtError::Storage { .. })
    ));
    chain.state.lock().unwrap().timestamp = alloy_primitives::U256::MAX;
    assert!(matches!(
        wallet.block_timestamp(1).await,
        Err(UsdtError::InvalidResponse)
    ));
}

#[tokio::test]
async fn each_payment_authorizes_the_current_nonce_after_delegation() {
    use alloy_primitives::{Bytes, U256};
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.account_code =
            Bytes::from([&[0xef, 0x01, 0x00][..], account::DELEGATE.as_slice()].concat());
        state.authorization_nonce = 1;
        state.nonce = 1;
        state.mined = true;
        state.tip += 3;
    }
    assert_eq!(
        wallet.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Confirmed
    );
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let state = chain.state.lock().unwrap();
    assert_eq!(state.operations.len(), 2);
    for (nonce, operation) in state.operations.iter().enumerate() {
        assert_eq!(operation.eip7702_auth.nonce, U256::from(nonce));
        assert_eq!(operation.nonce, U256::from(nonce));
    }
    assert_ne!(state.operations[0].signature, state.operations[1].signature);
}

#[tokio::test]
async fn consumed_authorization_preserves_pending_payment_until_signed_expiry() {
    use alloy_primitives::{Bytes, U256};
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let quote_data = wallet.store.quote(&quote.id).unwrap();
    let dummy = quote_data.plan.operation.eip7702_auth;
    assert_eq!(dummy, user_operation::Authorization::dummy(0));
    assert!(quote_data.plan.expires_at > quote.expires_at);
    chain.state.lock().unwrap().authorization_nonce = 1;
    assert!(matches!(
        wallet.send(quote.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::QuoteExpired)
    ));
    assert!(chain.state.lock().unwrap().operations.is_empty());
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    chain.state.lock().unwrap().reject_broadcast = true;
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let signed = wallet
        .store
        .pending_plan(&sent.id)
        .unwrap()
        .unwrap()
        .operation;
    assert_eq!(signed.eip7702_auth.nonce, U256::from(1));
    assert_ne!(signed.eip7702_auth.r, dummy.r);
    drop(wallet);
    {
        let mut state = chain.state.lock().unwrap();
        state.account_code =
            Bytes::from([&[0xef, 0x01, 0x00][..], account::DELEGATE.as_slice()].concat());
        state.authorization_nonce = 2;
        state.tip += 3;
    }
    let restored = chain.wallet(&dir);
    assert_eq!(
        restored.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Pending
    );
    assert_eq!(chain.state.lock().unwrap().operations.len(), 1);
    let retained = restored.store.pending_plan(&sent.id).unwrap().unwrap();
    assert_eq!(retained.operation.eip7702_auth, signed.eip7702_auth);
    assert_eq!(retained.operation.signature, signed.signature);
    assert!(matches!(
        restored.quote_transfer(RECIPIENT.into(), 1).await,
        Err(UsdtError::PendingTransfer)
    ));
    chain.state.lock().unwrap().timestamp = U256::from(retained.expires_at + 1);
    let history = restored.refresh_transfers().await.unwrap();
    assert_eq!(history[0].status, UsdtTransferStatus::Failed);
    assert_eq!(history[0].fee, Some(0));
    assert!(restored.store.pending_plan(&sent.id).unwrap().is_none());
    let quote = restored.quote_transfer(RECIPIENT.into(), 1).await.unwrap();
    assert_eq!(
        restored
            .store
            .quote(&quote.id)
            .unwrap()
            .plan
            .operation
            .eip7702_auth
            .nonce,
        U256::from(2)
    );
}

#[tokio::test]
async fn foreign_delegation_and_unbounded_paymaster_terms_cannot_authorize_payments() {
    use alloy_primitives::{Bytes, U256};
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    chain.state.lock().unwrap().account_code = Bytes::from_static(&[0xef, 0x01, 0x00, 1]);
    assert!(matches!(
        wallet.quote_transfer(RECIPIENT.into(), 1).await,
        Err(UsdtError::UnsupportedDelegation)
    ));
    assert_eq!(wallet.balance().await.unwrap(), 10_000_000);
    assert!(wallet.history().unwrap().is_empty());
    chain.state.lock().unwrap().account_code = Bytes::new();
    let timestamp = u64::try_from(chain.state.lock().unwrap().timestamp).unwrap();
    for expiry in [0, timestamp + 901] {
        chain.state.lock().unwrap().paymaster_valid_until = Some(expiry);
        assert!(matches!(
            wallet.quote_transfer(RECIPIENT.into(), 1).await,
            Err(UsdtError::InvalidResponse)
        ));
    }
    chain.state.lock().unwrap().paymaster_valid_until = Some(timestamp + 900);
    let quote = wallet.quote_transfer(RECIPIENT.into(), 1).await.unwrap();
    chain.state.lock().unwrap().account_code = Bytes::from(
        [
            &[0xef, 0x01, 0x00][..],
            alloy_primitives::Address::repeat_byte(4).as_slice(),
        ]
        .concat(),
    );
    assert!(matches!(
        wallet.send(quote.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::UnsupportedDelegation)
    ));
    assert!(chain.state.lock().unwrap().operations.is_empty());
    assert_eq!(chain.state.lock().unwrap().nonce, 0);
    assert_eq!(chain.state.lock().unwrap().balance, U256::from(10_000_000));
}

#[tokio::test]
async fn seed_restore_includes_external_token_sends_without_duplicate_operation_entries() {
    use alloy_primitives::Bytes;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet.quote_transfer(RECIPIENT.into(), 77).await.unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.external_outgoing = true;
        state.history_input = Some(Bytes::from_static(&[1, 2, 3, 4]));
        state.tip += 3;
    }
    for unsupported_batch in [false, true] {
        if unsupported_batch {
            let mut state = chain.state.lock().unwrap();
            state.mined = true;
            state.history_input = None;
            let mut calls = history::decode_calls(&state.operations[0].call_data).unwrap();
            calls.push((
                RECIPIENT.parse().unwrap(),
                Bytes::from_static(&[1, 2, 3, 4]),
            ));
            state.operations[0].call_data = account::batch(&calls);
        }
        let restored_dir = tempfile::tempdir().unwrap();
        let restored = chain.wallet(&restored_dir);
        sync_history_to_tip(&restored).await;
        let history = restored.history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].amount, 77);
        assert!(!history[0].is_incoming);
        assert_eq!(history[0].user_operation_hash, None);
        assert_eq!(history[0].recipient, RECIPIENT);
        sync_history_to_tip(&restored).await;
        assert_eq!(restored.history().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn gas_price_changes_require_a_new_quote_before_signing() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    chain.state.lock().unwrap().gas_price = 60_000_000;
    assert!(matches!(
        wallet.send(quote.id, TEST_PHRASE.into(), None).await,
        Err(UsdtError::QuoteExpired)
    ));
    assert!(wallet.history().unwrap().is_empty());
    assert!(chain.state.lock().unwrap().operations.is_empty());
}

#[tokio::test]
async fn consumed_nonce_recovery_requires_complete_receipts_and_resumes_after_restart() {
    use alloy_primitives::B256;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.nonce = 1;
        state.tip += 3;
        state.hide_logs = true;
        state.block_transactions = Some(vec![B256::repeat_byte(6), B256::repeat_byte(7)]);
        state.receipt_failure = Some(B256::repeat_byte(7));
    }
    assert!(wallet.refresh_transfers().await.is_err());
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    let block_hash = format!("{:#x}", B256::repeat_byte(9));
    assert_eq!(
        wallet.store.nonce_recovery(&sent.id, &block_hash).unwrap(),
        1
    );
    drop(wallet);
    chain.state.lock().unwrap().receipt_failure = None;
    let restored = chain.wallet(&dir);
    let history = restored.refresh_transfers().await.unwrap();
    assert_eq!(history[0].status, UsdtTransferStatus::Replaced);
    assert_eq!(history[0].fee, Some(0));
    assert_eq!(history[0].received_amount, 0);
    assert!(restored.store.pending_plan(&sent.id).unwrap().is_none());
    assert_eq!(
        restored
            .store
            .nonce_recovery(&sent.id, &block_hash)
            .unwrap(),
        0
    );
    restored
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
}

#[tokio::test]
async fn consuming_block_receipts_recover_a_payment_hidden_from_log_queries() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.nonce = 1;
        state.tip += 3;
        state.hide_logs = true;
        state.mined = true;
    }
    let history = wallet.refresh_transfers().await.unwrap();
    assert_eq!(history[0].id, sent.id);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    assert_eq!(history[0].fee, Some(123));
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_none());
}

#[tokio::test]
async fn dense_block_history_recovers_large_receipts_without_skipping_after_restart() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
        state.oversized_block = Some(20000);
        state.receipt_padding = 3 * 1024 * 1024;
        state.hide_receipts = true;
    }
    while let Ok(complete) = wallet.sync_history().await {
        assert!(!complete);
    }
    assert_eq!(wallet.store.history_progress().unwrap(), Some(20000));
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    drop(wallet);
    chain.state.lock().unwrap().hide_receipts = false;
    let restored = chain.wallet(&dir);
    sync_history_to_tip(&restored).await;
    let history = restored.history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, sent.id);
    assert_eq!(history[0].status, UsdtTransferStatus::Confirmed);
    assert_eq!(history[0].fee, Some(123));
    assert_eq!(restored.store.synced_block().unwrap(), Some(20001));
    let reads = chain.state.lock().unwrap().receipt_reads;
    drop(restored);
    let restored = chain.wallet(&dir);
    sync_history_to_tip(&restored).await;
    assert_eq!(chain.state.lock().unwrap().receipt_reads, reads);
}

#[tokio::test]
async fn zero_value_transfers_do_not_require_receipts_or_timestamps() {
    use alloy_primitives::{B256, U256};
    use alloy_sol_types::SolEvent;
    use serde_json::json;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let event = transaction::Erc20::Transfer {
        from: wallet.address,
        to: RECIPIENT.parse().unwrap(),
        value: U256::ZERO,
    }
    .encode_log_data();
    {
        let mut state = chain.state.lock().unwrap();
        state.log_response = Some(vec![
            json!({"address":types::TOKEN,"topics":event.topics(),"data":event.data,"transactionHash":B256::repeat_byte(7),"blockNumber":"0x1","logIndex":"0x0"}),
        ]);
        state.hide_receipts = true;
        state.fail_block_read_at = Some(1);
    }
    assert!(wallet.sync_history().await.unwrap());
    assert!(wallet.history().unwrap().is_empty());
}

#[tokio::test]
async fn first_submission_precheck_releases_an_operation_that_was_never_sent() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    chain
        .state
        .lock()
        .unwrap()
        .authorization_change_on_block_read = true;
    assert!(matches!(
        wallet
            .send(quote.id.clone(), TEST_PHRASE.into(), None)
            .await,
        Err(UsdtError::QuoteExpired)
    ));
    assert!(chain.state.lock().unwrap().operations.is_empty());
    assert!(wallet.store.pending_plan(&quote.id).unwrap().is_none());
    let failed = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    assert_eq!(failed.status, UsdtTransferStatus::Failed);
    assert_eq!(failed.received_amount, 0);
    assert_eq!(failed.fee, Some(0));
    wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
}

#[tokio::test]
async fn moderate_gas_price_movement_preserves_the_approved_fee() {
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let plan = wallet.store.quote(&quote.id).unwrap().plan;
    chain.state.lock().unwrap().gas_price = 52_000_000;
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    assert_eq!(
        chain.state.lock().unwrap().operations[0].max_fee_per_gas,
        plan.operation.max_fee_per_gas
    );
}

#[tokio::test]
async fn unknown_paymaster_history_preserves_principal_fee_and_refund() {
    use alloy_primitives::{Address, B256, U256};
    use alloy_sol_types::SolEvent;
    use serde_json::json;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let payer = Address::repeat_byte(0xab);
    let movement = |from, to, value, index| {
        let event = transaction::Erc20::Transfer {
            from,
            to,
            value: U256::from(value),
        }
        .encode_log_data();
        json!({"address": types::TOKEN, "topics":event.topics(), "data":event.data, "logIndex": U256::from(index)})
    };
    {
        let mut state = chain.state.lock().unwrap();
        let op = &state.operations[0];
        let event = transaction::EntryPoint::UserOperationEvent {
            userOpHash: op.hash(types::CHAIN_ID).unwrap(),
            sender: wallet.address,
            paymaster: payer,
            nonce: U256::ZERO,
            success: true,
            actualGasCost: U256::from(1),
            actualGasUsed: U256::from(1),
        }
        .encode_log_data();
        let log = json!({"address":account::ENTRY_POINT,"topics":event.topics(),"data":event.data,"transactionHash":B256::repeat_byte(7),"blockNumber":"0x4e20","logIndex":"0x3"});
        state.receipt_logs = Some(vec![
            movement(wallet.address, payer, 200u64, 0u64),
            movement(
                wallet.address,
                RECIPIENT.parse().unwrap(),
                1_000_000u64,
                1u64,
            ),
            movement(payer, wallet.address, 50u64, 2u64),
            log.clone(),
        ]);
        state.log_response = Some(vec![log]);
        state.tip += 3;
    }
    let restored_dir = tempfile::tempdir().unwrap();
    let restored = chain.wallet(&restored_dir);
    sync_history_to_tip(&restored).await;
    let history = restored.history().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(
        history
            .iter()
            .filter(|t| !t.is_incoming)
            .map(|t| t.amount)
            .sum::<u64>(),
        1_000_200
    );
    assert_eq!(
        history
            .iter()
            .filter(|t| t.is_incoming)
            .map(|t| t.amount)
            .sum::<u64>(),
        50
    );
}

#[tokio::test]
async fn settlement_requires_matching_canonical_receipts() {
    use alloy_primitives::B256;
    use serde_json::json;
    let chain = MockChain::start().await;
    let dir = tempfile::tempdir().unwrap();
    let wallet = chain.wallet(&dir);
    let quote = wallet
        .quote_transfer(RECIPIENT.into(), 1_000_000)
        .await
        .unwrap();
    let sent = wallet
        .send(quote.id, TEST_PHRASE.into(), None)
        .await
        .unwrap();
    let receipt = {
        let mut state = chain.state.lock().unwrap();
        state.mined = true;
        state.tip += 3;
        state
            .respond(&json!({"method":"eth_getTransactionReceipt","params":[B256::repeat_byte(7)]}))
            ["result"]
            .clone()
    };
    for (field, value) in [
        ("transactionHash", json!(B256::ZERO)),
        ("blockHash", json!(B256::ZERO)),
        ("blockNumber", json!("0x1")),
    ] {
        let mut invalid = receipt.clone();
        invalid[field] = value;
        chain.state.lock().unwrap().receipt_response = Some(invalid);
        assert!(matches!(
            wallet.refresh_transfers().await,
            Err(UsdtError::InvalidResponse)
        ));
        assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    }
    chain.state.lock().unwrap().receipt_response = Some(serde_json::Value::Null);
    assert!(matches!(
        wallet.refresh_transfers().await,
        Err(UsdtError::NetworkUnavailable)
    ));
    assert!(wallet.store.pending_plan(&sent.id).unwrap().is_some());
    chain.state.lock().unwrap().receipt_response = None;
    assert_eq!(
        wallet.refresh_transfers().await.unwrap()[0].status,
        UsdtTransferStatus::Confirmed
    );
}
