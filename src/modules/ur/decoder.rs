use super::{UrDecoderStatus, UrError, UrPayload, MAX_FRAGMENT_COUNT};
use base64::{engine::general_purpose::STANDARD, Engine};
use bitcoin::psbt::Psbt;
use minicbor::bytes::ByteVec;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

const MAX_FRAME_LENGTH: usize = 4_096;
const MAX_UNIQUE_FRAMES_PER_FRAGMENT: usize = 4;

#[derive(Default)]
struct DecoderState {
    decoder: ::ur::Decoder,
    completed: Option<UrPayload>,
    received_sequences: HashSet<u32>,
}

/// Stateful decoder for single-part and animated multipart UR QR scans.
#[derive(uniffi::Object)]
pub struct UrDecoder {
    state: Mutex<DecoderState>,
}

#[uniffi::export]
impl UrDecoder {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(DecoderState::default()),
        })
    }

    /// Accept one camera frame. Invalid or changed streams reset the decoder.
    pub fn receive(&self, frame: String) -> Result<UrDecoderStatus, UrError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = state.receive(&frame);
        if result.is_err() {
            *state = DecoderState::default();
        }
        result
    }

    /// Clear all frames so the decoder can receive another message.
    pub fn reset(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *state = DecoderState::default();
    }
}

impl DecoderState {
    fn receive(&mut self, frame: &str) -> Result<UrDecoderStatus, UrError> {
        if let Some(payload) = self.completed.clone() {
            return Ok(UrDecoderStatus {
                progress: 1.0,
                fragment_count: self.decoder.fragment_count().max(1) as u32,
                payload: Some(payload),
            });
        }

        let frame = frame.trim();
        if frame.len() > MAX_FRAME_LENGTH {
            return Err(UrError::TooLarge {
                reason: format!(
                    "frame has {} characters; maximum is {MAX_FRAME_LENGTH}",
                    frame.len()
                ),
            });
        }

        let normalized = frame.to_ascii_lowercase();
        let metadata = FrameMetadata::parse(&normalized)?;
        match metadata.kind {
            FrameKind::Single => self.receive_single(&normalized, metadata.ur_type),
            FrameKind::Multipart {
                sequence_number,
                fragment_count,
            } => self.receive_multipart(
                &normalized,
                metadata.ur_type,
                sequence_number,
                fragment_count,
            ),
        }
    }

    fn receive_single(&mut self, frame: &str, ur_type: String) -> Result<UrDecoderStatus, UrError> {
        if self.decoder.ur_type().is_some() {
            return Err(stream_changed());
        }

        let (kind, cbor) = ::ur::ur::decode(frame).map_err(invalid_ur)?;
        if kind != ::ur::ur::Kind::SinglePart {
            return Err(UrError::InvalidUr {
                reason: "expected a single-part UR".to_string(),
            });
        }

        let payload = decode_payload(&ur_type, cbor)?;
        self.completed = Some(payload.clone());
        Ok(UrDecoderStatus {
            progress: 1.0,
            fragment_count: 1,
            payload: Some(payload),
        })
    }

    fn receive_multipart(
        &mut self,
        frame: &str,
        ur_type: String,
        sequence_number: u32,
        fragment_count: u32,
    ) -> Result<UrDecoderStatus, UrError> {
        if fragment_count as usize > MAX_FRAGMENT_COUNT {
            return Err(UrError::TooLarge {
                reason: format!(
                    "message declares {fragment_count} fragments; maximum is {MAX_FRAGMENT_COUNT}"
                ),
            });
        }

        self.ensure_sequence_capacity(sequence_number, fragment_count)?;

        self.decoder.receive(frame).map_err(invalid_ur)?;
        self.received_sequences.insert(sequence_number);

        if self.decoder.complete() {
            let cbor =
                self.decoder
                    .message()
                    .map_err(invalid_ur)?
                    .ok_or_else(|| UrError::InvalidUr {
                        reason: "decoder completed without a message".to_string(),
                    })?;
            let payload = decode_payload(&ur_type, cbor)?;
            self.completed = Some(payload.clone());
            return Ok(UrDecoderStatus {
                progress: 1.0,
                fragment_count,
                payload: Some(payload),
            });
        }

        Ok(UrDecoderStatus {
            progress: self.progress(),
            fragment_count,
            payload: None,
        })
    }

