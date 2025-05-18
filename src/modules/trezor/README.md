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
    - Sign Bitcoin transactions with full parameter support
    - Sign messages using BIP32 derived private keys
    - Verify message signatures using address and signature
    - Handle callback responses from Trezor
- Comprehensive Parameter Support
    - Multiple address types and derivation paths
    - Support for multisig configurations
    - Various display options (show on device, chunkify, etc.)
    - Cross-chain functionality
    - Reference transactions (refTxs) for transaction signing
    - Payment requests (SLIP-24)
    - Various script types and amount units
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
        
        // Verify a message
        let verifyMessageResult = try trezorVerifyMessage(
            address: "3BD8TL6iShVzizQzvo789SuynEKGpLTms9",
            signature: "JO7vL3tOB1qQyfSeIVLvdEw9G1tCvL+lNj78XDAVM4t6UptADs3kXDTO2+2ZeEOLFL4/+wm+BBdSpo3kb3Cnsas=",
            message: "example message",
            coin: "btc",
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local,
            hex: false,
            common: nil
        )
        print("Verify message deep link: \(verifyMessageResult.url)")
        print("Request ID: \(verifyMessageResult.requestId)")
        
        // Verify a message
        val verifyMessageResult = trezorVerifyMessage(
            address = "3BD8TL6iShVzizQzvo789SuynEKGpLTms9",
            signature = "JO7vL3tOB1qQyfSeIVLvdEw9G1tCvL+lNj78XDAVM4t6UptADs3kXDTO2+2ZeEOLFL4/+wm+BBdSpo3kb3Cnsas=",
            message = "example message",
            coin = "btc",
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL,
            hex = false,
            common = null
        )
        println("Verify message deep link: ${verifyMessageResult.url}")
        println("Request ID: ${verifyMessageResult.requestId}")
        
        // Sign a message
        let signMessageResult = try trezorSignMessage(
            path: "m/44'/0'/0'",
            message: "Hello World!",
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local,
            coin: "btc",
            hex: false,
            noScriptType: false,
            common: nil
        )
        print("Sign message deep link: \(signMessageResult.url)")
        print("Request ID: \(signMessageResult.requestId)")
        
        // Sign a message
        val signMessageResult = trezorSignMessage(
            path = "m/44'/0'/0'",
            message = "Hello World!",
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL,
            coin = "btc",
            hex = false,
            noScriptType = false,
            common = null
        )
        println("Sign message deep link: ${signMessageResult.url}")
        println("Request ID: ${signMessageResult.requestId}")
        
        // Sign a transaction
        let inputs = [
            TxInputType(
                prevHash: "b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac",
                prevIndex: 0,
                amount: 3431747,
                sequence: nil,
                addressN: [
                    (44 | 0x80000000),
                    (0 | 0x80000000), 
                    (2 | 0x80000000),
                    1,
                    0
                ],
                scriptType: .spendAddress,
                multisig: nil,
                scriptPubkey: nil,
                scriptSig: nil,
                witness: nil,
                ownershipProof: nil,
                commitmentData: nil,
                origHash: nil,
                origIndex: nil,
                coinjoinFlags: nil
            )
        ]
        
        let outputs = [
            TxOutputType(
                address: nil,
                addressN: [
                    (44 | 0x80000000),
                    (0 | 0x80000000),
                    (2 | 0x80000000),
                    1,
                    1
                ],
                amount: 3181747,
                scriptType: .payToAddress,
                multisig: nil,
                opReturnData: nil,
                origHash: nil,
                origIndex: nil,
                paymentReqIndex: nil
            ),
            TxOutputType(
                address: "18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2",
                addressN: nil,
                amount: 200000,
                scriptType: .payToAddress,
                multisig: nil,
                opReturnData: nil,
                origHash: nil,
                origIndex: nil,
                paymentReqIndex: nil
            )
        ]
        
        let signResult = try trezorSignTransaction(
            coin: "btc",
            inputs: inputs,
            outputs: outputs,
            callbackUrl: "myapp://trezor/callback",
            requestId: nil,
            trezorEnvironment: .local,
            refTxs: nil,
            paymentRequests: nil,
            locktime: nil,
            version: nil,
            expiry: nil,
            versionGroupId: nil,
            overwintered: nil,
            timestamp: nil,
            branchId: nil,
            push: false,
            amountUnit: nil,
            unlockPath: nil,
            serialize: nil,
            chunkify: nil,
            common: nil
        )
        print("Sign transaction deep link: \(signResult.url)")
        print("Request ID: \(signResult.requestId)")
        
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
            
        case .verifyMessage(let verifyMessage):
            print("Received Verify Message Result:")
            print("Result: \(verifyMessage.message)")
            
        case .messageSignature(let messageSignature):
            print("Received Message Signature:")
            print("Address: \(messageSignature.address)")
            print("Signature: \(messageSignature.signature)")
            
        case .signedTransaction(let signedTx):
            print("Received Signed Transaction:")
            print("Signatures: \(signedTx.signatures)")
            print("Serialized TX: \(signedTx.serializedTx)")
            if let txid = signedTx.txid {
                print("Transaction ID: \(txid)")
            }
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
        
        // Sign a transaction
        val inputs = listOf(
            TxInputType(
                prevHash = "b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac",
                prevIndex = 0u,
                amount = 3431747u,
                sequence = null,
                addressN = listOf(
                    (44 or 0x80000000).toUInt(),
                    (0 or 0x80000000).toUInt(),
                    (2 or 0x80000000).toUInt(),
                    1u,
                    0u
                ),
                scriptType = ScriptType.SPEND_ADDRESS,
                multisig = null,
                scriptPubkey = null,
                scriptSig = null,
                witness = null,
                ownershipProof = null,
                commitmentData = null,
                origHash = null,
                origIndex = null,
                coinjoinFlags = null
            )
        )
        
        val outputs = listOf(
            TxOutputType(
                address = null,
                addressN = listOf(
                    (44 or 0x80000000).toUInt(),
                    (0 or 0x80000000).toUInt(),
                    (2 or 0x80000000).toUInt(),
                    1u,
                    1u
                ),
                amount = 3181747u,
                scriptType = ScriptType.PAY_TO_ADDRESS,
                multisig = null,
                opReturnData = null,
                origHash = null,
                origIndex = null,
                paymentReqIndex = null
            ),
            TxOutputType(
                address = "18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2",
                addressN = null,
                amount = 200000u,
                scriptType = ScriptType.PAY_TO_ADDRESS,
                multisig = null,
                opReturnData = null,
                origHash = null,
                origIndex = null,
                paymentReqIndex = null
            )
        )
        
        val signResult = trezorSignTransaction(
            coin = "btc",
            inputs = inputs,
            outputs = outputs,
            callbackUrl = "myapp://trezor/callback",
            requestId = null,
            trezorEnvironment = TrezorEnvironment.LOCAL,
            refTxs = null,
            paymentRequests = null,
            locktime = null,
            version = null,
            expiry = null,
            versionGroupId = null,
            overwintered = null,
            timestamp = null,
            branchId = null,
            push = false,
            amountUnit = null,
            unlockPath = null,
            serialize = null,
            chunkify = null,
            common = null
        )
        println("Sign transaction deep link: ${signResult.url}")
        println("Request ID: ${signResult.requestId}")
        
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
            is TrezorResponsePayload.VerifyMessage -> {
                println("Received Verify Message Result:")
                println("Result: ${response.message}")
            }
            is TrezorResponsePayload.MessageSignature -> {
                println("Received Message Signature:")
                println("Address: ${response.address}")
                println("Signature: ${response.signature}")
            }
            is TrezorResponsePayload.SignedTransaction -> {
                println("Received Signed Transaction:")
                println("Signatures: ${response.signatures}")
                println("Serialized TX: ${response.serializedTx}")
                response.txid?.let { txid ->
                    println("Transaction ID: $txid")
                }
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
from bitkitcore import (
    trezor_get_features, trezor_get_address, trezor_get_account_info, 
    trezor_sign_message, trezor_verify_message, trezor_sign_transaction, trezor_handle_deep_link, 
    TrezorEnvironment, TrezorConnectError, AccountInfoDetails,
    TxInputType, TxOutputType, ScriptType
)

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
    
    # Verify a message
    verify_message_result = trezor_verify_message(
        address="3BD8TL6iShVzizQzvo789SuynEKGpLTms9",
        signature="JO7vL3tOB1qQyfSeIVLvdEw9G1tCvL+lNj78XDAVM4t6UptADs3kXDTO2+2ZeEOLFL4/+wm+BBdSpo3kb3Cnsas=",
        message="example message",
        coin="btc",
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL,
        hex=False,
        common=None
    )
    print(f"Verify message deep link: {verify_message_result.url}")
    print(f"Request ID: {verify_message_result.request_id}")
    
    # Sign a message
    sign_message_result = trezor_sign_message(
        path="m/44'/0'/0'",
        message="Hello World!",
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL,
        coin="btc",
        hex=False,
        no_script_type=False,
        common=None
    )
    print(f"Sign message deep link: {sign_message_result.url}")
    print(f"Request ID: {sign_message_result.request_id}")
    
    # Sign a transaction
    inputs = [
        TxInputType(
            prev_hash="b035d89d4543ce5713c553d69431698116a822c57c03ddacf3f04b763d1999ac",
            prev_index=0,
            amount=3431747,
            sequence=None,
            address_n=[
                44 | 0x80000000,
                0 | 0x80000000,
                2 | 0x80000000,
                1,
                0
            ],
            script_type=ScriptType.SPEND_ADDRESS,
            multisig=None,
            script_pubkey=None,
            script_sig=None,
            witness=None,
            ownership_proof=None,
            commitment_data=None,
            orig_hash=None,
            orig_index=None,
            coinjoin_flags=None
        )
    ]
    
    outputs = [
        TxOutputType(
            address=None,
            address_n=[
                44 | 0x80000000,
                0 | 0x80000000,
                2 | 0x80000000,
                1,
                1
            ],
            amount=3181747,
            script_type=ScriptType.PAY_TO_ADDRESS,
            multisig=None,
            op_return_data=None,
            orig_hash=None,
            orig_index=None,
            payment_req_index=None
        ),
        TxOutputType(
            address="18WL2iZKmpDYWk1oFavJapdLALxwSjcSk2",
            address_n=None,
            amount=200000,
            script_type=ScriptType.PAY_TO_ADDRESS,
            multisig=None,
            op_return_data=None,
            orig_hash=None,
            orig_index=None,
            payment_req_index=None
        )
    ]
    
    sign_result = trezor_sign_transaction(
        coin="btc",
        inputs=inputs,
        outputs=outputs,
        callback_url="myapp://trezor/callback",
        request_id=None,
        trezor_environment=TrezorEnvironment.LOCAL,
        ref_txs=None,
        payment_requests=None,
        locktime=None,
        version=None,
        expiry=None,
        version_group_id=None,
        overwintered=None,
        timestamp=None,
        branch_id=None,
        push=False,
        amount_unit=None,
        unlock_path=None,
        serialize=None,
        chunkify=None,
        common=None
    )
    print(f"Sign transaction deep link: {sign_result.url}")
    print(f"Request ID: {sign_result.request_id}")
    
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
    elif isinstance(response, TrezorResponsePayload.VerifyMessage):
        print("Received Verify Message Result:")
        print(f"Result: {response.message}")
    elif isinstance(response, TrezorResponsePayload.MessageSignature):
        print("Received Message Signature:")
        print(f"Address: {response.address}")
        print(f"Signature: {response.signature}")
    elif isinstance(response, TrezorResponsePayload.SignedTransaction):
        print("Received Signed Transaction:")
        print(f"Signatures: {response.signatures}")
        print(f"Serialized TX: {response.serialized_tx}")
        if response.txid:
            print(f"Transaction ID: {response.txid}")
        
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

### Message Signing and Verification

The module supports both message signing and verification:

- **Message Signing** (`signMessage`): Signs a message using a private key derived from a BIP32 path
- **Message Verification** (`verifyMessage`): Verifies a signature against a message and signer address

Both operations support:
- **Text Messages**: Plain text messages
- **Hex Messages**: Messages encoded in hexadecimal format
- **Coin Support**: Specify the coin type for network-specific behavior
- **Address Types**: Support for all Bitcoin address types (Legacy, SegWit, Native SegWit)

### Transaction Signing

The `signTransaction` method supports:
- **Multiple Script Types**: SpendAddress, SpendMultisig, SpendWitness, SpendP2SHWitness, SpendTaproot, etc.
- **Reference Transactions**: Include `refTxs` for providing transaction history when using custom backends
- **Payment Requests**: Support for SLIP-24 payment requests
- **Various Coins**: Bitcoin, Testnet, and other supported cryptocurrencies
- **Advanced Features**: RBF (Replace-by-Fee), Coinjoin, Multisig transactions
- **Display Options**: Amount units, address chunking, transaction broadcasting

## Error Handling

### TrezorConnectError
- `SerdeError`: Serialization/deserialization errors with JSON data
- `UrlError`: URL parsing or formatting errors
- `EnvironmentError`: Environment-related errors (e.g., unavailable environment)
- `ClientError`: Failed to create the client or client operation errors
- `Other`: General errors not covered by other categories

Each error includes detailed information about what went wrong in the `error_details` field.