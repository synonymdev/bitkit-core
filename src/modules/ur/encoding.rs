use super::{UrError, MAX_FRAGMENT_COUNT, MAX_FRAME_LENGTH};
use base64::{engine::general_purpose::STANDARD, Engine};
use bitcoin::psbt::Psbt;
use minicbor::bytes::ByteVec;

const CRYPTO_PSBT: &str = "crypto-psbt";

/// Encode a base64 PSBT as one cycle of `crypto-psbt` UR frames.
///
/// The returned cycle contains every original fountain fragment once. Apps can
/// loop it while rendering an animated QR.
#[uniffi::export]
pub fn ur_encode_crypto_psbt(
    psbt: String,
    max_fragment_length: u32,
) -> Result<Vec<String>, UrError> {
    let psbt_bytes = STANDARD
        .decode(psbt)
        .map_err(|error| UrError::InvalidPsbt {
            reason: format!("base64 decoding failed: {error}"),
        })?;
    Psbt::deserialize(&psbt_bytes).map_err(|error| UrError::InvalidPsbt {
        reason: error.to_string(),
    })?;

    let cbor =
        minicbor::to_vec(ByteVec::from(psbt_bytes)).map_err(|error| UrError::InvalidPayload {
            reason: format!("CBOR encoding failed: {error}"),
        })?;
    encode_registry_item(
        CRYPTO_PSBT,
        &cbor,
        usize::try_from(max_fragment_length).map_err(|error| UrError::InvalidUr {
            reason: format!("invalid maximum fragment length: {error}"),
        })?,
    )
}

fn encode_registry_item(
    ur_type: &str,
    cbor: &[u8],
    max_fragment_length: usize,
) -> Result<Vec<String>, UrError> {
    if max_fragment_length == 0 {
        return Err(UrError::InvalidUr {
            reason: "maximum fragment length must be greater than zero".to_string(),
        });
    }

    let fragment_count = cbor.len().div_ceil(max_fragment_length);
    if fragment_count > MAX_FRAGMENT_COUNT {
        return Err(UrError::TooLarge {
            reason: format!(
                "message requires {fragment_count} fragments; maximum is {MAX_FRAGMENT_COUNT}"
            ),
        });
    }

    if cbor.len() <= max_fragment_length {
        let part =
            ::ur::ur::try_encode(cbor, &::ur::ur::Type::Custom(ur_type)).map_err(invalid_ur)?;
        return Ok(vec![validate_encoded_frame(part)?]);
    }

    let mut encoder = ::ur::Encoder::new(cbor, max_fragment_length, ur_type).map_err(invalid_ur)?;
    (0..fragment_count)
        .map(|_| {
            encoder
                .next_part()
                .map_err(invalid_ur)
                .and_then(validate_encoded_frame)
        })
        .collect()
}

fn validate_encoded_frame(frame: String) -> Result<String, UrError> {
    if frame.len() > MAX_FRAME_LENGTH {
        return Err(UrError::TooLarge {
            reason: format!(
                "encoded frame has {} characters; maximum is {MAX_FRAME_LENGTH}",
                frame.len()
            ),
        });
    }
    Ok(frame)
}

fn invalid_ur(error: ::ur::ur::Error) -> UrError {
    UrError::InvalidUr {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{absolute::LockTime, transaction::Version, Transaction};

    fn empty_psbt_base64() -> String {
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        let psbt = Psbt::from_unsigned_tx(transaction).unwrap();
        STANDARD.encode(psbt.serialize())
    }

    #[test]
    fn encodes_single_part_psbt_when_payload_fits() {
        let psbt = empty_psbt_base64();

        let single = ur_encode_crypto_psbt(psbt, 1_000).unwrap();
        assert_eq!(single.len(), 1);
        assert!(single[0].starts_with("ur:crypto-psbt/"));
        assert_eq!(single[0].matches('/').count(), 1);
    }

    #[test]
    fn matches_crypto_psbt_registry_vector() {
        let cbor =
            hex::decode("58208c05c4b4f3e88840a4f4b5f155cfd69473ea169f3d0431b7a6787a23777f08aa")
                .unwrap();
        let encoded = encode_registry_item("crypto-psbt", &cbor, 1_000).unwrap();

        assert_eq!(
            encoded,
            ["ur:crypto-psbt/hdcxlkahssqzwfvslofzoxwkrewngotktbmwjkwdcmnefsaaehrlolkskncnktlbaypkvoonhknt"]
        );
    }

    #[test]
    fn rejects_excessive_fragment_count_before_encoding() {
        let error = encode_registry_item("bytes", &vec![0; MAX_FRAGMENT_COUNT + 1], 1).unwrap_err();
        assert!(matches!(error, UrError::TooLarge { .. }));
    }

    #[test]
    fn rejects_oversized_single_part_frame() {
        let error = encode_registry_item("bytes", &vec![0; MAX_FRAME_LENGTH], MAX_FRAME_LENGTH)
            .unwrap_err();

        assert!(matches!(error, UrError::TooLarge { .. }));
    }

    #[test]
    fn rejects_oversized_multipart_frame() {
        let error = encode_registry_item("bytes", &vec![0; MAX_FRAME_LENGTH], MAX_FRAME_LENGTH / 2)
            .unwrap_err();

        assert!(matches!(error, UrError::TooLarge { .. }));
    }
}
