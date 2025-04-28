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
}