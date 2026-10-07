use super::*;
const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

#[test]
fn deposit_authorization_matches_viem_and_rejects_another_wallet() {
    let vector: Value =
        serde_json::from_str(include_str!("../fixtures/deposit-signature.json")).unwrap();
    let request: Value = serde_json::from_str(vector["request"].as_str().unwrap()).unwrap();
    let client = UsdtDepositClient::new(
        request["owner"].as_str().unwrap().into(),
        "https://example.com/v1/usdt/deposits".into(),
    )
    .unwrap();
    assert_eq!(
        client
            .authorize(
                request["payload"].clone(),
                PHRASE.to_owned().into(),
                None,
                1_800_000_000
            )
            .unwrap(),
        vector
    );
    assert!(matches!(
        client.authorize(
            request["payload"].clone(),
            PHRASE.to_owned().into(),
            Some("other".to_owned().into()),
            1_800_000_000
        ),
        Err(UsdtError::InvalidCredentials)
    ));
}

#[test]
fn deposit_addresses_and_transport_reject_wrong_networks() {
    let tron = "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8";
    for bad in [
        TRON_USDT,
        "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb",
        &"T".repeat(10000),
    ] {
        assert!(validate_source_address(bad, UsdtDepositNetwork::Tron).is_err());
    }
    assert!(validate_source_address(
        &super::super::UsdtDestination::Ethereum
            .token()
            .unwrap()
            .to_checksum(None),
        UsdtDepositNetwork::Ethereum
    )
    .is_err());
    assert!(validate_source_address(tron, UsdtDepositNetwork::Tron).is_ok());
    assert!(validate_source_address(tron, UsdtDepositNetwork::Ethereum).is_err());
    assert!(validate_source_address(
        "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6s",
        UsdtDepositNetwork::Tron
    )
    .is_err());
    let solana = "6G41T4zUUYm47xgYBFoUioUGhigxS98Cj79y7C5nAf1L";
    for network in [
        UsdtDepositNetwork::Ethereum,
        UsdtDepositNetwork::Polygon,
        UsdtDepositNetwork::Bsc,
        UsdtDepositNetwork::Base,
    ] {
        assert!(validate_source_address(network.evm_token().unwrap().1, network).is_err());
        assert!(
            validate_source_address("0x1111111111111111111111111111111111111111", network).is_ok()
        );
        assert!(validate_source_address(solana, network).is_err());
    }
    assert!(validate_source_address(solana, UsdtDepositNetwork::Solana).is_ok());
    for bad in [
        SOLANA_USDT,
        "11111111111111111111111111111111",
        "0x1111111111111111111111111111111111111111",
    ] {
        assert!(validate_source_address(bad, UsdtDepositNetwork::Solana).is_err());
    }
    for url in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com?key=secret",
    ] {
        assert!(UsdtDepositClient::new(
            "0x1111111111111111111111111111111111111111".into(),
            url.into()
        )
        .is_err());
    }
}

async fn service(responses: Vec<(u16, Value)>) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/usdt/deposits", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async move {
        let mut requests = Vec::new();
        for (status, value) in responses {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            let mut first_line = String::new();
            reader.read_line(&mut first_line).await.unwrap();
            assert!(first_line == "GET /v1/usdt/deposits HTTP/1.1\r\n" || first_line == "POST /v1/usdt/deposits HTTP/1.1\r\n");
            let mut length = 0;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).await.unwrap(), 0, "Request ended before its headers");
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).await.unwrap();
            if first_line.starts_with("POST") {
                requests.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            }
            let body = value.to_string();
            reader.get_mut().write_all(format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        requests
        }).await.expect("Deposit fixture requests must complete")
    });
    (url, task)
}

