//! Jade wire protocol: CBOR framing, request and reply envelopes, id correlation.
//!
//! Jade speaks a JSON-RPC shaped protocol encoded as CBOR. There is no length
//! prefix and no framing bytes: messages are self-delimiting CBOR maps written
//! back to back on the stream. A reader therefore has to buffer whatever the
//! transport hands it and attempt an incremental decode after each read until
//! one complete item is present.
//!
//! This module is pure. It performs no I/O and holds no state beyond a request
//! id counter, which makes the framing and correlation rules directly testable.

use serde::{Deserialize, Serialize};

use super::errors::JadeError;

/// Upper bound on a single buffered frame.
///
/// Jade's own `MAX_OUTPUT_MSG_SIZE` is 3 KiB, so this is generous. The cap
/// exists because a corrupt length header (for example `0x5b` followed by eight
/// `0xff` bytes) decodes as a byte string of nearly 2^64 bytes. Without a cap,
/// `skip()` would report "need more input" forever while the read buffer grew
/// without bound.
pub(crate) const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Take the first complete CBOR item out of `buf`, if one has arrived.
///
/// Returns `Ok(None)` when the buffer holds a valid but truncated item and the
/// caller should read more. Returns `Err` when the buffer cannot be a valid
/// frame, having cleared the buffer, because there is no way to find the next
/// frame boundary in a corrupt stream.
pub(crate) fn try_take_frame(buf: &mut Vec<u8>) -> Result<Option<Vec<u8>>, JadeError> {
    if buf.is_empty() {
        return Ok(None);
    }

    // A fresh decoder per attempt. Reusing one across reads would carry its
    // position forward and silently shift every subsequent frame boundary.
    let mut decoder = minicbor::Decoder::new(buf);
    match decoder.skip() {
        Ok(()) => {
            let length = decoder.position();
            Ok(Some(buf.drain(..length).collect()))
        }
        Err(error) if error.is_end_of_input() => {
            if buf.len() > MAX_FRAME_BYTES {
                buf.clear();
                return Err(JadeError::protocol(format!(
                    "incomplete frame exceeded {MAX_FRAME_BYTES} bytes"
                )));
            }
            Ok(None)
        }
        Err(error) => {
            buf.clear();
            Err(JadeError::protocol(format!(
                "malformed CBOR frame: {error}"
            )))
        }
    }
}

/// Generates request ids for one connection.
///
/// Jade caps ids at 16 characters (`MAXLEN_ID`), and `jadepy` asserts strictly
/// fewer than 16, so the counter wraps well before a `u64` would overflow the
/// limit. The counter is per connection rather than process wide: that keeps
/// ids deterministic inside a single test, and stops an id from encoding how
/// many operations the process has performed.
#[derive(Debug, Default)]
pub(crate) struct RequestIds {
    next: u64,
}

impl RequestIds {
    pub(crate) fn new() -> Self {
        Self { next: 0 }
    }

    pub(crate) fn next_id(&mut self) -> String {
        // Wrap at 15 digits so the rendered id always fits Jade's 16 character
        // limit, and start at 1 so an id is never the empty string.
        self.next = (self.next % 999_999_999_999_999) + 1;
        self.next.to_string()
    }
}

/// A request being sent to the device.
///
/// `params` is skipped entirely when absent rather than encoded as CBOR null.
/// Jade reads parameters with typed getters that treat a null as a missing
/// value but then fail with `BAD_PARAMETERS`, so an explicit null is worse than
/// no key at all. The blind pinserver flow also depends on being able to send
/// `pin` with no params, which is how the host reports an HTTP failure.
#[derive(Debug, Serialize)]
pub(crate) struct JadeRequest<'a, P: Serialize> {
    pub id: &'a str,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

/// Encode a request to CBOR.
pub(crate) fn encode_request<P: Serialize>(
    id: &str,
    method: &str,
    params: Option<P>,
) -> Result<Vec<u8>, JadeError> {
    let request = JadeRequest { id, method, params };
    let mut encoded = Vec::new();
    ciborium::into_writer(&request, &mut encoded)
        .map_err(|error| JadeError::protocol(format!("failed to encode {method}: {error}")))?;
    Ok(encoded)
}

