use super::{PassportAccount, PassportAccountExport, UrError};
use crate::onchain::{derive_base_path, detect_network_from_key, AccountType};
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpub};
use serde::Deserialize;
use std::str::FromStr;

#[derive(Deserialize)]
struct PassportJson {
    xfp: String,
    account: u32,
    bip44: Option<PassportJsonAccount>,
    bip49: Option<PassportJsonAccount>,
    bip84: Option<PassportJsonAccount>,
    bip86: Option<PassportJsonAccount>,
}

#[derive(Deserialize)]
struct PassportJsonAccount {
    deriv: String,
    xpub: String,
}

/// Parse Passport's generic JSON account export from a decoded `ur:bytes` payload.
///
/// Parses BIP44, BIP49, BIP84, and BIP86 single-signature accounts. BIP48
/// multisig entries are ignored.
#[uniffi::export]
pub fn passport_parse_account_export(data: Vec<u8>) -> Result<PassportAccountExport, UrError> {
    let export: PassportJson =
        serde_json::from_slice(&data).map_err(|error| UrError::InvalidPassportExport {
            reason: error.to_string(),
        })?;
    let master_fingerprint = normalize_fingerprint(&export.xfp)?;

    let entries = [
        (AccountType::Legacy, export.bip44),
        (AccountType::WrappedSegwit, export.bip49),
        (AccountType::NativeSegwit, export.bip84),
        (AccountType::Taproot, export.bip86),
    ];
    let accounts = entries
        .into_iter()
        .filter_map(|(account_type, account)| account.map(|account| (account_type, account)))
        .map(|(account_type, account)| parse_account(account_type, account, export.account))
        .collect::<Result<Vec<_>, _>>()?;
    if accounts.is_empty() {
        return Err(UrError::InvalidPassportExport {
            reason: "no supported single-signature accounts found".to_string(),
        });
    }

    Ok(PassportAccountExport {
        master_fingerprint,
        account_index: export.account,
        accounts,
    })
}

fn parse_account(
    account_type: AccountType,
    account: PassportJsonAccount,
    account_index: u32,
) -> Result<PassportAccount, UrError> {
    let xpub = account.xpub.trim();
    let parsed_xpub = Xpub::from_str(xpub).map_err(|error| UrError::InvalidPassportExport {
        reason: format!("invalid extended public key: {error}"),
    })?;
    let expected_child = ChildNumber::from_hardened_idx(account_index).map_err(|error| {
        UrError::InvalidPassportExport {
            reason: format!("invalid account index: {error}"),
        }
    })?;
    if parsed_xpub.depth != 3 || parsed_xpub.child_number != expected_child {
        return Err(UrError::InvalidPassportExport {
            reason: "extended public key does not match the exported account".to_string(),
        });
    }
    let network =
        detect_network_from_key(xpub).map_err(|error| UrError::InvalidPassportExport {
            reason: error.to_string(),
        })?;

    let derivation_path = account.deriv.trim();
    let parsed_path = DerivationPath::from_str(derivation_path).map_err(|error| {
        UrError::InvalidPassportExport {
            reason: format!("invalid derivation path: {error}"),
        }
    })?;
    let expected_path = derive_base_path(account_type, network, account_index);
    let expected_path = DerivationPath::from_str(&expected_path).map_err(|error| {
        UrError::InvalidPassportExport {
            reason: format!("invalid expected derivation path: {error}"),
        }
    })?;
    if parsed_path != expected_path {
        return Err(UrError::InvalidPassportExport {
            reason: format!(
                "derivation path {derivation_path} does not match the exported account"
            ),
        });
    }

    Ok(PassportAccount {
        account_type,
        xpub: xpub.to_string(),
        derivation_path: derivation_path.to_string(),
    })
}

fn normalize_fingerprint(fingerprint: &str) -> Result<String, UrError> {
    let fingerprint = fingerprint.trim();
    if fingerprint.len() != 8
        || !fingerprint
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(UrError::InvalidPassportExport {
            reason: "master fingerprint must be eight hexadecimal characters".to_string(),
        });
    }
    Ok(fingerprint.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ur::{UrDecoder, UrPayload};
    use minicbor::bytes::ByteVec;

    #[test]
    fn parses_supported_passport_accounts() {
        let json = br#"{
            "xfp": "0F056943",
            "account": 123,
            "bip44": {
                "deriv": "m/44'/1'/123'",
                "xpub": "tpubDCiHGUNYdRRGoSH22j8YnruUKgguCK1CC2NFQUf9PApeZh8ewAJJWGMUrhggDNK73iCTanWXv1RN5FYemUH8UrVUBjqDb8WF2VoKmDh9UTo"
            },
            "bip49": {
                "deriv": "m/49'/1'/123'",
                "xpub": "tpubDCDqt7XXvhAdy1MpSze5nMJA9x8DrdRaKALRRPasfxyHpiqWWEAr9cbDBQ9BcX7cB3up98Pk97U2QQ3xrvQsi5dNPmRYYhdcsKY9wwEY87T"
            },
            "bip84": {
                "deriv": "m/84'/1'/123'",
                "xpub": "tpubDC7jGaaSE66VDB6VhEDFYQSCAyugXmfnMnrMVyHNzW9wryyTxvha7TmfAHd7GRXrr2TaAn2HXn9T8ep4gyNX1bzGiieqcTUNcu2poyntrET"
            },
            "bip86": {
                "deriv": "m/86'/1'/123'",
                "xpub": "tpubDC7jGaaSE66VDB6VhEDFYQSCAyugXmfnMnrMVyHNzW9wryyTxvha7TmfAHd7GRXrr2TaAn2HXn9T8ep4gyNX1bzGiieqcTUNcu2poyntrET"
            }
        }"#;

        let cbor = minicbor::to_vec(ByteVec::from(json.to_vec())).unwrap();
        let frame = ::ur::try_encode(&cbor, &::ur::Type::Bytes).unwrap();
        let status = UrDecoder::new().receive(frame).unwrap();
        let UrPayload::Bytes { data } = status.payload.unwrap() else {
            panic!("expected a bytes payload");
        };
        let export = passport_parse_account_export(data).unwrap();
        assert_eq!(export.master_fingerprint, "0f056943");
        assert_eq!(export.account_index, 123);
        assert_eq!(export.accounts.len(), 4);
        assert_eq!(export.accounts[0].account_type, AccountType::Legacy);
        assert_eq!(export.accounts[1].account_type, AccountType::WrappedSegwit);
        assert_eq!(export.accounts[2].account_type, AccountType::NativeSegwit);
        assert_eq!(export.accounts[3].account_type, AccountType::Taproot);
    }

    #[test]
    fn rejects_xpub_from_a_different_account() {
        let mut xpub = Xpub::from_str(
            "tpubDC7jGaaSE66VDB6VhEDFYQSCAyugXmfnMnrMVyHNzW9wryyTxvha7TmfAHd7GRXrr2TaAn2HXn9T8ep4gyNX1bzGiieqcTUNcu2poyntrET",
        )
        .unwrap();
        xpub.child_number = ChildNumber::from_hardened_idx(124).unwrap();
        let json = serde_json::json!({
            "xfp": "0F056943",
            "account": 123,
            "bip84": {
                "deriv": "m/84'/1'/123'",
                "xpub": xpub.to_string(),
            },
        });

        let error = passport_parse_account_export(json.to_string().into_bytes()).unwrap_err();
        assert!(matches!(error, UrError::InvalidPassportExport { .. }));
    }
}
