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
}
