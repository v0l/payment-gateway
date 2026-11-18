# Concepts

## Tenant and project

A tenant is one customer of the gateway with its own API keys, providers, webhook endpoints, branding and limits. In the console a tenant is called a project, and one account can own many projects. Nothing is shared between tenants: every query is scoped to the tenant and one tenant cannot read or change another's payments.

New projects start in test mode: low limits, and webhooks only go to public https hosts. An operator lifts the limits when the project is trusted. Test mode keys start with `pgw_test_` and trusted project keys with `pgw_live_`.

## Payment

A payment is the thing your app wants paid: a reference, an amount in minor units (cents, or msats for BTC), a currency and an expiry. Amounts are always integers. Statuses are `pending`, `paid`, `expired`, `cancelled`, `refunded` and `partially_refunded`.

A payment id is 32 random bytes in hex. The hosted pay page uses it as the capability for its public endpoints, so treat the `pay_url` like a link you only give to the payer.

## Attempt

An attempt is one way of paying a payment: a Lightning invoice, an on-chain address, a card checkout, a bank or PayPal checkout. A payment can have several attempts, for example when the customer changes their mind. The first attempt that settles in full marks the payment paid and the others are cancelled.

The attempt response carries `details`: `bolt11` for Lightning, `address`, `amount_sats` and `uri` for on-chain, `checkout_url` for Stripe, Revolut and PayPal.

## Methods

A method is a kind of payment the project can accept: `lightning`, `onchain`, `stripe`, `revolut`, `paypal` and altcoins through `fixedfloat`. A method is enabled when the project has a provider configured for it. List what is available, with fees, using `GET /v1/methods?currency=EUR`.

## Settlement

Settlement is exactly once. When a provider reports the money, the gateway locks the payment row, checks its state, marks the attempt and payment paid and writes the webhook row in one database transaction. Delivery happens after commit. Duplicate and out-of-order provider events are safe.

Overpayment is accepted and recorded. On-chain underpayment stays pending until expiry and is then flagged for a manual decision. A payment that arrives after expiry produces a `payment.late_payment` event.

## Scopes

API keys carry scopes: `payments:write`, `payments:read`, `refunds:write`, `saved:write` and `admin`. A call without the needed scope returns 403 `insufficient_scope`.
