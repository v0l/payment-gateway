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

## Integration modes

All three sit on the same payment object, so a tenant can mix them.

1. **Pure API**: the app creates the payment, creates attempts, renders its own UI, and learns the result from the webhook or by polling.
2. **Hosted page (redirect)**: create with `success_url` and `cancel_url`. The response includes `pay_url` (`https://<gateway>/pay/{id}`). The gateway page shows the amount, a method picker, the bolt11 QR or address or a handoff to Stripe/Revolut checkout, live status, and then redirects to `success_url` with `payment_id` and a signed `sig`. The app must still confirm via webhook or `GET`, never trust the redirect alone.
3. **Embed (iframe)**: the same page at `/embed/{id}`, with CSP `frame-ancestors` limited to the tenant's `allowed_origins`. It posts `pgw:ready`, `pgw:resize`, `pgw:paid`, `pgw:expired` and `pgw:cancelled` to the parent. A small `gateway.js` opens the frame and exposes `onPaid`. Stripe and Revolut hosted checkouts refuse framing, so for fiat the embed redirects the top window (or uses Stripe Elements later).

The payment id is 32 random bytes and acts as the capability for the public endpoints (`GET /pay/{id}/state`, `POST /pay/{id}/attempts`, `GET /pay/{id}/events` as SSE). They expose only what the page needs, never provider config or tenant metadata. Tenants set branding (name, logo, accent colour), `allowed_origins` and default `success_url`/`cancel_url` in the admin API. The page is static assets embedded in the binary and has no tenant-specific build.

## Outbound webhooks

`POST tenant.webhook_url` with `X-Gateway-Signature: t=<unix>,v1=<hmac sha256 of "t.body">` and `X-Gateway-Event-Id`. Events: `payment.paid`, `payment.expired`, `payment.cancelled`, `refund.completed`, `attempt.failed`. Retries with exponential backoff for 3 days. Receivers must be idempotent on event id. Tenants can always poll `GET /v1/payments/{id}`.

Projects can add `webhook_endpoint` rows (url, sealed secret, event filter, enabled). `emit` writes one `gateway_event` row per event (also served by `GET /v1/events`), and one `webhook_delivery` per enabled matching endpoint (the legacy `webhook_url` is the endpoint with `is_default`, kept in sync by the settings API for one release), all in the settlement transaction. Test events are sent synchronously and leave no delivery row.

## API keys and contract

`api_key` rows hold a SHA-256 hash, prefix, scopes, optional expiry, last_used (throttled to one write a minute) and revoked_at. Tenant auth resolves the key, then each handler asks for one scope. The spec is built in `src/openapi.rs` and a test fails when a `/v1` route is missing from it. Pagination is keyset (`created,id` or `id`) with the cursor in `X-Next-Cursor`.

## Recurring billing

`billing_plan`, `subscription` and `subscription_period` (unique on subscription and period index) hold the model. Period dates come from `billing::period_bounds` (anchor plus N intervals, month ends clamped without drift). The worker claims a subscription by locking its row and setting `claimed_until`, inserts the period row (the unique key is the exactly once guard), creates a payment with idempotency key `sub-<id>-<period>-<attempt>` and charges the saved method. Settlement of a period payment calls `after_payment_paid`, which marks the period paid and emits `invoice.paid`. Events without a payment use nullable `payment_id` on `gateway_event` and `webhook_delivery`. The service reads time from an injected `Clock` so tests never sleep. Formulas and the retry schedule are in the README.

## Settlement rules

- Amounts are integer minor units (msats for BTC), as in `payments-rs` `CurrencyAmount`.
- Overpay is accepted and recorded. Underpay on-chain stays pending until expiry, then is flagged for a manual decision.
- Lightning settlement is driven by the LND invoice stream with a resume cursor, on-chain by the existing `PaymentCursor` logic. Both must survive restarts without missing or double counting.
- Settlement is a single DB transaction: attempt paid, payment paid, webhook row inserted. Delivery happens after commit from the webhook table.
- Altcoins go through FixedFloat into the tenant's on-chain wallet. The payment is paid when the swap lands, not when the customer sends.

## Custody ledger

Double entry ledger for funds the platform holds on behalf of tenants (`src/ledger/`). Settlement of shared provider payments and their refunds post to it (see Shared platform providers).

