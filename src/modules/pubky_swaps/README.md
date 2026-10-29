# Pubky Swaps Module

Routes a Paykit payment through a [Pubky Swaps](https://github.com/coreyphillips/pubky-swap)
provider when no balance the wallet holds reaches an endpoint the contact published. The user
never picks the swap: the router does, and the review screen shows the route.

The first route is a reverse swap: the wallet pays over Lightning, the provider locks bitcoin on
chain, and this module claims it straight to the contact's on-chain address. The contact receives
exactly the amount asked for (`exact_out`): the lock covers the wallet's own claim fee.

## Flow

| Step | Paykit Capabilities | pubky-swap message | Owner |
|------|---------------------|--------------------|-------|
| Pick a route | router (§9.1) | `OfferRequest` → `Offer` | `pubky_swaps_route_paykit_payment` |
| Ask for a price | `rfq` → `quote` | `QuoteRequest` → `Quote` | same call, up to 3 providers |
| Commit | `commit` → `contract` | `SwapRequest` → `SwapAccept` | `pubky_swaps_commit_route` |
| Fund | funding is the acceptance | hold invoice | **App** pays the invoice from spending |
| Claim | evidence from the rail | claim with the preimage | `pubky_swaps_advance` |

`pubky_swaps_route_paykit_payment` returns `Direct` when a balance reaches one of the contact's
endpoints, so the app only swaps when it must.

Before the invoice reaches the app, the module checks the quote against the request
(`validate_quote`), the contract against the quote and the chain tip (`validate_accept`), rebuilds
the HTLC from its own keys (`verify_contract`), and binds the hold invoice to the quote and the
payment hash (`validate_hold_invoice`). These checks are pubky-swap's own `swap-common`.

## Transport

Requests travel over iroh `pubky-swap/direct/1`. The provider authenticates the request by the
connection's key, so neither side needs a homeserver. Paykit Capabilities §13.1 allows the same
payloads over this carrier.

## Keys and storage

- The wallet's identity towards providers, its claim keys and its preimages derive from the seed
  with HMAC-SHA512 under their own domain strings. They never collide with the Boltz module's keys,
  and the identity is not the user's Pubky identity.
- `<base>/pubky_swaps/<swap_id>.json` holds the contract and the key index, never a secret. The
  index is reserved on disk before the commit is sent.

## What it does not do yet

- **Discovery:** candidates come from the caller, usually the user's contacts. Each is asked for
  its live offer over iroh; nothing reads published Capability Descriptors yet (§6, §7).
- **Paykit transport:** no `paykit-cap.*` messages over Encrypted Links. paykit-rs has no API to
  send extension kinds yet (§17).
- **Signed terms:** pubky-swap signs nothing at `main`, so quotes and contracts are not
  attributable to the provider yet.
- **Submarine routes** (savings to a Lightning-only contact) and USDT. Pubky Swaps is Bitcoin and
  Lightning only.
- **Refunds and recovery:** a reverse swap needs no wallet refund (the provider cancels its hold
  invoice), but there is no scan to rebuild swaps from the seed alone, and swap records are not in
  the wallet backup.