    fn ensure_sequence_capacity(
        &self,
        sequence_number: u32,
        fragment_count: u32,
    ) -> Result<(), UrError> {
        if self.received_sequences.contains(&sequence_number) {
            return Ok(());
        }

        let maximum = fragment_count as usize * MAX_UNIQUE_FRAMES_PER_FRAGMENT;
        if self.received_sequences.len() >= maximum {
            return Err(UrError::TooLarge {
                reason: format!("received more than {maximum} unique frames for this message"),
            });
        }
        Ok(())
    }

    fn progress(&self) -> f64 {
        let total = self.decoder.fragment_count();
        if total == 0 {
            return 0.0;
        }
        let resolved = self.decoder.resolved_fragment_count().unwrap_or(0);
        resolved as f64 / total as f64
    }
}

struct FrameMetadata {
    ur_type: String,
    kind: FrameKind,
}

enum FrameKind {
    Single,
    Multipart {
        sequence_number: u32,
        fragment_count: u32,
    },
}

impl FrameMetadata {
    fn parse(frame: &str) -> Result<Self, UrError> {
        let body = frame
            .strip_prefix("ur:")
            .ok_or_else(|| UrError::InvalidUr {
                reason: "scheme must be ur".to_string(),
            })?;
        let parts = body.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            [ur_type, payload] if !ur_type.is_empty() && !payload.is_empty() => Ok(Self {
                ur_type: (*ur_type).to_string(),
                kind: FrameKind::Single,
            }),
            [ur_type, sequence, payload]
                if !ur_type.is_empty() && !sequence.is_empty() && !payload.is_empty() =>
            {
                let (sequence_number, fragment_count) =
                    sequence.split_once('-').ok_or_else(invalid_sequence)?;
                let sequence_number = sequence_number
                    .parse::<u32>()
                    .map_err(|_| invalid_sequence())?;
                let fragment_count = fragment_count
                    .parse::<u32>()
                    .map_err(|_| invalid_sequence())?;
                if sequence_number == 0 || fragment_count == 0 {
                    return Err(invalid_sequence());
                }
                Ok(Self {
                    ur_type: (*ur_type).to_string(),
                    kind: FrameKind::Multipart {
                        sequence_number,
                        fragment_count,
                    },
                })
            }
            _ => Err(UrError::InvalidUr {
                reason: "expected ur:type/payload or ur:type/sequence-count/payload".to_string(),
            }),
        }
    }
}

fn decode_payload(ur_type: &str, cbor: Vec<u8>) -> Result<UrPayload, UrError> {
    match ur_type {
        "bytes" => Ok(UrPayload::Bytes {
            data: decode_byte_string(&cbor)?,
        }),
        "crypto-psbt" => {
            let bytes = decode_byte_string(&cbor)?;
            Psbt::deserialize(&bytes).map_err(|error| UrError::InvalidPsbt {
                reason: error.to_string(),
            })?;
            Ok(UrPayload::CryptoPsbt {
                psbt: STANDARD.encode(bytes),
            })
        }
        _ => {
            validate_cbor(&cbor)?;
            Ok(UrPayload::Cbor {
                ur_type: ur_type.to_string(),
                cbor,
            })
        }
    }
}

fn decode_byte_string(cbor: &[u8]) -> Result<Vec<u8>, UrError> {
    let mut decoder = minicbor::Decoder::new(cbor);
    let bytes = decoder
        .decode::<ByteVec>()
        .map_err(|error| UrError::InvalidPayload {
            reason: format!("expected a CBOR byte string: {error}"),
        })?;
    ensure_cbor_consumed(&decoder, cbor.len())?;
    Ok(bytes.into())
}