Accounts (`ledger_account`, unique on tenant, kind, currency). Currency is `BTC` (msats) or a fiat code (minor units). All amounts are signed `i64`, never floats.

- `tenant_balance`: what the platform owes the tenant. Credit positive.
- `platform_fees`: fees earned by the platform. No tenant.
- `platform_wallet`: funds in the platform's own Lightning and on-chain wallets. No tenant, may go negative by default.
- `pending_payout`: funds reserved for a payout in flight.
- `refund_reserve`: funds set aside for refunds.

Transactions (`ledger_transaction`) carry a kind (`payment_received`, `fee`, `payout_requested`, `payout_completed`, `payout_cancelled`, `refund`, `adjustment`), an optional reference (`reference_kind`, `reference_id`, for example `payment` and the payment id) and a unique `idempotency_key`. Entries (`ledger_entry`) are append only.

Invariants:

- Entries of a transaction sum to zero per currency. `ledger::post` refuses anything else before touching the database.
- `post` runs inside the caller's DB transaction. On any error the caller must roll back, which also removes the transaction row.
- Replaying an idempotency key returns the original transaction id and changes nothing.
- Non-overdraft accounts cannot go below zero (`allow_negative` per account). A debit that would do so fails with `Overdraw`.
- `ledger_account.balance` is a cache updated in the same transaction under a row lock, with accounts locked in a fixed order to avoid deadlocks. `ledger::reconcile` recomputes from entries and reports cached/computed mismatches and any unbalanced transaction.
- The sum of all balances is always zero, so value is never created or destroyed.

Read APIs: `GET /admin/tenants/{id}/balance`, `/admin/tenants/{id}/ledger?before=&limit=`, `/admin/ledger/reconcile`, and for members (viewer) `GET /console/api/projects/{id}/balance` and `/ledger`. The console project page has a Balance tab.

## Shared platform providers

An operator can register Lightning and on-chain LND wallets that belong to the platform (`shared_provider`, config sealed, same `lightning/lnd` and `onchain/lnd` implementations). A project enables one with a click and it appears in its methods marked `platform_managed`. The invoice or address is created on the platform wallet, and the attempt records the shared provider and the fee terms in force when it was created.

Custody model: the platform receives the money, so it holds the funds and owes them to the tenant. Settlement runs in the same DB transaction as the payment state change, with idempotency key `settle:{attempt_id}`:

- `platform_wallet` debit of the received amount
- `tenant_balance` credit of the net
- `platform_fees` credit of the fee

Fee rule: `fee = ceil(received * bps / 10000) + base`, capped at the received amount. The platform fee is rounded up and the tenant is never credited more than was received. Terms are the shared provider defaults, replaced per project by an override (`shared_provider_override`, either field may be left to inherit). Fee and net are on the attempt view (`platform_fee`, `net_credited`) and in the `payment.paid` and `payment.late_payment` webhook `detail`. Replayed or concurrent events credit once.

Refunds: the BTC amount refunded is debited from the tenant balance (`refund:{id}`) and paid out from the platform wallet. The platform keeps its fee. A refund the balance cannot cover is refused with a conflict unless an operator set `allow_negative` on the tenant balance (`POST /admin/tenants/{id}/allow-negative`). If the payout fails the debit is reversed.

Trust levels: each shared provider has a network (`mainnet`, `testnet`, `signet`, `regtest`) and two flags, `allow_untrusted` and `allow_trusted`. An untrusted (test mode) project can only use a shared provider on a test network that allows untrusted projects, never mainnet. The check runs when enabling and again when creating an attempt.

What the platform is responsible for: holding the funds in its wallets, paying out refunds, keeping the wallet balance solvent against what it owes tenants (`ledger::reconcile`), and securing the sealed wallet credentials. Withdrawals of the held balance are not built yet.

Admin API: `GET/POST /admin/shared`, `POST /admin/shared/{id}`, `POST /admin/shared/{id}/tenants/{tenant}/fee`. Console: operator page "Shared providers", and per project `GET /console/api/projects/{id}/shared` and `POST .../shared/{sid}/enabled`.

## Withdrawals and payouts

