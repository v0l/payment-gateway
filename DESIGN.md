# payment-gateway

A self contained payment service. Apps ask it to collect money and it tells them when it is paid. It knows nothing about VMs, subscriptions or LLM credit.

First consumers: YALR, apexstrata.ie. Last: LNVPS, which replaces `lnvps_api/src/payments/` and `payment_factory.rs`.

## Scope

In:
- Tenants with API keys, each with their own provider configs, fees, webhook URL and enabled methods
- Payment creation, per-method payment details, status, expiry, cancel
- Settlement: LND invoice stream, on-chain watcher, Stripe and Revolut webhooks, FixedFloat swap status
- Signed outbound webhooks to tenants with retries
- Refunds and payouts back to the customer where the provider supports it
- Saved payment methods and off-session charges (Revolut, Stripe)
- Exchange rates and processing fees (moved from `lnvps_api_common/src/exchange.rs`)

Out (stays in the apps): subscriptions, invoice line items, proration, `time_value`, users, VM or credit provisioning.

## Stack

Rust 2024, axum, sqlx (MySQL, same as LNVPS), `payments-rs` for provider clients. Single binary plus a MySQL database. Provider secrets are encrypted at rest with a key from the environment.

## Model

```
tenant            id, name, api_key_hash, webhook_url, webhook_secret, enabled
provider_config   id, tenant_id, method, provider_type, name, enabled, config (encrypted json),
                  fee_rate, fee_base, fee_currency
payment           id (32 bytes), tenant_id, reference, currency, amount, tax, status,
                  created, expires, paid_at, metadata json, idempotency_key
payment_attempt   id, payment_id, provider_config_id, method, amount, currency, rate,
                  processing_fee, external_id, external_data (encrypted), status, expires
saved_method      id, tenant_id, customer_ref, provider_config_id, external_id
refund            id, payment_id, amount, status, destination, external_id
webhook_delivery  id, tenant_id, payment_id, event, body, attempts, next_attempt, delivered
```

`status`: `pending`, `paid`, `expired`, `cancelled`, `refunded`, `partially_refunded`.

A payment is the thing the app wants paid. An attempt is one way of paying it (a bolt11, an address, a checkout URL). A payment can have several attempts. The first attempt to settle in full marks the payment paid and the others are cancelled.

## API (tenant, `Authorization: Bearer <api key>`)

```
POST   /v1/payments                      create (idempotent on Idempotency-Key)
GET    /v1/payments/{id}
POST   /v1/payments/{id}/attempts        {method, saved_method_id?}  returns bolt11 / address / url
POST   /v1/payments/{id}/cancel
POST   /v1/payments/{id}/refund
GET    /v1/methods?currency=EUR          enabled methods with fees for the tenant
GET    /v1/rates
POST   /v1/saved-methods/{id}/charge     off-session charge
DELETE /v1/saved-methods/{id}
```

Create body:

```json
{
  "reference": "order-123",
  "amount": 1500,
  "currency": "EUR",
  "tax": 0,
  "expires_in": 3600,
  "metadata": {"user": "abc"},
  "methods": ["lightning", "stripe"]
}
```

Provider callbacks, unauthenticated, verified by signature:

```
POST /hook/{tenant}/{provider_config_id}      stripe, revolut
```

Admin API (separate admin token) for tenants, provider configs, delivery inspection and manual replays.

## Outbound webhooks

`POST tenant.webhook_url` with `X-Gateway-Signature: t=<unix>,v1=<hmac sha256 of "t.body">` and `X-Gateway-Event-Id`. Events: `payment.paid`, `payment.expired`, `payment.cancelled`, `refund.completed`, `attempt.failed`. Retries with exponential backoff for 3 days. Receivers must be idempotent on event id. Tenants can always poll `GET /v1/payments/{id}`.

## Settlement rules

- Amounts are integer minor units (msats for BTC), as in `payments-rs` `CurrencyAmount`.
- Overpay is accepted and recorded. Underpay on-chain stays pending until expiry, then is flagged for a manual decision.
- Lightning settlement is driven by the LND invoice stream with a resume cursor, on-chain by the existing `PaymentCursor` logic. Both must survive restarts without missing or double counting.
- Settlement is a single DB transaction: attempt paid, payment paid, webhook row inserted. Delivery happens after commit from the webhook table.
- Altcoins go through FixedFloat into the tenant's on-chain wallet. The payment is paid when the swap lands, not when the customer sends.

## Milestones

1. Skeleton: workspace, config, migrations, tenants and API keys, admin API, health, CI
2. Core payment model, create and get, expiry worker, outbound webhook delivery
3. Lightning (LND): attempts, settlement stream, tests against a mock `LightningNode`
4. Stripe: checkout attempts, webhook verification, settlement
5. YALR integration: YALR credit top up using lightning and Stripe, behind a small client crate
6. On-chain (LND): address per attempt, watcher, confirmations
7. Revolut: orders, webhooks, saved methods, off-session charge
8. FixedFloat altcoins, exchange rates, processing fees
9. Refunds and payouts
10. apexstrata.ie integration
11. LNVPS migration: gateway client behind a trait, new payments via gateway, old `subscription_payment` rows still read, then remove `payments/` and `payment_factory.rs`, `payment_method_config` data imported into `provider_config`
12. Deploy on the k8s cluster, metrics, backups

## Testing

Mock `LightningNode`, `OnChainProvider` and `FiatPaymentService` (`payments-rs` has `mock` for on-chain) drive integration tests over a real MySQL. Every settlement path gets a test for restart mid-settle, duplicate event and out of order event. Provider sandboxes (Stripe test mode, Revolut sandbox) for manual end to end.
