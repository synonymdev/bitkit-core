//! USB CDC serial transport, for desktop and the Python bindings.
//!
//! Not built for iOS, which has no USB serial, or for Android, where the
//! application drives USB through the transport callback. Nothing in this file
//! is exported over FFI: the bindings are generated from the host library, so a
//! platform gated export would appear in the generated Swift and Kotlin while
//! being absent from the device library.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serialport::SerialPort;

use super::callbacks::JadeNativeDevice;
use super::errors::JadeError;
use super::transport::JadeTransport;
use super::types::JadeTransportKind;

/// Jade's serial link speed.
const BAUD_RATE: u32 = 115_200;

/// Serial has no MTU, but chunking keeps writes off the stack and matches the
/// Bluetooth path closely enough that both exercise the same code.
const CHUNK_BYTES: usize = 509;

/// USB vendor and product pairs seen on Jade and Jade Plus units, including the
/// bridge chips used by DIY builds.
const KNOWN_USB_IDS: &[(u16, u16)] = &[
    (0x10c4, 0xea60), // Silicon Labs CP210x, Jade v1
    (0x1a86, 0x55d4), // WCH CH9102
    (0x0403, 0x6001), // FTDI FT232
    (0x1a86, 0x7523), // WCH CH340
    (0x303a, 0x4001), // Espressif native USB, Jade Plus
    (0x303a, 0x1001), // Espressif USB serial/JTAG
];

/// Discover attached Jade units.
///
/// Only ports whose USB descriptor matches a known Jade bridge are returned, so
/// a modem or GPS receiver on the same machine is not offered as a Jade.
pub(crate) fn enumerate_devices() -> Vec<JadeNativeDevice> {
    // serialport's Linux path without libudev reads /sys/class/tty and panics
    // outright if it is missing. A wallet library must not carry that risk.
    #[cfg(target_os = "linux")]
    if !std::path::Path::new("/sys/class/tty").exists() {
        log::warn!("[jade] /sys/class/tty is missing, skipping serial enumeration");
        return Vec::new();
    }

    let ports = match serialport::available_ports() {
        Ok(ports) => ports,
        Err(error) => {
            log::warn!("[jade] could not enumerate serial ports: {error}");
            return Vec::new();
        }
    };

    ports
        .into_iter()
        .filter_map(|port| {
            let serialport::SerialPortType::UsbPort(info) = port.port_type else {
                return None;
            };
            if !KNOWN_USB_IDS.contains(&(info.vid, info.pid)) {
                return None;
            }
            Some(JadeNativeDevice {
                path: port.port_name,
                transport: JadeTransportKind::Serial,
                name: info.product.clone(),
                serial_number: info.serial_number.clone(),
            })
        })
        .collect()
}

/// A serial link to a device.
pub(crate) struct SerialTransport {
    /// A std mutex rather than a tokio one: the guard is taken inside
    /// `spawn_blocking`, where a tokio guard could not be held.
    port: Arc<Mutex<Box<dyn SerialPort>>>,
}

impl SerialTransport {
    pub(crate) fn open(path: &str) -> Result<Self, JadeError> {
        let mut port = serialport::new(path, BAUD_RATE)
            .timeout(Duration::from_millis(250))
            // Asserting DTR or RTS resets the ESP32 on several of the bridge
            // chips above, so the line has to come up with both clear.
            .dtr_on_open(false)
            .open()
            .map_err(|error| JadeError::ConnectionError {
                error_details: format!("could not open {path}: {error}"),
            })?;

        if let Err(error) = port.write_request_to_send(false) {
            log::warn!("[jade] could not clear RTS on {path}: {error}");
        }

        Ok(Self {
            port: Arc::new(Mutex::new(port)),
        })
    }
}

#[async_trait]
impl JadeTransport for SerialTransport {
    async fn write_all(&self, data: Vec<u8>) -> Result<(), JadeError> {
        let port = Arc::clone(&self.port);
        tokio::task::spawn_blocking(move || {
            let mut port = port
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for chunk in data.chunks(CHUNK_BYTES) {
                std::io::Write::write_all(&mut *port, chunk).map_err(|error| {
                    JadeError::transport(format!("serial write failed: {error}"))
                })?;
            }
            std::io::Write::flush(&mut *port)
                .map_err(|error| JadeError::transport(format!("serial flush failed: {error}")))
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("serial write task failed: {error}"),
        })?
    }

    async fn read_some(&self, timeout: Duration) -> Result<Vec<u8>, JadeError> {
        let port = Arc::clone(&self.port);
        tokio::task::spawn_blocking(move || {
            let mut port = port
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Err(error) = port.set_timeout(timeout) {
                log::debug!("[jade] could not set the serial timeout: {error}");
            }

            let mut buffer = vec![0u8; 4096];
            match std::io::Read::read(&mut *port, &mut buffer) {
                Ok(read) => {
                    buffer.truncate(read);
                    Ok(buffer)
                }
                // A timeout means nothing arrived, which is the normal state
                // while the user is deciding on the device.
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Ok(Vec::new()),
                Err(error) => Err(JadeError::transport(format!("serial read failed: {error}"))),
            }
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("serial read task failed: {error}"),
        })?
    }

    async fn close(&self) -> Result<(), JadeError> {
        let port = Arc::clone(&self.port);
        tokio::task::spawn_blocking(move || {
            let mut port = port
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Leaving DTR or RTS asserted on close resets the device.
            let _ = port.write_data_terminal_ready(false);
            let _ = port.write_request_to_send(false);
        })
        .await
        .map_err(|error| JadeError::IoError {
            error_details: format!("serial close task failed: {error}"),
        })
    }
}
