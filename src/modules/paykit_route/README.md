# Paykit Route Module

Picks how the wallet pays a Paykit contact, from the contact's published Payment Endpoints and what
the wallet can spend. The user never picks the route: `paykit_route_payment` does, and the review
screen shows it.

## Rules

1. **Direct.** When a balance the wallet holds reaches an endpoint the contact published, the
   wallet pays it directly: over Lightning first (`btc-lightning-bolt11`, then
   `btc-lightning-lnurl`) when `lightning_spendable_sat` covers the amount, else on chain
   (`btc-{network}-p2tr`, `p2wpkh`, `p2sh`, `p2pkh`, in that order) when `onchain_spendable_sat`
   does. Both amounts are net of the fees the payment costs. Spark's balance counts as
   Lightning-spendable, so paying a Lightning endpoint from Spark needs no leg. A BOLT11 invoice
   counts only when it is for the wallet's network, unexpired and amountless or for exactly the
   amount; an LNURL only needs a value, since checking it takes a network call.
2. **Service leg.** Otherwise the router keeps the cheapest of `service_legs` that delivers exactly
   the amount to an on-chain endpoint for the wallet's network the contact published and whose quote
   has not expired. A leg is a
   one-step route through a service the wallet holds a balance on, such as a Spark withdrawal to
   the contact's on-chain address. The app quotes each leg with that service's engine and runs the
   one the router returns; the router never talks to the service. `atomicity` states who the leg
   trusts (`trusted` for Spark's operators).
3. **None.** Nothing reaches the contact. `reason` is English detail for logs; the app shows its
   own localized text.

The rail segment of an on-chain Payment Endpoint Identifier names the network the apps publish for
(`bitcoin`, `testnet`, `signet`, `regtest`), so a contact's endpoint for another network is never
paid. The spec has no Testnet4 rail and BOLT11 has no Testnet4 currency, so a Testnet4 wallet
routes nothing: no Lightning endpoint, on-chain endpoint or service leg.
