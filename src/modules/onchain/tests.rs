#[cfg(test)]
mod tests {
    use crate::modules::onchain::{AddressType, BitcoinAddressValidator};
    use crate::modules::scanner::NetworkType;

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
}