# Trezor Module

The Trezor module enables integration with Trezor hardware wallets through deep linking according to the Trezor Suite Lite specification. It supports generating deep links for various Trezor operations and handling callback responses.

## Features
- Environment Support
    - Development environment
    - Local environment
    - Production environment (currently marked as unavailable)
- Deep Link Operations
    - Get device features and capabilities
    - Derive addresses for specified paths
    - Retrieve account information with various query options
    - Handle callback responses from Trezor
- Comprehensive Parameter Support
    - Multiple address types and derivation paths
    - Support for multisig configurations
    - Various display options (show on device, chunkify, etc.)
    - Cross-chain functionality
- Error Handling
    - Serialization/deserialization errors
    - URL parsing errors
    - Environment errors
    - Client creation errors

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func interactWithTrezor() async {
    do {
        // Get device features
        let featuresResult = try trezorGetFeatures(
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local
        )
        print("Features deep link: \(featuresResult.url)")
        print("Request ID: \(featuresResult.requestId)")
        
        // Get an address
        let addressResult = try trezorGetAddress(
            path: "m/84'/0'/0'/0/0",
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local,
            address: nil,
            showOnTrezor: true,
            chunkify: false,
            useEventListener: nil,
            coin: "btc",
            crossChain: nil,
            multisig: nil,
            scriptType: nil,
            unlockPath: nil,
            common: nil
        )
        print("Address deep link: \(addressResult.url)")
        print("Request ID: \(addressResult.requestId)")
        
        // Get account info
        let accountResult = try trezorGetAccountInfo(
            coin: "btc",
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local,
            path: "m/84'/0'/0'",
            descriptor: nil,
            details: .basic,
            tokens: nil,
            page: nil,
            pageSize: nil,
            from: nil,
            to: nil,
            gap: nil,
            contractFilter: nil,
            marker: nil,
            defaultAccountType: nil,
            suppressBackupWarning: nil,
            common: nil
        )
        print("Account info deep link: \(accountResult.url)")
        print("Request ID: \(accountResult.requestId)")
        
        // Handle a callback from Trezor when received
        let callbackUrl = "myapp://trezor/callback?id=abc123&response={...}"
        let response = try trezorHandleDeepLink(callbackUrl: callbackUrl)
        
        switch response {
        case .features(let features):
            print("Received Features:")
            print("Device: \(features.vendor) \(features.majorVersion).\(features.minorVersion).\(features.patchVersion)")
            print("Device ID: \(features.deviceId)")
            
        case .address(let address):
            print("Received Address:")
            print("Address: \(address.address)")
            print("Path: \(address.serializedPath)")
            
        case .publicKey(let publicKey):
            print("Received Public Key:")
            print("XPUB: \(publicKey.xpub)")
            print("Path: \(publicKey.serializedPath)")
            
        case .accountInfo(let accountInfo):
            print("Received Account Info:")
            print("Account ID: \(accountInfo.id)")
            print("Descriptor: \(accountInfo.descriptor)")
            print("Balance: \(accountInfo.balance)")
        }
        
    } catch let error as TrezorConnectError {
        switch error {
        case .serdeError(let details):
            print("Serialization error: \(details)")
        case .urlError(let details):
            print("URL error: \(details)")
        case .environmentError(let details):
            print("Environment error: \(details)")
        case .clientError(let details):
            print("Client error: \(details)")
        case .other(let details):
            print("Other error: \(details)")
        }
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

fun interactWithTrezor() {
    try {
        // Get device features
        val featuresResult = trezorGetFeatures(
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL
        )
        println("Features deep link: ${featuresResult.url}")
        println("Request ID: ${featuresResult.requestId}")
        
        // Get an address
        val addressResult = trezorGetAddress(
            path = "m/84'/0'/0'/0/0",
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL,
            address = null,
            showOnTrezor = true,
            chunkify = false,
            useEventListener = null,
            coin = "btc",
            crossChain = null,
            multisig = null,
            scriptType = null,
            unlockPath = null,
            common = null
        )
        println("Address deep link: ${addressResult.url}")
        println("Request ID: ${addressResult.requestId}")
        
        // Get account info
        val accountResult = trezorGetAccountInfo(
            coin = "btc",
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL,
            path = "m/84'/0'/0'",
            descriptor = null,
            details = AccountInfoDetails.BASIC,
            tokens = null,
            page = null,
            pageSize = null,
            from = null,
            to = null,
            gap = null,
            contractFilter = null,
            marker = null,
            defaultAccountType = null,
            suppressBackupWarning = null,
            common = null
        )
        println("Account info deep link: ${accountResult.url}")
        println("Request ID: ${accountResult.requestId}")
        
        // Handle a callback from Trezor when received
        val callbackUrl = "myapp://trezor/callback?id=abc123&response={...}"
        val response = trezorHandleDeepLink(callbackUrl = callbackUrl)
        
        when (response) {
            is TrezorResponsePayload.Features -> {
                println("Received Features:")
                println("Device: ${response.vendor} ${response.majorVersion}.${response.minorVersion}.${response.patchVersion}")
                println("Device ID: ${response.deviceId}")
            }
            is TrezorResponsePayload.Address -> {
                println("Received Address:")
                println("Address: ${response.address}")
                println("Path: ${response.serializedPath}")
            }
            is TrezorResponsePayload.PublicKey -> {
                println("Received Public Key:")
                println("XPUB: ${response.xpub}")
                println("Path: ${response.serializedPath}")
            }
            is TrezorResponsePayload.AccountInfo -> {
                println("Received Account Info:")
                println("Account ID: ${response.id}")
                println("Descriptor: ${response.descriptor}")
                println("Balance: ${response.balance}")
            }
        }
        
    } catch (e: TrezorConnectError) {
        when (e) {
            is TrezorConnectError.SerdeError -> println("Serialization error: ${e.errorDetails}")
            is TrezorConnectError.UrlError -> println("URL error: ${e.errorDetails}")
            is TrezorConnectError.EnvironmentError -> println("Environment error: ${e.errorDetails}")
            is TrezorConnectError.ClientError -> println("Client error: ${e.errorDetails}")
            is TrezorConnectError.Other -> println("Other error: ${e.errorDetails}")
        }
    }
}
```

### Python
```python
from bitkitcore import trezor_get_features, trezor_get_address, trezor_get_account_info, trezor_handle_deep_link, TrezorEnvironment, TrezorConnectError, AccountInfoDetails

try:
    # Get device features
    features_result = trezor_get_features(
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL
    )
    print(f"Features deep link: {features_result.url}")
    print(f"Request ID: {features_result.request_id}")
    
    # Get an address
    address_result = trezor_get_address(
        path="m/84'/0'/0'/0/0",
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL,
        address=None,
        show_on_trezor=True,
        chunkify=False,
        use_event_listener=None,
        coin="btc",
        cross_chain=None,
        multisig=None,
        script_type=None,
        unlock_path=None,
        common=None
    )
    print(f"Address deep link: {address_result.url}")
    print(f"Request ID: {address_result.request_id}")
    
    # Get account info
    account_result = trezor_get_account_info(
        coin="btc",
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL,
        path="m/84'/0'/0'",
        descriptor=None,
        details=AccountInfoDetails.BASIC,
        tokens=None,
        page=None,
        page_size=None,
        from_block=None,
        to_block=None,
        gap=None,
        contract_filter=None,
        marker=None,
        default_account_type=None,
        suppress_backup_warning=None,
        common=None
    )
    print(f"Account info deep link: {account_result.url}")
    print(f"Request ID: {account_result.request_id}")
    
    # Handle a callback from Trezor when received
    callback_url = "myapp://trezor/callback?id=abc123&response={...}"
    response = trezor_handle_deep_link(callback_url=callback_url)
    
    if isinstance(response, TrezorResponsePayload.Features):
        print("Received Features:")
        print(f"Device: {response.vendor} {response.major_version}.{response.minor_version}.{response.patch_version}")
        print(f"Device ID: {response.device_id}")
    elif isinstance(response, TrezorResponsePayload.Address):
        print("Received Address:")
        print(f"Address: {response.address}")
        print(f"Path: {response.serialized_path}")
    elif isinstance(response, TrezorResponsePayload.PublicKey):
        print("Received Public Key:")
        print(f"XPUB: {response.xpub}")
        print(f"Path: {response.serialized_path}")
    elif isinstance(response, TrezorResponsePayload.AccountInfo):
        print("Received Account Info:")
        print(f"Account ID: {response.id}")
        print(f"Descriptor: {response.descriptor}")
        print(f"Balance: {response.balance}")
        
except TrezorConnectError as e:
    if isinstance(e, TrezorConnectError.SerdeError):
        print(f"Serialization error: {e.error_details}")
    elif isinstance(e, TrezorConnectError.UrlError):
        print(f"URL error: {e.error_details}")
    elif isinstance(e, TrezorConnectError.EnvironmentError):
        print(f"Environment error: {e.error_details}")
    elif isinstance(e, TrezorConnectError.ClientError):
        print(f"Client error: {e.error_details}")
    elif isinstance(e, TrezorConnectError.Other):
        print(f"Other error: {e.error_details}")
```

## Implementation Details

The module uses deep linking to communicate with Trezor devices via the Trezor Suite Lite. The workflow involves:

1. Generating a deep link with the requested operation parameters
2. Opening the deep link, which launches Trezor Suite Lite
3. User interacts with their Trezor device through Trezor Suite Lite
4. After completion, Trezor Suite Lite calls the provided callback URL
5. The application handles the callback and processes the response

## Error Handling

### TrezorConnectError
- `SerdeError`: Serialization/deserialization errors with JSON data
- `UrlError`: URL parsing or formatting errors
- `EnvironmentError`: Environment-related errors (e.g., unavailable environment)
- `ClientError`: Failed to create the client or client operation errors
- `Other`: General errors not covered by other categories

Each error includes detailed information about what went wrong in the `error_details` field.