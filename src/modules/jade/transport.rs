//! Byte transport and the request/reply loop.
//!
//! `JadeTransport` is the internal seam: `CallbackTransport` drives the native
//! implementation, `SerialTransport` drives a serial port directly, and tests
//! substitute a scripted double. `JadeConnection` sits above it and owns the
//! read buffer, the request id counter and the correlation rules.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;

use super::callbacks::{JadeTransportCallback, JadeTransportErrorCode};
use super::errors::JadeError;
use super::protocol::{
    classify, decode_reply, encode_request, try_take_frame, JadeReply, ReplyMatch, RequestIds,
};

/// Bluetooth writes are capped here regardless of the reported MTU.
pub(crate) const MAX_CHUNK_BYTES: u32 = 509;

/// How long a single `read_chunk` may block.
///
/// Deliberately short. The long per-operation deadline is enforced by the loop
/// in `exchange`, so a user taking two minutes to confirm on the device does not
/// sit inside one uninterruptible native call.
const READ_CHUNK_TIMEOUT_MS: u32 = 250;

/// Floor on the polling interval when a read returns nothing.
///
/// A native implementation that returns immediately with no data would
/// otherwise turn the read loop into a busy spin that pins a blocking thread.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// A byte pipe to a device.
#[async_trait]
pub(crate) trait JadeTransport: Send + Sync {
    /// Write a complete request. Implementations chunk as the transport needs.
    ///
    /// Takes ownership because callback and serial implementations both hand the
    /// buffer to a blocking task, which needs a `'static` payload.
    async fn write_all(&self, data: Vec<u8>) -> Result<(), JadeError>;

    /// Read whatever has arrived, waiting at most `timeout`.
    ///
    /// An empty vector means nothing arrived, which is not an error.
    async fn read_some(&self, timeout: Duration) -> Result<Vec<u8>, JadeError>;

    /// Release the device. Safe to call more than once.
    async fn close(&self) -> Result<(), JadeError>;
}

fn code_to_error(code: Option<JadeTransportErrorCode>, message: String) -> JadeError {
    match code {
        Some(JadeTransportErrorCode::DeviceBusy) => JadeError::DeviceBusy,
        Some(JadeTransportErrorCode::NotConnected) => JadeError::NotConnected,
        Some(JadeTransportErrorCode::Disconnected) => JadeError::DeviceDisconnected,
        Some(JadeTransportErrorCode::Timeout) => JadeError::Timeout,
        Some(JadeTransportErrorCode::PermissionDenied) | None => JadeError::TransportError {
            error_details: message,
        },
    }
}

/// Transport backed by the native application.
pub(crate) struct CallbackTransport {
    callback: Arc<dyn JadeTransportCallback>,
    path: String,
    chunk_size: usize,
}

impl CallbackTransport {
    pub(crate) fn new(callback: Arc<dyn JadeTransportCallback>, path: String) -> Self {
        // Clamp whatever the native layer reports. A zero would make the write
        // loop fail to advance, and anything above the Bluetooth cap would be
        // rejected by the link layer.
        let reported = callback.get_chunk_size(path.clone());
        let chunk_size = reported.clamp(1, MAX_CHUNK_BYTES) as usize;
        Self {
            callback,
            path,
            chunk_size,
        }
    }
}

#[async_trait]
impl JadeTransport for CallbackTransport {
    async fn write_all(&self, data: Vec<u8>) -> Result<(), JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        let chunk_size = self.chunk_size;

        // Foreign callbacks are synchronous and can block. Running them on a
        // worker thread would park it for the duration; the blocking pool is
        // sized for exactly this.
        tokio::task::spawn_blocking(move || {
            for chunk in data.chunks(chunk_size) {
                let result = callback.write_chunk(path.clone(), chunk.to_vec());
                if !result.success {
                    return Err(code_to_error(result.error_code, result.error));
                }
            }
            Ok(())
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("write task failed: {error}"),
        })?
    }

    async fn read_some(&self, timeout: Duration) -> Result<Vec<u8>, JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;

        tokio::task::spawn_blocking(move || {
            let result = callback.read_chunk(path, timeout_ms);
            if !result.success {
                return Err(code_to_error(result.error_code, result.error));
            }
            Ok(result.data)
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("read task failed: {error}"),
        })?
    }

    async fn close(&self) -> Result<(), JadeError> {
        let callback = Arc::clone(&self.callback);
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let result = callback.close_device(path);
            if !result.success {
                return Err(code_to_error(result.error_code, result.error));
            }
            Ok(())
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("close task failed: {error}"),
        })?
    }
}

/// A request/reply session over one transport.
pub(crate) struct JadeConnection {
    transport: Arc<dyn JadeTransport>,
    buffer: Vec<u8>,
    ids: RequestIds,
    /// Set when the stream can no longer be trusted. A framing failure or a
    /// transport error leaves no way to find the next frame boundary, so the
    /// connection refuses further work rather than returning confusing errors
    /// far from the real cause.
    poisoned: bool,
    aborted: Arc<AtomicBool>,
    min_firmware: String,
}

impl JadeConnection {
    pub(crate) fn new(transport: Arc<dyn JadeTransport>, aborted: Arc<AtomicBool>) -> Self {
        Self {
            transport,
            buffer: Vec::new(),
            ids: RequestIds::new(),
            poisoned: false,
            aborted,
            min_firmware: super::types::MIN_JADE_FIRMWARE.to_string(),
        }
    }

    fn check_usable(&self) -> Result<(), JadeError> {
        if self.poisoned {
            return Err(JadeError::DeviceDisconnected);
        }
        if self.aborted.load(Ordering::SeqCst) {
            return Err(JadeError::UserCancelled);
        }
        Ok(())
    }

