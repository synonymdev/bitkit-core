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
