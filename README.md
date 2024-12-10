# Bitkit Core FFI

## Features
- Scanner Module
    - Decode and parse Lightning/Bitcoin payment formats
    - Support for BOLT-11, BIP21 & LNURL
- LNURL Module
    - Lightning Address invoice generation
- Onchain Module
    - Bitcoin address validation and type detection
    - Support for Legacy, SegWit, Native SegWit & Taproot addresses
    - Network validation (Mainnet, Testnet, Regtest)
- Activity Module
    - Store and manage transaction/activity history for both Bitcoin and Lightning Network payments

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
- Activity:
  - [init_db](src/modules/activity/README.md#usage-examples): Initialize database
    ```rust
    fn init_db(db_path: String) -> Result<String, DbError>
    ```
  - [insert_activity](src/modules/activity/README.md#usage-examples): Insert an activity (onchain or lightning)
    ```rust
    fn insert_activity(activity: Activity) -> Result<(), ActivityError>
    ```
  - [get_all_activities](src/modules/activity/README.md#usage-examples): Get all activities (both onchain and lightning) sorted by timestamp
    ```rust
    fn get_all_activities(limit: Option<u32>) -> Result<Vec<Activity>, ActivityError>
    ```
  - [get_all_onchain_activities](src/modules/activity/README.md#usage-examples): Get all onchain activities
    ```rust
    fn get_all_onchain_activities(limit: Option<u32>) -> Result<Vec<OnchainActivity>, ActivityError>
    ```
  - [get_all_lightning_activities](src/modules/activity/README.md#usage-examples): Get all lightning activities
    ```rust
    fn get_all_lightning_activities(limit: Option<u32>) -> Result<Vec<LightningActivity>, ActivityError>
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
  - [get_activities_by_tag](src/modules/activity/README.md#usage-examples): Get all activities with a specific tag
    ```rust
    fn get_activities_by_tag(tag: String, limit: Option<u32>) -> Result<Vec<Activity>, ActivityError>
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