Tenants withdraw from `tenant_balance` (BTC, msats) through the console (`/console/api/projects/{id}/payouts`, `/withdrawals`, owner role). Destinations are sealed: an on-chain address, a Lightning address or an LNURL-pay. A bolt11 invoice can instead be given per request. A destination needs verification (a small test payout that completes, or a signed message for a legacy P2PKH address) and waits `GATEWAY_PAYOUT_DESTINATION_DELAY_HOURS` (default 24) before use.

Requesting a withdrawal locks the project row, checks the daily maximum and posts `tenant_balance` to `pending_payout` for amount plus a fee reserve (`max_fee`) in the same transaction, so parallel requests cannot overdraw. Trusted projects at or under the auto threshold go straight to `approved`; others wait in `requested` for an operator (`/console/api/admin/withdrawals`). Reject and cancel release the reserve. Completion posts `pending_payout` to `platform_wallet` and returns the unused fee reserve.

The payout worker (`Service::run_payouts`) claims `approved` rows by moving them to `sending` before any node call. Lightning invoices are stored sealed with their payment hash before paying, and on-chain sends carry the label `pgw-withdrawal-<id>`. After every call, and for any `sending` row older than the lease, the worker looks the payment up in LND (`TrackPaymentV2`, `GetTransactions`) and only retries when LND does not know it. Unknown outcomes stay `sending` and keep reserving the funds. Enable it with `GATEWAY_PAYOUT_LND_URL`, `GATEWAY_PAYOUT_LND_CERT`, `GATEWAY_PAYOUT_LND_MACAROON`, and `GATEWAY_PAYOUT_NETWORK` (`bitcoin`, `regtest`). Limits: `GATEWAY_PAYOUT_MIN_SATS`, `GATEWAY_PAYOUT_DAILY_MAX_SATS`, `GATEWAY_PAYOUT_DAILY_MAX_UNTRUSTED_SATS`, `GATEWAY_PAYOUT_AUTO_SATS`; operators can override the daily maximum and auto threshold per project. Webhook events: `withdrawal.requested`, `withdrawal.completed`, `withdrawal.failed` (data under `data.withdrawal`, no destination), fanned out to webhook endpoints like payment events so event filters apply, and listed in `GET /v1/events`. On-chain batching is not implemented.

## Reconciliation, solvency and treasury

`Service::run_reconciliation` runs hourly on the lead worker (`reconcile_if_due`) and on demand from the operator Treasury page (`POST /console/api/admin/treasury/run`). It sums `tenant_balance`, `pending_payout` and `platform_fees` (liabilities) and compares them with the wallet balances the shared platform providers report (`Provider::wallet_balances`: LND channel local balance and confirmed on-chain balance). Every run is stored in `reconciliation_run` with the difference, the ledger drift and zero-sum findings, the stuck count and the alert text. An alert is raised when liabilities exceed assets by more than `GATEWAY_SOLVENCY_TOLERANCE_SATS` (default 0), when a wallet lookup fails (solvency unverified), when a cached balance differs from its entries, or when a transaction does not sum to zero. Alerts are emailed to operators (at most hourly) and shown as a banner on every console page for operators. The check only reports: nothing moves money or rewrites a balance.

The stuck report lists payments settled on a shared provider without a ledger credit, withdrawals `sending` for over an hour, on-chain receipts unconfirmed beyond the 24 hour grace, and paid attempts whose webhooks are all undelivered after an hour. `/metrics` (admin or metrics token) adds `gateway_liabilities_msat`, `gateway_assets_msat`, `gateway_solvency_difference_msat`, `gateway_ledger_drift_accounts`, `gateway_ledger_unbalanced_transactions`, `gateway_stuck_items`, `gateway_withdrawals_pending`, `gateway_fees_earned_msat`, plus the existing `gateway_webhook_oldest_pending_age_seconds` and `gateway_watcher_up{provider}`.

## Self-service console (SaaS)

The gateway is a multi-tenant service. Operators sign in at `/console`, which is a dependency-free single page served from the binary.

