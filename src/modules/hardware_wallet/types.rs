/// A hardware-wallet vendor recognized by Bitkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HardwareWalletVendor {
    Trezor,
    Foundation,
}

/// How an application exchanges data with a hardware wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HardwareWalletTransport {
    Usb,
    Bluetooth,
    Qr,
}

/// A hardware-wallet model Bitkit supports.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SupportedHardwareWallet {
    pub vendor: HardwareWalletVendor,
    /// Human-readable manufacturer name, e.g. "Foundation".
    pub vendor_name: String,
    /// Stable model identifier that applications can map to bundled assets.
    pub model: String,
    /// Full user-facing name.
    pub display_name: String,
    /// Transports over which the application can interact with this model.
    pub transports: Vec<HardwareWalletTransport>,
}
