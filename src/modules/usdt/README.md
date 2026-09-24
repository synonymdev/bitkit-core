# USDT on Arbitrum One

Bitkit keeps a separate USDT balance, receive address, authenticated payment flow and activity list. Rust owns account derivation, quotes, signing, persistence and chain access. The native apps retain their existing keychain, authentication, lifecycle and UI components.

## Account and recovery

The receive address is the Ethereum address derived from the existing Bitkit BIP39 mnemonic and passphrase at `m/44'/60'/0'/0/0`. EIP-7702 delegates that address to the pinned Simple7702Account implementation (`0xe6Cae83BdE06E4c305530e199D7217f42808555B`) using EntryPoint 0.8 (`0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`).

Each authenticated payment signs a chain-specific authorization for the pinned delegate at the current account nonce. This keeps the operation hash bound to that delegate and satisfies the bundler’s `0x7702` factory-marker policy, including after activation. Authorization gas is included in the quoted USDT maximum. Quotes use dummy authorization; they never receive a signing key. There is no factory deployment or separate smart-wallet address.

Restoring the same seed and passphrase reconstructs the same address and reads its balance and activity from Arbitrum. Another wallet using the same derivation can access that address, although its gas-payment and delegation support may differ.

Delegation persists onchain and can be applied even if the operation fails. Retries retain both signatures and require the authorization nonce to remain current. If authorization was consumed without a known payment result, the signed payment stays pending until an event identifies its outcome or its paymaster terms expire with the EntryPoint nonce unchanged. A different delegate blocks Bitkit sends; balances and incoming/outgoing token history remain readable. Bitkit does not silently replace another wallet's delegation.

The native keychain is the source of signing credentials. Rust zeroizes its owned mnemonic/passphrase/seed buffers and best-effort erases signing keys; FFI and library-internal copies cannot be guaranteed erased; SQLite stores no seed or private key.

Signed operations are stored before transmission. A lost response only retries the identical operation, and a quote ID cannot authorize a second payment.

Expiry of the signed paymaster terms with an unchanged on-chain nonce resolves an unmined submission. The shorter local quote deadline cannot release a signed payment. Network timestamps govern these checks. A higher nonce remains pending until an event identifies this payment or its replacement.

New attempts require another quote and native authorization. One source-chain payment can be pending at a time; confirmed bridge messages can continue delivery while another payment is made.

`balance()` returns USDT in millionths. `sync_history()` returns `true` when caught up and `false` when its 20-second work budget expires; request failures and invalid data return errors. Callers retain recovered activity and continue incomplete scans on a later refresh. Local activity has no fixed entry cap.

History scans bounded ranges from genesis, including deposits before delegation. Each completed page and partial-page receipt survives interruption; a completed scan revisits 4096 blocks for delayed indexing. Scans trail the reported tip by two blocks; this lag and revisit do not provide reorg rollback or retract previously saved orphaned activity.

Incoming and outgoing transfers decode from token logs, including transfers made with another wallet. A combined incoming/EntryPoint filter and an outgoing filter require two log queries per range. Known timestamps are reused.

Locally recorded payments are matched by their operation hash. For other payments, supported account calls recover payment and fee details only from transactions sent directly to the pinned EntryPoint. Their raw outgoing logs are excluded to avoid duplicates. Unknown wrappers and call shapes retain their token transfers without inventing payment or fee attribution.

Range and response-size limits reduce the query span; rate limits return a distinct error. Pending-payment recovery can fall back to historical nonce reads and a single-block event query when its full log range is rejected. The RPC provider must support those historical state reads as well as full-history filtered logs.

## Fees and infrastructure

Only the exact Arbitrum USDT0 token is supported: `0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9`, with six decimals. Amounts and fees are integers; no floating point or ETH balance is exposed to the user.

Pimlico's ERC20 paymaster is the initial provider. Its official supported-token list includes this token. The pinned EntryPoint 0.8 paymaster is `0x888888888888Ec68A58AB8094Cc1AD20Ba3D2402`.

A payment atomically batches a limited token approval and execution at the delegated address. Gas, token conversion, fixed charges and expiry are checked before the user approves the maximum fee. Final provider data, calldata and limits are signed together using the EntryPoint 0.8 EIP-712 hash, including the delegate.

Local quotes last at most 120 seconds; newly quoted paymaster terms must have a finite deadline within 15 minutes. Recovery of a signed operation always follows its actual signed deadline.

