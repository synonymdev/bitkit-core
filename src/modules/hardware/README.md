# Hardware Module

This module provides hardware wallet integration, currently supporting Trezor hardware wallets.

## Features

- Initialize Trezor hardware wallet connection
- Communication with Trezor device via Deno script bridge

## Usage Examples

### iOS (Swift) Example
```swift
import BitkitCore

func initializeTrezor() async {
    do {
        let result = try await initializeTrezorLibrary()
        print("Trezor initialized: \(result)")
    } catch let error as HardwareError {
        switch error {
        case .initializationError(let message):
            print("Initialization failed: \(message)")
        case .ioError(let message):
            print("IO error: \(message)")
        case .executableDirectoryError:
            print("Failed to get executable directory")
        case .communicationError(let message):
            print("Communication error: \(message)")
        case .jsonError(let message):
            print("JSON parsing error: \(message)")
        }
    }
}
```

### Android (Kotlin) Example
```kotlin
import com.synonym.bitkitcore.*

suspend fun initializeTrezor() {
    try {
        val result = initializeTrezorLibrary()
        println("Trezor initialized: $result")
    } catch (e: HardwareError) {
        when (e) {
            is HardwareError.InitializationError -> println("Initialization failed: ${e.message}")
            is HardwareError.IoError -> println("IO error: ${e.message}")
            is HardwareError.ExecutableDirectoryError -> println("Failed to get executable directory")
            is HardwareError.CommunicationError -> println("Communication error: ${e.message}")
            is HardwareError.JsonError -> println("JSON parsing error: ${e.message}")
        }
    }
}
```

### Python Example
```python
from bitkitcore import initialize_trezor_library, HardwareError

async def initialize_trezor():
    try:
        result = await initialize_trezor_library()
        print(f"Trezor initialized: {result}")
    except HardwareError as e:
        if isinstance(e, HardwareError.InitializationError):
            print(f"Initialization failed: {e.message}")
        elif isinstance(e, HardwareError.IoError):
            print(f"IO error: {e.message}")
        elif isinstance(e, HardwareError.ExecutableDirectoryError):
            print("Failed to get executable directory")
        elif isinstance(e, HardwareError.CommunicationError):
            print(f"Communication error: {e.message}")
        elif isinstance(e, HardwareError.JsonError):
            print(f"JSON parsing error: {e.message}")
```

## Error Handling

### HardwareError
- `InitializationError`: Failed to initialize hardware wallet, includes:
  - `message`: Detailed error message from the device
- `IoError`: I/O error occurred during communication, includes:
  - `message`: Detailed I/O error message
- `ExecutableDirectoryError`: Failed to get the executable directory path
- `CommunicationError`: Failed to communicate with the hardware device, includes:
  - `message`: Detailed communication error message
- `JsonError`: JSON serialization/deserialization error, includes:
  - `message`: Detailed JSON error message

## Implementation Details

The module uses a JavaScript bridge via Deno to communicate with the Trezor hardware wallet. This approach allows for cross-platform compatibility while leveraging the existing Trezor Connect API.

## Future Improvements

- Support for additional hardware wallet types (Ledger, etc.)
- Extended functionality for transaction signing
- Address verification features