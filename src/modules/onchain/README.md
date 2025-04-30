# Onchain Module

This module provides Bitcoin address validation, type detection, and key generation/derivation functionality.

## Features
- Validates Bitcoin addresses for different networks (Mainnet, Testnet, Regtest)
- Detects address types (Legacy, SegWit, Native SegWit, Taproot)
- Generates mnemonic phrases (BIP39)
- Derives Bitcoin addresses from mnemonic phrases
- Derives private keys from mnemonic phrases
- Batch derivation of multiple addresses

## Usage Examples

### Address Validation

#### iOS (Swift)
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

#### Android (Kotlin)
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

#### Python
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

### Mnemonic Generation and Key Derivation

#### iOS (Swift)
```swift
import BitkitCore

func generateMnemonicAndDeriveAddress() {
    do {
        // Generate a mnemonic phrase (default 12 words)
        let mnemonic = try generateMnemonic(wordCount: .words12)
        print("Generated mnemonic: \(mnemonic)")
        
        // Derive a Bitcoin address using the mnemonic
        let addressResult = try deriveBitcoinAddress(
            mnemonicPhrase: mnemonic,
            derivationPath: "m/84'/0'/0'/0/0",  // Native SegWit (P2WPKH)
            network: .bitcoin,
            bip39Passphrase: nil
        )
        print("Derived address: \(addressResult.address)")
        print("Path: \(addressResult.path)")
        
        // Derive private key
        let privateKey = try derivePrivateKey(
            mnemonicPhrase: mnemonic,
            derivationPath: "m/84'/0'/0'/0/0",
            network: .bitcoin,
            bip39Passphrase: nil
        )
        print("Private key: \(privateKey)")
        
        // Derive multiple addresses
        let addresses = try deriveBitcoinAddresses(
            mnemonicPhrase: mnemonic,
            derivationPath: "m/84'/0'/0'",
            network: .bitcoin,
            bip39Passphrase: nil,
            isChange: false,
            startIndex: 0,
            count: 5
        )
        
        for address in addresses.addresses {
            print("Address: \(address.address), Path: \(address.path)")
        }
    } catch let error as AddressError {
        print("Error: \(error)")
    }
}
```

#### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun generateMnemonicAndDeriveAddress() {
    try {
        // Generate a mnemonic phrase (default 12 words)
        val mnemonic = generateMnemonic(wordCount = WordCount.WORDS12)
        println("Generated mnemonic: $mnemonic")
        
        // Derive a Bitcoin address using the mnemonic
        val addressResult = deriveBitcoinAddress(
            mnemonicPhrase = mnemonic,
            derivationPath = "m/84'/0'/0'/0/0",  // Native SegWit (P2WPKH)
            network = Network.BITCOIN,
            bip39Passphrase = null
        )
        println("Derived address: ${addressResult.address}")
        println("Path: ${addressResult.path}")
        
        // Derive private key
        val privateKey = derivePrivateKey(
            mnemonicPhrase = mnemonic,
            derivationPath = "m/84'/0'/0'/0/0",
            network = Network.BITCOIN,
            bip39Passphrase = null
        )
        println("Private key: $privateKey")
        
        // Derive multiple addresses
        val addresses = deriveBitcoinAddresses(
            mnemonicPhrase = mnemonic,
            derivationPath = "m/84'/0'/0'",
            network = Network.BITCOIN,
            bip39Passphrase = null,
            isChange = false,
            startIndex = 0,
            count = 5
        )
        
        addresses.addresses.forEach { address ->
            println("Address: ${address.address}, Path: ${address.path}")
        }
    } catch (e: AddressError) {
        println("Error: $e")
    }
}
```

#### Python
```python
from bitkitcore import generate_mnemonic, derive_bitcoin_address, derive_bitcoin_addresses, derive_private_key, WordCount, Network, AddressError

try:
    # Generate a mnemonic phrase (default 12 words)
    mnemonic = generate_mnemonic(word_count=WordCount.WORDS12)
    print(f"Generated mnemonic: {mnemonic}")
    
    # Derive a Bitcoin address using the mnemonic
    address_result = derive_bitcoin_address(
        mnemonic_phrase=mnemonic,
        derivation_path="m/84'/0'/0'/0/0",  # Native SegWit (P2WPKH)
        network=Network.BITCOIN,
        bip39_passphrase=None
    )
    print(f"Derived address: {address_result.address}")
    print(f"Path: {address_result.path}")
    
    # Derive private key
    private_key = derive_private_key(
        mnemonic_phrase=mnemonic,
        derivation_path="m/84'/0'/0'/0/0",
        network=Network.BITCOIN,
        bip39_passphrase=None
    )
    print(f"Private key: {private_key}")
    
    # Derive multiple addresses
    addresses = derive_bitcoin_addresses(
        mnemonic_phrase=mnemonic,
        derivation_path="m/84'/0'/0'",
        network=Network.BITCOIN,
        bip39_passphrase=None,
        is_change=False,
        start_index=0,
        count=5
    )
    
    for address in addresses.addresses:
        print(f"Address: {address.address}, Path: {address.path}")
except AddressError as e:
    print(f"Error: {e}")
```

## Supported Address Types

- P2PKH (Legacy)
- P2SH (SegWit)
- P2WPKH (Pay to Witness Public Key Hash) - Native SegWit for single-sig addresses
- P2WSH (Pay to Witness Script Hash) - Native SegWit for multi-sig/script addresses
- P2TR (Taproot)

## Derivation Paths

- m/44'/0'/0'/0/n - Legacy (P2PKH)
- m/49'/0'/0'/0/n - SegWit (P2SH-WPKH)
- m/84'/0'/0'/0/n - Native SegWit (P2WPKH)
- m/86'/0'/0'/0/n - Taproot (P2TR)

## Error Handling

### AddressError
- `InvalidAddress`: The address format is invalid
- `InvalidNetwork`: The network type is invalid or mismatched
- `MnemonicGenerationFailed`: Failed to generate the mnemonic phrase
- `AddressDerivationFailed`: Failed to derive the address