fn validate_cbor(cbor: &[u8]) -> Result<(), UrError> {
    let mut decoder = minicbor::Decoder::new(cbor);
    decoder.skip().map_err(|error| UrError::InvalidPayload {
        reason: format!("invalid CBOR payload: {error}"),
    })?;
    ensure_cbor_consumed(&decoder, cbor.len())
}

fn ensure_cbor_consumed(
    decoder: &minicbor::Decoder<'_>,
    input_length: usize,
) -> Result<(), UrError> {
    if decoder.position() != input_length {
        return Err(UrError::InvalidPayload {
            reason: "CBOR payload contains trailing data".to_string(),
        });
    }
    Ok(())
}

fn invalid_ur(error: ::ur::ur::Error) -> UrError {
    UrError::InvalidUr {
        reason: error.to_string(),
    }
}

fn invalid_sequence() -> UrError {
    UrError::InvalidUr {
        reason: "invalid multipart sequence".to_string(),
    }
}

fn stream_changed() -> UrError {
    UrError::InvalidUr {
        reason: "UR stream changed before completion".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ur::encoding::ur_encode_crypto_psbt;
    use bitcoin::{absolute::LockTime, transaction::Version, Transaction};

    fn empty_psbt_base64() -> String {
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        STANDARD.encode(Psbt::from_unsigned_tx(transaction).unwrap().serialize())
    }

    #[test]
    fn decodes_crypto_psbt_starting_with_mixed_fragment() {
        let expected = empty_psbt_base64();
        let mut encoder = ::ur::Encoder::new(
            &minicbor::to_vec(ByteVec::from(STANDARD.decode(&expected).unwrap())).unwrap(),
            10,
            "crypto-psbt",
        )
        .unwrap();
        let source_count = encoder.fragment_count();
        let mut parts = (0..=source_count)
            .map(|_| encoder.next_part().unwrap())
            .collect::<Vec<_>>();
        let mixed = parts.pop().unwrap();
        parts.insert(0, mixed);

        let decoder = UrDecoder::new();
        let mut result = None;
        for part in parts {
            result = decoder.receive(part).unwrap().payload;
            if result.is_some() {
                break;
            }
        }

        assert_eq!(result, Some(UrPayload::CryptoPsbt { psbt: expected }));
    }

    #[test]
    fn rejects_oversized_fragment_count() {
        let decoder = UrDecoder::new();
        let error = decoder
            .receive("ur:bytes/1-1001/invalid".to_string())
            .unwrap_err();
        assert!(matches!(error, UrError::TooLarge { .. }));
    }

    #[test]
    fn bounds_unique_frames_without_counting_duplicates() {
        let mut state = DecoderState::default();
        let fragment_count = 2;
        let maximum = fragment_count as usize * MAX_UNIQUE_FRAMES_PER_FRAGMENT;
        for sequence_number in 1..=maximum as u32 {
            state
                .ensure_sequence_capacity(sequence_number, fragment_count)
                .unwrap();
            state.received_sequences.insert(sequence_number);
        }

        state.ensure_sequence_capacity(1, fragment_count).unwrap();
        let error = state
            .ensure_sequence_capacity(maximum as u32 + 1, fragment_count)
            .unwrap_err();
        assert!(matches!(error, UrError::TooLarge { .. }));
    }

    #[test]
    fn rejects_trailing_cbor_data() {
        let error = decode_payload("bytes", vec![0x41, b'a', 0x00]).unwrap_err();
        assert!(matches!(error, UrError::InvalidPayload { .. }));

        let error = decode_payload("crypto-output", vec![0x01, 0x00]).unwrap_err();
        assert!(matches!(error, UrError::InvalidPayload { .. }));
    }

    #[test]
    fn roundtrips_public_psbt_encoder() {
        let expected = empty_psbt_base64();
        let decoder = UrDecoder::new();
        let mut result = None;
        for part in ur_encode_crypto_psbt(expected.clone(), 10).unwrap() {
            result = decoder.receive(part).unwrap().payload;
        }
        assert_eq!(result, Some(UrPayload::CryptoPsbt { psbt: expected }));
    }
}
