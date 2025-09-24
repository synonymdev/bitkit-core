# Onchain Module

This module provides Bitcoin address validation, type detection, and key generation/derivation functionality.

## Features
- Validates Bitcoin addresses for different networks (Mainnet, Testnet, Regtest)
- Detects address types (Legacy, SegWit, Native SegWit, Taproot)
- Generates mnemonic phrases (BIP39)
- Validates and manages BIP39 mnemonic phrases
- Word validation and autocomplete suggestions
- Entropy and seed generation
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

### BIP39 Mnemonic Utilities

#### iOS (Swift)
```swift
import BitkitCore

func bip39Examples() {
    do {
        // Validate a mnemonic phrase
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        try validateMnemonic(mnemonicPhrase: mnemonic)
        print("✓ Mnemonic is valid")

        // Check if a word is valid
        let isValid = isValidBip39Word(word: "abandon")
        print("Is 'abandon' valid? \(isValid)")

        // Get word suggestions for autocomplete
        let suggestions = getBip39Suggestions(partialWord: "ab", limit: 5)
        print("Suggestions: \(suggestions)")
        // Output: ["abandon", "ability", "able", "about", "above"]

        // Get the full BIP39 wordlist
        let wordlist = getBip39Wordlist()
        print("Wordlist has \(wordlist.count) words")

        // Convert mnemonic to entropy
        let entropy = try mnemonicToEntropy(mnemonicPhrase: mnemonic)
        print("Entropy: \(entropy.count) bytes")

        // Convert entropy back to mnemonic
        let recoveredMnemonic = try entropyToMnemonic(entropy: entropy)
        print("Recovered: \(recoveredMnemonic)")

        // Generate seed from mnemonic (for key derivation)
        let seed = try mnemonicToSeed(mnemonicPhrase: mnemonic, passphrase: nil)
        print("Seed: \(seed.count) bytes")

        // Generate seed with passphrase
        let seedWithPass = try mnemonicToSeed(mnemonicPhrase: mnemonic, passphrase: "mypassphrase")
        print("Seed with passphrase: \(seedWithPass.count) bytes")

    } catch let error as AddressError {
        print("Error: \(error)")
    }
}
```

#### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun bip39Examples() {
    try {
        // Validate a mnemonic phrase
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        validateMnemonic(mnemonicPhrase = mnemonic)
        println("✓ Mnemonic is valid")

        // Check if a word is valid
        val isValid = isValidBip39Word(word = "abandon")
        println("Is 'abandon' valid? $isValid")

        // Get word suggestions for autocomplete
        val suggestions = getBip39Suggestions(partialWord = "ab", limit = 5u)
        println("Suggestions: $suggestions")
        // Output: [abandon, ability, able, about, above]

        // Get the full BIP39 wordlist
        val wordlist = getBip39Wordlist()
        println("Wordlist has ${wordlist.size} words")

        // Convert mnemonic to entropy
        val entropy = mnemonicToEntropy(mnemonicPhrase = mnemonic)
        println("Entropy: ${entropy.size} bytes")

        // Convert entropy back to mnemonic
        val recoveredMnemonic = entropyToMnemonic(entropy = entropy)
        println("Recovered: $recoveredMnemonic")

        // Generate seed from mnemonic (for key derivation)
        val seed = mnemonicToSeed(mnemonicPhrase = mnemonic, passphrase = null)
        println("Seed: ${seed.size} bytes")

        // Generate seed with passphrase
        val seedWithPass = mnemonicToSeed(mnemonicPhrase = mnemonic, passphrase = "mypassphrase")
        println("Seed with passphrase: ${seedWithPass.size} bytes")

    } catch (e: AddressError) {
        println("Error: $e")
    }
}
```

#### Python
```python
from bitkitcore import (
    validate_mnemonic, is_valid_bip39_word, get_bip39_suggestions,
    get_bip39_wordlist, mnemonic_to_entropy, entropy_to_mnemonic,
    mnemonic_to_seed, AddressError
)

try:
    # Validate a mnemonic phrase
    mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    validate_mnemonic(mnemonic_phrase=mnemonic)
    print("✓ Mnemonic is valid")

    # Check if a word is valid
    is_valid = is_valid_bip39_word(word="abandon")
    print(f"Is 'abandon' valid? {is_valid}")

    # Get word suggestions for autocomplete
    suggestions = get_bip39_suggestions(partial_word="ab", limit=5)
    print(f"Suggestions: {suggestions}")
    # Output: ['abandon', 'ability', 'able', 'about', 'above']

    # Get the full BIP39 wordlist
    wordlist = get_bip39_wordlist()
    print(f"Wordlist has {len(wordlist)} words")

    # Convert mnemonic to entropy
    entropy = mnemonic_to_entropy(mnemonic_phrase=mnemonic)
    print(f"Entropy: {len(entropy)} bytes")

    # Convert entropy back to mnemonic
    recovered_mnemonic = entropy_to_mnemonic(entropy=entropy)
    print(f"Recovered: {recovered_mnemonic}")

    # Generate seed from mnemonic (for key derivation)
    seed = mnemonic_to_seed(mnemonic_phrase=mnemonic, passphrase=None)
    print(f"Seed: {len(seed)} bytes")

    # Generate seed with passphrase
    seed_with_pass = mnemonic_to_seed(mnemonic_phrase=mnemonic, passphrase="mypassphrase")
    print(f"Seed with passphrase: {len(seed_with_pass)} bytes")

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
- `InvalidMnemonic`: The mnemonic phrase format is invalid
- `InvalidEntropy`: The entropy data is invalid for mnemonic generation
- `AddressDerivationFailed`: Failed to derive the address

## BIP39 Functions Reference

| Function | Description | Returns |
|----------|-------------|---------|
| `validate_mnemonic(mnemonic_phrase)` | Validates a BIP39 mnemonic phrase | `Result<(), AddressError>` |
| `is_valid_bip39_word(word)` | Checks if a word is in the BIP39 wordlist (case-insensitive) | `bool` |
| `get_bip39_suggestions(partial_word, limit)` | Returns autocomplete suggestions for partial word input | `Vec<String>` |
| `get_bip39_wordlist()` | Returns the complete BIP39 English wordlist (2048 words) | `Vec<String>` |
| `mnemonic_to_entropy(mnemonic_phrase)` | Converts a mnemonic phrase to entropy bytes | `Result<Vec<u8>, AddressError>` |
| `entropy_to_mnemonic(entropy)` | Converts entropy bytes to a mnemonic phrase | `Result<String, AddressError>` |
| `mnemonic_to_seed(mnemonic_phrase, passphrase)` | Generates a 64-byte seed from mnemonic with optional passphrase | `Result<Vec<u8>, AddressError>` |