# Paykit Module

This module provides UniFFI bindings for the [paykit-rs](https://github.com/pubky/paykit-rs) library, enabling Bitkit to interact with Pubky homeservers for payment endpoint management.

## Overview

Paykit enables applications to:
- Import authenticated sessions from Pubky Ring via deeplinks
- Publish payment endpoints (Bitcoin addresses, Lightning invoices) to a user's Pubky homeserver
- Retrieve payment endpoints from other Pubky users
- Manage session lifecycle and authentication

## Table of Contents
- [Core Concepts](#core-concepts)
- [User Flow](#user-flow)
- [Swift (iOS) Implementation](#swift-ios-implementation)
- [Kotlin (Android) Implementation](#kotlin-android-implementation)
- [API Reference](#api-reference)
- [Error Handling](#error-handling)
- [Testing](#testing)

## Core Concepts

### Sessions
Authentication with Pubky homeservers requires a session. Bitkit receives authenticated sessions as:
- **Session tokens** - Base64-encoded tokens received via deeplinks from Pubky Ring
- Sessions are created and managed by Pubky Ring, not Bitkit

### Payment Endpoints
Payment information stored on Pubky homeservers, identified by:
- **MethodId** - Payment method type (e.g., "onchain", "lightning")
- **EndpointData** - Payment details (e.g., Bitcoin address, Lightning invoice)

### Transports
- **PubkyAuthenticatedTransport** - For write operations (requires session)
- **PubkyUnauthenticatedTransport** - For read operations (no session needed)

## User Flow

### 1. Initial Setup - Connecting to Pubky Ring

1. User navigates to Settings → "Connect Pubky"
2. Bitkit displays QR code or "Open Pubky Ring" button with URI:
   ```
   pubkyring://session?callback=bitkit://session-data
   ```
3. Pubky Ring authenticates and returns session via deeplink
4. Bitkit saves session and shows "Connected"

### 2. Publishing Payment Endpoints

1. Bitkit generates new Bitcoin address
2. Calls `set_payment_endpoint()` with authenticated session
3. Address stored on user's homeserver
4. Bitkit monitors address for incoming payments

### 3. Retrieving Payment Endpoints

1. User scans/enters recipient's Pubky
2. Calls `get_payment_endpoint()` (no session required)
3. Retrieves and displays Bitcoin address
4. User can send payment

## Swift (iOS) Implementation

### Setup and Imports

```swift
import bitkitcore

class PaykitManager {
    private var currentSession: PubkyAuthenticatedTransport?
    private var sessionToken: String?

    init() {
        // Restore session on init if available
        Task {
            await restoreSession()
        }
    }
}
```

### Connecting to Pubky Ring

```swift
// Generate connection URI for QR code or deeplink
func generateConnectionURI() throws -> String {
    return try createPubkyRingSessionRequest(
        callbackUrl: "bitkit://paykit/session-data",
        additionalParams: nil
    )
    // Result: "pubkyring://session?callback=bitkit%3A%2F%2Fpaykit%2Fsession-data"
}

// Open Pubky Ring app
func openPubkyRing() {
    guard let uri = try? generateConnectionURI(),
          let url = URL(string: uri) else { return }
    UIApplication.shared.open(url)
}

// Handle callback from Pubky Ring
func handlePubkyCallback(url: URL) async throws {
    let urlString = url.absoluteString

    // Parse the deeplink
    let deeplink = try parsePaykitDeeplink(url: urlString)

    // Extract and validate token
    guard let token = deeplink.sessionToken else {
        throw PaykitError.InvalidToken(message: "No session token in deeplink")
    }

    // Create authenticated transport
    let sessionToken = SessionToken(token: token)
    let transport = try await createTransportFromSessionToken(token: sessionToken)

    // Save session
    self.currentSession = transport
    self.sessionToken = token

    // Persist to Keychain
    saveToKeychain(key: "pubky_session", value: token)

    // Update UI
    updateConnectionStatus(connected: true)
}
```

### Publishing Bitcoin Address

```swift
func publishBitcoinAddress(address: String) async throws {
    guard let session = currentSession else {
        throw PaykitError.SessionError("No active session")
    }

    let method = MethodId(id: "onchain")
    let endpointData = EndpointData(data: address)

    try await session.setPaymentEndpoint(method: method, data: endpointData)
    print("✅ Bitcoin address published: \(address)")
}

// Publish Lightning invoice
func publishLightningInvoice(invoice: String) async throws {
    guard let session = currentSession else {
        throw PaykitError.SessionError("No active session")
    }

    let method = MethodId(id: "lightning")
    let endpointData = EndpointData(data: invoice)

    try await session.setPaymentEndpoint(method: method, data: endpointData)
}
```

### Retrieving Payment Endpoints

```swift
func getRecipientBitcoinAddress(pubky: String) async throws -> String? {
    // No session needed for reading
    let transport = try PubkyUnauthenticatedTransport()
    let recipientKey = PublicKey(key: pubky)
    let method = MethodId(id: "onchain")

    if let endpoint = try await transport.getPaymentEndpoint(
        payee: recipientKey,
        method: method
    ) {
        return endpoint.data
    }
    return nil
}

// Get all available payment methods
func getRecipientPaymentMethods(pubky: String) async throws -> [String: String] {
    let transport = try PubkyUnauthenticatedTransport()
    let recipientKey = PublicKey(key: pubky)

    let payments = try await transport.getPaymentList(payee: recipientKey)

    var methods: [String: String] = [:]
    for (methodId, endpointData) in payments.entries {
        methods[methodId.id] = endpointData.data
    }
    return methods
}
```

### Session Management

```swift
// Restore saved session
func restoreSession() async throws {
    guard let savedToken = loadFromKeychain(key: "pubky_session") else { return }

    let sessionToken = SessionToken(token: savedToken)
    self.currentSession = try await createTransportFromSessionToken(token: sessionToken)
    self.sessionToken = savedToken
}

// Clear session (logout)
func clearSession() {
    currentSession = nil
    sessionToken = nil
    deleteFromKeychain(key: "pubky_session")
    updateConnectionStatus(connected: false)
}

// Check if session exists
func hasActiveSession() -> Bool {
    return currentSession != nil
}
```

## Kotlin (Android) Implementation

### Setup and Initialization

```kotlin
import bitkitcore.*
import kotlinx.coroutines.*

class PaykitManager(private val context: Context) {
    private var currentSession: PubkyAuthenticatedTransport? = null
    private var sessionToken: String? = null
    private val prefs = context.getSharedPreferences("paykit_prefs", Context.MODE_PRIVATE)

    init {
        // Restore session on init
        GlobalScope.launch {
            restoreSession()
        }
    }
}
```

### Connecting to Pubky Ring

```kotlin
// Generate connection URI for QR code or deeplink
fun generateConnectionURI(): String {
    return createPubkyRingSessionRequest(
        callbackUrl = "bitkit://paykit/session-data",
        additionalParams = null
    )
    // Result: "pubkyring://session?callback=bitkit%3A%2F%2Fpaykit%2Fsession-data"
}

// Open Pubky Ring app
fun openPubkyRing() {
    val uri = generateConnectionURI()
    val intent = Intent(Intent.ACTION_VIEW, Uri.parse(uri))

    if (intent.resolveActivity(context.packageManager) != null) {
        context.startActivity(intent)
    } else {
        // Prompt to install Pubky Ring
        showInstallPrompt()
    }
}

// Handle callback from Pubky Ring
suspend fun handlePubkyCallback(uri: Uri) {
    try {
        // Parse the deeplink
        val deeplink = parsePaykitDeeplink(uri.toString())

        // Extract and validate token
        val token = deeplink.sessionToken
            ?: throw PaykitError.InvalidToken("No session token in deeplink")

        // Create authenticated transport
        val sessionToken = SessionToken(token)
        val transport = createTransportFromSessionToken(sessionToken)

        // Save session
        currentSession = transport
        this.sessionToken = token

        // Persist to SharedPreferences
        prefs.edit().putString("pubky_session", token).apply()

        // Update UI
        updateConnectionStatus(true)
    } catch (e: PaykitError) {
        handleError(e)
    }
}
```

### Publishing Bitcoin Address

```kotlin
suspend fun publishBitcoinAddress(address: String) {
    val session = currentSession
        ?: throw PaykitError.SessionError("No active session")

    val method = MethodId("onchain")
    val endpointData = EndpointData(address)

    session.setPaymentEndpoint(method, endpointData)
    Log.d("Paykit", "✅ Bitcoin address published: $address")
}

// Publish Lightning invoice
suspend fun publishLightningInvoice(invoice: String) {
    val session = currentSession
        ?: throw PaykitError.SessionError("No active session")

    val method = MethodId("lightning")
    val endpointData = EndpointData(invoice)

    session.setPaymentEndpoint(method, endpointData)
}
```

### Retrieving Payment Endpoints

```kotlin
suspend fun getRecipientBitcoinAddress(pubky: String): String? {
    return try {
        // No session needed for reading
        val transport = PubkyUnauthenticatedTransport()
        val recipientKey = PublicKey(pubky)
        val method = MethodId("onchain")

        transport.getPaymentEndpoint(recipientKey, method)?.data
    } catch (e: PaykitError) {
        Log.e("Paykit", "Failed to get recipient address", e)
        null
    }
}

// Get all available payment methods
suspend fun getRecipientPaymentMethods(pubky: String): Map<String, String> {
    val transport = PubkyUnauthenticatedTransport()
    val recipientKey = PublicKey(pubky)

    val payments = transport.getPaymentList(recipientKey)

    return payments.entries.mapKeys { it.key.id }
        .mapValues { it.value.data }
}
```

### Session Management

```kotlin
// Restore saved session
suspend fun restoreSession() {
    prefs.getString("pubky_session", null)?.let { savedToken ->
        try {
            val sessionToken = SessionToken(savedToken)
            currentSession = createTransportFromSessionToken(sessionToken)
            this.sessionToken = savedToken
        } catch (e: PaykitError) {
            // Session invalid or expired
            clearSession()
        }
    }
}

// Clear session (logout)
fun clearSession() {
    currentSession = null
    sessionToken = null
    prefs.edit().remove("pubky_session").apply()
    updateConnectionStatus(false)
}
```

## API Reference

### Core Types

| Type | Description | Fields |
|------|-------------|--------|
| `SessionToken` | Wrapper for session token string | `token: String` |
| `MethodId` | Payment method identifier | `id: String` |
| `EndpointData` | Payment endpoint data | `data: String` |
| `PublicKey` | Pubky public key | `key: String` |
| `SupportedPayments` | Collection of payment methods | `entries: Map<MethodId, EndpointData>` |

### Session Functions

| Function | Description | Parameters | Returns |
|----------|-------------|------------|---------|
| `createPubkyRingSessionRequest` | Generate URL to request session from Pubky Ring | `callbackUrl: String`, `additionalParams: Map?` | `String` |
| `parsePaykitDeeplink` | Parse deeplink URL | `url: String` | `PaykitDeeplink` |
| `createTransportFromSessionToken` | Create session from token | `token: SessionToken` | `PubkyAuthenticatedTransport` |

### Authenticated Operations (Requires Session)

| Function | Description | Parameters | Returns |
|----------|-------------|------------|---------|
| `setPaymentEndpoint` | Store payment endpoint | `method: MethodId`, `data: EndpointData` | `void` |
| `removePaymentEndpoint` | Remove payment endpoint | `method: MethodId` | `void` |

### Unauthenticated Operations (No Session Required)

| Function | Description | Parameters | Returns |
|----------|-------------|------------|---------|
| `getPaymentEndpoint` | Retrieve specific payment endpoint | `pubky: PublicKey`, `method: MethodId` | `EndpointData?` |
| `getPaymentList` | Get all payment methods | `pubky: PublicKey` | `SupportedPayments` |
| `getKnownContacts` | Get user's contacts | `pubky: PublicKey` | `List<PublicKey>` |

## Error Handling

### Error Types

```swift
// Swift
enum PaykitError: Error {
    case SessionError(String)    // Session-related errors
    case Transport(String)        // Network/transport errors
    case InvalidToken(String)     // Token validation errors
    case NotFound(String)         // Resource not found
}
```

```kotlin
// Kotlin
sealed class PaykitError : Exception() {
    data class SessionError(val message: String) : PaykitError()
    data class Transport(val message: String) : PaykitError()
    data class InvalidToken(val message: String) : PaykitError()
    data class NotFound(val message: String) : PaykitError()
}
```

### Error Handling Best Practices

1. **Session Expiration**: When a session expires, clear it and prompt for reconnection
2. **Network Errors**: Implement retry logic with exponential backoff
3. **Invalid Tokens**: Clear invalid tokens and request new authentication
4. **Not Found**: Handle gracefully when payment endpoints don't exist

## Testing

### Unit Test Example (Swift)

```swift
func testPaykitDeeplinkFlow() async throws {
    // Test deeplink parsing
    let deeplink = "bitkit://paykit/session?token=eyJ0ZXN0IjoidG9rZW4ifQ"
    let parsed = try parsePaykitDeeplink(url: deeplink)
    XCTAssertNotNil(parsed.sessionToken)

    // Test session token validation
    let sessionToken = SessionToken(token: "eyJ0ZXN0IjoidG9rZW4ifQ")
    XCTAssertNoThrow(try sessionToken.validate())

    // Mock session creation from token
    // In real tests, you would use a valid token from Pubky Ring
    let mockToken = SessionToken(token: "valid_base64_encoded_session_data")
    // let transport = try await createTransportFromSessionToken(token: mockToken)
    // XCTAssertNotNil(transport)
}

func testPaymentEndpoints() async throws {
    // Test unauthenticated read operations
    let transport = try PubkyUnauthenticatedTransport()
    let pubkey = PublicKey(key: "test_pubky_address")
    let method = MethodId(id: "onchain")

    // Try to retrieve payment endpoint (may return nil if not set)
    let endpoint = try await transport.getPaymentEndpoint(
        payee: pubkey,
        method: method
    )
    // Assert based on expected test data
}
```

### Integration Test Example (Kotlin)

```kotlin
@Test
fun testPaykitDeeplinkFlow() = runBlocking {
    // Test deeplink parsing
    val deeplink = "bitkit://paykit/session?token=eyJ0ZXN0IjoidG9rZW4ifQ"
    val parsed = parsePaykitDeeplink(deeplink)
    assertNotNull(parsed.sessionToken)

    // Test session token validation
    val sessionToken = SessionToken("eyJ0ZXN0IjoidG9rZW4ifQ")
    // Validation happens during transport creation

    // Mock session creation from token
    // In real tests, you would use a valid token from Pubky Ring
    val mockToken = SessionToken("valid_base64_encoded_session_data")
    // val transport = createTransportFromSessionToken(mockToken)
    // assertNotNull(transport)
}

@Test
fun testPaymentEndpoints() = runBlocking {
    // Test unauthenticated read operations
    val transport = PubkyUnauthenticatedTransport()
    val pubkey = PublicKey("test_pubky_address")
    val method = MethodId("onchain")

    // Try to retrieve payment endpoint (may return null if not set)
    val endpoint = transport.getPaymentEndpoint(pubkey, method)
    // Assert based on expected test data
}
```

## Security Considerations

1. **Session Storage**: Always store session tokens in secure storage:
   - iOS: Use Keychain Services
   - Android: Use Android Keystore or encrypted SharedPreferences

2. **Token Expiration**: Implement proper token refresh when sessions expire

3. **Transport Security**: All communication with homeservers uses HTTPS

4. **Key Management**: Never store unencrypted private keys

## Platform-Specific Notes

### iOS
- Use `@MainActor` for UI updates after async operations
- Handle `Info.plist` URL scheme registration for deeplinks
- Implement `application(_:open:options:)` in AppDelegate

### Android
- Register intent filters in AndroidManifest.xml for deeplinks
- Use Coroutines for async operations
- Handle lifecycle-aware session management

## Support

For issues or questions:
- Review test examples in `src/modules/paykit/tests.rs`
- Check integration tests in `src/modules/paykit/integration_tests.rs`
- File issues at: https://github.com/synonymdev/bitkit-core/issues