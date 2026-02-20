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
        };

        let result: TrezorSignedTx = response.into();

        assert_eq!(result.signatures, vec!["sig1", "sig2"]);
        assert_eq!(result.serialized_tx, "rawtx");
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
}
