# USDT on Arbitrum One

`UsdtWallet` owns seed-derived accounts, fee quotes, signing, durable payment recovery and activity. Amounts use millionths of USDT. The native apps own credentials, payment authentication and presentation.

## Account and authorization

The address derives from the Bitkit BIP39 mnemonic and passphrase at `m/44'/60'/0'/0/0`. Arbitrum One (42161), USDT0, EntryPoint 0.8 (`0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`) and Simple7702Account (`0xe6Cae83BdE06E4c305530e199D7217f42808555B`) are pinned in code.

Each payment signs a chain-specific EIP-7702 authorization at the current account nonce and an EntryPoint operation committing to that delegate. Authorization costs are included in the USDT fee. There is no separate smart-wallet address or factory deployment. Another wallet with the same derivation can access the address; gas-payment support may differ.

Delegation can persist even when an operation fails. A foreign delegate blocks sends without blocking balance/history reads. Bitkit never silently replaces it.

Owned mnemonic/passphrase/seed buffers are zeroized and signing keys are erased on drop on a best-effort basis. FFI, compiler and library-internal copies cannot be guaranteed erased. SQLite contains no seed or private key.

## Quotes and fees

`quote_transfer` takes a raw recipient, positive atomic amount and destination; it never receives signing credentials. Local quotes last at most 120 seconds and newly quoted paymaster terms must expire within 15 minutes. `send` validates the owner, nonces, balance, gas estimates, the current slow gas-price recommendation and deadlines before signing the stored plan. Quotes use the fast gas-price recommendation; a modest price increase does not invalidate a quote that still covers the current slow recommendation. Changes beyond the approved bounds require a new review; signing cannot raise the approved fee.

The pinned ERC-20 paymaster collects USDT. Its finite approval includes a 5% margin; the displayed maximum fee comes from signed gas limits and paymaster terms, not the allowance. Call/pre-verification estimates receive 10% execution/L1-data headroom; the charged pre-verification margin is included in the maximum. A residual paymaster allowance can remain and is reset to a finite amount on the next payment.

`usdt_parse_payment_request` accepts raw addresses and chain-qualified ERC-681 requests for the pinned token, with exact atomic/scientific amounts. Ambiguous or unsupported parameters are rejected. The returned `chain_id` preserves explicit network restrictions; bare addresses leave it unset. Callers must honor it and review the parsed amount before requesting a quote.

## Persistence and recovery

Signed operations persist atomically before submission. Lost or rejected submission responses do not prove nonexecution: recovery retries only the identical signed operation. A quote ID cannot authorize a second payment. One source-chain payment remains pending at a time.

A matching event in a canonical receipt settles the payment. Discovery logs alone never decide the outcome; unavailable log queries allow independent nonce/receipt proofs to proceed, while rate limits retain backoff. Expired signed paymaster terms and a confirmed EntryPoint nonce that has not passed the signed nonce release an unmined operation; the shorter quote deadline does not. With an advanced nonce and missing indexed events, recovery checks every receipt in the consuming block. A matching event settles/replaces the payment; complete absence proves external nonce consumption. Missing receipts preserve the pending operation. Progress is stored by payment and block hash so interruption does not restart the proof or carry it onto another block.

Seed restoration recovers deposits and outgoing activity from genesis, including transfers before delegation and sends through another wallet. Supported direct EntryPoint calls and paymaster modes recover payment/fee attribution; unknown wrappers or payment modes preserve raw token transfers instead of guessing their intent. Failed payments retain attempted amounts but have no delivered amount. Transaction hashes are absent until execution is observed; callers derive explorer links from the source transaction hash rather than storing a second copy of it.

`sync_history` returns `true` when caught up and `false` when more work remains. It uses adaptive log ranges and a 20-second soft budget between persisted receipts; an in-flight receipt may finish later. Learned range limits survive budget exits. A single-block log overflow falls back to that block's individual receipts. Complete receipt enrichment and fallback scans are retained by canonical block hash within the revisit window; log-only incoming observations and successful bridge receipts missing tracking or fee evidence remain eligible for later enrichment. Incomplete bridge metadata does not keep an executed source payment pending. Zero/self transfers are discarded before enrichment. Network failures preserve completed work and never silently skip a block. Callers should defer catch-up during payment review/submission so historical work does not delay sends.