The approval has a 5% margin, while the displayed maximum fee uses the signed gas limits and paymaster terms, not the allowance. Call gas and pre-verification gas receive a 10% margin for execution and Arbitrum L1 data-cost variation; provider verification estimates are retained. The pre-verification margin is charged as part of the payment fee and included in the displayed maximum. Changed or expired terms require another review.

A residual allowance to the pinned paymaster can remain after collection; each subsequent payment sets a new finite allowance. The helper allowance is revoked in the bridge batch.

Mainnet paymaster access requires a Pimlico project. Keep provider credentials on the server and validate the configured provider's pricing and policy before release.

Configure `USDT_BUNDLER_URL` with a controlled HTTPS endpoint forwarding the provider's bundler/paymaster JSON-RPC methods. Do not distribute a private provider key in the apps. A production proxy must retain the key, restrict chain/token/methods and apply abuse controls. Its provisioning belongs to the backend deployment, not to the mobile keychain.

The apps intentionally show USDT as unavailable when this endpoint is missing. Both endpoints are required by the core constructor; there is no implicit public RPC.

`USDT_RPC_URL` must point to the service's credential-free `/v1/usdt/chain-rpc` endpoint in both mobile apps. The private upstream URL is configured on the service as `ARBITRUM_RPC_URL`; it must never enter a mobile build. Rust allows plain HTTP only on localhost for fixtures.

Chain and bundler calls share a budget of 80 requests per minute with an initial burst of 20. Native wallets poll every 30 seconds while idle and every ten seconds while receiving or awaiting a pending payment, with a shared ten-second minimum and one-minute backoff after rate limits. History scanning continues after a settlement error, but cancellation and throttling stop the refresh.

The standalone `bitkit-usdt-service` implements this endpoint at `/v1/usdt/rpc`. It runs independently of Blocktank, validates the pinned EIP-7702/USDT request shapes, and applies per-IP/global request and concurrency limits. Configure the apps with its full HTTPS route.

It adds no wallet login or custody; the endpoint is public with constrained operations, not proof of official-app identity. Its README documents provider credentials, reverse-proxy configuration, limits and local testing. Provisioning and live-provider acceptance remain deployment prerequisites.

- iOS reads these names from build settings/Info.plist or process configuration for local testing.
- Android reads these names through its existing local-properties/environment build configuration.
- Arbitrum One (42161) is the only supported USDT chain, including development builds. The UI always labels it. Bitcoin's network selection does not imply USDT testnet support. Storage remains under the existing Bitcoin-network/wallet directory, with a USDT-specific database and identity check.

## Deposits and withdrawals

Receive shares an ERC-681 token-transfer request identifying Arbitrum One and USDT0, and allows copying the raw account address. The sender must select Arbitrum One or bridge into it.

`usdt_parse_payment_request()` accepts the pinned token-transfer URI with an optional `uint256` amount, including exact scientific notation in atomic units. Native send screens prefill that amount for editing and review. Duplicate or unsupported parameters are rejected. `quote_transfer()` takes a raw recipient address and an explicit amount after payment-request parsing and user review; it rejects URIs rather than discarding their requested amounts.

Plain transfers to that address on Ethereum, TRON or another chain are not automatically forwarded.

The optional Orchestra receive flow instead provides a separate reusable deposit address for each enabled source network. Ethereum and TRON USDT are supported by the adapter; the backend advertises only explicitly enabled, provider-supported routes into the pinned Arbitrum USDT0 account. The sender pays the source network fee, while routing costs are deducted from the deposited USDT. There is no Rhino or Relay integration.

Configure `USDT_DEPOSITS_URL` with the service’s HTTPS `/v1/usdt/deposits` route and keep `ORCHESTRA_API_KEY` on the server. Wallet-signed requests bind deposit registration, status and explicit refund requests to the seed-derived address. Estimates are indicative; held deposits and refunds remain visible without implying settlement.

A partner account, route acceptance, funded delivery and refund validation are release prerequisites. Without the endpoint, direct Arbitrum receive remains available.

The native release exposes Arbitrum only. Bridge routes stay disabled at the service until destination delivery has passed funded acceptance. Same-chain payments transfer USDT directly.

Cross-chain payments use the deployed USDT0 TransactionValueHelper, `0xa90f03c856D01F698E7071B393387cd75a8a319A`, and pinned Arbitrum OFT, `0x14E4A1B13bf7F943c8ff7C51fb60FA964A298D92`. The helper supplies native messaging value and collects USDT. Calls carry zero ETH from the account. The bridge approval covers principal plus a bounded token fee and is revoked atomically after sending.

OFT amount/peer checks and helper balance/maxGas checks reject unavailable routes. The review screen shows the exact expected receipt and maximum additional fees.