    /// Mark the stream unusable and drop anything half read.
    fn poison(&mut self) {
        self.poisoned = true;
        self.buffer.clear();
    }

    /// Send a request and wait for its reply.
    pub(crate) async fn exchange<P: Serialize>(
        &mut self,
        method: &str,
        params: Option<P>,
        timeout: Duration,
    ) -> Result<JadeReply, JadeError> {
        self.check_usable()?;

        let id = self.ids.next_id();
        let request = encode_request(&id, method, params)?;
        log::debug!("[jade] -> {method} id={id} ({} bytes)", request.len());

        if let Err(error) = self.transport.write_all(request).await {
            self.poison();
            return Err(error);
        }

        self.await_reply(&id, method, timeout).await
    }

    /// Wait for the reply to `id`, discarding log frames and stale replies.
    async fn await_reply(
        &mut self,
        id: &str,
        method: &str,
        timeout: Duration,
    ) -> Result<JadeReply, JadeError> {
        let deadline = Instant::now() + timeout;

        loop {
            // Drain everything already buffered before reading again, so two
            // frames arriving in one read are both seen.
            loop {
                let frame = match try_take_frame(&mut self.buffer) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => break,
                    Err(error) => {
                        self.poison();
                        return Err(error);
                    }
                };

                let reply = match decode_reply(&frame) {
                    Ok(reply) => reply,
                    Err(error) => {
                        self.poison();
                        return Err(error);
                    }
                };

                match classify(reply, id) {
                    ReplyMatch::Matched(reply) => {
                        log::debug!("[jade] <- {method} id={id}");
                        return Ok(reply);
                    }
                    ReplyMatch::Unattributed(error) => {
                        // The device rejected the message before it could
                        // recover the id. This is terminal for the request in
                        // flight; ignoring it would strand the caller until the
                        // deadline.
                        log::debug!("[jade] <- {method} unattributed error {}", error.code);
                        return Err(JadeError::from_rpc(
                            error.code,
                            error.message,
                            &self.min_firmware,
                        ));
                    }
                    ReplyMatch::Ignore => continue,
                }
            }

            if self.aborted.load(Ordering::SeqCst) {
                self.poison();
                return Err(JadeError::UserCancelled);
            }
            let now = Instant::now();
            if now >= deadline {
                self.poison();
                return Err(JadeError::Timeout);
            }

            let remaining = deadline - now;
            let slice = remaining.min(Duration::from_millis(u64::from(READ_CHUNK_TIMEOUT_MS)));
            let chunk = match self.transport.read_some(slice).await {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.poison();
                    return Err(error);
                }
            };

            if chunk.is_empty() {
                // Nothing yet. Yield so a native implementation that returns
                // immediately cannot spin a blocking thread at full tilt.
                tokio::time::sleep(IDLE_POLL_INTERVAL.min(remaining)).await;
            } else {
                self.buffer.extend_from_slice(&chunk);
            }
        }
    }

    /// Send a request whose reply may arrive in `seqnum`/`seqlen` fragments and
    /// return the concatenated bytes.
    ///
    /// Fragments are fetched with `get_extended_data`. Each of those carries its
    /// own fresh request id while `origid` names the original request, so the id
    /// being matched changes on every round. `seqnum` must advance by exactly
    /// one and `seqlen` must be echoed unchanged, or the device aborts with a
    /// protocol error.
    ///
    /// Any failure part way through poisons the connection: the device stays
    /// blocked waiting for the next fragment request, so the link has to be torn
    /// down rather than reused.
    pub(crate) async fn exchange_reassembled<P: Serialize>(
        &mut self,
        method: &str,
        params: Option<P>,
        timeout: Duration,
    ) -> Result<Vec<u8>, JadeError> {
        self.check_usable()?;

        let origid = self.ids.next_id();
        let request = encode_request(&origid, method, params)?;
        log::debug!("[jade] -> {method} id={origid} ({} bytes)", request.len());
        if let Err(error) = self.transport.write_all(request).await {
            self.poison();
            return Err(error);
        }

        let reply = self.await_reply(&origid, method, timeout).await?;
        let seqlen = reply.seqlen.unwrap_or(1).max(1);
        let mut seqnum = reply.seqnum.unwrap_or(1);
        let mut payload = super::protocol::result_bytes(&reply.into_result(&self.min_firmware)?)?;

        if seqlen > 1 {
            log::debug!("[jade] {method} reply spans {seqlen} fragments");
        }

        while seqnum < seqlen {
            let next = seqnum + 1;
            let fragment = self
                .fetch_fragment(&origid, method, next, seqlen, timeout)
                .await
                .inspect_err(|_| {
                    // Leaving the device mid-stream desynchronises it; the
                    // connection cannot be reused.
                    self.poisoned = true;
                })?;
            payload.extend_from_slice(&fragment);
            seqnum = next;
        }

        Ok(payload)
    }

    async fn fetch_fragment(
        &mut self,
        origid: &str,
        orig: &str,
        seqnum: u32,
        seqlen: u32,
        timeout: Duration,
    ) -> Result<Vec<u8>, JadeError> {
        #[derive(Serialize)]
        struct ExtendedDataParams<'a> {
            origid: &'a str,
            orig: &'a str,
            seqnum: u32,
            seqlen: u32,
        }

        let params = ExtendedDataParams {
            origid,
            orig,
            seqnum,
            seqlen,
        };
        let reply = self
            .exchange("get_extended_data", Some(params), timeout)
            .await?;

        if let Some(reported) = reply.seqnum {
            if reported != seqnum {
                return Err(JadeError::protocol(format!(
                    "expected fragment {seqnum}, device sent {reported}"
                )));
            }
        }
        super::protocol::result_bytes(&reply.into_result(&self.min_firmware)?)
    }
}
