# LNURL Module

This module handles LNURL-related functionality, including Lightning Address invoice generation.

## Usage Examples

### iOS (Swift) Example
```swift
import BitkitCore

func generateInvoice() async {
    do {
        let invoice = try await getLnurlInvoice(address: "user@domain.com", amountSatoshis: 1000)
        print("Generated invoice: \(invoice)")
    } catch let error as LnurlError {
        switch error {
        case .invalidAddress:
            print("Invalid Lightning Address format")
        case .clientCreationFailed:
            print("Failed to create LNURL client")
        case .requestFailed:
            print("LNURL request failed")
        case .invalidResponse:
            print("Received invalid response from LNURL service")
        case .invalidAmount(let amount, let min, let max):
            print("Amount \(amount) is outside allowed range (\(min) - \(max) sats)")
        case .invoiceCreationFailed(let message):
            print("Failed to generate invoice: \(message)")
        }
    }
}
```

### Android (Kotlin) Example
```kotlin
import com.synonym.bitkitcore.*

suspend fun generateInvoice() {
    try {
        val invoice = getLnurlInvoice("user@domain.com", 1000)
        println("Generated invoice: $invoice")
    } catch (e: LnurlError) {
        when (e) {
            is LnurlError.InvalidAddress -> println("Invalid Lightning Address format")
            is LnurlError.ClientCreationFailed -> println("Failed to create LNURL client")
            is LnurlError.RequestFailed -> println("LNURL request failed")
            is LnurlError.InvalidResponse -> println("Received invalid response from LNURL service")
            is LnurlError.InvalidAmount -> println(
                "Amount ${e.amountSatoshis} is outside allowed range " +
                "(${e.min} - ${e.max} sats)"
            )
            is LnurlError.InvoiceCreationFailed -> println("Failed to generate invoice: ${e.message}")
        }
    }
}
```

### Python Example
```python
from bitkitcore import get_lnurl_invoice, LnurlError

async def generate_invoice():
    try:
        invoice = await get_lnurl_invoice("user@domain.com", 1000)  # 1000 sats
        print(f"Generated invoice: {invoice}")
    except LnurlError as e:
        if isinstance(e, LnurlError.InvalidAddress):
            print("Invalid Lightning Address format")
        elif isinstance(e, LnurlError.ClientCreationFailed):
            print("Failed to create LNURL client")
        elif isinstance(e, LnurlError.RequestFailed):
            print("LNURL request failed")
        elif isinstance(e, LnurlError.InvalidResponse):
            print("Received invalid response from LNURL service")
        elif isinstance(e, LnurlError.InvalidAmount):
            print(f"Amount {e.amount_satoshis} is outside allowed range " +
                  f"({e.min} - {e.max} sats)")
        elif isinstance(e, LnurlError.InvoiceCreationFailed):
            print(f"Failed to generate invoice: {e.message}")
```

## Error Handling

### LnurlError
- `InvalidAddress`: The Lightning Address format is invalid
- `ClientCreationFailed`: Failed to create the LNURL client
- `RequestFailed`: The LNURL request failed
- `InvalidResponse`: Received an invalid response from LNURL service
- `InvalidAmount`: Amount is outside the allowed range, includes:
  - `amount_satoshis`: The invalid amount that was provided
  - `min`: Minimum allowed amount in satoshis
  - `max`: Maximum allowed amount in satoshis
- `InvoiceCreationFailed`: Failed to generate the invoice, includes:
  - `message`: Detailed error message explaining the failure