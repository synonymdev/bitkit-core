#[cfg(test)]
mod tests {
    use crate::modules::onchain::{AddressType, BitcoinAddressValidator};
    use crate::modules::scanner::NetworkType;
    use crate::onchain::types::WordCount;
    use bitcoin::Network;

    #[test]
    fn test_address_types() {
        let test_cases = vec![
            ("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2", AddressType::P2PKH, "Legacy"),
            ("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", AddressType::P2SH, "SegWit"),
            ("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", AddressType::P2WPKH, "Native SegWit"),
            ("bc1pt2a0lztpd6ejcswsxaw3n5l56jvf0yu0ah6fcapgqfs7hx9fyf0sufnaej", AddressType::P2TR, "Taproot"),
        ];

        for (address, expected_type, expected_common) in test_cases {
            let result = BitcoinAddressValidator::validate_address(address).unwrap();
            assert_eq!(result.address_type, expected_type);
            assert_eq!(result.address_type.common_name(), expected_common);
            println!("Address Type: {}", result.address_type.common_name());
        }
    }

    #[test]
    fn test_valid_mainnet_addresses() {
        let test_cases = vec![
            "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2",
            "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
            "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4",
        ];

        for address in test_cases {
            let result = BitcoinAddressValidator::validate_address(address);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().network, NetworkType::Bitcoin);
        }
    }

    #[test]
    fn test_valid_testnet_addresses() {
        let test_cases = vec![
            "n31ZLqqyfoYu4fjd16u7ZQaSqgGfrmw8wC",
            "2N2BF5jm57eetVzT4DhxFak2rVpQuFHkyF3",
            "tb1q7hau47t3mflfne784w8wdupu6wga0k3dgpquzr",
        ];

        for address in test_cases {
            let result = BitcoinAddressValidator::validate_address(address);
            assert!(result.is_ok());
            assert_eq!(result.unwrap().network, NetworkType::Testnet);
        }
    }

    #[test]
    fn test_invalid_addresses() {
        let test_cases = vec![
            "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN3",
            "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7",
            "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVO0",
        ];

        for address in test_cases {
            assert!(BitcoinAddressValidator::validate_address(address).is_err());
        }
    }

    #[test]
    fn test_generate_mnemonic() {
        // Test default word count (should be 12 words)
        let mnemonic = BitcoinAddressValidator::genenerate_mnemonic(None).unwrap();
        assert_eq!(mnemonic.split_whitespace().count(), 12);

        // Test with 24 words
        let mnemonic = BitcoinAddressValidator::genenerate_mnemonic(Some(WordCount::Words24)).unwrap();
        assert_eq!(mnemonic.split_whitespace().count(), 24);
    }

    #[test]
    fn test_derive_bitcoin_address() {
        // Use the standard test mnemonic
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        // Test P2PKH (Legacy) address derivation
        let path = "m/44'/0'/0'/0/0";
        let result = BitcoinAddressValidator::derive_bitcoin_address(
            mnemonic,
            Some(path),
            Some(Network::Bitcoin),
            None,
        ).unwrap();

        assert_eq!(result.address, "1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA");
        assert_eq!(result.path, path);

        // Test P2WPKH (Native SegWit) address derivation
        let path = "m/84'/0'/0'/0/0";
        let result = BitcoinAddressValidator::derive_bitcoin_address(
            mnemonic,
            Some(path),
            Some(Network::Bitcoin),
            None,
        ).unwrap();

        assert_eq!(result.address, "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
        assert_eq!(result.path, path);

        // Test P2SH-WPKH address derivation
        let path = "m/49'/0'/0'/0/0";
        let result = BitcoinAddressValidator::derive_bitcoin_address(
            mnemonic,
            Some(path),
            Some(Network::Bitcoin),
            None,
        ).unwrap();

        assert_eq!(result.address, "37VucYSaXLCAsxYyAPfbSi9eh4iEcbShgf");
        assert_eq!(result.path, path);
    }

    #[test]
    fn test_derive_bitcoin_addresses() {
        // Use the standard test mnemonic
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let base_path = "m/84'/0'/0'";

        // Test deriving multiple addresses
        let result = BitcoinAddressValidator::derive_bitcoin_addresses(
            mnemonic,
            Some(base_path),
            Some(Network::Bitcoin),
            None,
            None,
            None,
            Some(3),
        ).unwrap();

        // Check count and correct paths
        assert_eq!(result.addresses.len(), 3);
        assert_eq!(result.addresses[0].path, "m/84'/0'/0'/0/0");
        assert_eq!(result.addresses[1].path, "m/84'/0'/0'/0/1");
        assert_eq!(result.addresses[2].path, "m/84'/0'/0'/0/2");

        // Verify first address matches expected
        assert_eq!(result.addresses[0].address, "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");

        // Test change addresses derivation
        let result = BitcoinAddressValidator::derive_bitcoin_addresses(
            mnemonic,
            Some(base_path),
            Some(Network::Bitcoin),
            None,
            Some(true),
            None,
            Some(2),
        ).unwrap();

        // Check change addresses use correct paths
        assert_eq!(result.addresses.len(), 2);
        assert_eq!(result.addresses[0].path, "m/84'/0'/0'/1/0");
        assert_eq!(result.addresses[1].path, "m/84'/0'/0'/1/1");

        // Test with custom start index
        let result = BitcoinAddressValidator::derive_bitcoin_addresses(
            mnemonic,
            Some(base_path),
            Some(Network::Bitcoin),
            None,
            None,
            Some(5),
            Some(2),
        ).unwrap();

        assert_eq!(result.addresses.len(), 2);
        assert_eq!(result.addresses[0].path, "m/84'/0'/0'/0/5");
        assert_eq!(result.addresses[1].path, "m/84'/0'/0'/0/6");
    }

    #[test]
    fn test_derive_private_key() {
        // Use the standard test mnemonic
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        // Test for P2WPKH path
        let path = "m/84'/0'/0'/0/0";
        let private_key = BitcoinAddressValidator::derive_private_key(
            mnemonic,
            Some(path),
            Some(Network::Bitcoin),
            None,
        ).unwrap();

        assert_eq!(private_key, "KyZpNDKnfs94vbrwhJneDi77V6jF64PWPF8x5cdJb8ifgg2DUc9d");

        // Test for P2PKH path
        let path = "m/44'/0'/0'/0/0";
        let private_key = BitcoinAddressValidator::derive_private_key(
            mnemonic,
            Some(path),
            Some(Network::Bitcoin),
            None,
        ).unwrap();

        assert_eq!(private_key, "L4p2b9VAf8k5aUahF1JCJUzZkgNEAqLfq8DDdQiyAprQAKSbu8hf");
    }

    #[test]
    fn test_validate_mnemonic() {
        let valid_mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(BitcoinAddressValidator::validate_mnemonic(valid_mnemonic).is_ok());

        let invalid_mnemonic = "invalid word sequence that is not valid";
        assert!(BitcoinAddressValidator::validate_mnemonic(invalid_mnemonic).is_err());
    }

    #[test]
    fn test_is_valid_bip39_word() {
        assert!(BitcoinAddressValidator::is_valid_bip39_word("abandon"));
        assert!(BitcoinAddressValidator::is_valid_bip39_word("ABANDON"));
        assert!(!BitcoinAddressValidator::is_valid_bip39_word("notaword"));
    }

    #[test]
    fn test_get_bip39_suggestions() {
        let suggestions = BitcoinAddressValidator::get_bip39_suggestions("ab", 5);
        assert!(!suggestions.is_empty());
        assert!(suggestions.contains(&"abandon".to_string()));
        assert!(suggestions.len() <= 5);
    }

    #[test]
    fn test_get_bip39_wordlist() {
        let wordlist = BitcoinAddressValidator::get_bip39_wordlist();
        assert_eq!(wordlist.len(), 2048);
        assert!(wordlist.contains(&"abandon".to_string()));
        assert!(wordlist.contains(&"zoo".to_string()));
    }

    #[test]
    fn test_mnemonic_entropy_conversion() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let entropy = BitcoinAddressValidator::mnemonic_to_entropy(mnemonic).unwrap();
        assert_eq!(entropy.len(), 16);

        let recovered_mnemonic = BitcoinAddressValidator::entropy_to_mnemonic(&entropy).unwrap();
        assert_eq!(mnemonic, recovered_mnemonic);
    }

    #[test]
    fn test_mnemonic_to_seed() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        let seed1 = BitcoinAddressValidator::mnemonic_to_seed(mnemonic, None).unwrap();
        assert_eq!(seed1.len(), 64);

        let seed2 = BitcoinAddressValidator::mnemonic_to_seed(mnemonic, Some("passphrase")).unwrap();
        assert_eq!(seed2.len(), 64);

        assert_ne!(seed1, seed2);
    }

    const REGTEST_ELECTRUM_URL: &str = "ssl://fulcrum.bitkit.stag0.blocktank.to:18484";

    #[tokio::test]
    #[ignore]
    async fn test_check_sweepable_balances_no_utxos() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let electrum_url = REGTEST_ELECTRUM_URL;

        let result = BitcoinAddressValidator::check_sweepable_balances(
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
        )
        .await;

        assert!(result.is_ok());
        let balances = result.unwrap();
        assert_eq!(balances.legacy_balance, 0);
        assert_eq!(balances.p2sh_balance, 0);
        assert_eq!(balances.taproot_balance, 0);
        assert_eq!(balances.total_balance, 0);
        assert_eq!(balances.legacy_utxos_count, 0);
        assert_eq!(balances.p2sh_utxos_count, 0);
        assert_eq!(balances.taproot_utxos_count, 0);
        assert_eq!(balances.total_utxos_count, 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_check_sweepable_balances_invalid_electrum_url() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let invalid_url = "invalid://url";

        let result = BitcoinAddressValidator::check_sweepable_balances(
            mnemonic,
            Network::Regtest,
            None,
            invalid_url,
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_check_sweepable_balances_invalid_mnemonic() {
        let invalid_mnemonic = "invalid mnemonic phrase";
        let electrum_url = REGTEST_ELECTRUM_URL;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(BitcoinAddressValidator::check_sweepable_balances(
            invalid_mnemonic,
            Network::Regtest,
            None,
            electrum_url,
        ));

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::onchain::SweepError::InvalidMnemonic => {}
            crate::onchain::SweepError::SweepFailed(_) => {}
            _ => {}
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_prepare_sweep_transaction_no_utxos() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let electrum_url = REGTEST_ELECTRUM_URL;
        let destination = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"; // Regtest address

        let result = BitcoinAddressValidator::prepare_sweep_transaction(
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
            destination,
            None,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::onchain::SweepError::NoUtxosFound => {}
            _ => panic!("Expected NoUtxosFound error"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_prepare_sweep_transaction_invalid_electrum_url() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let invalid_url = "invalid://url";
        let destination = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

        let result = BitcoinAddressValidator::prepare_sweep_transaction(
            mnemonic,
            Network::Regtest,
            None,
            invalid_url,
            destination,
            None,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore]
    async fn test_prepare_sweep_transaction_invalid_destination_address() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let electrum_url = REGTEST_ELECTRUM_URL;
        let invalid_destination = "invalid-address";

        let result = BitcoinAddressValidator::prepare_sweep_transaction(
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
            invalid_destination,
            None,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::onchain::SweepError::SweepFailed(msg)
                if msg.contains("Invalid destination address") => {}
            _ => panic!("Expected invalid destination address error"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_broadcast_sweep_transaction_invalid_psbt() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let electrum_url = REGTEST_ELECTRUM_URL;
        let invalid_psbt = "invalid-base64-psbt";

        let result = BitcoinAddressValidator::broadcast_sweep_transaction(
            invalid_psbt,
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::onchain::SweepError::SweepFailed(msg)
                if msg.contains("Failed to decode PSBT") => {}
            _ => panic!("Expected PSBT decode error"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_broadcast_sweep_transaction_invalid_electrum_url() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let invalid_url = "invalid://url";
        let dummy_psbt = "cHNidP8BAH0CAAAAASu6BAgAAAAAGXapFGDDrd5by2g1111wz6DkNzqotA8jAQAAAAD9////AgAAAAAB6kQAAAAAGXapFNDrd5by2g1111wz6DkNzqotA8jAQAAAAD9////AAAAAAEBIICWmAAAAAAZAAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+P0BBQkNERUZHSElKS0xNTk9QUVJTVFVWV1hZWltcXV5fYGFiY2RlZmdoaWprbG1ub3BxcnN0dXZ3eHl6e3x9fn+AgYKDhIWGh4iJiouMjY6PkJGSk5SVlpeYmZqbnJ2en6ChoqOkpaanqKmqq6ytrq+wsbKztLW2t7i5uru8vb6/wMHCw8TFxsfIycrLzM3Oz9DR0tPU1dbX2Nna29zd3t/g4eLj5OXm5+jp6uvs7e7v8PHy8/T19vf4+fr7/P3+/wAAAAD/////AQAAAAAAAAAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4/QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl9gYWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXp7fH1+f4CBgoOEhYaHiImKi4yNjo+QkZKTlJWWl5iZmpucnZ6foKGio6SlpqeoqaqrrK2ur7CxsrO0tba3uLm6u7y9vr/AwcLDxMXGx8jJysvMzc7P0NHS09TV1tfY2drb3N3e3+Dh4uPk5ebn6Onq6+zt7u/w8fLz9PX29/j5+vv8/f7/";

        let result = BitcoinAddressValidator::broadcast_sweep_transaction(
            dummy_psbt,
            mnemonic,
            Network::Regtest,
            None,
            invalid_url,
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_prepare_sweep_transaction_parameter_validation() {
        let invalid_mnemonic = "invalid mnemonic phrase";
        let electrum_url = REGTEST_ELECTRUM_URL;
        let destination = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(BitcoinAddressValidator::prepare_sweep_transaction(
            invalid_mnemonic,
            Network::Regtest,
            None,
            electrum_url,
            destination,
            None,
        ));

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::onchain::SweepError::InvalidMnemonic => {}
            crate::onchain::SweepError::SweepFailed(_) => {}
            _ => {}
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_sweep_success() {
        // Requires: funded mnemonic on regtest with UTXOs in legacy/p2sh/taproot addresses
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let electrum_url = REGTEST_ELECTRUM_URL;
        let destination = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";

        let balances = BitcoinAddressValidator::check_sweepable_balances(
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
        )
        .await
        .expect("Failed to check balances");

        assert!(balances.total_balance > 0, "Mnemonic must be funded to run this test");

        let preview = BitcoinAddressValidator::prepare_sweep_transaction(
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
            destination,
            Some(1),
        )
        .await
        .expect("Failed to prepare sweep");

        assert!(preview.total_amount > 0);
        assert!(preview.estimated_fee > 0);
        assert!(preview.amount_after_fees > 0);
        assert!(preview.utxos_count > 0);

        let result = BitcoinAddressValidator::broadcast_sweep_transaction(
            &preview.psbt,
            mnemonic,
            Network::Regtest,
            None,
            electrum_url,
        )
        .await
        .expect("Failed to broadcast sweep");

        assert!(!result.txid.is_empty());
        assert!(result.amount_swept > 0);
        assert!(result.fee_paid > 0);
    }

    #[test]
    fn test_broadcast_raw_tx_invalid_hex() {
        use crate::modules::onchain::broadcast_raw_tx;
        use crate::modules::onchain::BroadcastError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(broadcast_raw_tx(
            "not_valid_hex".to_string(),
            "ssl://electrum.blockstream.info:60002",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BroadcastError::InvalidHex { .. }));
    }

    #[test]
    fn test_broadcast_raw_tx_invalid_tx_data() {
        use crate::modules::onchain::broadcast_raw_tx;
        use crate::modules::onchain::BroadcastError;

        let rt = tokio::runtime::Runtime::new().unwrap();
        // Valid hex but not a valid transaction
        let result = rt.block_on(broadcast_raw_tx(
            "deadbeef".to_string(),
            "ssl://electrum.blockstream.info:60002",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BroadcastError::InvalidTransaction { .. }));
    }

    // ========================================================================
    // Account Info Tests
    // ========================================================================

    const ACCOUNT_INFO_ELECTRUM_URL: &str = "ssl://fulcrum.bitkit.stag0.blocktank.to:18484";

    const TEST_TPUB: &str = "tpubDDWohsp5dx2iMJ9N7iHbgAEDhH4BJB9NWW1fEW3yA3AFNDREmpzteCXNqppMLUmKFY5q5e3PXtS5CuqWCQbYcGhpPqYAgQSYdwknW9J6sQv";
    const TEST_UPUB: &str = "upub5DWPhYKrgLiETEmgLykymFzBK6gXUNvkE67HLSEh5zcWNRdx2Hxd8HypxaDaK1p62a1kXwe9eBcV3pGm7yEpHh4ebrspSoer4x8Ko29egtv";
    const TEST_VPUB: &str = "vpub5ZgC33hDPuhHSDYrGeoi685MKtdAUB3pkS4rarJ5dv3RABneKLCbhJu2FNfcajo1GukBAwEMBXZWvU7fykTeyKEJrgV6E6NC7BZ3jzp3ffp";

    const TEST_LEGACY_ADDR: &str = "mixttbUXpVWVpx3qHh7KUiVnxWiNxL2uu9";
    const TEST_P2SH_ADDR: &str = "2N7mA1KBJX8iprzzoFjcbkY1Z2WRZ3bjnsK";
    const TEST_REGTEST_BECH32_ADDR: &str = "bcrt1qj2gz3meule5mc4r4knv65vjds3g88rlxs0jlmq";

    // --- Unit Tests: Helper Functions ---

    #[test]
    fn test_detect_account_type() {
        use crate::modules::onchain::detect_account_type;
        use crate::modules::onchain::AccountType;

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
        use crate::modules::onchain::detect_network_from_key;
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
        use crate::modules::onchain::normalize_extended_key;

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
        use crate::modules::onchain::build_descriptors;
        use crate::modules::onchain::AccountType;

        let test_key = "tpub_test_key";

        let (ext, int) = build_descriptors(test_key, AccountType::Legacy, None);
        assert_eq!(ext, "pkh(tpub_test_key/0/*)");
        assert_eq!(int, "pkh(tpub_test_key/1/*)");

        let (ext, int) = build_descriptors(test_key, AccountType::WrappedSegwit, None);
        assert_eq!(ext, "sh(wpkh(tpub_test_key/0/*))");
        assert_eq!(int, "sh(wpkh(tpub_test_key/1/*))");

        let (ext, int) = build_descriptors(test_key, AccountType::NativeSegwit, None);
        assert_eq!(ext, "wpkh(tpub_test_key/0/*)");
        assert_eq!(int, "wpkh(tpub_test_key/1/*)");

        let (ext, int) = build_descriptors(test_key, AccountType::Taproot, None);
        assert_eq!(ext, "tr(tpub_test_key/0/*)");
        assert_eq!(int, "tr(tpub_test_key/1/*)");

        // With key origin info
        let origin = Some(("73c5da0a", "84'/0'/0'"));
        let (ext, int) = build_descriptors(test_key, AccountType::NativeSegwit, origin);
        assert_eq!(ext, "wpkh([73c5da0a/84'/0'/0']tpub_test_key/0/*)");
        assert_eq!(int, "wpkh([73c5da0a/84'/0'/0']tpub_test_key/1/*)");
    }

    #[test]
    fn test_derive_base_path() {
        use crate::modules::onchain::derive_base_path;
        use crate::modules::onchain::AccountType;
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

    // --- Integration Tests: get_account_info ---

    #[tokio::test]
    #[ignore]
    async fn test_get_account_info_tpub() {
        use crate::modules::onchain::get_account_info;
        use crate::modules::onchain::AccountType;

        let result = get_account_info(
            TEST_TPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
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
        use crate::modules::onchain::get_account_info;
        use crate::modules::onchain::AccountType;

        let result = get_account_info(
            TEST_UPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
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
        use crate::modules::onchain::get_account_info;
        use crate::modules::onchain::AccountType;

        let result = get_account_info(
            TEST_VPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
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
        use crate::modules::onchain::get_address_info;

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
        use crate::modules::onchain::get_address_info;

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
        use crate::modules::onchain::get_address_info;

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
        use crate::modules::onchain::get_account_info;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(get_account_info(
            "not_a_valid_xpub",
            ACCOUNT_INFO_ELECTRUM_URL,
            None,
            None,
            None,
        ));

        assert!(result.is_err(), "Expected error for invalid key");
    }

    #[test]
    fn test_get_account_info_network_mismatch() {
        use crate::modules::onchain::get_account_info;
        use crate::modules::onchain::{AccountInfoError, Network as OnchainNetwork};

        let rt = tokio::runtime::Runtime::new().unwrap();
        // tpub is testnet, but we specify Bitcoin (mainnet) — should get NetworkMismatch
        let result = rt.block_on(get_account_info(
            TEST_TPUB,
            ACCOUNT_INFO_ELECTRUM_URL,
            Some(OnchainNetwork::Bitcoin),
            None,
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
        use crate::modules::onchain::get_address_info;
        use crate::modules::onchain::AccountInfoError;

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
        use crate::modules::onchain::get_address_info;

        let result = get_address_info(
            TEST_LEGACY_ADDR,
            "invalid://url",
            None,
        )
        .await;

        assert!(result.is_err(), "Expected error for invalid electrum URL");
    }

    // ========================================================================
    // Compose Transaction Tests (BDK-based, signer-agnostic)
    // ========================================================================

    fn test_wallet_params(fingerprint: Option<String>) -> crate::modules::onchain::WalletParams {
        crate::modules::onchain::WalletParams {
            extended_key: TEST_VPUB.to_string(),
            electrum_url: ACCOUNT_INFO_ELECTRUM_URL.to_string(),
            fingerprint,
            network: None,
            account_type: None,
            gap_limit: None,
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_compose_basic_payment() {
        use crate::modules::onchain::{compose_transaction, ComposeParams, ComposeOutput, ComposeResult};

        let params = ComposeParams {
            wallet: test_wallet_params(None),
            outputs: vec![ComposeOutput::Payment {
                address: TEST_REGTEST_BECH32_ADDR.to_string(),
                amount_sats: 5_000,
            }],
            fee_rates: vec![2.0],
            coin_selection: None,
        };

        let results = compose_transaction(params).await;
        assert_eq!(results.len(), 1);

        match &results[0] {
            ComposeResult::Success { psbt, fee, vsize, total_spent, .. } => {
                assert!(!psbt.is_empty(), "PSBT should not be empty");
                assert!(*fee > 0, "Fee should be > 0");
                assert!(*vsize > 0, "vsize should be > 0");
                assert!(*total_spent > 5_000, "total_spent should be > payment amount");

                use base64::{engine::general_purpose, Engine as _};
                let decoded = general_purpose::STANDARD.decode(psbt);
                assert!(decoded.is_ok(), "PSBT should be valid base64");
            }
            ComposeResult::Error { error } => panic!("Compose failed: {}", error),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_compose_send_max() {
        use crate::modules::onchain::{compose_transaction, ComposeParams, ComposeOutput, ComposeResult};

        let params = ComposeParams {
            wallet: test_wallet_params(None),
            outputs: vec![ComposeOutput::SendMax {
                address: TEST_REGTEST_BECH32_ADDR.to_string(),
            }],
            fee_rates: vec![1.0],
            coin_selection: None,
        };

        let results = compose_transaction(params).await;
        assert_eq!(results.len(), 1);

        match &results[0] {
            ComposeResult::Success { fee, total_spent, .. } => {
                assert!(*fee > 0, "Fee should be > 0");
                assert!(*total_spent > 0, "Should have funds to send");
            }
            ComposeResult::Error { error } => panic!("SendMax compose failed: {}", error),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_compose_insufficient_funds() {
        use crate::modules::onchain::{compose_transaction, ComposeParams, ComposeOutput, ComposeResult};

        let params = ComposeParams {
            wallet: test_wallet_params(None),
            outputs: vec![ComposeOutput::Payment {
                address: TEST_REGTEST_BECH32_ADDR.to_string(),
                amount_sats: 999_999_999_999,
            }],
            fee_rates: vec![2.0],
            coin_selection: None,
        };

        let results = compose_transaction(params).await;
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], ComposeResult::Error { .. }));
    }

    #[tokio::test]
    #[ignore]
    async fn test_compose_multiple_fee_rates() {
        use crate::modules::onchain::{compose_transaction, ComposeParams, ComposeOutput, ComposeResult};

        let params = ComposeParams {
            wallet: test_wallet_params(None),
            outputs: vec![ComposeOutput::Payment {
                address: TEST_REGTEST_BECH32_ADDR.to_string(),
                amount_sats: 5_000,
            }],
            fee_rates: vec![1.0, 5.0, 20.0],
            coin_selection: None,
        };

        let results = compose_transaction(params).await;
        assert_eq!(results.len(), 3);

        let mut prev_fee = 0u64;
        for (i, result) in results.iter().enumerate() {
            match result {
                ComposeResult::Success { fee, .. } => {
                    assert!(*fee > prev_fee, "Fee level {} ({} sats) should be > previous ({} sats)", i, fee, prev_fee);
                    prev_fee = *fee;
                }
                ComposeResult::Error { error } => panic!("Fee level {} failed: {}", i, error),
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_compose_with_fingerprint() {
        use crate::modules::onchain::{compose_transaction, ComposeParams, ComposeOutput, ComposeResult};

        let params = ComposeParams {
            wallet: test_wallet_params(Some("73c5da0a".to_string())),
            outputs: vec![ComposeOutput::Payment {
                address: TEST_REGTEST_BECH32_ADDR.to_string(),
                amount_sats: 5_000,
            }],
            fee_rates: vec![2.0],
            coin_selection: None,
        };

        let results = compose_transaction(params).await;
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], ComposeResult::Success { .. }), "Compose with fingerprint should succeed");
    }
}
