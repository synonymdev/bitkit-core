use super::SeedQrError;
use crate::onchain::BitcoinAddressValidator;

const COMPACT_ENTROPY_LENGTH: usize = 16;
const STANDARD_PAYLOAD_LENGTH: usize = 48;
const WORD_COUNT: usize = 12;
const WORD_INDEX_LENGTH: usize = 4;

#[uniffi::export]
pub fn decode_standard_seed_qr(payload: String) -> Result<String, SeedQrError> {
    if payload.len() != STANDARD_PAYLOAD_LENGTH
        || !payload.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(SeedQrError::InvalidStandardPayload);
    }

    let wordlist = BitcoinAddressValidator::get_bip39_wordlist();
    let mut words = Vec::with_capacity(WORD_COUNT);
    for chunk in payload.as_bytes().chunks_exact(WORD_INDEX_LENGTH) {
        let index = std::str::from_utf8(chunk)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|index| *index < wordlist.len())
            .ok_or(SeedQrError::InvalidStandardPayload)?;
        words.push(wordlist[index].as_str());
    }

    let mnemonic = words.join(" ");
    BitcoinAddressValidator::validate_mnemonic(&mnemonic)
        .map(|()| mnemonic)
        .map_err(|_| SeedQrError::InvalidMnemonic)
}

#[uniffi::export]
pub fn decode_compact_seed_qr(entropy: Vec<u8>) -> Result<String, SeedQrError> {
    if entropy.len() != COMPACT_ENTROPY_LENGTH {
        return Err(SeedQrError::InvalidCompactPayload);
    }

    BitcoinAddressValidator::entropy_to_mnemonic(&entropy)
        .map_err(|_| SeedQrError::InvalidCompactPayload)
}