`check_recent_execution` checks one recent direct Arbitrum payment with a five-second request budget. It requires the expected operation outcome and token transfer in a matching canonical receipt and does not scan history, rebroadcast, expire payments or reconcile nonces. It can confirm execution at the current L2 tip; this is provisional sequencer execution, not parent-chain finality. Native send screens may call it approximately once per second during a short foreground window, with cancellation and rate-limit backoff between checks. Missing evidence leaves Pending intact. Normal recovery handles older payments outside its 64-block lookup window.

Scans trail the reported tip by two blocks and revisit 4096 blocks for delayed indexing. This is not reorg rollback: previously recorded orphaned activity is not retracted. Providers must supply complete filtered logs, canonical blocks/receipts and historical state. The first scan starts at block zero, including pre-Nitro ranges; providers must answer those queries or return a supported range-limit error so the scan can narrow them. Logs first indexed more than 4096 blocks late can fall outside the revisit window. This is a block-count limit, not a guaranteed time interval or a measured provider indexing guarantee.

Payment outcomes and expiry decisions trust the configured chain RPC. A malicious RPC can fabricate or suppress evidence and mislead a user into authorizing another payment; these checks are not light-client proofs.

Storage is wallet-specific and must have one owning `UsdtWallet` object. Drop it before deleting its database during an explicit wallet wipe. Async exports use UniFFI's Tokio adapter; Kotlin cancellation can drop the polled future, whereas the current Swift bindings may finish an in-flight call after task cancellation. Callers must check cancellation between calls. Sends are not detached onto the global runtime used by stateless exports.

## Transport and cross-network APIs

Both chain and bundler endpoints must be controlled, credential-free HTTPS URLs; HTTP is accepted only on loopback for fixtures. Provider keys belong on the server. Chain/bundler calls share an 80/minute budget with a burst of 20. Responses are bounded to 2 MiB, except protocol-projected receipts up to 16 MiB. The companion service documents provider requirements, receipt projection and deployment limits.

`UsdtDepositClient` signs Orchestra deposit registration, history, detail and explicit refund requests for the derived account. It uses a separate optional service endpoint; estimates do not imply delivery. A clock-skew error requires correcting the device clock. Amount-limit errors carry the provider’s known USD limits so callers can explain rejected amounts. Source-network fees are paid by the sender. Partner provisioning, delivered deposits and refund acceptance are separate release checks.

The outbound bridge API supports Ethereum (30101), Polygon (30109), Plasma (30383) and Stable (30396), alongside direct Arbitrum transfers. Native release flows expose Arbitrum only; bridge routes require explicit service enablement and destination acceptance. Plain deposits on another chain are not automatically forwarded. Recipient validation rejects the destination token and the pinned EntryPoint, paymaster and Simple7702 delegate addresses on every destination. Direct Arbitrum sends also reject the source OFT and helper.

Bridge quotes include 10% native messaging-fee headroom and 20% token-conversion headroom, both within the displayed maximum USDT fee. Before signing or rebroadcasting, the stored native fee, helper liquidity and token approval are checked against current requirements without raising approved limits. The service reports OFT/helper execution reverts as sanitized RPC code `3`, which core maps to `UnsupportedRoute` during initial quoting. A reverted fee recheck for an already reviewed bridge quote requires a fresh quote (`QuoteExpired`); insufficient helper liquidity remains `UnsupportedRoute`. Provider outages remain retryable network errors. Delivery checks process up to three transfers concurrently outside the send lock, with a ten-second request budget, even when source recovery fails; failed lookups retain the last known status, while an explicit `INFLIGHT` or `CONFIRMING` update clears a previous needs-attention state.

Bridges use the pinned OFT and TransactionValueHelper with zero account ETH, a finite USDT approval covering principal/fee, and atomic helper-allowance revocation. The deployed helper requires native liquidity and retains behaviors noted in its OpenZeppelin audit; its verified runtime is not the audit-remediated implementation. Source success means bridging, not delivered.

`Pending` means source execution is unresolved; `Failed` means the source payment failed or was proved unexecuted; `Replaced` means another operation consumed its nonce. `Bridging` means source execution succeeded and destination delivery is unresolved. For bridges, `Confirmed` means delivery was reported. `BridgeNeedsAttention` covers retryable delivery problems or missing message evidence; without a GUID, no delivery lookup is possible. `BridgeFailed` means LayerZero reports a burned or skipped message: delivery polling stops, while source transaction, GUID, amount and fees remain visible. Neither bridge status implies a refund. Terminal delivery states survive restart and source-history rescans for the same transaction and GUID.

LayerZero status must match the operation GUID/pathway before confirmation; blocked delivery remains visible and never triggers an automatic paid retry.

