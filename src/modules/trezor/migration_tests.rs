use prost::Message;
use trezor_connect_rs::protos::{bitcoin as proto, common, MessageType};

use super::test_transport::{
    device_operations, public_key_reply, Exchange, TestTransport, PUBLIC_KEY, XPUB,
};
use super::*;

fn address_params(
    path: &str,
    coin: Option<TrezorCoinType>,
    cross_chain: bool,
) -> TrezorGetAddressParams {
    TrezorGetAddressParams {
        path: path.into(),
        coin,
        show_on_trezor: false,
        script_type: None,
        cross_chain,
    }
}

fn key_params(
    path: &str,
    coin: Option<TrezorCoinType>,
    cross_chain: bool,
) -> TrezorGetPublicKeyParams {
    TrezorGetPublicKeyParams {
        path: path.into(),
        coin,
        show_on_trezor: false,
        cross_chain,
    }
}

fn message_params(
    path: &str,
    coin: Option<TrezorCoinType>,
    cross_chain: bool,
) -> TrezorSignMessageParams {
    TrezorSignMessageParams {
        path: path.into(),
        message: "test message".into(),
        coin,
        cross_chain,
    }
}

fn request_coins(transport: &TestTransport) -> Vec<String> {
    let calls = transport.calls();
    vec![
        proto::GetAddress::decode(calls[0].1.as_slice())
            .unwrap()
            .coin_name
            .unwrap(),
        proto::GetPublicKey::decode(calls[1].1.as_slice())
            .unwrap()
            .coin_name
            .unwrap(),
        proto::SignMessage::decode(calls[2].1.as_slice())
            .unwrap()
            .coin_name
            .unwrap(),
    ]
}

#[test]
fn request_conversions_preserve_coin_and_cross_chain() {
    for coin in [
        None,
        Some(TrezorCoinType::Bitcoin),
        Some(TrezorCoinType::Testnet),
        Some(TrezorCoinType::Signet),
        Some(TrezorCoinType::Regtest),
    ] {
        for cross_chain in [false, true] {
            let path = "m/84'/1'/0'/0/0";
            let address: trezor_connect_rs::GetAddressParams =
                address_params(path, coin, cross_chain).into();
            let key: trezor_connect_rs::GetPublicKeyParams =
                key_params(path, coin, cross_chain).into();
            let message: trezor_connect_rs::SignMessageParams =
                message_params(path, coin, cross_chain).into();
            assert_eq!(address.coin, coin.map(Into::into));
            assert_eq!(key.coin, address.coin);
            assert_eq!(message.coin, address.coin);
            assert_eq!(
                [address.cross_chain, key.cross_chain, message.cross_chain],
                [cross_chain; 3]
            );
            assert_eq!(address.path, path);
            assert_eq!(key.path, path);
            assert_eq!(message.path, path);
            assert_eq!(message.message, "test message");
            assert!(!message.no_script_type);
            assert!(address.multisig.is_none());
        }
    }
}

#[tokio::test]
async fn omitted_coins_infer_network_for_all_path_operations() {
    for (path, expected) in [
        ("m/84'/0'/0'/0/0", "Bitcoin"),
        ("m/84'/1'/0'/0/0", "Testnet"),
        ("m/0'", "Bitcoin"),
        ("m/45'/0/0/0", "Bitcoin"),
        ("m/45'/1/0/0", "Bitcoin"),
        ("m/45'/2/0/0", "Bitcoin"),
    ] {
        let (manager, transport) = TestTransport::manager(device_operations());
        manager
            .get_address(address_params(path, None, false))
            .await
            .unwrap();
        manager
            .get_public_key(key_params(path, None, false))
            .await
            .unwrap();
        manager
            .sign_message(message_params(path, None, false))
            .await
            .unwrap();
        assert_eq!(request_coins(&transport), vec![expected; 3], "{path}");
        transport.assert_finished();
    }
}

#[tokio::test]
async fn explicit_networks_reach_firmware_without_shortcut_names() {
    for (coin, path, expected) in [
        (TrezorCoinType::Bitcoin, "m/84'/0'/0'/0/0", "Bitcoin"),
        (TrezorCoinType::Testnet, "m/84'/1'/0'/0/0", "Testnet"),
        (TrezorCoinType::Signet, "m/84'/1'/0'/0/0", "Testnet"),
        (TrezorCoinType::Regtest, "m/84'/1'/0'/0/0", "Regtest"),
        (TrezorCoinType::Testnet, "m/45'/2/0/0", "Testnet"),
    ] {
        let (manager, transport) = TestTransport::manager(device_operations());
        manager
            .get_address(address_params(path, Some(coin), false))
            .await
            .unwrap();
        manager
            .get_public_key(key_params(path, Some(coin), false))
            .await
            .unwrap();
        manager
            .sign_message(message_params(path, Some(coin), false))
            .await
            .unwrap();
        assert_eq!(request_coins(&transport), vec![expected; 3]);
        assert_eq!(coin.coin_name(), expected);
        transport.assert_finished();
    }
}

#[tokio::test]
async fn invalid_coin_paths_are_rejected_before_device_calls() {
    for (path, coin, expected) in [
        ("m/44'/60'/0'/0/0", None, "Unknown coin"),
        (
            "m/84'/1'/0'/0/0",
            Some(TrezorCoinType::Bitcoin),
            "Invalid parameter",
        ),
        (
            "m/44'/60'/0'/0/0",
            Some(TrezorCoinType::Bitcoin),
            "Invalid parameter",
        ),
    ] {
        let (manager, transport) = TestTransport::manager(vec![]);
        let errors = [
            manager
                .get_address(address_params(path, coin, false))
                .await
                .unwrap_err(),
            manager
                .get_public_key(key_params(path, coin, false))
                .await
                .unwrap_err(),
            manager
                .sign_message(message_params(path, coin, false))
                .await
                .unwrap_err(),
        ];
        for error in errors {
            assert!(error.to_string().contains(expected));
        }
        assert!(transport.calls().is_empty());
    }
}

