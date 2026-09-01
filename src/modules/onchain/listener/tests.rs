use std::str::FromStr;

use bdk::bitcoin::absolute::LockTime;
use bdk::bitcoin::bip32::{ChildNumber, ExtendedPubKey};
use bdk::bitcoin::{Transaction, TxIn, TxOut};
use bdk::database::BatchOperations;
use bdk::{BlockTime, KeychainKind, TransactionDetails};

use super::*;
use crate::modules::activity::Activity;
use crate::modules::onchain::implementation::WalletSetup;

const TEST_TPUB: &str = "tpubDC7jGaaSE66VDB6VhEDFYQSCAyugXmfnMnrMVyHNzW9wryyTxvha7TmfAHd7GRXrr2TaAn2HXn9T8ep4gyNX1bzGiieqcTUNcu2poyntrET";
const BARE_PUBKEY_DESCRIPTOR: &str =
    "pk(0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798)";

fn test_setup(account_index: u32) -> WalletSetup {
    let mut xpub = ExtendedPubKey::from_str(TEST_TPUB).unwrap();
    xpub.child_number = ChildNumber::from_hardened_idx(account_index).unwrap();
    resolve_wallet_setup(
        &xpub.to_string(),
        Some(OnchainNetwork::Regtest),
        Some(AccountType::NativeSegwit),
        None,
    )
    .unwrap()
}

fn wallet_with_received_output(
    setup: &WalletSetup,
    keychain: KeychainKind,
) -> Wallet<MemoryDatabase> {
    let derivation_wallet = create_wallet(setup).unwrap();
    let address = match keychain {
        KeychainKind::External => derivation_wallet.get_address(BdkAddressIndex::Peek(0)),
        KeychainKind::Internal => derivation_wallet.get_internal_address(BdkAddressIndex::Peek(0)),
    }
    .unwrap();
    let transaction = Transaction {
        version: 2,
        lock_time: LockTime::ZERO,
        input: vec![TxIn::default()],
        output: vec![TxOut {
            value: 25_000,
            script_pubkey: address.address.script_pubkey(),
        }],
    };
    let txid = transaction.txid();
    let mut database = MemoryDatabase::new();
    database.set_last_index(keychain, 0).unwrap();
    database
        .set_tx(&TransactionDetails {
            transaction: Some(transaction),
            txid,
            received: 25_000,
            sent: 0,
            fee: Some(500),
            confirmation_time: Some(BlockTime {
                height: 100,
                timestamp: 1_700_000_000,
            }),
        })
        .unwrap();

    let wallet = Wallet::new(
        &setup.external_desc,
        Some(&setup.internal_desc),
        setup.network,
        database,
    )
    .unwrap();
    wallet.ensure_addresses_cached(1).unwrap();
    wallet
}

#[test]
fn transactions_changed_event_updates_receive_address_after_external_use() {
    let setup = test_setup(7);
    let initial_wallet = create_wallet(&setup).unwrap();
    let expected_initial_address = initial_wallet
        .get_address(BdkAddressIndex::Peek(0))
        .unwrap()
        .address
        .to_string();

    let initial_event = build_tx_changed_event(
        &initial_wallet,
        144,
        setup.account_type,
        "hardware-wallet",
        setup.network,
        &setup.base_path,
    );
    let WatcherEvent::TransactionsChanged {
        activities,
        transaction_details,
        balance,
        tx_count,
        block_height,
        account_type,
        next_unused_external_address,
    } = initial_event
    else {
        panic!("expected initial transaction event");
    };
    assert!(activities.is_empty());
    assert!(transaction_details.is_empty());
    assert_eq!(balance.total, 0);
    assert_eq!(tx_count, 0);
    assert_eq!(block_height, 144);
    assert_eq!(account_type, AccountType::NativeSegwit);
    assert_eq!(
        next_unused_external_address.address,
        expected_initial_address
    );
    assert_eq!(next_unused_external_address.path, "m/84'/1'/7'/0/0");
    assert_eq!(next_unused_external_address.transfers, 0);

    let updated_wallet = wallet_with_received_output(&setup, KeychainKind::External);
    let expected_updated_address = updated_wallet
        .get_address(BdkAddressIndex::Peek(1))
        .unwrap()
        .address
        .to_string();
    let updated_event = build_tx_changed_event(
        &updated_wallet,
        145,
        setup.account_type,
        "hardware-wallet",
        setup.network,
        &setup.base_path,
    );
    let WatcherEvent::TransactionsChanged {
        activities,
        transaction_details,
        balance,
        tx_count,
        block_height,
        account_type,
        next_unused_external_address,
    } = updated_event
    else {
        panic!("expected updated transaction event");
    };
    assert_eq!(activities.len(), 1);
    assert_eq!(transaction_details.len(), 1);
    assert_eq!(balance.total, 0);
    assert_eq!(tx_count, 1);
    assert_eq!(block_height, 145);
    assert_eq!(account_type, AccountType::NativeSegwit);
    assert_eq!(
        next_unused_external_address.address,
        expected_updated_address
    );
    assert_eq!(next_unused_external_address.path, "m/84'/1'/7'/0/1");
    assert_eq!(next_unused_external_address.transfers, 0);
    let Activity::Onchain(activity) = &activities[0] else {
        panic!("expected onchain activity");
    };
    assert_eq!(activity.wallet_id, "hardware-wallet");
    assert_eq!(activity.value, 25_000);
    assert_eq!(transaction_details[0].wallet_id, "hardware-wallet");
    assert_eq!(transaction_details[0].amount_sats, 25_000);
}

#[test]
fn change_use_does_not_advance_external_receive_address() {
    let setup = test_setup(0);
    let wallet = wallet_with_received_output(&setup, KeychainKind::Internal);

    let address = next_unused_external_address(&wallet, &setup.base_path).unwrap();

    assert_eq!(address.path, "m/84'/1'/0'/0/0");
    assert_eq!(address.transfers, 0);
}

#[test]
fn transactions_changed_event_reports_address_derivation_error() {
    let wallet = Wallet::new(
        BARE_PUBKEY_DESCRIPTOR,
        None,
        BdkNetwork::Regtest,
        MemoryDatabase::new(),
    )
    .unwrap();

    let event = build_tx_changed_event(
        &wallet,
        144,
        AccountType::NativeSegwit,
        "hardware-wallet",
        BdkNetwork::Regtest,
        "m/84'/1'/0'",
    );

    let WatcherEvent::Error { message } = event else {
        panic!("expected watcher error event");
    };
    assert_eq!(
        message,
        "Wallet error: Failed to get last unused external address: Script doesn't have address form"
    );
}
