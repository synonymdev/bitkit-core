use super::{decode_compact_seed_qr, decode_standard_seed_qr, SeedQrError};

const EXPECTED_MNEMONIC: &str =
    "forum undo fragile fade shy sign arrest garment culture tube off merit";
const STANDARD_PAYLOAD: &str = "073318950739065415961602009907670428187212261116";

#[test]
fn decodes_standard_seedqr() {
    assert_eq!(
        decode_standard_seed_qr(STANDARD_PAYLOAD.to_string()).unwrap(),
        EXPECTED_MNEMONIC
    );
}

#[test]
fn rejects_standard_seedqr_with_short_length() {
    let payload = STANDARD_PAYLOAD.strip_suffix('6').unwrap().to_string();

    assert_eq!(
        decode_standard_seed_qr(payload),
        Err(SeedQrError::InvalidStandardPayload)
    );
}

#[test]
fn rejects_standard_seedqr_with_overlong_length() {
    let payload = format!("{STANDARD_PAYLOAD}0");

    assert_eq!(
        decode_standard_seed_qr(payload),
        Err(SeedQrError::InvalidStandardPayload)
    );
}

#[test]
fn rejects_standard_seedqr_with_invalid_checksum() {
    let payload = "0000".repeat(12);

    assert_eq!(
        decode_standard_seed_qr(payload),
        Err(SeedQrError::InvalidMnemonic)
    );
}

#[test]
fn rejects_standard_seedqr_with_non_digit() {
    let payload = format!("{}x", "0000".repeat(12).strip_suffix('0').unwrap());

    assert_eq!(
        decode_standard_seed_qr(payload),
        Err(SeedQrError::InvalidStandardPayload)
    );
}

#[test]
fn rejects_standard_seedqr_with_out_of_range_index() {
    let payload = format!("2048{}", "0000".repeat(11));

    assert_eq!(
        decode_standard_seed_qr(payload),
        Err(SeedQrError::InvalidStandardPayload)
    );
}

#[test]
fn decodes_compact_seedqr() {
    let entropy = hex::decode("5bbd9d71a8ec7990831aff359d426545").unwrap();

    assert_eq!(decode_compact_seed_qr(entropy).unwrap(), EXPECTED_MNEMONIC);
}

#[test]
fn decodes_compact_seedqr_containing_null_bytes() {
    assert_eq!(
        decode_compact_seed_qr(vec![0; 16]).unwrap(),
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    );
}

#[test]
fn rejects_compact_seedqr_with_invalid_length() {
    assert_eq!(
        decode_compact_seed_qr(vec![0; 15]),
        Err(SeedQrError::InvalidCompactPayload)
    );
}
