# Scanner Module

The Scanner module is the core component for decoding and parsing various types of Bitcoin and Lightning Network invoices and addresses.

## Features
- Bitcoin Address Support
  - Decodes multiple address formats (P2PKH, P2SH, P2WPKH, P2WSH, P2TR)
  - Processes BIP21 Bitcoin payment URIs
  - Network support for Mainnet, Testnet, Regtest, and Signet
- Lightning Network Features
  - Decodes BOLT-11 Lightning invoices
  - Supports Lightning Addresses
  - Handles multiple LNURL types:
    - LNURL-pay
    - LNURL-withdraw
    - LNURL-auth
    - LNURL-channel
  - Node connection string parsing
- Pubky authentication string handling
- Treasure Hunt and Orange Ticket decoding

## Usage Examples

### iOS (Swift)
```swift
import BitkitCore

func decodeInvoice() async {
    do {
        let result = try await Scanner.decode("lnbc500n1ps...")
        switch result {
        case .lightning(let invoice):
            print("Lightning Invoice:")
            print("Amount: \(invoice.amountSatoshis) sats")
            print("Payment Hash: \(invoice.paymentHash.hex)")
            print("Timestamp: \(invoice.timestampSeconds)")
            print("Expiry: \(invoice.expirySeconds) seconds")
            print("Is Expired: \(invoice.isExpired)")
            if let desc = invoice.description {
                print("Description: \(desc)")
            }
            print("Network: \(invoice.networkType)")
            if let nodeId = invoice.payeeNodeId {
                print("Node ID: \(nodeId.hex)")
            }
            
        case .onChain(let invoice):
            print("On-chain Invoice:")
            print("Address: \(invoice.address)")
            print("Amount: \(invoice.amountSatoshis) sats")
            if let label = invoice.label {
                print("Label: \(label)")
            }
            if let message = invoice.message {
                print("Message: \(message)")
            }
            if let params = invoice.params {
                print("Parameters:")
                params.forEach { key, value in
                    print("\t\(key): \(value)")
                }
            }
            
        case .lnurlPay(let data):
            print("LNURL-pay:")
            print("URI: \(data.uri)")
            print("Callback: \(data.callback)")
            print("Min Sendable: \(data.minSendable) sats")
            print("Max Sendable: \(data.maxSendable) sats")
            print("Metadata: \(data.metadataStr)")
            if let commentLength = data.commentAllowed {
                print("Comment allowed (max length): \(commentLength)")
            }
            print("Allows Nostr: \(data.allowsNostr)")
            if let pubkey = data.nostrPubkey {
                print("Nostr Pubkey: \(pubkey.hex)")
            }
            
        case .lnurlWithdraw(let data):
            print("LNURL-withdraw:")
            print("URI: \(data.uri)")
            print("Callback: \(data.callback)")
            print("K1: \(data.k1)")
            print("Description: \(data.defaultDescription)")
            print("Max Withdrawable: \(data.maxWithdrawable) sats")
            if let min = data.minWithdrawable {
                print("Min Withdrawable: \(min) sats")
            }
            print("Tag: \(data.tag)")
            
        case .lnurlAuth(let data):
            print("LNURL-auth:")
            print("URI: \(data.uri)")
            print("Tag: \(data.tag)")
            print("K1: \(data.k1)")
            
        case .lnurlChannel(let data):
            print("LNURL-channel:")
            print("URI: \(data.uri)")
            print("Callback: \(data.callback)")
            print("K1: \(data.k1)")
            print("Tag: \(data.tag)")
            
        case .lnurlAddress(let data):
            print("Lightning Address:")
            print("URI: \(data.uri)")
            print("Username: \(data.username)")
            print("Domain: \(data.domain)")
            
        case .nodeId(let url, let network):
            print("Node Connection:")
            print("URL: \(url)")
            print("Network: \(network)")
            
        case .treasureHunt(let chestId):
            print("Treasure Hunt:")
            print("Chest ID: \(chestId)")
            
        case .orangeTicket(let ticketId):
            print("Orange Ticket:")
            print("Ticket ID: \(ticketId)")
            
        case .pubkyAuth(let auth):
            print("pubky Auth:")
            print("Data: \(auth.data)")
        }
    } catch let error as DecodingError {
        switch error {
        case .invalidFormat:
            print("Invalid invoice format")
        case .invalidNetwork:
            print("Invalid network type")
        case .invalidAmount:
            print("Invalid amount")
        case .invalidLNURLPayAmount(let amount, let min, let max):
            print("Invalid LNURL pay amount: \(amount) sats (must be between \(min) and \(max) sats)")
        case .invalidTimestamp:
            print("Invalid timestamp")
        case .invalidChecksum:
            print("Invalid checksum")
        case .invalidResponse:
            print("Invalid response")
        case .unsupportedType:
            print("Unsupported invoice type")
        case .invalidAddress:
            print("Invalid address")
        case .requestFailed:
            print("LNURL request failed")
        case .clientCreationFailed:
            print("Client creation failed")
        case .invoiceCreationFailed(let message):
            print("Invoice creation failed: \(message)")
        }
    }
}
```

### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

suspend fun decodeInvoice() {
    try {
        when (val result = Scanner.decode("lnbc500n1ps...")) {
            is Scanner.Lightning -> with(result.invoice) {
                println("Lightning Invoice:")
                println("Amount: $amountSatoshis sats")
                println("Payment Hash: ${paymentHash.joinToString("") { "%02x".format(it) }}")
                println("Timestamp: $timestampSeconds")
                println("Expiry: $expirySeconds seconds")
                println("Is Expired: $isExpired")
                description?.let { println("Description: $it") }
                println("Network: $networkType")
                payeeNodeId?.let { 
                    println("Node ID: ${it.joinToString("") { b -> "%02x".format(b) }}")
                }
            }
            
            is Scanner.OnChain -> with(result.invoice) {
                println("On-chain Invoice:")
                println("Address: $address")
                println("Amount: $amountSatoshis sats")
                label?.let { println("Label: $it") }
                message?.let { println("Message: $it") }
                params?.forEach { (key, value) ->
                    println("\t$key: $value")
                }
            }
            
            is Scanner.LnurlPay -> with(result.data) {
                println("LNURL-pay:")
                println("URI: $uri")
                println("Callback: $callback")
                println("Min Sendable: $minSendable sats")
                println("Max Sendable: $maxSendable sats")
                println("Metadata: $metadataStr")
                commentAllowed?.let { println("Comment allowed (max length): $it") }
                println("Allows Nostr: $allowsNostr")
                nostrPubkey?.let { 
                    println("Nostr Pubkey: ${it.joinToString("") { b -> "%02x".format(b) }}")
                }
            }
            
            is Scanner.LnurlWithdraw -> with(result.data) {
                println("LNURL-withdraw:")
                println("URI: $uri")
                println("Callback: $callback")
                println("K1: $k1")
                println("Description: $defaultDescription")
                println("Max Withdrawable: $maxWithdrawable sats")
                minWithdrawable?.let { println("Min Withdrawable: $it sats") }
                println("Tag: $tag")
            }
            
            is Scanner.LnurlAuth -> with(result.data) {
                println("LNURL-auth:")
                println("URI: $uri")
                println("Tag: $tag")
                println("K1: $k1")
            }
            
            is Scanner.LnurlChannel -> with(result.data) {
                println("LNURL-channel:")
                println("URI: $uri")
                println("Callback: $callback")
                println("K1: $k1")
                println("Tag: $tag")
            }
            
            is Scanner.LnurlAddress -> with(result.data) {
                println("Lightning Address:")
                println("URI: $uri")
                println("Username: $username")
                println("Domain: $domain")
            }
            
            is Scanner.NodeId -> {
                println("Node Connection:")
                println("URL: ${result.url}")
                println("Network: ${result.network}")
            }
            
            is Scanner.TreasureHunt -> {
                println("Treasure Hunt:")
                println("Chest ID: ${result.chestId}")
            }
            
            is Scanner.OrangeTicket -> {
                println("Orange Ticket:")
                println("Ticket ID: ${result.ticketId}")
            }
            
            is Scanner.PubkyAuth -> {
                println("pubky Auth:")
                println("Data: ${result.auth.data}")
            }
        }
    } catch (e: DecodingError) {
        when (e) {
            is DecodingError.InvalidFormat -> println("Invalid invoice format")
            is DecodingError.InvalidNetwork -> println("Invalid network type")
            is DecodingError.InvalidAmount -> println("Invalid amount")
            is DecodingError.InvalidLNURLPayAmount -> println(
                "Invalid LNURL pay amount: ${e.amount_satoshis} sats " +
                "(must be between ${e.min} and ${e.max} sats)"
            )
            is DecodingError.InvalidTimestamp -> println("Invalid timestamp")
            is DecodingError.InvalidChecksum -> println("Invalid checksum")
            is DecodingError.InvalidResponse -> println("Invalid response")
            is DecodingError.UnsupportedType -> println("Unsupported invoice type")
            is DecodingError.InvalidAddress -> println("Invalid address")
            is DecodingError.RequestFailed -> println("LNURL request failed")
            is DecodingError.ClientCreationFailed -> println("Client creation failed")
            is DecodingError.InvoiceCreationFailed -> println("Invoice creation failed: ${e.error_message}")
        }
    }
}
```

### Python
```python
from bitkitcore import Scanner, DecodingError