#[tokio::test]
async fn cross_chain_requires_an_explicit_coin_to_override_inference() {
    for path in ["m/84'/1'/0'/0/0", "m/44'/60'/0'/0/0"] {
        let (manager, transport) = TestTransport::manager(device_operations());
        let coin = Some(TrezorCoinType::Bitcoin);
        manager
            .get_address(address_params(path, coin, true))
            .await
            .unwrap();
        manager
            .get_public_key(key_params(path, coin, true))
            .await
            .unwrap();
        manager
            .sign_message(message_params(path, coin, true))
            .await
            .unwrap();
        assert_eq!(request_coins(&transport), vec!["Bitcoin"; 3]);
        transport.assert_finished();
    }
    let (manager, transport) = TestTransport::manager(vec![]);
    assert!(manager
        .get_address(address_params("m/44'/60'/0'/0/0", None, true))
        .await
        .is_err());
    assert!(transport.calls().is_empty());
}

#[tokio::test]
async fn verify_message_preserves_core_default_and_explicit_networks() {
    for (coin, expected) in [
        (None, "Bitcoin"),
        (Some(TrezorCoinType::Bitcoin), "Bitcoin"),
        (Some(TrezorCoinType::Testnet), "Testnet"),
        (Some(TrezorCoinType::Signet), "Testnet"),
        (Some(TrezorCoinType::Regtest), "Regtest"),
    ] {
        let (manager, transport) = TestTransport::manager(vec![Exchange::new(
            MessageType::VerifyMessage,
            MessageType::Success,
            common::Success { message: None },
        )]);
        let params = TrezorVerifyMessageParams {
            address: "test-address".into(),
            signature: "AQ==".into(),
            message: "test message".into(),
            coin,
        };
        assert!(manager.verify_message(params).await.unwrap());
        let calls = transport.calls();
        let request = proto::VerifyMessage::decode(calls[0].1.as_slice()).unwrap();
        assert_eq!(request.coin_name.as_deref(), Some(expected));
        assert_eq!(request.message, b"test message");
        transport.assert_finished();
    }
}

#[tokio::test]
async fn public_key_fields_preserve_all_prefixes_and_taproot_display() {
    for (version, legacy, path, descriptor) in [
        (0x0488b21e_u32, 0x0488b21e_u32, "m/44'/0'/0'", None),
        (0x049d7cb2, 0x0488b21e, "m/49'/0'/0'", None),
        (0x04b24746, 0x0488b21e, "m/84'/0'/0'", None),
        (0x043587cf, 0x043587cf, "m/44'/1'/0'", None),
        (0x044a5262, 0x043587cf, "m/49'/1'/0'", None),
        (0x045f1cf6, 0x043587cf, "m/84'/1'/0'", None),
        (
            0x0488b21e,
            0x0488b21e,
            "m/86'/0'/0'",
            Some("tr(test-key/0/*)"),
        ),
        (0x0488b21e, 0x0488b21e, "m/86'/0'/0'", None),
    ] {
        let mut payload = bitcoin::base58::decode_check(XPUB).unwrap();
        payload[..4].copy_from_slice(&version.to_be_bytes());
        let firmware_key = bitcoin::base58::encode_check(&payload);
        payload[..4].copy_from_slice(&legacy.to_be_bytes());
        let normalized = bitcoin::base58::encode_check(&payload);
        let (manager, transport) = TestTransport::manager(vec![Exchange::new(
            MessageType::GetPublicKey,
            MessageType::PublicKey,
            public_key_reply(firmware_key.clone(), descriptor.map(str::to_string)),
        )]);
        let result: TrezorPublicKeyResponse = manager
            .get_public_key(key_params(path, None, false))
            .await
            .unwrap();
        assert_eq!(result.xpub, normalized);
        assert_eq!(
            result.xpub_segwit.as_deref(),
            descriptor.or_else(|| (version != legacy).then_some(firmware_key.as_str()))
        );
        assert_eq!(result.descriptor.as_deref(), descriptor);
        assert_eq!(
            result.displayable_public_key,
            descriptor.unwrap_or(&firmware_key)
        );
        assert_eq!(result.path, path);
        assert_eq!(result.public_key, PUBLIC_KEY);
        assert_eq!(result.chain_code, hex::encode([7; 32]));
        assert_eq!(result.depth, 3);
        assert_eq!(result.fingerprint, 42);
        assert_eq!(result.root_fingerprint, Some(0x73c5da0a));
        transport.assert_finished();
    }
}

#[tokio::test]
async fn public_key_optional_metadata_can_be_absent() {
    let mut reply = public_key_reply(XPUB.into(), None);
    reply.root_fingerprint = None;
    let (manager, _) = TestTransport::manager(vec![Exchange::new(
        MessageType::GetPublicKey,
        MessageType::PublicKey,
        reply,
    )]);
    let response: TrezorPublicKeyResponse = manager
        .get_public_key(key_params("m/0'", None, false))
        .await
        .unwrap();
    assert!(response.xpub_segwit.is_none());
    assert!(response.descriptor.is_none());
    assert!(response.root_fingerprint.is_none());
    assert_eq!(response.displayable_public_key, response.xpub);
}