#[tokio::test]
async fn service_responses_cover_receive_history_detail_and_refund() {
    let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
    let address = "0x1111111111111111111111111111111111111111";
    let deposit = json!({"id":"dep_one","network":"ethereum","asset":"USDT","source_tx":"0xsource","status":"held","code":null,"refund_tx":null});
    let (url, server) = service(vec![
        (200, json!({"networks":["ethereum","tron","bitcoin"]})),
        (200, json!({"network":"ethereum","address":address,"recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50,"min_usd_cents":"200","max_usd_cents":"10000","uri":123})),
        (200, json!({"deposits":[deposit],"next_offset":50})),
        (200, json!({"deposit":deposit,"order":{"status":"held"}})),
        (202, json!({"status":"refund_requested"})),
    ]).await;
    let client = UsdtDepositClient::new(owner, url).unwrap();
    assert_eq!(
        client.networks().await.unwrap(),
        vec![UsdtDepositNetwork::Ethereum, UsdtDepositNetwork::Tron]
    );
    let received = client
        .receive(
            UsdtDepositNetwork::Ethereum,
            100_000_000,
            PHRASE.into(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(received.estimated_received, 98_500_000);
    assert_eq!(received.min_usd_cents.as_deref(), Some("200"));
    assert_eq!(received.max_usd_cents.as_deref(), Some("10000"));
    assert!(received.uri.contains(address));
    let page = client.history(0, PHRASE.into(), None).await.unwrap();
    assert_eq!(page.deposits[0].amount, None);
    assert_eq!(page.next_offset, Some(50));
    let detail = client
        .detail("dep_one".into(), 0, PHRASE.into(), None)
        .await
        .unwrap();
    assert_eq!(detail.order.unwrap().amount_out, None);
    client
        .request_refund(
            "dep_one".into(),
            0,
            "0x2222222222222222222222222222222222222222".into(),
            UsdtDepositNetwork::Ethereum,
            PHRASE.into(),
            None,
        )
        .await
        .unwrap();
    let requests = server.await.unwrap();
    let expected = [
        json!({"action":"receive","network":"ethereum","amount":"100000000"}),
        json!({"action":"history","offset":0}),
        json!({"action":"detail","depositId":"dep_one","offset":0}),
        json!({"action":"refund","depositId":"dep_one","offset":0,"refundAddress":"0x2222222222222222222222222222222222222222"}),
    ];
    assert_eq!(requests.len(), expected.len());
    for (signed, payload) in requests.into_iter().zip(expected) {
        let request: Value = serde_json::from_str(signed["request"].as_str().unwrap()).unwrap();
        assert_eq!(request["payload"], payload);
        assert_eq!(
            signed,
            client
                .authorize(
                    payload,
                    PHRASE.to_string().into(),
                    None,
                    request["timestamp"].as_u64().unwrap()
                )
                .unwrap()
        );
    }
}

#[tokio::test]
async fn supported_networks_receive_with_pinned_token_uris() {
    let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
    let networks = [
        (
            UsdtDepositNetwork::Ethereum,
            "0x1111111111111111111111111111111111111111",
            "ethereum:0xdac17f958d2ee523a2206206994597c13d831ec7@1/transfer?address=0x1111111111111111111111111111111111111111",
        ),
        (
            UsdtDepositNetwork::Tron,
            "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
            "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
        ),
        (
            UsdtDepositNetwork::Solana,
            "6G41T4zUUYm47xgYBFoUioUGhigxS98Cj79y7C5nAf1L",
            "6G41T4zUUYm47xgYBFoUioUGhigxS98Cj79y7C5nAf1L",
        ),
        (
            UsdtDepositNetwork::Polygon,
            "0x1111111111111111111111111111111111111111",
            "ethereum:0xc2132d05d31c914a87c6611c10748aeb04b58e8f@137/transfer?address=0x1111111111111111111111111111111111111111",
        ),
        (
            UsdtDepositNetwork::Base,
            "0x1111111111111111111111111111111111111111",
            "ethereum:0xfde4c96c8593536e31f229ea8f37b2ada2699bb2@8453/transfer?address=0x1111111111111111111111111111111111111111",
        ),
        (
            UsdtDepositNetwork::Bsc,
            "0x1111111111111111111111111111111111111111",
            "ethereum:0x55d398326f99059ff775485246999027b3197955@56/transfer?address=0x1111111111111111111111111111111111111111",
        ),
    ];
    let supported: Vec<_> = networks.iter().map(|(network, _, _)| *network).collect();
    let mut responses = vec![(200, json!({"networks":supported}))];
    responses.extend(networks.iter().map(|(network, address, _)| {
        (
            200,
            json!({
                "network":network,"address":address,"recipient":owner,"amount":"100000000",
                "estimated_received":"98500000","slippage_bps":50
            }),
        )
    }));
    let (url, server) = service(responses).await;
    let client = UsdtDepositClient::new(owner, url).unwrap();
    assert_eq!(client.networks().await.unwrap(), supported);
    for (network, address, uri) in networks {
        let received = client
            .receive(network, 100_000_000, PHRASE.into(), None)
            .await
            .unwrap();
        assert_eq!(received.amount, 100_000_000);
        assert_eq!(received.estimated_received, 98_500_000);
        assert_eq!(received.address, address);
        assert_eq!(received.uri, uri);
    }
    let requests = server.await.unwrap();
    for (request, (network, _, _)) in requests.iter().zip(networks) {
        let signed: Value = serde_json::from_str(request["request"].as_str().unwrap()).unwrap();
        assert_eq!(
            signed["payload"],
            json!({"action":"receive","network":network,"amount":"100000000"})
        );
    }
}

#[tokio::test]
async fn service_errors_preserve_recovery_actions() {
    for (code, expected) in [
        ("not_found", UsdtError::DepositNotFound),
        (
            "invalid_authorization",
            UsdtError::DepositAuthorizationRejected,
        ),
        ("clock_skew", UsdtError::ClockSkew),
        (
            "amount_too_small",
            UsdtError::DepositAmountOutOfRange {
                min_usd_cents: None,
                max_usd_cents: None,
            },
        ),
        (
            "amount_too_large",
            UsdtError::DepositAmountOutOfRange {
                min_usd_cents: None,
                max_usd_cents: None,
            },
        ),
        ("amount_exceeds_liquidity", UsdtError::UnsupportedRoute),
        (
            "standing_tron_refund_requires_operator",
            UsdtError::DepositNeedsAttention,
        ),
        ("provider_unavailable", UsdtError::NetworkUnavailable),
    ] {
        let (url, server) = service(vec![(
            400,
            json!({"error":code,"min_usd_cents":"200","max_usd_cents":"10000"}),
        )])
        .await;
        let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
        let client = UsdtDepositClient::new(owner, url).unwrap();
        let error = client.history(0, PHRASE.into(), None).await.unwrap_err();
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected)
        );
        if let UsdtError::DepositAmountOutOfRange {
            min_usd_cents,
            max_usd_cents,
        } = error
        {
            assert_eq!(min_usd_cents.as_deref(), Some("200"));
            assert_eq!(max_usd_cents.as_deref(), Some("10000"));
        }
        server.await.unwrap();
    }
    for (status, expected) in [
        (429, UsdtError::RateLimited),
        (401, UsdtError::DepositAuthorizationRejected),
        (403, UsdtError::DepositAuthorizationRejected),
        (404, UsdtError::InvalidResponse),
        (502, UsdtError::NetworkUnavailable),
    ] {
        let (url, server) = service(vec![(status, json!({}))]).await;
        let client = UsdtDepositClient::new(
            super::super::usdt_address(PHRASE.into(), None).unwrap(),
            url,
        )
        .unwrap();
        let error = client.networks().await.unwrap_err();
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected)
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn optional_limits_preserve_receive_and_amount_errors() {
    for (limit, expected) in [
        (json!("200"), Some("200")),
        (Value::Null, None),
        (json!("unknown"), None),
        (json!(200), None),
        (json!(""), None),
        (json!("1".repeat(41)), None),
    ] {
        let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
        let (url, server) = service(vec![
            (200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50,"min_usd_cents":limit,"max_usd_cents":limit})),
            (400, json!({"error":"amount_too_small","min_usd_cents":limit,"max_usd_cents":limit})),
        ]).await;
        let client = UsdtDepositClient::new(owner, url).unwrap();
        let received = client
            .receive(
                UsdtDepositNetwork::Ethereum,
                100_000_000,
                PHRASE.into(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(received.min_usd_cents.as_deref(), expected);
        assert_eq!(received.max_usd_cents.as_deref(), expected);
        let UsdtError::DepositAmountOutOfRange {
            min_usd_cents,
            max_usd_cents,
        } = client
            .receive(
                UsdtDepositNetwork::Ethereum,
                100_000_000,
                PHRASE.into(),
                None,
            )
            .await
            .unwrap_err()
        else {
            panic!("Optional limits must not hide the amount error");
        };
        assert_eq!(min_usd_cents.as_deref(), expected);
        assert_eq!(max_usd_cents.as_deref(), expected);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn invalid_receive_terms_and_nonadvancing_pages_are_rejected() {
    let owner = super::super::usdt_address(PHRASE.into(), None).unwrap();
    let mut responses = vec![
        (
            200,
            json!({"network":"ethereum","address":super::super::UsdtDestination::Ethereum.token(),"recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":50}),
        ),
        (
            200,
            json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":"invalid","amount":"100000000","estimated_received":"98500000","slippage_bps":50}),
        ),
    ];
    for slippage_bps in [0, 49, 51] {
        responses.push((200, json!({"network":"ethereum","address":"0x1111111111111111111111111111111111111111","recipient":owner,"amount":"100000000","estimated_received":"98500000","slippage_bps":slippage_bps})));
    }
    let invalid_receives = responses.len();
    responses.push((200, json!({"deposits":[],"next_offset":0})));
    let (url, server) = service(responses).await;
    let client = UsdtDepositClient::new(owner, url).unwrap();
    for _ in 0..invalid_receives {
        assert!(matches!(
            client
                .receive(
                    UsdtDepositNetwork::Ethereum,
                    100_000_000,
                    PHRASE.into(),
                    None
                )
                .await,
            Err(UsdtError::InvalidResponse)
        ));
    }
    assert!(matches!(
        client.history(0, PHRASE.into(), None).await,
        Err(UsdtError::InvalidResponse)
    ));
    server.await.unwrap();
}
