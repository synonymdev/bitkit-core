# Boltz Module

This module integrates [Boltz](https://boltz.exchange) submarine and reverse
swaps so funds can move between onchain Bitcoin and Lightning channels:

- **Submarine swap** (onchain → Lightning): you lock onchain BTC, Boltz pays a
  Lightning invoice your node generated. Use this to **add** Lightning balance
  from onchain funds.
- **Reverse swap** (Lightning → onchain): you pay a Boltz hold invoice over
  Lightning, Boltz locks onchain BTC, and this module claims it to your onchain
  address. Use this to **drain** Lightning balance to onchain.

The dangerous cryptography (MuSig2 Taproot cooperative signing, swap scripts,
claim/refund transaction construction) is handled by the
[`boltz-client`](https://crates.io/crates/boltz-client) crate. This module adds
persistence, lifecycle tracking, automatic claiming, and the FFI surface.

## Responsibility split with the app

bitkit-core does **not** own the Lightning node. The app (via LDK Node) and
bitkit-core cooperate:

| Step | Owner |
|------|-------|
| Generate the BOLT11 invoice (submarine) | **App** — `node.bolt11Payment().receive(...)` |
| Pay the hold invoice (reverse) | **App** — `node.bolt11Payment().send(...)` |
| Provide an onchain claim address (reverse) | **App** — `node.onchainPayment().newAddress()` |
| Send the onchain lockup (submarine) | **App** — `node.onchainPayment().sendToAddress(...)` |
| Call Boltz, derive keys & preimage, track status | **bitkit-core** |
| Build, sign and broadcast claim/refund transactions | **bitkit-core** |

The app passes its **wallet mnemonic** (and BIP39 passphrase, if any) to the
create/claim/refund/start-updates calls.

## Keys, secrets & recovery

Swap keys are **derived deterministically from the wallet seed**, never randomly
generated and never stored. Each swap uses a unique index under Boltz's BIP85
scheme (`m/26589'/0'/0'/{index}`); for reverse swaps the preimage is
`sha256(swapKey)`. `boltz.db` persists only that **index** (plus status and
metadata) — it holds **no key material**, so a leaked database cannot move funds.

This makes swaps recoverable two independent ways:

1. **Same device / restored `boltz.db`** — the index is on disk; combine it with
   the in-memory seed to re-derive keys. Use `boltzListPendingSwaps` after
   `boltzStartSwapUpdates` on startup.
2. **Seed only (`boltz.db` lost)** — because keys derive from the seed, an
   in-flight swap can still be recovered: the same BIP85 swap mnemonic can be
   registered with Boltz's rescue API
   (`https://boltz.exchange/rescue/external?mode=rescue-key`) to re-enumerate
   swaps and re-derive their keys by scanning indices.

> **The BIP39 passphrase must match the wallet's.** Keys derived under the wrong
> passphrase (or a typo'd mnemonic) will not control the locked funds. Pass the
> exact same `mnemonic`/`bip39Passphrase` used by the wallet to every Boltz call.

Secrets exist in process memory only while a swap is being created, claimed or
refunded; the background updates stream additionally holds the mnemonic in memory
for its lifetime (dropped on `boltzStopSwapUpdates`) so it can auto-claim. As
elsewhere in the app, `boltz.db` relies on the platform's app-sandbox/filesystem
encryption at rest.

## Lifecycle

Status strings mirror the [Boltz lifecycle](https://api.docs.boltz.exchange/lifecycle.html)
and are surfaced as the typed `BoltzSwapStatus` enum (unknown future states fall
back to `Unknown { raw }`). Register a `BoltzEventListener` via
`boltzStartSwapUpdates` to receive `BoltzSwapEvent`s over a managed WebSocket.

**Reverse swaps are claimed automatically**: once Boltz's lockup reaches
`transaction.confirmed`, this module builds and broadcasts the claim transaction
(cooperative key-path first, script-path fallback) and emits
`BoltzSwapEvent.Claimed { txid }`. Claiming on confirmation (not mempool) avoids
revealing the preimage before the lockup is final; call `boltzClaimReverseSwap`
manually if you accept the 0-conf risk. The auto-claim fee rate is the
`feeRateSatPerVb` passed to `boltzStartSwapUpdates` — **Bitkit owns fee
estimation** and should pass its current recommended rate so the claim confirms
before the swap times out; restart the stream to apply an updated rate.

**Submarine refunds are manual** (the module needs a destination address): on
`invoice.failedToPay` / `transaction.lockupFailed` / `swap.expired`, call
`boltzRefundSubmarineSwap` with an onchain address.

## FFI surface

```
boltzGetSubmarineLimits(network)                         -> BoltzPairInfo
boltzGetReverseLimits(network)                           -> BoltzPairInfo
boltzCreateSubmarineSwap(network, electrumUrl, invoice, mnemonic, bip39Passphrase?)         -> SubmarineSwapResponse
boltzCreateReverseSwap(network, electrumUrl, amountSat, claimAddress, mnemonic, bip39Passphrase?) -> ReverseSwapResponse
boltzListSwaps()                                         -> [BoltzSwap]
boltzListPendingSwaps()                                  -> [BoltzSwap]
boltzGetSwap(swapId)                                     -> BoltzSwap?
boltzClaimReverseSwap(swapId, mnemonic, bip39Passphrase?, feeRateSatPerVb?)          -> String (txid)
boltzRefundSubmarineSwap(swapId, refundAddress, mnemonic, bip39Passphrase?, feeRateSatPerVb?) -> String (txid)
boltzStartSwapUpdates(network, listener, mnemonic, bip39Passphrase?, feeRateSatPerVb?) // managed WebSocket
boltzStopSwapUpdates()
```

`network` is `BoltzNetwork.{Mainnet, Testnet, Regtest}`. `electrumUrl` accepts
`ssl://host:port`, `tcp://host:port`, or a bare `host:port` (treated as TLS) and
is stored per-swap for later claim/refund broadcasting. `mnemonic` is the wallet
mnemonic; pass the same `bip39Passphrase` the wallet uses (omit/`null` if none).

`boltzClaimReverseSwap` and `boltzRefundSubmarineSwap` are **idempotent**: if the
swap already has a recorded claim/refund tx, the existing txid is returned
without re-broadcasting.

**Only one updates stream runs at a time.** `boltzStartSwapUpdates` stops any
previous stream, so a single network is tracked at once; call it again to switch
networks.

## Usage Examples

### Reverse swap — Lightning → onchain

#### iOS (Swift)
```swift
import BitkitCore

// 1. Register a listener once (auto-claims reverse swaps).
final class SwapListener: BoltzEventListener {
    func onEvent(event: BoltzSwapEvent) {
        switch event {
        case .statusUpdate(let swapId, let status):
            print("swap \(swapId): \(status)")
        case .claimed(let swapId, let txid):
            print("reverse swap \(swapId) claimed in \(txid)")
        case .refunded(let swapId, let txid):
            print("swap \(swapId) refunded in \(txid)")
        case .error(let swapId, let message):
            print("swap \(swapId) error: \(message)")
        }
    }
}
// `mnemonic` is the wallet's seed phrase; pass the wallet's BIP39 passphrase too
// (or nil). It's held in memory for the stream's lifetime to auto-claim.
try await boltzStartSwapUpdates(
    network: .mainnet,
    listener: SwapListener(),
    mnemonic: wallet.mnemonic,
    bip39Passphrase: nil,
    feeRateSatPerVb: feeService.currentSatPerVb()   // Bitkit-provided fee for auto-claims
)

func drainToOnchain(amountSat: UInt64) async throws {
    // 2. A fresh onchain address from the LDK node receives the funds.
    let claimAddress = try lightningService.node.onchainPayment().newAddress()

    // 3. Create the swap and pay its hold invoice over Lightning.
    let swap = try await boltzCreateReverseSwap(
        network: .mainnet,
        electrumUrl: "ssl://electrum.blockstream.info:50002",
        amountSat: amountSat,
        claimAddress: claimAddress,
        mnemonic: wallet.mnemonic,
        bip39Passphrase: nil
    )
    _ = try lightningService.node.bolt11Payment().send(invoice: swap.invoice, sendingParameters: nil)

    // 4. Once Boltz locks & confirms onchain, the module auto-claims and the
    //    listener reports `.claimed`. Nothing else to do.
}
```

#### Android (Kotlin)
```kotlin
import com.synonym.bitkitcore.*

class SwapListener : BoltzEventListener {
    override fun onEvent(event: BoltzSwapEvent) {
        when (event) {
            is BoltzSwapEvent.StatusUpdate -> println("swap ${event.swapId}: ${event.status}")
            is BoltzSwapEvent.Claimed -> println("reverse swap ${event.swapId} claimed in ${event.txid}")
            is BoltzSwapEvent.Refunded -> println("swap ${event.swapId} refunded in ${event.txid}")
            is BoltzSwapEvent.Error -> println("swap ${event.swapId} error: ${event.message}")
        }
    }
}

suspend fun drainToOnchain(amountSat: ULong) {
    // mnemonic = wallet seed phrase; pass the wallet's BIP39 passphrase or null.
    // The last arg is Bitkit's fee rate (sat/vB) for auto-claims.
    boltzStartSwapUpdates(BoltzNetwork.MAINNET, SwapListener(), wallet.mnemonic, null, feeService.currentSatPerVb())

    val claimAddress = lightningService.node.onchainPayment().newAddress()
    val swap = boltzCreateReverseSwap(
        network = BoltzNetwork.MAINNET,
        electrumUrl = "ssl://electrum.blockstream.info:50002",
        amountSat = amountSat,
        claimAddress = claimAddress,
        mnemonic = wallet.mnemonic,
        bip39Passphrase = null,
    )
    lightningService.node.bolt11Payment().send(swap.invoice, null)
    // Auto-claimed on confirmation; listener emits Claimed.
}
```

#### Python
```python
from bitkitcore import (
    boltz_create_reverse_swap, boltz_start_swap_updates,
    BoltzNetwork, BoltzEventListener,
)

class SwapListener(BoltzEventListener):
    def on_event(self, event):
        print(event)

await boltz_start_swap_updates(BoltzNetwork.MAINNET, SwapListener(), wallet_mnemonic, None, 5.0)
swap = await boltz_create_reverse_swap(
    network=BoltzNetwork.MAINNET,
    electrum_url="ssl://electrum.blockstream.info:50002",
    amount_sat=50_000,
    claim_address=claim_address,   # from your onchain wallet
    mnemonic=wallet_mnemonic,
    bip39_passphrase=None,
)
# Pay swap.invoice over Lightning; the module claims onchain automatically.
```

### Submarine swap — onchain → Lightning

#### iOS (Swift)
```swift
func topUpLightning(amountSat: UInt64) async throws {
    // 1. Your LDK node issues the invoice Boltz will pay.
    let invoice = try lightningService.node.bolt11Payment()
        .receive(amountMsat: amountSat * 1000, description: "Boltz top-up", expirySecs: 3600)

    // 2. Create the swap; Boltz returns the lockup address & exact amount.
    let swap = try await boltzCreateSubmarineSwap(
        network: .mainnet,
        electrumUrl: "ssl://electrum.blockstream.info:50002",
        invoice: invoice,
        mnemonic: wallet.mnemonic,
        bip39Passphrase: nil
    )

    // 3. Fund the lockup from your onchain wallet.
    _ = try lightningService.node.onchainPayment()
        .sendToAddress(address: swap.address, amountSats: swap.expectedAmountSat)

    // 4. Boltz pays the invoice on confirmation (.invoicePaid → .transactionClaimed).
    //    If it fails, refund onchain (key re-derived from the mnemonic):
    //    try await boltzRefundSubmarineSwap(
    //        swapId: swap.id, refundAddress: addr,
    //        mnemonic: wallet.mnemonic, bip39Passphrase: nil, feeRateSatPerVb: nil)
}
```

#### Android (Kotlin)
```kotlin
suspend fun topUpLightning(amountSat: ULong) {
    val invoice = lightningService.node.bolt11Payment()
        .receive(amountSat * 1000u, "Boltz top-up", 3600u)

    val swap = boltzCreateSubmarineSwap(
        network = BoltzNetwork.MAINNET,
        electrumUrl = "ssl://electrum.blockstream.info:50002",
        invoice = invoice,
        mnemonic = wallet.mnemonic,
        bip39Passphrase = null,
    )
    lightningService.node.onchainPayment().sendToAddress(swap.address, swap.expectedAmountSat)
    // On failure: boltzRefundSubmarineSwap(swap.id, refundAddress, wallet.mnemonic, null, null)
}
```

## Recovery after restart

On startup (after `initDb`), resume tracking and surface anything actionable:

```kotlin
// re-subscribes all pending swaps; holds the mnemonic to auto-claim
boltzStartSwapUpdates(BoltzNetwork.MAINNET, SwapListener(), wallet.mnemonic, null, feeService.currentSatPerVb())
val pending = boltzListPendingSwaps()                       // for UI / manual refunds
```

`boltzListPendingSwaps` returns every non-terminal swap; combined with the wallet
seed (keys are re-derived from each swap's index), an interrupted reverse swap can
still be claimed and a failed submarine swap refunded. If `boltz.db` itself was
lost, recover via Boltz's rescue API using the seed (see *Keys, secrets &
recovery* above).

## Testing

```bash
cargo test modules::boltz                                   # unit tests (offline)
cargo test modules::boltz -- --ignored --nocapture          # live E2E (testnet)
BOLTZ_LIVE_NETWORK=mainnet cargo test modules::boltz -- --ignored --nocapture
```

The offline tests cover status mapping, DB round-trip/recovery, monotonic index
reservation, and deterministic key/preimage derivation (same seed+index →
same key; distinct indices/passphrases → distinct keys). The **claim/refund
broadcast paths are not yet covered by an automated test** — they require a
regtest Boltz + Electrum stack; the live test exercises swap creation and
cryptographically validates the locally-derived redeem script and invoice
against Boltz's response, but does not broadcast. A regtest end-to-end test that
actually claims and refunds is the recommended follow-up.

The live test creates a real reverse swap and asserts the locally-derived redeem
script and invoice match Boltz's response (the guarantee that a later claim is
valid). The swap is never paid and simply expires — no funds move. It skips
gracefully if the Boltz endpoint is temporarily unavailable.