Supported destinations are Arbitrum One, Ethereum (30101), Polygon (30109), Plasma (30383), and Stable (30396). Arbitrum's LayerZero ID is 30110. TRON, Solana, TON and other deployments are not inferred from USDT0 branding; they are not offered.

The helper is an operational dependency: its operator must maintain native liquidity and its oracle/markup affect pricing. Its verified deployed runtime (`0x9d4c3b4b79a3d31843ec75a03ce6ef43735421fdee489714519afc052f60ef27`) retains behaviors discussed in the OpenZeppelin audit; it is not the audit-remediated implementation.

Bitkit constrains calls to zero native value, a pinned OFT, exact amounts, and finite approvals. Failed execution can still incur the authorized paymaster gas fee.

Bridge-status polling shares a five-second refresh budget and rotates the first unresolved bridge checked, leaving source-chain recovery and sends available when delivery status is unavailable.

A successful source receipt means the bridge has started. Bitkit matches the operation's GUID and LayerZero pathway before marking delivery confirmed. Failed, blocked or `PAYLOAD_STORED` delivery remains visible as requiring attention, with a tracking link. It does not silently retry a destination transaction or charge an additional recovery fee.

The RPC sees queried wallet addresses, and LayerZero Scan sees bridge transaction hashes. Neither service holds the signing key.

## Bindings and local builds

Build core with the repository's `build_ios.sh` and `build_android.sh`, sequentially: the Android script temporarily changes the manifest and example source. Generated Swift/Kotlin bindings and native binaries must come from the same core source.

For local iOS builds, `scripts/build-usdt-local.sh` in bitkit-ios takes the core directory followed by normal xcodebuild arguments and sets the `BITKIT_CORE_LOCAL` package override. Android accepts the absolute generated release AAR through the `bitkitCoreAar` Gradle property.

## Validation

The focused Rust suite covers independent address/signature parity, exact decimal amounts, chain-qualified QR requests, bounded paymaster and bridge fees, own-operation receipt attribution, uncertain submission, nonce/expiry recovery, and seed-restored history. The reference vector uses the public test mnemonic and ethers 6.17.0; it contains no production credential.

The ignored deployed-contract test uses a fresh local Anvil fork of Arbitrum One on port 18545. Install the pinned test-only dependency in `tests/usdt-fork` and start its `provider.mjs` on port 18546, then run the ignored `deployed_contracts_collect_usdt_fees_and_revert_failed_bridges_atomically` Rust test.

The fixture first requires an Anvil client, sets only local token balances and a local paymaster test signer, and uses the deployed contract bytecode. It exercises EIP-7702 activation, USDT post-operation collection, helper send/revoke, and helper failure.

Its fixed estimates and test signatures do not validate Pimlico's real API policy, pricing or production credentials. Restart both fixture processes with a fresh fork for another run.

To include the provider service in that same test, start it with `NODE_ENV=test ARBITRUM_RPC_URL=http://127.0.0.1:18545 LOCAL_PROVIDER_URL=http://127.0.0.1:18546`, then set `USDT_FORK_RPC_URL=http://127.0.0.1:3100/v1/usdt/chain-rpc` and `USDT_FORK_BUNDLER_URL=http://127.0.0.1:3100/v1/usdt/rpc` when running the ignored test. These overrides affect only the fork test; normal wallet configuration uses `USDT_RPC_URL` and `USDT_BUNDLER_URL`.

## References

- [EIP-7702](https://eips.ethereum.org/EIPS/eip-7702)
- [Alto EIP-7702 request validation](https://github.com/pimlicolabs/alto/blob/96529592b67a69be23c013359cbc9990657af64a/src/rpc/rpcHandler.ts)
- [Simple7702Account](https://github.com/eth-infinitism/account-abstraction/blob/releases/v0.8/contracts/accounts/Simple7702Account.sol)
- [Pimlico supported tokens](https://docs.pimlico.io/references/paymaster/erc20-paymaster/supported-tokens)
- [Pimlico pricing](https://www.pimlico.io/pricing)
- [Pimlico public endpoint limits](https://docs.pimlico.io/references/bundler/public-endpoint)
- [USDT0 documentation](https://docs.usdt0.to/)
- [Transaction helper audit](https://www.openzeppelin.com/news/usdt0-transaction-helper-audit)
- [Verified deployed helper](https://arbitrum.blockscout.com/api/v2/smart-contracts/0xa90f03c856d01f698e7071b393387cd75a8a319a)
- [LayerZero message statuses](https://docs.layerzero.network/v2/tools/layerzeroscan/mainnet/messages/get-messagesstatus)
