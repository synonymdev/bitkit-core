# Bitkit Core FFI

## Features
- Scanner Module
  - Decode and parse Lightning/Bitcoin payment formats
  - Support for BOLT-11, BIP21 & LNURL
- LNURL Module
  - Lightning Address invoice generation
- Onchain Module
  - Bitcoin address validation and type detection
  - BIP39 mnemonic phrase generation
  - Bitcoin address derivation from mnemonic phrases
  - Private key derivation
  - Batch address derivation
  - Support for Legacy, SegWit, Native SegWit & Taproot addresses
  - Network validation (Mainnet, Testnet, Regtest)
- Activity Module
  - Store and manage transaction/activity history for both Bitcoin and Lightning Network payments
- Blocktank Module
  - Create and manage Lightning Service Provider (LSP) orders
  - Channel opening and management
  - Just-in-time channel creation
- Trezor Module
  - Integration with Trezor hardware wallets through deep linking
  - Get device features and capabilities
  - Derive addresses for specified paths
  - Retrieve account information
  - Handle responses from Trezor devices

## Available Modules: Methods
- Scanner
  - [decode](src/modules/scanner/README.md#usage-examples): Decodes input strings from various sources (QR codes, clipboard, etc.).
      ```rust
      async fn decode(invoice: String) -> Result<Scanner, DecodingError>
      ```
- LNURL:
  - [get_lnurl_invoice](src/modules/lnurl/README.md#usage-examples): Generates an invoice from a Lightning Address.
    ```rust
      async fn get_lnurl_invoice(address: String, amount_satoshis: u64) -> Result<String, LnurlError>
    ```
- Onchain:
  - [validate_bitcoin_address](src/modules/onchain/README.md#usage-examples): Validates a Bitcoin address and returns its type and network.
    ```rust
    fn validate_bitcoin_address(address: String) -> Result<ValidationResult, AddressError>
    ```
  - [genenerate_mnemonic](src/modules/onchain/README.md#usage-examples): Generates a BIP39 mnemonic phrase.
    ```rust
    fn genenerate_mnemonic(word_count: Option<WordCount>) -> Result<String, AddressError>
    ```
  - [derive_bitcoin_address](src/modules/onchain/README.md#usage-examples): Derives a Bitcoin address from a mnemonic phrase.
    ```rust
    fn derive_bitcoin_address(
        mnemonic_phrase: String,
        derivation_path_str: Option<String>,
        network: Option<Network>,
        bip39_passphrase: Option<String>
    ) -> Result<GetAddressResponse, AddressError>
    ```
  - [derive_bitcoin_addresses](src/modules/onchain/README.md#usage-examples): Derives multiple Bitcoin addresses from a mnemonic phrase.
    ```rust
    fn derive_bitcoin_addresses(
        mnemonic_phrase: String,
        derivation_path_str: Option<String>,
        network: Option<Network>,
        bip39_passphrase: Option<String>,
        is_change: Option<bool>,
        start_index: Option<u32>,
        count: Option<u32>
    ) -> Result<GetAddressesResponse, AddressError>
    ```
  - [derive_private_key](src/modules/onchain/README.md#usage-examples): Derives a private key from a mnemonic phrase.
    ```rust
    fn derive_private_key(
        mnemonic_phrase: String,
        derivation_path_str: Option<String>,
        network: Option<Network>,
        bip39_passphrase: Option<String>
    ) -> Result<String, AddressError>
    ```
- Activity:
  - [init_db](src/modules/activity/README.md#usage-examples): Initialize database
    ```rust
    fn init_db(base_path: String) -> Result<String, DbError>
    ```
  - [insert_activity](src/modules/activity/README.md#usage-examples): Insert an activity (onchain or lightning)
    ```rust
    fn insert_activity(activity: Activity) -> Result<(), ActivityError>
    ```
  - [get_activities](src/modules/activity/README.md#usage-examples): Get activities with optional filtering, limit and sort direction
    ```rust
    fn get_activities(filter: ActivityFilter, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError>
    ```  
  - [get_activity_by_id](src/modules/activity/README.md#usage-examples): Look up any activity by its ID
    ```rust
    fn get_activity_by_id(activity_id: String) -> Result<Option<Activity>, ActivityError>
    ```
  - [update_activity](src/modules/activity/README.md#usage-examples): Update an existing activity (onchain or lightning)
    ```rust
    fn update_activity(activity_id: String, activity: Activity) -> Result<(), ActivityError>
    ```
  - [delete_activity_by_id](src/modules/activity/README.md#usage-examples): Delete any activity (onchain or lightning) by its ID. Returns true if activity was found and deleted, false if not found
    ```rust
    fn delete_activity_by_id(activity_id: String) -> Result<bool, ActivityError>
    ```
  - [add_tags](src/modules/activity/README.md#usage-examples): Add tags to an activity
    ```rust
    fn add_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError>
    ```
  - [remove_tags](src/modules/activity/README.md#usage-examples): Remove tags from an activity
    ```rust
    fn remove_tags(activity_id: String, tags: Vec<String>) -> Result<(), ActivityError>
    ```
  - [get_tags](src/modules/activity/README.md#usage-examples): Get all tags for an activity
    ```rust
    fn get_tags(activity_id: String) -> Result<Vec<String>, ActivityError>
    ```
  - [get_all_unique_tags](src/modules/activity/README.md#usage-examples): Get all unique tags in the database sorted alphabetically
    ```rust
    fn get_all_unique_tags() -> Result<Vec<String>, ActivityError>
    ```
  - [get_activities_by_tag](src/modules/activity/README.md#usage-examples): Get all activities with a specific tag
    ```rust
    fn get_activities_by_tag(tag: String, limit: Option<u32>, sort_direction: Option<SortDirection>) -> Result<Vec<Activity>, ActivityError>
    ```
  - [upsert_activity](src/modules/activity/README.md#usage-examples): Insert or update an activity
    ```rust
    fn upsert_activity(activity: Activity) -> Result<(), ActivityError>
    ```
- Blocktank:
  - [init_db](src/modules/blocktank/README.md#usage-examples): Initialize database
    ```rust
    fn init_db(base_path: String) -> Result<String, DbError>
    ```
    - [update_blocktank_url](src/modules/blocktank/README.md#usage-examples): Update the Blocktank URL
        ```rust
        async fn update_blocktank_url(new_url: String) -> Result<(), BlocktankError>
        ```
    - [get_info](src/modules/blocktank/README.md#usage-examples): Get service information with optional refresh
        ```rust
        async fn get_info(refresh: Option<bool>) -> Result<Option<IBtInfo>, BlocktankError>
        ```
    - [create_order](src/modules/blocktank/README.md#usage-examples): Create a new order
        ```rust
        async fn create_order(
            lsp_balance_sat: u64,
            channel_expiry_weeks: u32,
            options: Option<CreateOrderOptions>,
        ) -> Result<IBtOrder, BlocktankError>
        ```
    - [open_channel](src/modules/blocktank/README.md#usage-examples): Open a channel for an order
        ```rust
        async fn open_channel(
            order_id: String,
            connection_string: String,
        ) -> Result<IBtOrder, BlocktankError>
        ```
    - [get_orders](src/modules/blocktank/README.md#usage-examples): Get orders with optional filtering
        ```rust
        async fn get_orders(
            order_ids: Option<Vec<String>>,
            filter: Option<BtOrderState2>,
            refresh: bool,
        ) -> Result<Vec<IBtOrder>, BlocktankError>
        ```
    - [refresh_active_orders](src/modules/blocktank/README.md#usage-examples): Refresh all active orders
        ```rust
        async fn refresh_active_orders() -> Result<Vec<IBtOrder>, BlocktankError>
        ```
    - [get_min_zero_conf_tx_fee](src/modules/blocktank/README.md#usage-examples): Get minimum zero-conf transaction fee for an order
        ```rust
        async fn get_min_zero_conf_tx_fee(
            order_id: String,
        ) -> Result<IBt0ConfMinTxFeeWindow, BlocktankError>
        ```
    - [estimate_order_fee](src/modules/blocktank/README.md#usage-examples): Estimate order fee
        ```rust
        async fn estimate_order_fee(
            lsp_balance_sat: u64,
            channel_expiry_weeks: u32,
            options: Option<CreateOrderOptions>,
        ) -> Result<IBtEstimateFeeResponse, BlocktankError>
        ```
    - [estimate_order_fee_full](src/modules/blocktank/README.md#usage-examples): Estimate order fee with full breakdown
        ```rust
        async fn estimate_order_fee_full(
            lsp_balance_sat: u64,
            channel_expiry_weeks: u32,
            options: Option<CreateOrderOptions>,
        ) -> Result<IBtEstimateFeeResponse2, BlocktankError>
        ```
    - [create_cjit_entry](src/modules/blocktank/README.md#usage-examples): Create a CJIT entry
        ```rust
        async fn create_cjit_entry(
            channel_size_sat: u64,
            invoice_sat: u64,
            invoice_description: String,
            node_id: String,
            channel_expiry_weeks: u32,
            options: Option<CreateCjitOptions>,
        ) -> Result<ICJitEntry, BlocktankError>
        ```
    - [get_cjit_entries](src/modules/blocktank/README.md#usage-examples): Get CJIT entries with optional filtering
        ```rust
        async fn get_cjit_entries(
            entry_ids: Option<Vec<String>>,
            filter: Option<CJitStateEnum>,
            refresh: bool,
        ) -> Result<Vec<ICJitEntry>, BlocktankError>
        ```
    - [refresh_active_cjit_entries](src/modules/blocktank/README.md#usage-examples): Refresh all active CJIT entries
        ```rust
        async fn refresh_active_cjit_entries() -> Result<Vec<ICJitEntry>, BlocktankError>
        ```
    - [register_device](src/modules/blocktank/README.md#usage-examples): Register a device for notifications
        ```rust
        async fn register_device(
          device_token: String,
          public_key: String,
          features: Vec<String>,
          node_id: String,
          iso_timestamp: String,
          signature: String,
          custom_url: Option<String>
        ) -> Result<String, BlocktankError>
        ```
    - [test_notification](src/modules/blocktank/README.md#usage-examples): Send a test notification to a registered device
        ```rust
        async fn test_notification(
          device_token: String,
          secret_message: String,
          notification_type: Option<String>,
          custom_url: Option<String>
        ) -> Result<String, BlocktankError>
        ```
    - [regtest_mine](src/modules/blocktank/README.md#usage-examples): Mine blocks in regtest mode
        ```rust
        async fn regtest_mine(count: Option<u32>) -> Result<(), BlocktankError>
        ```
    - [regtest_deposit](src/modules/blocktank/README.md#usage-examples): Deposit funds to an address in regtest mode
        ```rust
        async fn regtest_deposit(
          address: String,
          amount_sat: Option<u64>,
        ) -> Result<String, BlocktankError>
        ```
    - [regtest_pay](src/modules/blocktank/README.md#usage-examples): Pay an invoice in regtest mode
        ```rust
        async fn regtest_pay(
          invoice: String,
          amount_sat: Option<u64>,
        ) -> Result<String, BlocktankError>
        ```
    - [regtest_get_payment](src/modules/blocktank/README.md#usage-examples): Get payment information in regtest mode
        ```rust
        async fn regtest_get_payment(payment_id: String) -> Result<IBtBolt11Invoice, BlocktankError>
        ```
    - [regtest_close_channel](src/modules/blocktank/README.md#usage-examples): Close a channel in regtest mode
        ```rust
        async fn regtest_close_channel(
          funding_tx_id: String,
          vout: u32,
          force_close_after_s: Option<u64>,
        ) -> Result<String, BlocktankError>        
        ```
- Trezor:
  - [trezor_get_features](src/modules/trezor/README.md#usage-examples): Get device features and capabilities
    ```rust
    fn trezor_get_features(
        callback_url: String,
        request_id: Option<String>,
        trezor_environment: Option<TrezorEnvironment>
    ) -> Result<DeepLinkResult, TrezorConnectError>
    ```
  - [trezor_get_address](src/modules/trezor/README.md#usage-examples): Get address for the specified path
    ```rust
    fn trezor_get_address(
        path: String,
        callback_url: String,
        request_id: Option<String>,
        trezor_environment: Option<TrezorEnvironment>,
        address: Option<String>,
        showOnTrezor: Option<bool>,
        chunkify: Option<bool>,
        useEventListener: Option<bool>,
        coin: Option<String>,
        crossChain: Option<bool>,
        multisig: Option<MultisigRedeemScriptType>,
        scriptType: Option<String>,
        unlockPath: Option<UnlockPath>,
        common: Option<CommonParams>,
    ) -> Result<DeepLinkResult, TrezorConnectError>
    ```
  - [trezor_get_account_info](src/modules/trezor/README.md#usage-examples): Get account info for the specified parameters
    ```rust
    fn trezor_get_account_info(
        coin: String,
        callback_url: String,
        request_id: Option<String>,
        trezor_environment: Option<TrezorEnvironment>,
        path: Option<String>,
        descriptor: Option<String>,
        details: Option<AccountInfoDetails>,
        tokens: Option<TokenFilter>,
        page: Option<u32>,
        pageSize: Option<u32>,
        from: Option<u32>,
        to: Option<u32>,
        gap: Option<u32>,
        contractFilter: Option<String>,
        marker: Option<XrpMarker>,
        defaultAccountType: Option<DefaultAccountType>,
        suppressBackupWarning: Option<bool>,
        common: Option<CommonParams>,
    ) -> Result<DeepLinkResult, TrezorConnectError>
    ```
  - [trezor_handle_deep_link](src/modules/trezor/README.md#usage-examples): Handle a callback URL from Trezor
    ```rust
    fn trezor_handle_deep_link(
        callback_url: String,
    ) -> Result<TrezorResponsePayload, TrezorConnectError>
    ```

## Building the Bindings

### All Platforms
```
./build.sh all
```

### Platform-Specific Builds
```
./build.sh ios      # iOS only
./build.sh android  # Android only
./build.sh python   # Python only
```

### Run examples
```
cargo run --bin example
```

### Run Tests:
```
cargo test
```

### Run Specific Tests:
```
# Run tests for the Scanner module
cargo test modules::scanner
    
# Run tests for the LNURL module
cargo test modules::lnurl
    
# Run tests for the Onchain module
cargo test modules::onchain
    
# Run tests for the Activity module
cargo test modules::activity
    
# Run tests for the Blocktank module
cargo test modules::blocktank  

# Run tests for the Trezor module
cargo test modules::trezor  
```