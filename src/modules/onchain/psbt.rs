use base64::{engine::general_purpose::STANDARD, Engine};
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Secp256k1;
use miniscript::psbt::{interpreter_check, PsbtExt};

use super::{CompletedTransaction, PsbtCompletionError};

/// Combine and finalize a signed PSBT, then extract its broadcastable transaction.
#[uniffi::export]
pub fn finalize_psbt(
    original_psbt: String,
    signed_psbt: String,
) -> Result<CompletedTransaction, PsbtCompletionError> {
    let mut combined = parse_psbt(&original_psbt, "original")?;
    let signed = parse_psbt(&signed_psbt, "signed")?;
    let secp = Secp256k1::verification_only();
    combined
        .combine(signed)
        .map_err(|error| PsbtCompletionError::CombineFailed {
            reason: error.to_string(),
        })?;
    combined
        .finalize_mut(&secp)
        .map_err(|errors| PsbtCompletionError::FinalizationFailed {
            reason: errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })?;
    interpreter_check(&combined, &secp).map_err(|error| {
        PsbtCompletionError::VerificationFailed {
            reason: error.to_string(),
        }
    })?;

    let transaction =
        combined
            .extract_tx()
            .map_err(|error| PsbtCompletionError::ExtractionFailed {
                reason: error.to_string(),
            })?;
    Ok(CompletedTransaction {
        serialized_tx: serialize_hex(&transaction),
        txid: transaction.compute_txid().to_string(),
    })
}

fn parse_psbt(encoded: &str, label: &str) -> Result<Psbt, PsbtCompletionError> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| PsbtCompletionError::InvalidPsbt {
            reason: format!("{label} PSBT base64 decoding failed: {error}"),
        })?;
    Psbt::deserialize(&bytes).map_err(|error| PsbtCompletionError::InvalidPsbt {
        reason: format!("{label} PSBT parsing failed: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use bitcoin::key::CompressedPublicKey;
    use bitcoin::secp256k1::{Message, SecretKey};
    use bitcoin::sighash::{EcdsaSighashType, SighashCache};
    use bitcoin::transaction::Version;
    use bitcoin::{
        ecdsa, Address, Amount, Network, OutPoint, Sequence, Transaction, TxIn, TxOut, Txid,
    };

    #[test]
    fn finalizes_partially_signed_native_segwit_psbt() {
        let (original, signed) = native_segwit_psbts();

        let completed = finalize_psbt(original, signed).unwrap();
        let transaction: Transaction =
            bitcoin::consensus::deserialize(&hex::decode(&completed.serialized_tx).unwrap())
                .unwrap();

        assert_eq!(completed.txid, transaction.compute_txid().to_string());
        assert_eq!(transaction.input[0].witness.len(), 2);
    }

    #[test]
    fn rejects_signed_psbt_for_different_transaction() {
        let (original, signed) = native_segwit_psbts();
        let mut different = parse_psbt(&signed, "signed").unwrap();
        different.unsigned_tx.output[0].value = Amount::from_sat(1);

        let error = finalize_psbt(original, encode_psbt(&different)).unwrap_err();

        assert!(matches!(error, PsbtCompletionError::CombineFailed { .. }));
    }

    #[test]
    fn rejects_unsigned_psbt() {
        let (original, _) = native_segwit_psbts();

        let error = finalize_psbt(original.clone(), original).unwrap_err();

        assert!(matches!(
            error,
            PsbtCompletionError::FinalizationFailed { .. }
        ));
    }

    #[test]
    fn rejects_invalid_final_witness() {
        let (original, signed) = native_segwit_psbts();
        let mut invalid = parse_psbt(&signed, "signed").unwrap();
        invalid.inputs[0].partial_sigs.clear();
        invalid.inputs[0].final_script_witness =
            Some(bitcoin::Witness::from_slice(&[vec![0; 72], vec![0; 33]]));

        let error = finalize_psbt(original, encode_psbt(&invalid)).unwrap_err();

        assert!(matches!(
            error,
            PsbtCompletionError::VerificationFailed { .. }
        ));
    }

    fn native_segwit_psbts() -> (String, String) {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[1; 32]).unwrap();
        let public_key = bitcoin::PublicKey::new(secret_key.public_key(&secp));
        let compressed_key = CompressedPublicKey::try_from(public_key).unwrap();
        let script_pubkey = Address::p2wpkh(&compressed_key, Network::Regtest).script_pubkey();
        let spent_output = TxOut {
            value: Amount::from_sat(50_000),
            script_pubkey: script_pubkey.clone(),
        };
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::new(Txid::all_zeros(), 0),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                ..Default::default()
            }],
            output: vec![TxOut {
                value: Amount::from_sat(49_000),
                script_pubkey,
            }],
        };
        let mut signed = Psbt::from_unsigned_tx(transaction).unwrap();
        signed.inputs[0].witness_utxo = Some(spent_output.clone());
        let original = signed.clone();

        let sighash_type = EcdsaSighashType::All;
        let sighash = SighashCache::new(&signed.unsigned_tx)
            .p2wpkh_signature_hash(
                0,
                &spent_output.script_pubkey,
                spent_output.value,
                sighash_type,
            )
            .unwrap();
        let signature =
            secp.sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), &secret_key);
        signed.inputs[0].partial_sigs.insert(
            public_key,
            ecdsa::Signature {
                signature,
                sighash_type,
            },
        );

        (encode_psbt(&original), encode_psbt(&signed))
    }

    fn encode_psbt(psbt: &Psbt) -> String {
        STANDARD.encode(psbt.serialize())
    }
}
