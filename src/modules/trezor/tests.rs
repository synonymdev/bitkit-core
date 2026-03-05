//! Tests for the Trezor module.

#[cfg(test)]
mod tests {
    use crate::modules::trezor::{
        TrezorDeviceInfo, TrezorError, TrezorFeatures, TrezorScriptType, TrezorTransportType,
        TrezorTxInput, TrezorTxOutput, TrezorSignTxParams, TrezorSignedTx, TrezorCoinType,
    };

    // ========================================================================
    // Error Conversion Tests
    // ========================================================================

    #[test]
    fn test_error_conversion_device_not_found() {
        use trezor_connect_rs::error::TransportError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Transport(TransportError::DeviceNotFound);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::DeviceNotFound));
    }

    #[test]
    fn test_error_conversion_device_disconnected() {
        use trezor_connect_rs::error::TransportError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Transport(TransportError::DeviceDisconnected);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::DeviceDisconnected));
    }

    #[test]
    fn test_error_conversion_timeout() {
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Timeout;
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::Timeout));
    }

    #[test]
    fn test_error_conversion_cancelled() {
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Cancelled;
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::UserCancelled));
    }

    #[test]
    fn test_error_conversion_pin_required() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::PinRequired);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PinRequired));
    }

    #[test]
    fn test_error_conversion_invalid_pin() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::InvalidPin);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::InvalidPin));
    }

    #[test]
    fn test_error_conversion_pin_cancelled() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::PinCancelled);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PinCancelled));
    }

    #[test]
    fn test_error_conversion_passphrase_required() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::PassphraseRequired);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PassphraseRequired));
    }

    #[test]
    fn test_error_conversion_action_cancelled() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::ActionCancelled);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::UserCancelled));
    }

    #[test]
    fn test_error_conversion_not_connected() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Device(DeviceError::NotConnected);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::NotConnected));
    }

    #[test]
    fn test_error_conversion_pairing_required() {
        use trezor_connect_rs::error::ThpError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Thp(ThpError::PairingRequired);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PairingRequired));
    }

    #[test]
    fn test_error_conversion_pairing_failed() {
        use trezor_connect_rs::error::ThpError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Thp(ThpError::PairingFailed("code mismatch".to_string()));
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::PairingFailed { error_details } => {
                assert!(error_details.contains("code mismatch"));
            }
            _ => panic!("Expected PairingFailed error"),
        }
    }

    #[test]
    fn test_error_conversion_invalid_path() {
        use trezor_connect_rs::error::BitcoinError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Bitcoin(BitcoinError::InvalidPath("bad path format".to_string()));
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::InvalidPath { error_details } => {
                assert!(error_details.contains("bad path format"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_error_conversion_protocol_error() {
        use trezor_connect_rs::error::ProtocolError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Protocol(ProtocolError::InvalidHeader);
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::ProtocolError { error_details } => {
                assert!(error_details.contains("Invalid header"));
            }
            _ => panic!("Expected ProtocolError"),
        }
    }

    #[test]
    fn test_error_conversion_session_expired() {
        use trezor_connect_rs::error::SessionError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Session(SessionError::Expired);
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::SessionError { error_details } => {
                assert!(error_details.contains("expired"));
            }
            _ => panic!("Expected SessionError"),
        }
    }

    #[test]
    fn test_error_conversion_io_error() {
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::IoError("file not found".to_string());
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::IoError { error_details } => {
                assert!(error_details.contains("file not found"));
            }
            _ => panic!("Expected IoError"),
        }
    }

    // ========================================================================
    // Type Conversion Tests
    // ========================================================================

    #[test]
    fn test_script_type_conversion_spend_address() {
        let trezor_type = TrezorScriptType::SpendAddress;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(tc_type, trezor_connect_rs::ScriptType::SpendAddress));
    }

    #[test]
    fn test_script_type_conversion_spend_p2sh_witness() {
        let trezor_type = TrezorScriptType::SpendP2shWitness;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(tc_type, trezor_connect_rs::ScriptType::SpendP2SHWitness));
    }

    #[test]
    fn test_script_type_conversion_spend_witness() {
        let trezor_type = TrezorScriptType::SpendWitness;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(tc_type, trezor_connect_rs::ScriptType::SpendWitness));
    }

    #[test]
    fn test_script_type_conversion_spend_taproot() {
        let trezor_type = TrezorScriptType::SpendTaproot;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(tc_type, trezor_connect_rs::ScriptType::SpendTaproot));
    }

    #[test]
    fn test_script_type_conversion_external() {
        let trezor_type = TrezorScriptType::External;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(tc_type, trezor_connect_rs::ScriptType::External));
    }

    #[test]
    fn test_transport_type_from_trezor_connect() {
        use trezor_connect_rs::TransportType;

        let usb_type = TransportType::Usb;
        let result: TrezorTransportType = usb_type.into();
        assert!(matches!(result, TrezorTransportType::Usb));

        let bt_type = TransportType::Bluetooth;
        let result: TrezorTransportType = bt_type.into();
        assert!(matches!(result, TrezorTransportType::Bluetooth));
    }

    #[test]
    fn test_tx_input_conversion() {
        let input = TrezorTxInput {
            prev_hash: "abcd1234".to_string(),
            prev_index: 0,
            path: "m/84'/0'/0'/0/0".to_string(),
            amount: 100000,
            script_type: TrezorScriptType::SpendWitness,
            sequence: Some(0xFFFFFFFD),
            orig_hash: None,
            orig_index: None,
        };

        let tc_input: trezor_connect_rs::SignTxInput = input.into();

        assert_eq!(tc_input.prev_hash, "abcd1234");
        assert_eq!(tc_input.prev_index, 0);
        assert_eq!(tc_input.path, "m/84'/0'/0'/0/0");
        assert_eq!(tc_input.amount, 100000);
        assert!(matches!(tc_input.script_type, trezor_connect_rs::ScriptType::SpendWitness));
        assert_eq!(tc_input.sequence, Some(0xFFFFFFFD));
    }

    #[test]
    fn test_tx_output_external_conversion() {
        let output = TrezorTxOutput {
            address: Some("bc1qtest...".to_string()),
            path: None,
            amount: 90000,
            script_type: None,
            op_return_data: None,
            orig_hash: None,
            orig_index: None,
        };

        let tc_output: trezor_connect_rs::SignTxOutput = output.into();

        assert_eq!(tc_output.address, Some("bc1qtest...".to_string()));
        assert!(tc_output.path.is_none());
        assert_eq!(tc_output.amount, 90000);
    }

    #[test]
    fn test_tx_output_change_conversion() {
        let output = TrezorTxOutput {
            address: None,
            path: Some("m/84'/0'/0'/1/0".to_string()),
            amount: 5000,
            script_type: Some(TrezorScriptType::SpendWitness),
            op_return_data: None,
            orig_hash: None,
            orig_index: None,
        };

        let tc_output: trezor_connect_rs::SignTxOutput = output.into();

        assert!(tc_output.address.is_none());
        assert_eq!(tc_output.path, Some("m/84'/0'/0'/1/0".to_string()));
        assert_eq!(tc_output.amount, 5000);
        assert!(matches!(tc_output.script_type, Some(trezor_connect_rs::ScriptType::SpendWitness)));
    }

    #[test]
    fn test_tx_output_op_return_conversion() {
        let output = TrezorTxOutput {
            address: None,
            path: None,
            amount: 0,
            script_type: None,
            op_return_data: Some("deadbeef".to_string()),
            orig_hash: None,
            orig_index: None,
        };

        let tc_output: trezor_connect_rs::SignTxOutput = output.into();

        assert!(tc_output.address.is_none());
        assert!(tc_output.path.is_none());
        assert_eq!(tc_output.amount, 0);
        assert_eq!(tc_output.op_return_data, Some("deadbeef".to_string()));
    }

    #[test]
    fn test_sign_tx_params_conversion() {
        let params = TrezorSignTxParams {
            inputs: vec![TrezorTxInput {
                prev_hash: "abcd".to_string(),
                prev_index: 0,
                path: "m/84'/0'/0'/0/0".to_string(),
                amount: 100000,
                script_type: TrezorScriptType::SpendWitness,
                sequence: None,
                orig_hash: None,
                orig_index: None,
            }],
            outputs: vec![TrezorTxOutput {
                address: Some("bc1q...".to_string()),
                path: None,
                amount: 90000,
                script_type: None,
                op_return_data: None,
                orig_hash: None,
                orig_index: None,
            }],
            coin: Some(TrezorCoinType::Bitcoin),
            lock_time: Some(0),
            version: Some(2),
            prev_txs: vec![],
        };

        let tc_params: trezor_connect_rs::SignTxParams = params.into();

        assert_eq!(tc_params.inputs.len(), 1);
        assert_eq!(tc_params.outputs.len(), 1);
        assert_eq!(tc_params.coin, Some(trezor_connect_rs::Network::Bitcoin));
        assert_eq!(tc_params.lock_time, Some(0));
        assert_eq!(tc_params.version, Some(2));
    }

    #[test]
    fn test_signed_tx_response_conversion() {
        let response = trezor_connect_rs::SignedTxResponse {
            signatures: vec!["sig1".to_string(), "sig2".to_string()],
            serialized_tx: "rawtx".to_string(),
            txid: None,
        };

        let result: TrezorSignedTx = response.into();

        assert_eq!(result.signatures, vec!["sig1", "sig2"]);
        assert_eq!(result.serialized_tx, "rawtx");
        assert!(result.txid.is_none());
    }

    // ========================================================================
    // Device Info Conversion Tests
    // ========================================================================

    #[test]
    fn test_device_info_from_trezor_connect() {
        use trezor_connect_rs::{DeviceInfo, TransportType};

        let tc_info = DeviceInfo {
            id: "device123".to_string(),
            transport_type: TransportType::Usb,
            name: Some("Trezor Safe 5".to_string()),
            path: "/dev/trezor0".to_string(),
            label: Some("My Trezor".to_string()),
            model: Some("Safe 5".to_string()),
            is_bootloader: false,
        };

        let result: TrezorDeviceInfo = tc_info.into();

        assert_eq!(result.id, "device123");
        assert!(matches!(result.transport_type, TrezorTransportType::Usb));
        assert_eq!(result.name, Some("Trezor Safe 5".to_string()));
        assert_eq!(result.path, "/dev/trezor0");
        assert_eq!(result.label, Some("My Trezor".to_string()));
        assert_eq!(result.model, Some("Safe 5".to_string()));
        assert!(!result.is_bootloader);
    }

    #[test]
    fn test_features_from_trezor_connect() {
        use trezor_connect_rs::device::Features;

        let tc_features = Features {
            vendor: Some("trezor.io".to_string()),
            model: Some("Safe 5".to_string()),
            label: Some("My Wallet".to_string()),
            device_id: Some("ABC123".to_string()),
            major_version: Some(2),
            minor_version: Some(8),
            patch_version: Some(0),
            pin_protection: Some(true),
            passphrase_protection: Some(false),
            initialized: Some(true),
            needs_backup: Some(false),
            ..Default::default()
        };

        let result: TrezorFeatures = tc_features.into();

        assert_eq!(result.vendor, Some("trezor.io".to_string()));
        assert_eq!(result.model, Some("Safe 5".to_string()));
        assert_eq!(result.label, Some("My Wallet".to_string()));
        assert_eq!(result.device_id, Some("ABC123".to_string()));
        assert_eq!(result.major_version, Some(2));
        assert_eq!(result.minor_version, Some(8));
        assert_eq!(result.patch_version, Some(0));
        assert_eq!(result.pin_protection, Some(true));
        assert_eq!(result.passphrase_protection, Some(false));
        assert_eq!(result.initialized, Some(true));
        assert_eq!(result.needs_backup, Some(false));
    }

    // ========================================================================
    // Path Validation Tests
    // ========================================================================

    #[test]
    fn test_valid_bip84_path() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        assert!(validate_derivation_path("m/84'/0'/0'/0/0").is_ok());
        assert!(validate_derivation_path("m/84'/0'/0'/1/0").is_ok());
        assert!(validate_derivation_path("m/84'/0'/0'").is_ok());
    }

    #[test]
    fn test_valid_bip44_path() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        assert!(validate_derivation_path("m/44'/0'/0'/0/0").is_ok());
        assert!(validate_derivation_path("m/44'/0'/0'/1/0").is_ok());
    }

    #[test]
    fn test_valid_bip49_path() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        assert!(validate_derivation_path("m/49'/0'/0'/0/0").is_ok());
    }

    #[test]
    fn test_valid_bip86_path() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        assert!(validate_derivation_path("m/86'/0'/0'/0/0").is_ok());
    }

    #[test]
    fn test_valid_bip84_path_h_notation() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        assert!(validate_derivation_path("m/84h/0h/0h/0/0").is_ok());
        assert!(validate_derivation_path("m/84h/0h/0h/1/0").is_ok());
        assert!(validate_derivation_path("m/84h/0h/0h").is_ok());
    }

    #[test]
    fn test_invalid_path_missing_m() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        let result = validate_derivation_path("84'/0'/0'/0/0");
        assert!(result.is_err());
        match result {
            Err(TrezorError::InvalidPath { error_details }) => {
                assert!(error_details.contains("must start with 'm/'"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_invalid_path_empty_after_m() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        let result = validate_derivation_path("m/");
        assert!(result.is_err());
        match result {
            Err(TrezorError::InvalidPath { error_details }) => {
                assert!(error_details.contains("cannot be empty"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_invalid_path_non_numeric() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        let result = validate_derivation_path("m/84'/abc/0'/0/0");
        assert!(result.is_err());
        match result {
            Err(TrezorError::InvalidPath { error_details }) => {
                assert!(error_details.contains("must be a number"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_invalid_path_empty_component() {
        use crate::modules::trezor::implementation::validate_derivation_path;

        let result = validate_derivation_path("m/84'//0'/0/0");
        assert!(result.is_err());
        match result {
            Err(TrezorError::InvalidPath { error_details }) => {
                assert!(error_details.contains("Empty path component"));
            }
            _ => panic!("Expected InvalidPath error"),
        }
    }

    // ========================================================================
    // Error Display Tests
    // ========================================================================

    #[test]
    fn test_error_display_messages() {
        let err = TrezorError::DeviceNotFound;
        assert_eq!(err.to_string(), "No Trezor device found");

        let err = TrezorError::PinRequired;
        assert_eq!(err.to_string(), "PIN is required");

        let err = TrezorError::UserCancelled;
        assert_eq!(err.to_string(), "Action cancelled by user");

        let err = TrezorError::Timeout;
        assert_eq!(err.to_string(), "Operation timed out");

        let err = TrezorError::NotInitialized;
        assert_eq!(err.to_string(), "Trezor not initialized. Call trezor_initialize first.");

        let err = TrezorError::NotConnected;
        assert_eq!(err.to_string(), "No device connected. Call trezor_connect first.");
    }

    // ========================================================================
    // Account Info Helper Tests
    // ========================================================================

    const ACCOUNT_INFO_ELECTRUM_URL: &str = "ssl://fulcrum.bitkit.stag0.blocktank.to:18484";

    // Test mnemonic (for reference): "wet sea trial spice sheriff bronze total swift slide near easily inhale"
    // Account extended public keys derived from the above mnemonic (each funded with 100,000 sats)
    const TEST_TPUB: &str = "tpubDDWohsp5dx2iMJ9N7iHbgAEDhH4BJB9NWW1fEW3yA3AFNDREmpzteCXNqppMLUmKFY5q5e3PXtS5CuqWCQbYcGhpPqYAgQSYdwknW9J6sQv";
    const TEST_UPUB: &str = "upub5DWPhYKrgLiETEmgLykymFzBK6gXUNvkE67HLSEh5zcWNRdx2Hxd8HypxaDaK1p62a1kXwe9eBcV3pGm7yEpHh4ebrspSoer4x8Ko29egtv";
    const TEST_VPUB: &str = "vpub5ZgC33hDPuhHSDYrGeoi685MKtdAUB3pkS4rarJ5dv3RABneKLCbhJu2FNfcajo1GukBAwEMBXZWvU7fykTeyKEJrgV6E6NC7BZ3jzp3ffp";

    // Test addresses derived from the above mnemonic (each funded with 100,000 sats)
    const TEST_LEGACY_ADDR: &str = "mixttbUXpVWVpx3qHh7KUiVnxWiNxL2uu9";
    const TEST_P2SH_ADDR: &str = "2N7mA1KBJX8iprzzoFjcbkY1Z2WRZ3bjnsK";
    const TEST_REGTEST_BECH32_ADDR: &str = "bcrt1qj2gz3meule5mc4r4knv65vjds3g88rlxs0jlmq";

    // --- Unit Tests: Helper Functions ---

    #[test]
    fn test_detect_account_type() {
        use crate::modules::trezor::account_info::detect_account_type;
        use crate::modules::trezor::AccountType;

        // Standard prefixes
        assert_eq!(detect_account_type("xpub6ABC").unwrap(), AccountType::Legacy);
        assert_eq!(detect_account_type("tpub6ABC").unwrap(), AccountType::Legacy);
        assert_eq!(detect_account_type("ypub6ABC").unwrap(), AccountType::WrappedSegwit);
        assert_eq!(detect_account_type("upub6ABC").unwrap(), AccountType::WrappedSegwit);
        assert_eq!(detect_account_type("zpub6ABC").unwrap(), AccountType::NativeSegwit);
        assert_eq!(detect_account_type("vpub6ABC").unwrap(), AccountType::NativeSegwit);

        // Actual test keys
        assert_eq!(detect_account_type(TEST_TPUB).unwrap(), AccountType::Legacy);
        assert_eq!(detect_account_type(TEST_UPUB).unwrap(), AccountType::WrappedSegwit);
        assert_eq!(detect_account_type(TEST_VPUB).unwrap(), AccountType::NativeSegwit);

        // Error cases
        assert!(detect_account_type("invalid_key").is_err());
        assert!(detect_account_type("ab").is_err()); // too short
    }

    #[test]
    fn test_detect_network_from_key() {
        use crate::modules::trezor::account_info::detect_network_from_key;
        use bdk::bitcoin::Network as BdkNetwork;

        // Mainnet prefixes
        assert_eq!(detect_network_from_key("xpub6ABC").unwrap(), BdkNetwork::Bitcoin);
        assert_eq!(detect_network_from_key("ypub6ABC").unwrap(), BdkNetwork::Bitcoin);
        assert_eq!(detect_network_from_key("zpub6ABC").unwrap(), BdkNetwork::Bitcoin);

        // Testnet prefixes
        assert_eq!(detect_network_from_key("tpub6ABC").unwrap(), BdkNetwork::Testnet);
        assert_eq!(detect_network_from_key("upub6ABC").unwrap(), BdkNetwork::Testnet);
        assert_eq!(detect_network_from_key("vpub6ABC").unwrap(), BdkNetwork::Testnet);

        // Actual test keys
        assert_eq!(detect_network_from_key(TEST_TPUB).unwrap(), BdkNetwork::Testnet);
        assert_eq!(detect_network_from_key(TEST_UPUB).unwrap(), BdkNetwork::Testnet);
        assert_eq!(detect_network_from_key(TEST_VPUB).unwrap(), BdkNetwork::Testnet);

        // Error cases
        assert!(detect_network_from_key("invalid").is_err());
        assert!(detect_network_from_key("ab").is_err());
    }

    #[test]
    fn test_normalize_extended_key() {
        use crate::modules::trezor::account_info::normalize_extended_key;

        // tpub should remain unchanged
        let normalized_tpub = normalize_extended_key(TEST_TPUB).unwrap();
        assert!(normalized_tpub.starts_with("tpub"), "tpub should remain as tpub");
        assert_eq!(normalized_tpub, TEST_TPUB);

        // upub should be converted to tpub
        let normalized_upub = normalize_extended_key(TEST_UPUB).unwrap();
        assert!(normalized_upub.starts_with("tpub"), "upub should be converted to tpub, got: {}", &normalized_upub[..4]);

        // vpub should be converted to tpub
        let normalized_vpub = normalize_extended_key(TEST_VPUB).unwrap();
        assert!(normalized_vpub.starts_with("tpub"), "vpub should be converted to tpub, got: {}", &normalized_vpub[..4]);

        // Error cases
        assert!(normalize_extended_key("ab").is_err());
        assert!(normalize_extended_key("invalidkey").is_err());
    }

    #[test]
    fn test_build_descriptors() {
        use crate::modules::trezor::account_info::build_descriptors;
        use crate::modules::trezor::AccountType;

        let test_key = "tpub_test_key";

        let (ext, int) = build_descriptors(test_key, AccountType::Legacy);
        assert_eq!(ext, "pkh(tpub_test_key/0/*)");
        assert_eq!(int, "pkh(tpub_test_key/1/*)");

        let (ext, int) = build_descriptors(test_key, AccountType::WrappedSegwit);
        assert_eq!(ext, "sh(wpkh(tpub_test_key/0/*))");
        assert_eq!(int, "sh(wpkh(tpub_test_key/1/*))");

        let (ext, int) = build_descriptors(test_key, AccountType::NativeSegwit);
        assert_eq!(ext, "wpkh(tpub_test_key/0/*)");
        assert_eq!(int, "wpkh(tpub_test_key/1/*)");

        let (ext, int) = build_descriptors(test_key, AccountType::Taproot);
        assert_eq!(ext, "tr(tpub_test_key/0/*)");
        assert_eq!(int, "tr(tpub_test_key/1/*)");
    }

    #[test]
    fn test_derive_base_path() {
        use crate::modules::trezor::account_info::derive_base_path;
        use crate::modules::trezor::AccountType;
        use bdk::bitcoin::Network as BdkNetwork;

        assert_eq!(derive_base_path(AccountType::Legacy, BdkNetwork::Bitcoin, 0), "m/44'/0'/0'");
        assert_eq!(derive_base_path(AccountType::WrappedSegwit, BdkNetwork::Bitcoin, 0), "m/49'/0'/0'");
        assert_eq!(derive_base_path(AccountType::NativeSegwit, BdkNetwork::Bitcoin, 0), "m/84'/0'/0'");
        assert_eq!(derive_base_path(AccountType::Taproot, BdkNetwork::Bitcoin, 0), "m/86'/0'/0'");

        // Testnet uses coin_type 1
        assert_eq!(derive_base_path(AccountType::Legacy, BdkNetwork::Testnet, 0), "m/44'/1'/0'");
        assert_eq!(derive_base_path(AccountType::NativeSegwit, BdkNetwork::Testnet, 0), "m/84'/1'/0'");

        // Non-zero account index
        assert_eq!(derive_base_path(AccountType::WrappedSegwit, BdkNetwork::Bitcoin, 2), "m/49'/0'/2'");
        assert_eq!(derive_base_path(AccountType::NativeSegwit, BdkNetwork::Testnet, 5), "m/84'/1'/5'");
    }

    #[test]
    fn test_account_type_to_script_type() {
        use crate::modules::trezor::account_info::account_type_to_script_type;
        use crate::modules::trezor::AccountType;

        assert!(matches!(account_type_to_script_type(AccountType::Legacy), TrezorScriptType::SpendAddress));
        assert!(matches!(account_type_to_script_type(AccountType::WrappedSegwit), TrezorScriptType::SpendP2shWitness));
        assert!(matches!(account_type_to_script_type(AccountType::NativeSegwit), TrezorScriptType::SpendWitness));
        assert!(matches!(account_type_to_script_type(AccountType::Taproot), TrezorScriptType::SpendTaproot));
    }

    // --- Integration Tests: get_account_info ---

    #[tokio::test]
    #[ignore]
    async fn test_get_account_info_tpub() {
        use crate::modules::trezor::account_info::get_account_info;
        use crate::modules::trezor::AccountType;

        let result = get_account_info(
            TEST_TPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
            None,
        )
        .await;

        let info = result.expect("get_account_info(tpub) should succeed");
        assert_eq!(info.account_type, AccountType::Legacy);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(info.utxo_count >= 1, "Expected at least 1 UTXO, got {}", info.utxo_count);
        assert!(info.block_height > 0, "Expected block_height > 0");
        assert!(info.account.path.starts_with("m/44'/1'/"), "Expected BIP44 testnet path, got {}", info.account.path);
        assert!(!info.account.utxo.is_empty(), "Expected non-empty UTXOs");

        // Verify address structure
        assert!(!info.account.addresses.unused.is_empty(), "Expected unused addresses");
        for addr in &info.account.addresses.used {
            assert!(!addr.address.is_empty());
            assert!(addr.path.starts_with("m/44'/1'/"));
            assert!(addr.transfers > 0);
        }

        for utxo in &info.account.utxo {
            assert!(!utxo.txid.is_empty(), "UTXO should have non-empty txid");
            let amount: u64 = utxo.amount;
            assert!(amount > 0, "UTXO amount should be > 0");
            assert!(!utxo.path.is_empty(), "UTXO should have a derivation path");
        }

        println!("tpub account info: balance={}, utxos={}, path={}, block_height={}",
            info.balance, info.utxo_count, info.account.path, info.block_height);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_account_info_upub() {
        use crate::modules::trezor::account_info::get_account_info;
        use crate::modules::trezor::AccountType;

        let result = get_account_info(
            TEST_UPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
            None,
        )
        .await;

        let info = result.expect("get_account_info(upub) should succeed");
        assert_eq!(info.account_type, AccountType::WrappedSegwit);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(info.utxo_count >= 1, "Expected at least 1 UTXO, got {}", info.utxo_count);
        assert!(info.block_height > 0);
        assert!(info.account.path.starts_with("m/49'/1'/"), "Expected BIP49 testnet path, got {}", info.account.path);
        assert!(!info.account.utxo.is_empty());

        for utxo in &info.account.utxo {
            assert!(!utxo.txid.is_empty());
            let amount: u64 = utxo.amount;
            assert!(amount > 0);
        }

        println!("upub account info: balance={}, utxos={}, path={}, block_height={}",
            info.balance, info.utxo_count, info.account.path, info.block_height);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_account_info_vpub() {
        use crate::modules::trezor::account_info::get_account_info;
        use crate::modules::trezor::AccountType;

        let result = get_account_info(
            TEST_VPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
            None,
        )
        .await;

        let info = result.expect("get_account_info(vpub) should succeed");
        assert_eq!(info.account_type, AccountType::NativeSegwit);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(info.utxo_count >= 1, "Expected at least 1 UTXO, got {}", info.utxo_count);
        assert!(info.block_height > 0);
        assert!(info.account.path.starts_with("m/84'/1'/"), "Expected BIP84 testnet path, got {}", info.account.path);
        assert!(!info.account.utxo.is_empty());

        for utxo in &info.account.utxo {
            assert!(!utxo.txid.is_empty());
            let amount: u64 = utxo.amount;
            assert!(amount > 0);
        }

        println!("vpub account info: balance={}, utxos={}, path={}, block_height={}",
            info.balance, info.utxo_count, info.account.path, info.block_height);
    }

    // --- Integration Tests: get_address_info ---

    #[tokio::test]
    #[ignore]
    async fn test_get_address_info_legacy() {
        use crate::modules::trezor::account_info::get_address_info;

        let result = get_address_info(
            TEST_LEGACY_ADDR,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
        )
        .await;

        let info = result.expect("get_address_info(legacy) should succeed");
        assert_eq!(info.address, TEST_LEGACY_ADDR);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(!info.utxos.is_empty(), "Expected non-empty UTXOs");
        assert!(info.transfers >= 1, "Expected at least 1 transfer, got {}", info.transfers);
        assert!(info.block_height > 0);

        for utxo in &info.utxos {
            assert_eq!(utxo.address, TEST_LEGACY_ADDR);
            assert!(!utxo.txid.is_empty());
            let amount: u64 = utxo.amount;
            assert!(amount > 0);
        }

        println!("Legacy address info: balance={}, utxos={}, transfers={}, block_height={}",
            info.balance, info.utxos.len(), info.transfers, info.block_height);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_address_info_p2sh() {
        use crate::modules::trezor::account_info::get_address_info;

        let result = get_address_info(
            TEST_P2SH_ADDR,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
        )
        .await;

        let info = result.expect("get_address_info(p2sh) should succeed");
        assert_eq!(info.address, TEST_P2SH_ADDR);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(!info.utxos.is_empty());
        assert!(info.transfers >= 1);
        assert!(info.block_height > 0);

        for utxo in &info.utxos {
            assert_eq!(utxo.address, TEST_P2SH_ADDR);
        }

        println!("P2SH address info: balance={}, utxos={}, transfers={}, block_height={}",
            info.balance, info.utxos.len(), info.transfers, info.block_height);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_address_info_regtest_bech32() {
        use crate::modules::trezor::account_info::get_address_info;

        let result = get_address_info(
            TEST_REGTEST_BECH32_ADDR,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
        )
        .await;

        let info = result.expect("get_address_info(regtest bech32) should succeed");
        assert_eq!(info.address, TEST_REGTEST_BECH32_ADDR);
        let balance: u64 = info.balance;
        assert!(balance >= 100_000, "Expected balance >= 100,000 sats, got {}", balance);
        assert!(!info.utxos.is_empty());
        assert!(info.transfers >= 1);
        assert!(info.block_height > 0);

        for utxo in &info.utxos {
            assert_eq!(utxo.address, TEST_REGTEST_BECH32_ADDR);
        }

        println!("Regtest bech32 address info: balance={}, utxos={}, transfers={}, block_height={}",
            info.balance, info.utxos.len(), info.transfers, info.block_height);
    }

    // --- Error / Edge Case Tests ---

    #[test]
    fn test_get_account_info_invalid_key() {
        use crate::modules::trezor::account_info::get_account_info;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_account_info(
            "not_a_valid_xpub",
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
            None,
        ));

        assert!(result.is_err(), "Expected error for invalid key");
    }

    #[test]
    fn test_get_account_info_network_mismatch() {
        use crate::modules::trezor::account_info::get_account_info;
        use crate::modules::trezor::AccountInfoError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        // tpub is testnet, but we specify Bitcoin (mainnet) — should get NetworkMismatch
        let result = rt.block_on(get_account_info(
            TEST_TPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            Some(TrezorCoinType::Bitcoin),
            None,
        ));

        assert!(result.is_err(), "Expected NetworkMismatch error");
        match result.unwrap_err() {
            AccountInfoError::NetworkMismatch { .. } => {}
            other => panic!("Expected NetworkMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn test_get_address_info_invalid_address() {
        use crate::modules::trezor::account_info::get_address_info;
        use crate::modules::trezor::AccountInfoError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_address_info(
            "not_a_valid_address",
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
        ));

        assert!(result.is_err(), "Expected error for invalid address");
        match result.unwrap_err() {
            AccountInfoError::InvalidAddress { .. } => {}
            other => panic!("Expected InvalidAddress, got: {:?}", other),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_address_info_invalid_electrum() {
        use crate::modules::trezor::account_info::get_address_info;

        let result = get_address_info(
            TEST_LEGACY_ADDR,
            "invalid://url",
            None,
        )
        .await;

        assert!(result.is_err(), "Expected error for invalid electrum URL");
    }

    // ========================================================================
    // Compose Integration Tests
    // ========================================================================

    use crate::modules::trezor::{
        TrezorFeeLevel, TrezorSortingStrategy, TrezorPrecomposeOutput,
        TrezorPrecomposeParams, TrezorPrecomposedResult,
        AccountUtxo, ComposeAccount, AccountAddresses, AddressInfo,
    };
    use crate::modules::trezor::compose::{precompose_transaction, precomposed_to_sign_params};

    fn test_compose_account() -> ComposeAccount {
        ComposeAccount {
            path: "m/84'/1'/0'".to_string(),
            addresses: AccountAddresses {
                used: vec![
                    AddressInfo {
                        address: "bcrt1qj2gz3meule5mc4r4knv65vjds3g88rlxs0jlmq".to_string(),
                        path: "m/84'/1'/0'/0/0".to_string(),
                        transfers: 2,
                    },
                ],
                unused: vec![
                    AddressInfo {
                        address: "bcrt1qeyn4amkfpuz589f6x7adzclqx98akv6mvzvndp".to_string(),
                        path: "m/84'/1'/0'/0/1".to_string(),
                        transfers: 0,
                    },
                ],
                change: vec![
                    AddressInfo {
                        address: "bcrt1q8lahff3lcealxhv2ygde4k08fsy0v5a95020r0".to_string(),
                        path: "m/84'/1'/0'/1/0".to_string(),
                        transfers: 0,
                    },
                ],
            },
            utxo: vec![
                AccountUtxo {
                    txid: "559a6e22b4064c6d1dd3e1ec72a0f65e89093924aba760f7d71d6c4f551e99ba".to_string(),
                    vout: 1,
                    amount: 100_000,
                    block_height: 71692,
                    address: "bcrt1qj2gz3meule5mc4r4knv65vjds3g88rlxs0jlmq".to_string(),
                    path: "m/84'/1'/0'/0/0".to_string(),
                    confirmations: 3684,
                    coinbase: false,
                    own: true,
                    required: None,
                },
            ],
        }
    }

    #[test]
    fn test_precompose_basic_payment() {
        let params = TrezorPrecomposeParams {
            outputs: vec![
                TrezorPrecomposeOutput::Payment {
                    address: "bcrt1qeyn4amkfpuz589f6x7adzclqx98akv6mvzvndp".to_string(),
                    amount: "50000".to_string(),
                },
            ],
            coin: "Regtest".to_string(),
            account: test_compose_account(),
            fee_levels: vec![
                TrezorFeeLevel {
                    fee_per_unit: "2".to_string(),
                    base_fee: None,
                    floor_base_fee: None,
                },
            ],
            sequence: None,
            sorting_strategy: Some(TrezorSortingStrategy::None),
        };

        let results = precompose_transaction(params);
        assert_eq!(results.len(), 1);

        match &results[0] {
            TrezorPrecomposedResult::Final { total_spent, fee, inputs, outputs, .. } => {
                let total: u64 = total_spent.parse().unwrap();
                let fee_val: u64 = fee.parse().unwrap();
                assert!(fee_val > 0, "Fee should be > 0");
                assert_eq!(total, 50_000 + fee_val, "total_spent = amount + fee");
                assert!(!inputs.is_empty(), "Should have selected inputs");
                assert!(outputs.len() >= 1, "Should have at least one output");
            }
            TrezorPrecomposedResult::Error { error } => panic!("Compose failed: {}", error),
            TrezorPrecomposedResult::NonFinal { .. } => panic!("Expected Final result"),
        }
    }

    #[test]
    fn test_precompose_to_sign_params_conversion() {
        let params = TrezorPrecomposeParams {
            outputs: vec![
                TrezorPrecomposeOutput::Payment {
                    address: "bcrt1qeyn4amkfpuz589f6x7adzclqx98akv6mvzvndp".to_string(),
                    amount: "50000".to_string(),
                },
            ],
            coin: "Regtest".to_string(),
            account: test_compose_account(),
            fee_levels: vec![
                TrezorFeeLevel {
                    fee_per_unit: "2".to_string(),
                    base_fee: None,
                    floor_base_fee: None,
                },
            ],
            sequence: None,
            sorting_strategy: Some(TrezorSortingStrategy::None),
        };

        let results = precompose_transaction(params);
        match &results[0] {
            TrezorPrecomposedResult::Final { inputs, outputs, .. } => {
                let sign_params = precomposed_to_sign_params(
                    inputs.clone(),
                    outputs.clone(),
                    Some(TrezorCoinType::Regtest),
                );

                assert!(!sign_params.inputs.is_empty());
                assert!(!sign_params.outputs.is_empty());
                assert!(sign_params.prev_txs.is_empty(), "prev_txs should be empty (caller provides)");

                for input in &sign_params.inputs {
                    assert!(!input.prev_hash.is_empty());
                    assert!(input.amount > 0);
                    assert!(input.path.starts_with("m/84'/1'/0'"));
                    assert!(matches!(input.script_type, TrezorScriptType::SpendWitness));
                }

                let has_payment = sign_params.outputs.iter().any(|o| o.address.is_some());
                assert!(has_payment, "Should have at least one payment output");
            }
            other => panic!("Expected Final, got {:?}", other),
        }
    }

    #[test]
    fn test_precompose_insufficient_funds() {
        let params = TrezorPrecomposeParams {
            outputs: vec![
                TrezorPrecomposeOutput::Payment {
                    address: "bcrt1qeyn4amkfpuz589f6x7adzclqx98akv6mvzvndp".to_string(),
                    amount: "999999999".to_string(),
                },
            ],
            coin: "Regtest".to_string(),
            account: test_compose_account(),
            fee_levels: vec![
                TrezorFeeLevel {
                    fee_per_unit: "2".to_string(),
                    base_fee: None,
                    floor_base_fee: None,
                },
            ],
            sequence: None,
            sorting_strategy: None,
        };

        let results = precompose_transaction(params);
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], TrezorPrecomposedResult::Error { .. }));
    }

    #[test]
    fn test_precompose_multiple_fee_levels() {
        let params = TrezorPrecomposeParams {
            outputs: vec![
                TrezorPrecomposeOutput::Payment {
                    address: "bcrt1qeyn4amkfpuz589f6x7adzclqx98akv6mvzvndp".to_string(),
                    amount: "50000".to_string(),
                },
            ],
            coin: "Regtest".to_string(),
            account: test_compose_account(),
            fee_levels: vec![
                TrezorFeeLevel { fee_per_unit: "1".to_string(), base_fee: None, floor_base_fee: None },
                TrezorFeeLevel { fee_per_unit: "5".to_string(), base_fee: None, floor_base_fee: None },
                TrezorFeeLevel { fee_per_unit: "20".to_string(), base_fee: None, floor_base_fee: None },
            ],
            sequence: None,
            sorting_strategy: Some(TrezorSortingStrategy::None),
        };

        let results = precompose_transaction(params);
        assert_eq!(results.len(), 3);

        let mut prev_fee = 0u64;
        for (i, result) in results.iter().enumerate() {
            match result {
                TrezorPrecomposedResult::Final { fee, .. } => {
                    let fee_val: u64 = fee.parse().unwrap();
                    assert!(fee_val > prev_fee, "Fee level {} should be higher than previous", i);
                    prev_fee = fee_val;
                }
                TrezorPrecomposedResult::Error { error } => panic!("Fee level {} failed: {}", i, error),
                _ => panic!("Expected Final for fee level {}", i),
            }
        }
    }

    #[test]
    fn test_sorting_strategy_conversion() {
        use trezor_connect_rs::compose::sorting::SortingStrategy;

        let bip69: SortingStrategy = TrezorSortingStrategy::Bip69.into();
        assert_eq!(bip69, SortingStrategy::Bip69);

        let random: SortingStrategy = TrezorSortingStrategy::Random.into();
        assert_eq!(random, SortingStrategy::Random);

        let none: SortingStrategy = TrezorSortingStrategy::None.into();
        assert_eq!(none, SortingStrategy::None);

        // Reverse
        let back: TrezorSortingStrategy = SortingStrategy::Bip69.into();
        assert!(matches!(back, TrezorSortingStrategy::Bip69));
    }

    #[test]
    fn test_script_type_reverse_conversion() {
        use trezor_connect_rs::ScriptType;

        let cases = vec![
            (ScriptType::SpendAddress, TrezorScriptType::SpendAddress),
            (ScriptType::SpendP2SHWitness, TrezorScriptType::SpendP2shWitness),
            (ScriptType::SpendWitness, TrezorScriptType::SpendWitness),
            (ScriptType::SpendTaproot, TrezorScriptType::SpendTaproot),
            (ScriptType::SpendMultisig, TrezorScriptType::SpendMultisig),
            (ScriptType::External, TrezorScriptType::External),
        ];

        for (tc_type, expected) in cases {
            let result: TrezorScriptType = tc_type.into();
            assert_eq!(result, expected);
        }
    }

    // ========================================================================
    // Previous Transaction Fetch Tests
    // ========================================================================

    #[test]
    fn test_transaction_to_prev_tx_conversion() {
        use bdk::bitcoin::consensus::deserialize;
        use bdk::bitcoin::Transaction;
        use crate::modules::trezor::account_info::transaction_to_prev_tx;

        // A minimal valid raw transaction (1 input, 1 output, version 2, locktime 0)
        // Decoded: version=2, 1 input (prev_hash=all-ones, vout=0, scriptsig=empty, seq=0xffffffff),
        //          1 output (value=50000 sats, scriptpubkey=OP_0 <20-byte-hash>)
        let raw_tx_hex = concat!(
            "02000000",                                                             // version 2
            "01",                                                                   // 1 input
            "0101010101010101010101010101010101010101010101010101010101010101",       // prev_hash
            "00000000",                                                             // prev_index 0
            "00",                                                                   // empty scriptsig
            "ffffffff",                                                             // sequence
            "01",                                                                   // 1 output
            "50c3000000000000",                                                     // value 50000 sats
            "160014",                                                               // scriptpubkey length + OP_0 PUSH20
            "0000000000000000000000000000000000000000",                             // 20 zero bytes
            "00000000",                                                             // locktime 0
        );
        let raw_tx_bytes = hex::decode(raw_tx_hex).unwrap();
        let tx: Transaction = deserialize(&raw_tx_bytes).unwrap();

        let prev_tx = transaction_to_prev_tx(&tx);

        // Verify hash matches computed txid
        assert_eq!(prev_tx.hash, tx.txid().to_string());
        assert_eq!(prev_tx.version, 2);
        assert_eq!(prev_tx.lock_time, 0);

        // Verify input mapping
        assert_eq!(prev_tx.inputs.len(), 1);
        let input = &prev_tx.inputs[0];
        assert_eq!(input.prev_hash, tx.input[0].previous_output.txid.to_string());
        assert_eq!(input.prev_index, 0);
        assert_eq!(input.script_sig, ""); // empty scriptsig
        assert_eq!(input.sequence, 0xffffffff);

        // Verify output mapping
        assert_eq!(prev_tx.outputs.len(), 1);
        let output = &prev_tx.outputs[0];
        assert_eq!(output.amount, 50000);
        assert_eq!(output.script_pubkey, "00140000000000000000000000000000000000000000");
    }

    #[test]
    fn test_fetch_prev_txs_invalid_txid() {
        use crate::modules::trezor::account_info::fetch_prev_txs;
        use crate::modules::trezor::AccountInfoError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(fetch_prev_txs(
            vec!["not_a_valid_txid".to_string()],
            "ssl://electrum.blockstream.info:60002",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AccountInfoError::InvalidTxid { .. }));
    }

    #[tokio::test]
    #[ignore]
    async fn test_fetch_prev_txs_from_electrum() {
        use crate::modules::trezor::account_info::fetch_prev_txs;

        // Use a known funded testnet address to discover txids to fetch
        let result = crate::modules::trezor::account_info::get_address_info(
            TEST_LEGACY_ADDR,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
        ).await;

        let info = result.expect("get_address_info should succeed");
        assert!(!info.utxos.is_empty(), "Need at least one UTXO for test");

        let txids: Vec<String> = info.utxos.iter().map(|u| u.txid.clone()).collect();
        let prev_txs = fetch_prev_txs(txids.clone(), ACCOUNT_INFO_ELECTRUM_URL)
            .await
            .expect("fetch_prev_txs should succeed");

        // Should have fetched at least one prev tx (deduplication may reduce count)
        assert!(!prev_txs.is_empty());

        // Every fetched tx should have non-empty inputs and outputs
        for prev_tx in &prev_txs {
            assert!(!prev_tx.hash.is_empty());
            assert!(!prev_tx.inputs.is_empty());
            assert!(!prev_tx.outputs.is_empty());
        }
    }

    #[test]
    fn test_broadcast_raw_tx_invalid_hex() {
        use crate::modules::trezor::account_info::broadcast_raw_tx;
        use crate::modules::trezor::AccountInfoError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(broadcast_raw_tx(
            "not_valid_hex".to_string(),
            "ssl://electrum.blockstream.info:60002",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AccountInfoError::ElectrumError { .. }));
    }

    #[test]
    fn test_broadcast_raw_tx_invalid_tx_data() {
        use crate::modules::trezor::account_info::broadcast_raw_tx;
        use crate::modules::trezor::AccountInfoError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        // Valid hex but not a valid transaction
        let result = rt.block_on(broadcast_raw_tx(
            "deadbeef".to_string(),
            "ssl://electrum.blockstream.info:60002",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AccountInfoError::ElectrumError { .. }));
    }
}