try:
  result = await Scanner.decode("lnbc500n1ps...")
  if isinstance(result, Scanner.Lightning):
    print("Lightning Invoice:")
    print(f"Amount: {result.invoice.amount_satoshis} sats")
    print(f"Payment Hash: {result.invoice.payment_hash.hex()}")
    print(f"Timestamp: {result.invoice.timestamp_seconds}")
    print(f"Expiry: {result.invoice.expiry_seconds} seconds")
    print(f"Is Expired: {result.invoice.is_expired}")
    if result.invoice.description:
      print(f"Description: {result.invoice.description}")
    print(f"Network: {result.invoice.network_type}")
    if result.invoice.payee_node_id:
      print(f"Node ID: {result.invoice.payee_node_id.hex()}")

  elif isinstance(result, Scanner.OnChain):
    print("On-chain Invoice:")
    print(f"Address: {result.invoice.address}")
    print(f"Amount: {result.invoice.amount_satoshis} sats")
    if result.invoice.label:
      print(f"Label: {result.invoice.label}")
    if result.invoice.message:
      print(f"Message: {result.invoice.message}")
    if result.invoice.params:
      print("Parameters:")
      for key, value in result.invoice.params.items():
        print(f"\t{key}: {value}")

  elif isinstance(result, Scanner.LnurlPay):
    print("LNURL-pay:")
    print(f"URI: {result.data.uri}")
    print(f"Callback: {result.data.callback}")
    print(f"Min Sendable: {result.data.min_sendable} sats")
    print(f"Max Sendable: {result.data.max_sendable} sats")
    print(f"Metadata: {result.data.metadata_str}")
    if result.data.comment_allowed:
      print(f"Comment allowed (max length): {result.data.comment_allowed}")
    print(f"Allows Nostr: {result.data.allows_nostr}")
    if result.data.nostr_pubkey:
      print(f"Nostr Pubkey: {result.data.nostr_pubkey.hex()}")

  elif isinstance(result, Scanner.LnurlWithdraw):
    print("LNURL-withdraw:")
    print(f"URI: {result.data.uri}")
    print(f"Callback: {result.data.callback}")
    print(f"K1: {result.data.k1}")
    print(f"Description: {result.data.default_description}")
    print(f"Max Withdrawable: {result.data.max_withdrawable} sats")
    if result.data.min_withdrawable:
      print(f"Min Withdrawable: {result.data.min_withdrawable} sats")
    print(f"Tag: {result.data.tag}")

  elif isinstance(result, Scanner.LnurlAuth):
    print("LNURL-auth:")
    print(f"URI: {result.data.uri}")
    print(f"Tag: {result.data.tag}")
    print(f"K1: {result.data.k1}")

  elif isinstance(result, Scanner.LnurlChannel):
    print("LNURL-channel:")
    print(f"URI: {result.data.uri}")
    print(f"Callback: {result.data.callback}")
    print(f"K1: {result.data.k1}")
    print(f"Tag: {result.data.tag}")

  elif isinstance(result, Scanner.LnurlAddress):
    print("Lightning Address:")
    print(f"URI: {result.data.uri}")
    print(f"Username: {result.data.username}")
    print(f"Domain: {result.data.domain}")

  elif isinstance(result, Scanner.NodeId):
    print("Node Connection:")
    print(f"URL: {result.url}")
    print(f"Network: {result.network}")

  elif isinstance(result, Scanner.TreasureHunt):
    print("Treasure Hunt:")
    print(f"Chest ID: {result.chest_id}")

  elif isinstance(result, Scanner.OrangeTicket):
    print("Orange Ticket:")
    print(f"Ticket ID: {result.ticket_id}")

  elif isinstance(result, Scanner.PubkyAuth):
    print("pubky Auth:")
    print(f"Data: {result.auth.data}")