RPC providers see queried addresses. Delivery checks use `bitkit_getBridgeMessages([sourceTransactionHash])` on the existing chain-service endpoint. The service queries LayerZero Scan without forwarding device headers, projects only message identity/pathway/status fields, and applies its shared request and response limits. LayerZero sees the service IP and the transaction hash; the service still sees the requesting device. Manually opening LayerZero Scan from transaction details connects the browser directly. No delivery requests are made for Arbitrum-only transfers.

Unavailable, unmatched or unknown delivery responses preserve the last status. Those lookups and retryable problems (`FAILED`, `BLOCKED`, `PAYLOAD_STORED`) wait at least one minute before another automatic attempt in the same wallet session. `APPLICATION_BURNED` and `APPLICATION_SKIPPED` stop polling as `BridgeFailed`; `DELIVERED` stops polling as `Confirmed`. No status triggers an automatic paid retry or refund.

## Incoming deposit instructions

Reusable deposit addresses use an immutable 50-bps slippage policy shared by the service and client. The receive response must confirm those exact terms; accepting a lower number alone would not prove that an existing standing instruction changed. Altering this policy requires coordinating new instructions and client/service behavior. Minimum and maximum USD-cent limits are optional display metadata; malformed limits are omitted without hiding a usable address or the amount-limit error.

Detail and refund calls take the list offset at which the owned deposit was found. The service checks that page and the following page; if new deposits move it further, callers refresh the list and retry with the updated offset. Unknown provider statuses/networks remain visible for support rather than being guessed as successful or refundable.

## Validation and bindings

Run `cargo test --locked --lib modules::usdt`; CI runs these deterministic tests. They cover independent signing/address vectors, fee bounds, uncertain submission, nonce recovery and restored history. Fixtures use public test credentials.

For the ignored deployed-contract test, start a fresh Arbitrum Anvil fork on port 18545 and `tests/usdt-fork/provider.mjs` on 18546 after installing its pinned dependencies. Run `cargo test deployed_contracts_collect_usdt_fees_and_revert_failed_bridges_atomically -- --ignored`. The fixture requires Anvil, sets local balances/signing terms and executes deployed contracts; it does not establish real provider pricing or destination delivery.

To include the service, start it with `USDT_BRIDGE_NETWORKS=ethereum,polygon,plasma,stable NODE_ENV=test ARBITRUM_RPC_URL=http://127.0.0.1:18545 LOCAL_PROVIDER_URL=http://127.0.0.1:18546`, then pass `USDT_FORK_RPC_URL=http://127.0.0.1:3100/v1/usdt/chain-rpc` and `USDT_FORK_BUNDLER_URL=http://127.0.0.1:3100/v1/usdt/rpc` to the ignored test.

Build iOS and Android sequentially with the repository scripts; Android temporarily edits the manifest/example. Generated bindings and native artifacts must use the same source. App configuration and local package overrides belong in each native repository's USDT documentation.

## References

- [EIP-7702](https://eips.ethereum.org/EIPS/eip-7702)
- [Alto EIP-7702 request validation](https://github.com/pimlicolabs/alto/blob/96529592b67a69be23c013359cbc9990657af64a/src/rpc/rpcHandler.ts)
- [Simple7702Account](https://github.com/eth-infinitism/account-abstraction/blob/releases/v0.8/contracts/accounts/Simple7702Account.sol)
- [Pimlico supported tokens](https://docs.pimlico.io/references/paymaster/erc20-paymaster/supported-tokens)
- [Pimlico paymaster deployments](https://docs.pimlico.io/references/paymaster/erc20-paymaster/contract-addresses)
- [Pimlico pricing](https://www.pimlico.io/pricing)
- [Pimlico public endpoint limits](https://docs.pimlico.io/references/bundler/public-endpoint)
- [USDT0 documentation](https://docs.usdt0.to/)
- [Transaction helper audit](https://www.openzeppelin.com/news/usdt0-transaction-helper-audit)
- [Verified deployed helper](https://arbitrum.blockscout.com/api/v2/smart-contracts/0xa90f03c856d01f698e7071b393387cd75a8a319a)
- [LayerZero message statuses](https://docs.layerzero.network/v2/tools/layerzeroscan/mainnet/messages/get-messagesstatus)

Destination token addresses follow the official USDT0 ecosystem listings for [Polygon](https://usdt0.to/ecosystem/polygon), [Plasma](https://usdt0.to/ecosystem/plasma) and [Stable](https://usdt0.to/ecosystem/stable).
