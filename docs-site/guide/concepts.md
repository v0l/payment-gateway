# Concepts

## Tenant and project

A tenant is one customer of the gateway with its own API keys, providers, webhook endpoints, branding and limits. In the console a tenant is called a project, and one account can own many projects. Nothing is shared between tenants: every query is scoped to the tenant and one tenant cannot read or change another's payments.

New projects start in test mode: low limits, and webhooks only go to public https hosts. An operator lifts the limits when the project is trusted. Test mode keys start with `pgw_test_` and trusted project keys with `pgw_live_`.

## Payment

A payment is the thing your app wants paid: a reference, an amount in minor units (cents, or msats for BTC), a currency and an expiry. Amounts are always integers. Statuses are `pending`, `paid`, `expired`, `cancelled`, `refunded` and `partially_refunded`.

A payment id is 32 random bytes in hex. The hosted pay page uses it as the capability for its public endpoints, so treat the `pay_url` like a link you only give to the payer.

## Attempt

An attempt is one way of paying a payment: a Lightning invoice, an on-chain address, a card checkout, a bank or PayPal checkout. A payment can have several attempts, for example when the customer changes their mind. The first attempt that settles in full marks the payment paid and the others are cancelled.

The attempt response carries `details`, and `details` is the whole contract for an integrator that only uses the API. The hosted pay page is optional: you never need to send the customer to it.

| Method | `details` fields | What your client does |
| --- | --- | --- |
| Lightning | `bolt11` | Show the invoice as text and a QR code |
| On-chain | `address`, `amount_sats`, `uri` | Show the address and amount, or the BIP21 `uri` as a QR code |
| Stripe | `session_id`, `checkout_url` | Redirect to `checkout_url`. Stripe sends the customer back to the `success_url` and `cancel_url` you passed when creating the attempt, with no gateway page in between |
| Revolut | `widget` (`provider`, `token`, `mode`, `email`), `checkout_url` | Open Revolut's embedded card widget in your own page with `widget.token` and `widget.mode` (`prod` or `sandbox`). `checkout_url` is Revolut's hosted page, for clients that prefer a redirect. The widget is present on every card attempt, with or without `save_method` |
| PayPal | `checkout_url` | Redirect to `checkout_url` |

Fields are only ever added, never renamed or removed. Settlement still arrives by webhook or by reading the payment.

## Methods

A method is a kind of payment the project can accept: `lightning`, `onchain`, `stripe`, `revolut`, `paypal` and altcoins through `fixedfloat`. A method is enabled when the project has a provider configured for it. List what is available, with fees, using `GET /v1/methods?currency=EUR`.

## Settlement

Settlement is exactly once. When a provider reports the money, the gateway locks the payment row, checks its state, marks the attempt and payment paid and writes the webhook row in one database transaction. Delivery happens after commit. Duplicate and out-of-order provider events are safe.

Overpayment is accepted and recorded. On-chain underpayment stays pending until expiry and is then flagged for a manual decision. A payment that arrives after expiry produces a `payment.late_payment` event.

## Receipts

A paid payment has a public receipt at `/receipt/{payment_id}`, linked from the payment's `receipt_url`. See the [hosted quickstart](/guide/quickstart-hosted).

## Scopes

API keys carry scopes: `payments:write`, `payments:read`, `refunds:write`, `saved:write`, `withdrawals:read`, `withdrawals:write` and `admin`. A call without the needed scope returns 403 `insufficient_scope`.
