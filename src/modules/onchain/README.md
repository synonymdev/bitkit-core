# Onchain Module

This module provides Bitcoin address validation and type detection functionality.

## Features
- Validates Bitcoin addresses for different networks (Mainnet, Testnet, Regtest)
- Detects address types (Legacy, SegWit, Native SegWit, Taproot)

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func validateAddress() {
    do {
        let result = try validateBitcoinAddress("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2")
        print("Address Type: \(result.addressType.commonName())")
        print("Network: \(result.network)")
    } catch let error as AddressError {
        switch error {
        case .InvalidAddress:
            print("Invalid Bitcoin address format")
        case .InvalidNetwork:
            print("Invalid network type")
        }
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun validateAddress() {
    try {
        val result = validateBitcoinAddress("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2")
        println("Address Type: ${result.addressType.commonName()}")
        println("Network: ${result.network}")
    } catch (e: AddressError) {
        when (e) {
            is AddressError.InvalidAddress -> println("Invalid Bitcoin address format")
            is AddressError.InvalidNetwork -> println("Invalid network type")
        }
    }
}
```

### Python
```python
from bitkitcore import validate_bitcoin_address, AddressError

try:
    result = validate_bitcoin_address("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2")
    print(f"Address Type: {result.address_type.common_name()}")
    print(f"Network: {result.network}")
except AddressError as e:
    if isinstance(e, AddressError.InvalidAddress):
        print("Invalid Bitcoin address format")
    elif isinstance(e, AddressError.InvalidNetwork):
        print("Invalid network type")
```

## Supported Address Types

- P2PKH (Legacy)
- P2SH (SegWit)
- P2WPKH (Pay to Witness Public Key Hash) - Native SegWit for single-sig addresses
- P2WSH (Pay to Witness Script Hash) - Native SegWit for multi-sig/script addresses
- P2TR (Taproot)

## Error Handling

### AddressError
- `InvalidAddress`: The address format is invalid
- `InvalidNetwork`: The network type is invalid or mismatched