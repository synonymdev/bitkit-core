use super::types::{HardwareWalletTransport, HardwareWalletVendor, SupportedHardwareWallet};

/// The hardware-wallet models supported by Bitkit and their available transports.
#[uniffi::export]
pub fn get_supported_hardware_wallets() -> Vec<SupportedHardwareWallet> {
    use HardwareWalletTransport::{Bluetooth, Qr, Usb};

    let trezor = |model: &str, transports: Vec<HardwareWalletTransport>| SupportedHardwareWallet {
        vendor: HardwareWalletVendor::Trezor,
        vendor_name: "Trezor".to_string(),
        model: model.to_string(),
        display_name: format!("Trezor {model}"),
        transports,
    };

    vec![
        trezor("Model One", vec![Usb]),
        trezor("Model T", vec![Usb]),
        trezor("Safe 3", vec![Usb]),
        trezor("Safe 5", vec![Usb]),
        trezor("Safe 7", vec![Usb, Bluetooth]),
        SupportedHardwareWallet {
            vendor: HardwareWalletVendor::Foundation,
            vendor_name: "Foundation".to_string(),
            model: "Passport".to_string(),
            display_name: "Foundation Passport".to_string(),
            transports: vec![Qr],
        },
    ]
}
