use std::str::FromStr;

use bitcoin::bip32::Xpub;

use super::OnchainError;

/// Decode a standard BIP32 extended public key into its canonical 78-byte payload.
pub fn serialized_extended_pubkey(xpub: &str) -> Result<Vec<u8>, OnchainError> {
    Xpub::from_str(xpub)
        .map(|extended_pubkey| extended_pubkey.encode().to_vec())
        .map_err(|error| OnchainError::InvalidExtendedPublicKey {
            error_details: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAINNET_XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const MAINNET_SERIALIZED_HEX: &str = "0488b21e000000000000000000873dff81c02f525623fd1fe5167eac3a55a049de3d314bb42ee227ffed37d5080339a36013301597daef41fbe593a02cc513d0b55527ec2df1050e2e8ff49c85c2";
    const TESTNET_TPUB: &str = "tpubDDWohsp5dx2iMJ9N7iHbgAEDhH4BJB9NWW1fEW3yA3AFNDREmpzteCXNqppMLUmKFY5q5e3PXtS5CuqWCQbYcGhpPqYAgQSYdwknW9J6sQv";
    const TESTNET_SERIALIZED_HEX: &str = "043587cf03caafd489800000004b5fcc4a5fe210d9fba6616b4db1d025237dd7f035101f11f562401bc710469902e0bf22b51a6a49e0b149b995670d0ed9bb1fd99417748bacefba88fae655572d";

    #[test]
    fn mainnet_xpub_returns_canonical_78_byte_payload() {
        let serialized = serialized_extended_pubkey(MAINNET_XPUB).unwrap();

        assert_eq!(serialized.len(), 78);
        assert_eq!(hex::encode(serialized), MAINNET_SERIALIZED_HEX);
    }

    #[test]
    fn testnet_and_regtest_tpub_returns_canonical_78_byte_payload() {
        let serialized = serialized_extended_pubkey(TESTNET_TPUB).unwrap();

        assert_eq!(serialized.len(), 78);
        assert_eq!(hex::encode(serialized), TESTNET_SERIALIZED_HEX);
    }

    #[test]
    fn invalid_base58_character_is_rejected() {
        let invalid_xpub = format!("0{}", &MAINNET_XPUB[1..]);

        assert!(matches!(
            serialized_extended_pubkey(&invalid_xpub),
            Err(OnchainError::InvalidExtendedPublicKey { .. })
        ));
    }

    #[test]
    fn invalid_base58check_checksum_is_rejected() {
        let invalid_xpub = format!("{}1", &MAINNET_XPUB[..MAINNET_XPUB.len() - 1]);

        assert!(matches!(
            serialized_extended_pubkey(&invalid_xpub),
            Err(OnchainError::InvalidExtendedPublicKey { .. })
        ));
    }

    #[test]
    fn base58check_payload_without_valid_extended_public_key_is_rejected() {
        let mut payload = [0_u8; 78];
        payload[..4].copy_from_slice(&[0x04, 0x88, 0xb2, 0x1e]);
        let encoded = bitcoin::base58::encode_check(&payload);

        assert!(matches!(
            serialized_extended_pubkey(&encoded),
            Err(OnchainError::InvalidExtendedPublicKey { .. })
        ));
    }

    #[test]
    fn returned_bytes_round_trip_through_rust_bitcoin_serializer() {
        let serialized = serialized_extended_pubkey(MAINNET_XPUB).unwrap();
        let decoded = Xpub::decode(&serialized).unwrap();

        assert_eq!(decoded.to_string(), MAINNET_XPUB);
    }
}