except DecodingError as e:
  if isinstance(e, DecodingError.InvalidFormat):
    print("Invalid invoice format")
  elif isinstance(e, DecodingError.InvalidNetwork):
    print("Invalid network type")
  elif isinstance(e, DecodingError.InvalidAmount):
    print("Invalid amount")
  elif isinstance(e, DecodingError.InvalidLNURLPayAmount):
    print(f"Invalid LNURL pay amount: {e.amount_satoshis} sats " +
          f"(must be between {e.min} and {e.max} sats)")
  elif isinstance(e, DecodingError.InvalidTimestamp):
    print("Invalid timestamp")
  elif isinstance(e, DecodingError.InvalidChecksum):
    print("Invalid checksum")
  elif isinstance(e, DecodingError.InvalidResponse):
    print("Invalid response")
  elif isinstance(e, DecodingError.UnsupportedType):
    print("Unsupported invoice type")
  elif isinstance(e, DecodingError.InvalidAddress):
    print("Invalid address")
  elif isinstance(e, DecodingError.RequestFailed):
    print("LNURL request failed")
  elif isinstance(e, DecodingError.ClientCreationFailed):
    print("Client creation failed")
  elif isinstance(e, DecodingError.InvoiceCreationFailed):
    print(f"Invoice creation failed: {e.error_message}")
```

## Supported Types

The Scanner can decode:
- Lightning Network BOLT-11 invoices
- Bitcoin addresses (P2PKH, P2SH, P2WPKH, P2WSH, P2TR)
- LNURL-pay requests
- LNURL-withdraw requests
- LNURL-auth requests
- LNURL-channel requests
- Lightning Addresses
- BIP21 Bitcoin URIs
- Pubky authentication strings

## Error Handling

The module uses the following error types:
- `InvalidFormat`: The input string format is invalid
- `InvalidNetwork`: The network type is invalid or mismatched
- `InvalidAmount`: The amount specified is invalid
- `InvalidTimestamp`: The timestamp is invalid
- `InvalidChecksum`: The checksum verification failed
- `InvalidResponse`: Received an invalid response
- `UnsupportedType`: The invoice type is not supported
- `InvalidAddress`: The address format is invalid
- `RequestFailed`: The LNURL request failed
- `ClientCreationFailed`: Failed to create the client
- `InvoiceCreationFailed`: Failed to create the invoice