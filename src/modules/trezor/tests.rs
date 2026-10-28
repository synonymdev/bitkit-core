//! Tests for the Trezor module.

#[cfg(test)]
mod tests {
    use crate::modules::trezor::{
        encode_callback_transport_error, TrezorCoinType, TrezorDeviceInfo, TrezorError,
        TrezorFeatures, TrezorScriptType, TrezorSignTxParams, TrezorSignedTx,
        TrezorTransportErrorCode, TrezorTransportType, TrezorTxInput, TrezorTxOutput,
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
    fn test_error_conversion_device_busy() {
        use trezor_connect_rs::error::TransportError;
        use trezor_connect_rs::TrezorError as TcError;

        let tc_err = TcError::Transport(TransportError::DeviceBusy);
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::DeviceBusy));
    }

    #[test]
    fn test_callback_open_device_busy_maps_to_device_busy() {
        use trezor_connect_rs::error::TransportError;
        use trezor_connect_rs::TrezorError as TcError;

        let callback_error = encode_callback_transport_error(
            "native transport busy".to_string(),
            Some(TrezorTransportErrorCode::DeviceBusy),
        );
        let tc_err = TcError::Transport(TransportError::UnableToOpen(callback_error));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::DeviceBusy));
    }

    #[test]
    fn test_callback_data_transfer_busy_maps_to_device_busy() {
        use trezor_connect_rs::error::TransportError;
        use trezor_connect_rs::TrezorError as TcError;

        let callback_error = encode_callback_transport_error(
            String::new(),
            Some(TrezorTransportErrorCode::DeviceBusy),
        );
        let tc_err = TcError::Transport(TransportError::DataTransfer(callback_error));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::DeviceBusy));
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

    // The following tests start from raw Trezor protocol `Failure` codes
    // (FailureType in proto/messages-common.proto) and assert they surface as
    // typed bitkit-core errors through the existing conversion path — i.e. a
    // wrong-PIN `Failure { code: 7 }` reaches the app as TrezorError::InvalidPin
    // rather than a generic device error.

    #[test]
    fn test_failure_code_pin_invalid_surfaces_as_invalid_pin() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        // Failure_PinInvalid = 7
        let tc_err = TcError::Device(DeviceError::from_failure(
            Some(7),
            "invalid pin".to_string(),
        ));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::InvalidPin));
    }

    #[test]
    fn test_failure_code_pin_cancelled_surfaces_as_pin_cancelled() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        // Failure_PinCancelled = 6
        let tc_err = TcError::Device(DeviceError::from_failure(Some(6), "cancelled".to_string()));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PinCancelled));
    }

    #[test]
    fn test_failure_code_pin_expected_surfaces_as_pin_required() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        // Failure_PinExpected = 5
        let tc_err = TcError::Device(DeviceError::from_failure(
            Some(5),
            "pin expected".to_string(),
        ));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::PinRequired));
    }

    #[test]
    fn test_failure_code_action_cancelled_surfaces_as_user_cancelled() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        // Failure_ActionCancelled = 4
        let tc_err = TcError::Device(DeviceError::from_failure(
            Some(4),
            "action cancelled".to_string(),
        ));
        let err: TrezorError = tc_err.into();

        assert!(matches!(err, TrezorError::UserCancelled));
    }

    #[test]
    fn test_failure_code_unknown_stays_generic_device_error() {
        use trezor_connect_rs::error::DeviceError;
        use trezor_connect_rs::TrezorError as TcError;

        // Unknown code (Failure_FirmwareError = 99) must remain a generic device
        // error so existing behavior is preserved.
        let tc_err = TcError::Device(DeviceError::from_failure(Some(99), "boom".to_string()));
        let err: TrezorError = tc_err.into();

        match err {
            TrezorError::DeviceError { error_details } => {
                assert!(error_details.contains("99"));
                assert!(error_details.contains("boom"));
            }
            other => panic!("expected generic DeviceError, got {other:?}"),
        }
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
        assert!(matches!(
            tc_type,
            trezor_connect_rs::ScriptType::SpendAddress
        ));
    }

    #[test]
    fn test_script_type_conversion_spend_p2sh_witness() {
        let trezor_type = TrezorScriptType::SpendP2shWitness;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(
            tc_type,
            trezor_connect_rs::ScriptType::SpendP2SHWitness
        ));
    }

    #[test]
    fn test_script_type_conversion_spend_witness() {
        let trezor_type = TrezorScriptType::SpendWitness;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(
            tc_type,
            trezor_connect_rs::ScriptType::SpendWitness
        ));
    }

    #[test]
    fn test_script_type_conversion_spend_taproot() {
        let trezor_type = TrezorScriptType::SpendTaproot;
        let tc_type: trezor_connect_rs::ScriptType = trezor_type.into();
        assert!(matches!(
            tc_type,
            trezor_connect_rs::ScriptType::SpendTaproot
        ));
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
        assert!(matches!(
            tc_input.script_type,
            trezor_connect_rs::ScriptType::SpendWitness
        ));
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
        assert!(matches!(
            tc_output.script_type,
            Some(trezor_connect_rs::ScriptType::SpendWitness)
        ));
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
            witnesses: None,
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
            unlocked: Some(true),
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
        assert_eq!(result.unlocked, Some(true));
        assert_eq!(result.passphrase_protection, Some(false));
        assert_eq!(result.initialized, Some(true));
        assert_eq!(result.needs_backup, Some(false));
    }

    #[test]
    fn test_features_from_trezor_connect_maps_unlocked_false() {
        use trezor_connect_rs::device::Features;

        let tc_features = Features {
            pin_protection: Some(true),
            unlocked: Some(false),
            ..Default::default()
        };

        let result: TrezorFeatures = tc_features.into();

        assert_eq!(result.pin_protection, Some(true));
        assert_eq!(result.unlocked, Some(false));
    }

    #[test]
    fn test_features_from_trezor_connect_maps_unlocked_none() {
        use trezor_connect_rs::device::Features;

        let tc_features = Features {
            pin_protection: Some(true),
            unlocked: None,
            ..Default::default()
        };

        let result: TrezorFeatures = tc_features.into();

        assert_eq!(result.pin_protection, Some(true));
        assert_eq!(result.unlocked, None);
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
        assert_eq!(
            err.to_string(),
            "Trezor not initialized. Call trezor_initialize first."
        );

        let err = TrezorError::NotConnected;
        assert_eq!(
            err.to_string(),
            "No device connected. Call trezor_connect first."
        );
    }

    #[test]
    fn test_account_type_to_script_type() {
        use crate::modules::onchain::AccountType;
        use crate::modules::trezor::account_info::account_type_to_script_type;

        assert!(matches!(
            account_type_to_script_type(AccountType::Legacy),
            TrezorScriptType::SpendAddress
        ));
        assert!(matches!(
            account_type_to_script_type(AccountType::WrappedSegwit),
            TrezorScriptType::SpendP2shWitness
        ));
        assert!(matches!(
            account_type_to_script_type(AccountType::NativeSegwit),
            TrezorScriptType::SpendWitness
        ));
        assert!(matches!(
            account_type_to_script_type(AccountType::Taproot),
            TrezorScriptType::SpendTaproot
        ));
    }

    #[test]
    fn test_script_type_reverse_conversion() {
        use trezor_connect_rs::ScriptType;

        let cases = vec![
            (ScriptType::SpendAddress, TrezorScriptType::SpendAddress),
            (
                ScriptType::SpendP2SHWitness,
                TrezorScriptType::SpendP2shWitness,
            ),
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
    // UI Callback Adapter Tests
    // ========================================================================
    //
    // These tests lock down the bridge between bitkit-core's UniFFI-friendly
    // `TrezorUiCallback` and trezor-connect-rs's `TrezorUiCallback`. The
    // passphrase variants are variant-for-variant identical across the two
    // crates; the adapter just retypes them so UniFFI's foreign-callback
    // requirements (which trezor-connect-rs intentionally doesn't depend on)
    // can stay isolated to bitkit-core.

    use crate::modules::trezor::implementation::UiCallbackAdapter;
    use crate::modules::trezor::PassphraseResponse;
    use std::sync::{Arc, Mutex};
    use trezor_connect_rs::TrezorUiCallback as TcUiCallback;

    /// Test double that returns canned responses and records call args.
    struct MockUiCallback {
        pin_response: String,
        passphrase_response: Mutex<Option<PassphraseResponse>>,
        last_passphrase_on_device: Mutex<Option<bool>>,
    }

    impl MockUiCallback {
        fn new(pin: &str, passphrase: PassphraseResponse) -> Arc<Self> {
            Arc::new(Self {
                pin_response: pin.to_string(),
                passphrase_response: Mutex::new(Some(passphrase)),
                last_passphrase_on_device: Mutex::new(None),
            })
        }
    }

    impl crate::TrezorUiCallback for MockUiCallback {
        fn on_pin_request(&self) -> String {
            self.pin_response.clone()
        }

        fn on_passphrase_request(&self, on_device: bool) -> PassphraseResponse {
            *self.last_passphrase_on_device.lock().unwrap() = Some(on_device);
            self.passphrase_response
                .lock()
                .unwrap()
                .take()
                .expect("passphrase_response consumed twice")
        }
    }

    fn adapter_with(mock: Arc<MockUiCallback>) -> UiCallbackAdapter {
        UiCallbackAdapter { callback: mock }
    }

    #[test]
    fn test_pin_adapter_empty_string_is_cancel() {
        let mock = MockUiCallback::new("", PassphraseResponse::Cancel);
        let adapter = adapter_with(mock);
        assert_eq!(adapter.on_pin_request(), None);
    }

    #[test]
    fn test_pin_adapter_value_passes_through() {
        let mock = MockUiCallback::new("123456", PassphraseResponse::Cancel);
        let adapter = adapter_with(mock);
        assert_eq!(adapter.on_pin_request(), Some("123456".to_string()));
    }

    #[test]
    fn test_passphrase_adapter_cancel_maps_to_upstream_cancel() {
        let mock = MockUiCallback::new("", PassphraseResponse::Cancel);
        let adapter = adapter_with(Arc::clone(&mock));
        assert_eq!(
            adapter.on_passphrase_request(false),
            trezor_connect_rs::PassphraseResponse::Cancel
        );
        assert_eq!(*mock.last_passphrase_on_device.lock().unwrap(), Some(false));
    }

    #[test]
    fn test_passphrase_adapter_standard_maps_to_upstream_standard() {
        // The crucial case: standard wallet must reach the device as the
        // `Standard` variant, not `Cancel`, otherwise the device will think
        // the user cancelled.
        let mock = MockUiCallback::new("", PassphraseResponse::Standard);
        let adapter = adapter_with(mock);
        assert_eq!(
            adapter.on_passphrase_request(false),
            trezor_connect_rs::PassphraseResponse::Standard
        );
    }

    #[test]
    fn test_passphrase_adapter_hidden_passes_value() {
        let mock = MockUiCallback::new(
            "",
            PassphraseResponse::Hidden {
                value: "hunter2".to_string(),
            },
        );
        let adapter = adapter_with(mock);
        assert_eq!(
            adapter.on_passphrase_request(false),
            trezor_connect_rs::PassphraseResponse::Hidden {
                value: "hunter2".to_string()
            }
        );
    }

    #[test]
    fn test_passphrase_adapter_on_device_maps_to_upstream_on_device() {
        // On-device entry: the app defers passphrase entry to the Trezor, which
        // must reach the device as the `OnDevice` variant so the library acks
        // with `on_device = true` instead of sending a host passphrase.
        let mock = MockUiCallback::new("", PassphraseResponse::OnDevice);
        let adapter = adapter_with(mock);
        assert_eq!(
            adapter.on_passphrase_request(true),
            trezor_connect_rs::PassphraseResponse::OnDevice
        );
    }

    #[test]
    fn test_passphrase_adapter_forwards_on_device_flag() {
        let mock = MockUiCallback::new("", PassphraseResponse::Standard);
        let adapter = adapter_with(Arc::clone(&mock));
        let _ = adapter.on_passphrase_request(true);
        assert_eq!(*mock.last_passphrase_on_device.lock().unwrap(), Some(true));
    }

    mod log_sanitizer {
        use crate::modules::trezor::log_sanitizer::sanitize_debug_log;

        /// Literal fixtures, so the regression test below can grep for them.
        const TEST_CREDENTIAL: &str =
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        const TEST_XPUB: &str = "xpub6ERApfZwUNrhLCkDtcHTcxd75RbzS1ed54G1LkBUHQVHQKqhMkhgbmJbZRkrgZw4koxb5JaHWkY4ALHY2grBGRjaDMzQLcgJvLJuZZvRcEL";
        const TEST_PSBT: &str = "cHNidP8BAHUCAAAAASaBcTce3/KF6Tet7qSze3gADAVmy7OtZGQXE8pCFxv2AAAAAAD+////AtPf9QUAAAAAGXapFQ==";
        const TEST_FRAME_HEX: &str =
            "042000ff0a20d3f1e5c7b9a84206ff1e2d3c4b5a69788796a5b4c3d2e1f00112233445566778899";
        const TEST_FRAME_GROUPS: &str = "04, 20, 00, ff, 0a, 20, d3, f1, e5, c7, b9, a8";
        const TEST_MNEMONIC: &str =
            "abandon ability able about above absent absorb abstract absurd abuse access accident";

        fn sanitized(message: &str) -> String {
            let (_, message) = sanitize_debug_log("THP", message);
            message
        }

        #[test]
        fn test_labeled_credential_is_redacted() {
            let output = sanitized(&format!("Loaded credential={}", TEST_CREDENTIAL));
            assert_eq!(output, "Loaded credential=<redacted>");
        }

        #[test]
        fn test_labeled_pair_like_secret_is_redacted() {
            for (message, expected) in [
                ("credential=SGVsbG8gV29ybGQ=", "credential=<redacted>"),
                (
                    "host_static_key=c2VjcmV0a2V5bWF0ZXJpYWwxMjM0NTY3ODkw=",
                    "host_static_key=<redacted>",
                ),
                ("session_id=trezor:abcd1234", "session_id=<redacted>"),
                ("token=user:hunter2", "token=<redacted>"),
            ] {
                assert_eq!(sanitized(message), expected);
            }
        }

        #[test]
        fn test_nested_secret_under_innocuous_label_is_redacted() {
            for (message, expected) in [
                (
                    r#"context={"token":"hunter2"}"#,
                    r#"context={"token":"<redacted>"}"#,
                ),
                (
                    "request=[passphrase=hunter2]",
                    "request=[passphrase=<redacted>]",
                ),
                ("detail=\"pin=1234\"", "detail=\"pin=<redacted>\""),
                ("state=token:hunter2", "state=token:<redacted>"),
            ] {
                assert_eq!(sanitized(message), expected);
            }
        }

        #[test]
        fn test_two_word_label_is_redacted() {
            let output = sanitized(&format!("seed phrase: {}", TEST_MNEMONIC));
            assert_eq!(output, "seed phrase: <redacted>");

            let output = sanitized("wallet passphrase: correct horse battery");
            assert_eq!(output, "wallet passphrase: <redacted>");
        }

        #[test]
        fn test_spaced_separator_after_multiword_value_is_redacted() {
            let output = sanitized("passphrase=hunter2 pin = 1234");
            assert_eq!(output, "passphrase=<redacted> pin = <redacted>");
        }

        #[test]
        fn test_escaped_quote_does_not_end_a_quoted_secret() {
            let output = sanitized(r#"passphrase="hunter\"tail""#);
            assert_eq!(output, r#"passphrase="<redacted>""#);
        }

        #[test]
        fn test_unclosed_quote_does_not_defeat_redaction() {
            // An unclosed quote used to match no value at all, forwarding the
            // whole pair verbatim.
            assert_eq!(sanitized(r#"passphrase="hunter2"#), "passphrase=<redacted>");
            assert_eq!(
                sanitized(&format!(r#"{{"credential": "{}"#, TEST_CREDENTIAL)),
                r#"{"credential": <redacted>"#
            );
            // An innocuous value must not lose its last character to a closing
            // delimiter that was never there.
            assert_eq!(sanitized(r#"state="paired"#), r#"state="paired"#);
        }

        #[test]
        fn test_labeled_psbt_is_redacted() {
            let output = sanitized(&format!("signing psbt={}", TEST_PSBT));
            assert_eq!(output, "signing psbt=<redacted>");
        }

        #[test]
        fn test_unexpected_secret_label_is_redacted() {
            // Matching on key fragments catches labels nobody enumerated.
            let output = sanitized(&format!(
                "thp_credential={} master_key={}",
                TEST_CREDENTIAL, TEST_XPUB
            ));
            assert_eq!(output, "thp_credential=<redacted> master_key=<redacted>");
        }

        #[test]
        fn test_json_string_and_array_values_are_redacted() {
            let output = sanitized(&format!(
                r#"{{"host_static_key": "{}", "credential": [1, 2, 3]}}"#,
                TEST_CREDENTIAL
            ));
            assert_eq!(
                output,
                r#"{"host_static_key": "<redacted>", "credential": [<redacted>]}"#
            );
        }

        #[test]
        fn test_bare_xpub_is_redacted() {
            let output = sanitized(&format!("account descriptor {} at m/84'/0'/0'", TEST_XPUB));
            assert_eq!(output, "account descriptor <redacted> at m/84'/0'/0'");
        }

        #[test]
        fn test_bare_frame_hex_is_redacted() {
            let output = sanitized(&format!("wrote frame {}", TEST_FRAME_HEX));
            assert_eq!(output, "wrote frame <redacted>");
        }

        #[test]
        fn test_grouped_frame_bytes_are_redacted() {
            // Debug formatters split a frame into byte groups, which the
            // contiguous-hex pass alone does not recognise.
            let output = sanitized(
                "wrote frame [04, 20, 00, ff, 0a, 20, d3, f1, e5, c7, b9, a8, 42, 06, ff, 1e]",
            );
            assert_eq!(output, "wrote frame [<redacted>]");

            let output = sanitized("<< 04 20 00 ff 0a 20 d3 f1 e5 c7 b9 a8");
            assert_eq!(output, "<< <redacted>");

            let output = sanitized("read [4, 32, 0, 255, 10, 32, 211, 241, 229, 199]");
            assert_eq!(output, "read [<redacted>]");
        }

        #[test]
        fn test_multiword_secret_is_redacted_in_full() {
            let output =
                sanitized("mnemonic=abandon ability able about above absent absorb abstract abuse");
            assert_eq!(output, "mnemonic=<redacted>");

            // Neighbouring diagnostics still survive.
            let output = sanitized("passphrase=correct horse battery staple, state=paired");
            assert_eq!(output, "passphrase=<redacted>, state=paired");

            let output = sanitized("passphrase=correct horse battery device=trezor");
            assert_eq!(output, "passphrase=<redacted> device=trezor");
        }

        #[test]
        fn test_bare_numeric_secret_is_redacted() {
            let output = sanitized("token=1234567890 session_id=4815162342");
            assert_eq!(output, "token=<redacted> session_id=<redacted>");

            let output = sanitized("credential_count=3, key_len=32");
            assert_eq!(output, "credential_count=3, key_len=32");
        }

        #[test]
        fn test_bare_psbt_is_redacted() {
            let output = sanitized(&format!("tx {}", TEST_PSBT));
            assert_eq!(output, "tx <redacted>");
        }

        #[test]
        fn test_connection_state_passes_through() {
            let message = "trezor_state=1 (0=needs pairing, 1=paired, 2=autoconnect)";
            assert_eq!(sanitized(message), message);
        }

        #[test]
        fn test_error_codes_pass_through() {
            let message = "Attempt 2 FAILED: THP Error: DecryptionFailed (error_code: 17)";
            assert_eq!(sanitized(message), message);
        }

        #[test]
        fn test_byte_lengths_and_booleans_pass_through() {
            // Counts and flags under a sensitive label are the diagnostics
            // worth keeping, so they must survive redaction.
            let message = "Completion payload: 48 bytes (credential_sent=true)";
            assert_eq!(sanitized(message), message);

            let message = "try_to_unlock=false, has_credentials=true";
            assert_eq!(sanitized(message), message);

            let message = "Parsed credential: host_key=32bytes, credential=139bytes";
            assert_eq!(sanitized(message), message);
        }

        #[test]
        fn test_short_hex_metadata_passes_through() {
            let message = "Channel allocated: a1b2";
            assert_eq!(sanitized(message), message);
        }

        #[test]
        fn test_tag_passes_through() {
            let (tag, _) = sanitize_debug_log("HANDSHAKE", "Creating THP session...");
            assert_eq!(tag, "HANDSHAKE");
        }

        #[test]
        fn test_long_message_is_truncated() {
            let output = sanitized(&"chunk ".repeat(200));
            assert!(output.ends_with("\u{2026}<truncated>"));
            assert!(output.chars().count() < 530);
        }

        #[test]
        fn test_multibyte_message_truncation_does_not_panic() {
            let output = sanitized(&"\u{e9}".repeat(1000));
            assert!(output.ends_with("\u{2026}<truncated>"));
        }

        #[test]
        fn test_deeply_chained_pairs_do_not_exhaust_the_stack() {
            // Redaction descends into nested values, so an unbounded input
            // used to recurse once per link and abort the process. Run on a
            // thread with a small stack to catch a regression on mobile.
            let worker = std::thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(|| sanitized(&("a=".repeat(20_000) + "1")))
                .expect("spawn");
            assert!(worker.join().expect("no stack overflow").len() < 600);
        }

        #[test]
        fn test_no_fixture_secret_survives_sanitization() {
            let fixtures = [
                format!("credential={}", TEST_CREDENTIAL),
                format!(
                    "Stored credential {} for ble:AA:BB:CC:DD:EE:FF",
                    TEST_CREDENTIAL
                ),
                format!(r#"{{"credential":"{}"}}"#, TEST_CREDENTIAL),
                format!("thp_credential={}", TEST_CREDENTIAL),
                format!("xpub={}", TEST_XPUB),
                format!("derived {} for account 0", TEST_XPUB),
                format!("psbt={}", TEST_PSBT),
                format!("Signing {}", TEST_PSBT),
                format!("frame={}", TEST_FRAME_HEX),
                format!("<< {}", TEST_FRAME_HEX),
                format!("frame=[{}]", TEST_FRAME_GROUPS),
                format!("<< {}", TEST_FRAME_GROUPS),
                format!("mnemonic={}", TEST_MNEMONIC),
                format!("recovery: {} (12 words)", TEST_MNEMONIC),
                "passphrase=hunter2 pin=1234 mnemonic=[a, b, c]".to_string(),
                "session_token=4815162342".to_string(),
                "passphrase=hunter2 pin = 1234".to_string(),
                r#"passphrase="hunter2\"1234""#.to_string(),
                r#"passphrase="hunter2"#.to_string(),
                format!(
                    r#"context={{"passphrase":"hunter2","seed":"{}"}}"#,
                    TEST_MNEMONIC
                ),
                format!("seed phrase: {}", TEST_MNEMONIC),
                format!("recovery seed = {} pin = 1234", TEST_MNEMONIC),
            ];

            for fixture in fixtures {
                for (tag, message) in [
                    sanitize_debug_log("THP", &fixture),
                    sanitize_debug_log(&fixture, "THP"),
                ] {
                    let output = format!("{} {}", tag, message);
                    for secret in [
                        TEST_CREDENTIAL,
                        TEST_XPUB,
                        TEST_PSBT,
                        TEST_FRAME_HEX,
                        "hunter2",
                        "1234",
                        "4815162342",
                        // Tails, so a redaction covering only the first word or
                        // byte group still fails the test.
                        "ability",
                        "d3, f1",
                    ] {
                        assert!(
                            !output.contains(secret),
                            "leaked {:?} from fixture {:?}: {:?}",
                            secret,
                            fixture,
                            output
                        );
                    }
                }
            }
        }
    }
}
