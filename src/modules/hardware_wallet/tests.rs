use super::{get_supported_hardware_wallets, HardwareWalletTransport, HardwareWalletVendor};

#[test]
fn catalog_lists_supported_models_and_transports() {
    let wallets = get_supported_hardware_wallets();

    assert_eq!(wallets.len(), 8);
    assert!(wallets
        .iter()
        .filter(|wallet| wallet.vendor == HardwareWalletVendor::Trezor)
        .all(|wallet| wallet.transports.contains(&HardwareWalletTransport::Usb)));

    let safe_7 = wallets
        .iter()
        .find(|wallet| wallet.model == "Safe 7")
        .unwrap();
    assert!(safe_7
        .transports
        .contains(&HardwareWalletTransport::Bluetooth));

    let passport = wallets
        .iter()
        .find(|wallet| wallet.model == "Passport")
        .unwrap();
    assert_eq!(passport.vendor, HardwareWalletVendor::Foundation);
    assert_eq!(passport.transports, [HardwareWalletTransport::Qr]);

    let jades: Vec<_> = wallets
        .iter()
        .filter(|wallet| wallet.vendor == HardwareWalletVendor::Blockstream)
        .collect();
    assert_eq!(jades.len(), 2);
    assert!(jades.iter().all(|wallet| {
        wallet.transports.contains(&HardwareWalletTransport::Usb)
            && wallet
                .transports
                .contains(&HardwareWalletTransport::Bluetooth)
    }));
    assert!(jades.iter().any(|wallet| wallet.model == "Jade Plus"));
}