/// The error member of a reply.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(crate) struct JadeRpcError {
    pub code: i64,
    #[serde(default)]
    pub message: String,
    /// Jade writes this with `cbor_encode_byte_string`, so it must be read as a
    /// byte string rather than through the generic `Vec<u8>` deserializer.
    #[serde(default)]
    pub data: Option<serde_bytes::ByteBuf>,
}

/// A decoded reply frame.
///
/// Every field is optional because the device also emits unsolicited `{"log":
/// ...}` frames on the same stream. Those carry no `id`, and a required `id`
/// field would make a single device log line fail the whole decode.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JadeReply {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub result: Option<ciborium::Value>,
    #[serde(default)]
    pub error: Option<JadeRpcError>,
    #[serde(default)]
    pub seqnum: Option<u32>,
    #[serde(default)]
    pub seqlen: Option<u32>,
}

/// Decode one complete frame.
pub(crate) fn decode_reply(frame: &[u8]) -> Result<JadeReply, JadeError> {
    ciborium::from_reader(frame)
        .map_err(|error| JadeError::protocol(format!("failed to decode reply: {error}")))
}

/// The id Jade uses when it cannot recover the id of the request it is
/// rejecting, for example when the request was never parsed as valid CBOR or
/// exceeded the device's input buffer.
pub(crate) const UNATTRIBUTED_ID: &str = "00";

/// What to do with a decoded reply, given the request currently outstanding.
#[derive(Debug)]
pub(crate) enum ReplyMatch {
    /// The reply for the outstanding request.
    Matched(JadeReply),
    /// A terminal error the device could not attribute to a request id. Jade
    /// sends these with id "00" when it rejects a message before recovering its
    /// id. Treating them as unmatched and ignoring them would turn every such
    /// rejection into a full length timeout.
    Unattributed(JadeRpcError),
    /// Not for us. A device log frame, or a late reply to a request that has
    /// already timed out. Discard and keep reading rather than failing, so one
    /// stale reply does not poison the next operation.
    Ignore,
}

/// Classify a reply against the outstanding request id.
pub(crate) fn classify(reply: JadeReply, outstanding_id: &str) -> ReplyMatch {
    match reply.id.as_deref() {
        Some(id) if id == outstanding_id => ReplyMatch::Matched(reply),
        Some(UNATTRIBUTED_ID) => match reply.error {
            Some(error) => ReplyMatch::Unattributed(error),
            None => ReplyMatch::Ignore,
        },
        _ => ReplyMatch::Ignore,
    }
}

impl JadeReply {
    /// Take the result, converting an error member into a typed error.
    pub(crate) fn into_result(self, min_firmware: &str) -> Result<ciborium::Value, JadeError> {
        if let Some(error) = self.error {
            return Err(JadeError::from_rpc(error.code, error.message, min_firmware));
        }
        self.result
            .ok_or_else(|| JadeError::protocol("reply carried neither result nor error"))
    }
}

/// Read a binary result.
///
/// Binary values must come off the `ciborium::Value` as bytes rather than
/// through `Value::deserialized::<Vec<u8>>()`. The `Value` deserializer maps
/// `deserialize_seq` onto `Value::Array` only, so a CBOR byte string would be
/// rejected as a type mismatch.
pub(crate) fn result_bytes(value: &ciborium::Value) -> Result<Vec<u8>, JadeError> {
    value
        .as_bytes()
        .map(|bytes| bytes.to_vec())
        .ok_or_else(|| JadeError::protocol("expected a byte string result"))
}

/// Read a text result.
pub(crate) fn result_text(value: &ciborium::Value) -> Result<String, JadeError> {
    value
        .as_text()
        .map(str::to_string)
        .ok_or_else(|| JadeError::protocol("expected a text result"))
}

/// Read a boolean result.
pub(crate) fn result_bool(value: &ciborium::Value) -> Result<bool, JadeError> {
    value
        .as_bool()
        .ok_or_else(|| JadeError::protocol("expected a boolean result"))
}
