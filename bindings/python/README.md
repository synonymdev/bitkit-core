# Bitkit Scanner Logic

A Python library for parsing Bitcoin and Lightning Network invoices.

## Installation

```bash
pip install .
```

## Usage

```python
from bitkitcore import Scanner, DecodingError

# Example Lightning invoice
lightning_invoice = "lightning:lnbc543210n1pnjdrvfpp5s720f4z6wzvjwpdnrlpffgct375l46yu9c6cpe7gdvvdfay47cnsdqqcqzzsxqrrsssp53uty4kfw8k3wmw4ga802udavz7e64tc7dmaz2cmtkj9srfxaq3ps9p4gqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpqysgqwl2tdhzm9e6mtedt7a4263yw7dqxehdwjnjk23r4g8tuppk6rs994f6scunwsev3w207tjldwkpdt32rcegzphgk05c0lctv8he7smgqyfn5xq"

try:
    result = Scanner.decode(lightning_invoice)
    if isinstance(result, Scanner.Lightning):
        print(f"Amount (sats): {result.invoice.amount_satoshis}")
        print(f"Payment Hash: {result.invoice.payment_hash.hex()}")
        if result.invoice.description:
            print(f"Description: {result.invoice.description}")
        print(f"Network: {result.invoice.network_type}")
        print(f"Is Expired: {result.invoice.is_expired}")

# Example Bitcoin invoice
    bitcoin_invoice = "bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq?amount=0.00001&label=Test&message=Test%20Payment"
    result = Scanner.decode(bitcoin_invoice)
    if isinstance(result, Scanner.OnChain):
        print(f"Address: {result.invoice.address}")
        print(f"Amount (sats): {result.invoice.amount_satoshis}")
        if result.invoice.label:
            print(f"Label: {result.invoice.label}")
        if result.invoice.message:
            print(f"Message: {result.invoice.message}")

except DecodingError as e:
    print(f"Failed to decode invoice: {e}")
```

The library supports:
- Lightning Network BOLT-11 invoices
- Bitcoin addresses (with BIP-21 parameters)
- PubKey authentication strings