- **Accounts:** email and password (argon2id), cookie sessions with a CSRF token, login lockout, invite-only signup. Operators create invites in the console or with the admin token. TOTP is planned.
- **Projects:** one account owns many projects, each a tenant with its own API key, providers, webhook and branding. Roles per project: viewer, admin, owner. Teammates join by invite link.
- **Test mode:** customer projects start untrusted. An operator lifts the limits per project. Untrusted projects cannot use mock providers, local credential files, custom provider URLs, or non-public addresses, and their webhooks are only sent to public https hosts. Operators and trusted projects keep file paths and internal hosts.
- **Secrets:** provider credentials are sealed at rest and never returned. The console shows `set` and keeps the stored value when a field is left alone.
- **Audit log:** `audit_event` is append only (app code never updates or deletes rows). `audit::Event` records who (account, api key, admin token, system), what (dotted action from `audit::Action`), target, ip, user agent and details. Details hold field names only, never values. A failed write is logged and never fails the request. Readable per project in the console Activity tab (viewers included), platform wide on the operator Audit page and `GET /admin/audit`. Actions: login.ok, login.failed, logout, password.changed, invite.created, invite.used, member.added, member.removed, member.role_changed, project.created, key.rotated, webhook_secret.rotated, provider.created, updated, enabled, disabled, deleted, settings.changed, shared_provider.created, shared_provider.updated, shared_method.enabled, shared_method.disabled, shared_fee.overridden, balance.allow_negative_changed, refund.requested, refund.result, payment.cancelled, project.trust_changed, project.enabled_changed, account.disabled, account.enabled, delivery.replayed.
- **Console v2:** payment filters, streamed CSV export, saved views, notes, bulk cancel, manual mark as paid (per project switch, admin, reason, `manual` flag on the event), customer pages, refund preview, webhook delivery detail and health, dashboard, operator tenant list, support view and global search, legal details, notification preferences and a token plus cooling-off project deletion. Storage is `payment_note`, `saved_view` and `project_config`, plus capture columns on `webhook_delivery`. Subscriptions on the customer page stay empty until recurring billing exists.
- **Not built yet:** withdrawals and payouts of the held balance, TOTP, password reset by email, billing for platform fees.

## Milestones

1. Skeleton: workspace, config, migrations, tenants and API keys, admin API, health, CI
2. Core payment model, create and get, expiry worker, outbound webhook delivery
2b. Public pay page: tenant branding and allowed origins, `pay_url`, `/pay/{id}` page with method picker and live status over SSE, `/embed/{id}` with postMessage and CSP, `gateway.js`, signed redirect, tests for origin enforcement and redirect signing
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
- **Limits and rate limiting:** every project has usage limits stored on the tenant (nullable `limit_*` columns override the defaults, which are low for untrusted projects and high for trusted ones): payments per 24 hours, live pending payments (pending and not past expiry), amount per payment and volume per 24 hours (both in EUR cents, other currencies converted at the current rate), providers, webhook endpoints, attempts per payment. Payment creation checks them inside a transaction that locks the tenant row. Breaches return 403 `{"code": "limit_exceeded", "limit", "current", "max", "error"}`. Operators edit them at `POST /console/api/admin/projects/{id}/limits` (a field set to null returns to the default); members read limits and usage at `GET /console/api/projects/{id}/limits`.
- Token buckets in memory (one replica) return 429 with `Retry-After` and `{"code": "rate_limited"}`: per API key and per IP on `/v1/*`, per IP on `/pay/*` and `/embed/*` (stricter for `POST /pay/{id}/attempts`), signup, login and `/hook/*`. The client IP is the last `X-Forwarded-For` hop, the one our ingress adds. Request bodies over 1 MiB get 413.
- **Provider events and health:** provider callbacks map to `ProviderEvent` (`Paid`, `Failed`, `Refunded`, `RefundFailed`, `Dispute`). `/hook` caps the body at 256 KiB and dedupes on the provider event id in `provider_event` (released if processing fails, so the provider retries). Refunds seen from a provider are matched to gateway refund rows by external id so a refund made through the API and its webhook count once. A dispute sets `payment.dispute_status` without changing the payment status. `Guarded` reports every real provider call to `provider_health` (rolling 24 hour counters, consecutive failures, last error). Ten failures in a row auto-disable a provider of an untrusted project; a trusted project's provider only alerts the operators. PayPal has no `payments-rs` support, so it is a direct Orders v2 client (`providers/paypal.rs`).
