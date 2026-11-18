# payment-gateway

A self contained payment service. Apps ask it to collect money and it tells them when it is paid. It knows nothing about VMs, subscriptions or credit: that stays in the app.

Methods: Lightning (LND), on-chain Bitcoin (LND), Stripe, Revolut, and altcoins through FixedFloat swaps into the on-chain wallet. Design notes are in [DESIGN.md](DESIGN.md).

## Run

```
GATEWAY_ENCRYPTION_KEY=$(openssl rand -hex 32)   # keep this, provider secrets are sealed with it
# or several, newest first: GATEWAY_ENCRYPTION_KEYS=k2:<hex>,k1:<hex>
GATEWAY_ADMIN_TOKEN=$(openssl rand -hex 24)
DATABASE_URL=mysql://user:pass@host/gateway
GATEWAY_PUBLIC_URL=https://pay.example.com        # used for pay pages and provider webhooks
GATEWAY_LISTEN=0.0.0.0:8080                       # optional
GATEWAY_ALLOW_MOCK=1                              # tests only, never in production
```

Migrations run on start. Every replica serves the API. Background work (provider watchers, expiry sweep, webhook sender, reconcile) runs only on the replica holding the `workers` lease in the `worker_lease` table. The lease is renewed every third of its TTL and taken over after it expires, so rolling deploys with more than one replica are safe. On SIGTERM or ctrl-c the process stops accepting, finishes in-flight requests and settlements, releases the lease and exits.

## Configuration

All variables are validated at start and every problem is reported at once (exit code 2). `DATABASE_URL`, `GATEWAY_ENCRYPTION_KEY`, `GATEWAY_ADMIN_TOKEN` and `METRICS_TOKEN` also accept a `_FILE` variant naming a file that holds the value (for mounted secrets).

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | required | MySQL or MariaDB URL |
| `GATEWAY_ENCRYPTION_KEY` | required | 64 hex chars, seals secrets at rest |
| `GATEWAY_ADMIN_TOKEN` | required | admin API token, at least 24 chars |
| `METRICS_TOKEN` | unset | extra bearer token accepted by `/metrics` only |
| `GATEWAY_PUBLIC_URL` | required | base URL for pay pages and provider webhooks |
| `GATEWAY_LISTEN` | `0.0.0.0:8080` | listen address |
| `GATEWAY_ALLOW_MOCK` | off | enable the mock provider, tests only |
| `GATEWAY_OPERATOR_2FA_GRACE` | off | let operator accounts without 2FA sign in during rollout |
| `LOG_LEVEL` | `info` | error, warn, info, debug, trace (falls back to `RUST_LOG`) |
| `LOG_FORMAT` | `json` | `json` or `text` |
| `GATEWAY_BODY_LIMIT_BYTES` | `1048576` | request body limit on every route |
| `GATEWAY_REQUEST_TIMEOUT_SECS` | `60` | per request timeout (SSE streams excluded) |
| `DB_ACQUIRE_TIMEOUT_SECS` | `10` | wait for a free DB connection |
| `DB_MAX_CONNECTIONS` | `10` | pool size |
| `SHUTDOWN_GRACE_SECS` | `25` | time allowed to drain on SIGTERM |
| `LEASE_TTL_SECS` | `30` | worker lease lifetime |
| `WORKER_TICK_SECS` | `5` | background loop interval |
| `GATEWAY_INSTANCE_ID` | hostname | name shown in the lease and logs |

The regtest sandbox is enabled by its own `SANDBOX_*` variables. Provider HTTP calls and LND calls time out after 45 seconds; the rates client after 15.

## Operations

```
GET /health    liveness, always cheap
GET /ready     200 when the DB answers, migrations are applied and the process is not draining; 503 otherwise
GET /metrics   Prometheus text, bearer admin token or METRICS_TOKEN
GET /status    operator status (JSON, or HTML with Accept: text/html), bearer admin token
```

Logs are one JSON object per line with `ts`, `level`, `msg`, `request_id` and `tenant_id` when known. Send `x-request-id` and it is used and echoed back, otherwise one is generated. Known secrets are redacted from log lines and webhook bodies are never logged.

Metrics: `gateway_http_requests_total`, `gateway_http_request_duration_seconds`, `gateway_settlements_total`, `gateway_settlement_duration_seconds`, `gateway_webhook_deliveries_total{outcome}`, `gateway_webhook_pending`, `gateway_webhook_oldest_pending_age_seconds`, `gateway_provider_calls_total{method,op,outcome}`, `gateway_provider_call_duration_seconds`, `gateway_watcher_up{provider}`, `gateway_leader`, `gateway_db_pool_connections{state}`.

## Keys, backups and data

The encryption key seals provider credentials and webhook secrets in the database. It lives only in the deployment secret (`GATEWAY_ENCRYPTION_KEY` or `GATEWAY_ENCRYPTION_KEYS`) and must also be escrowed offline, separately from database backups: for example in a password manager vault and a sealed paper copy held by a second person. If every copy is lost, provider configs and webhook secrets cannot be read and must be re-entered; payments, attempts, refunds and receipts are not encrypted and survive. A leaked key plus a database backup exposes every provider credential.

Operator commands (`payment-gateway <command>`, same environment): `rotate-keys`, `purge`, `check-integrity`, `privacy-export`, `privacy-delete`. Runbooks: [key rotation](docs/runbooks/key-rotation.md), [backup, restore and disaster recovery](docs/runbooks/restore.md), [retention and personal data](docs/runbooks/retention-and-privacy.md).

## Console

Open `/console`. Create the first operator account with the admin token, then everything else is in the console:

```
curl -X POST $GW/admin/invites -H "Authorization: Bearer $ADMIN" -H 'content-type: application/json' \
  -d '{"email":"you@example.com","operator":true}'
```

Open the returned `invite_url`, choose a password, then create projects, add providers (credentials are write-only), watch payments and webhooks, refund, and invite teammates. Customer projects start in test mode until an operator lifts the limits from the project's Settings page.

Two factor: each account can enrol a TOTP authenticator in Account settings (RFC 6238, SHA1, 6 digits, 30 s, one step of drift). Activation needs a valid code and returns 10 single use recovery codes, shown once and stored hashed. With 2FA on, `POST /console/api/login` returns `{"two_factor": true, "challenge": ...}` and no session; `POST /console/api/login/2fa` with `challenge` plus `code` or `recovery_code` creates it. A TOTP step cannot be used twice, and failed codes count against the login limiter. Disabling and regenerating recovery codes need the password and a current code. Owners can set `require_2fa` on a project (`POST /console/api/projects/{id}/require-2fa`), which blocks console access to it for members without 2FA. Operators must have 2FA once `GATEWAY_OPERATOR_2FA_GRACE` is not `1`; set it to `1` while bootstrapping.

## Set up a tenant with the admin API

```
curl -X POST $GW/admin/tenants -H "Authorization: Bearer $ADMIN" -H 'content-type: application/json' \
  -d '{"name":"yalr","webhook_url":"https://yalr.example/hooks/payments"}'
```

The reply holds `api_key` and `webhook_secret`. They are shown once. Then add providers:

```
curl -X POST $GW/admin/tenants/1/providers -H "Authorization: Bearer $ADMIN" -H 'content-type: application/json' -d '{
  "method":"lightning","provider_type":"lnd","name":"main",
  "config":{"url":"https://lnd:10009","cert_path":"/lnd/tls.cert","macaroon_path":"/lnd/admin.macaroon"},
  "fee_rate":0.0}'
```

| method | provider_type | config |
| --- | --- | --- |
| lightning | lnd | `url`, `cert_path`, `macaroon_path` |
| onchain | lnd | `url`, `cert_path`, `macaroon_path`, `address_type` (p2tr, p2wkh, np2wkh), `min_confirmations` |
| stripe | stripe | `secret_key`, `webhook_secret` |
| revolut | revolut | `url`, `token`, `api_version`, `public_key` (webhook secret is registered and stored automatically) |
| fixedfloat | fixedfloat | `api_key`, `api_secret`, `coins` (for example `["XMR","LTC"]`), optional `refcode`, `afftax`. Needs an onchain provider on the same tenant to receive into |

`fee_rate` (percent), `fee_base` and `fee_currency` add a processing fee to what the customer pays on that method.

Stripe webhooks go to the `hook_url` returned when the provider is created (`/hook/{tenant}/{provider}`), subscribed to `checkout.session.completed`, `checkout.session.async_payment_succeeded` and `checkout.session.async_payment_failed`.

Branding, allowed iframe origins and default return URLs: `POST /admin/tenants/{id}/settings`.

## Take a payment

Pure API:

```
POST /v1/payments            Idempotency-Key: order-123
{"reference":"order-123","amount":1500,"currency":"EUR","expires_in":3600,
 "customer_ref":"user-9","success_url":"https://app/ok","cancel_url":"https://app/no"}

POST /v1/payments/{id}/attempts   {"method":"lightning"}
```

The attempt response carries `details`: `bolt11`, an on-chain `address` and `uri`, a `checkout_url` for Stripe and Revolut, or a `deposit_address` and `deposit_amount` for an altcoin.

Limits: each project has usage limits (payments and volume per 24 hours, live pending payments, amount per payment, providers, webhook endpoints, attempts per payment), low in test mode and high once trusted. Breaching one returns 403 with `code: "limit_exceeded"`, the `limit` name, `current` and `max`. Request rate is limited per API key and per IP: a 429 carries `Retry-After`. Bodies over 1 MiB get 413. Operators edit limits per project in the console.

Hosted page: send the customer to the `pay_url` in the create response. It lets them pick a method, shows the QR or hands off to the card form, updates live, then redirects to `success_url` with `payment_id`, `reference`, `status` and `sig`. `sig` is `HMAC-SHA256(webhook_secret, "{payment_id}.{reference}.{status}")`. `success_url` and `cancel_url` (and the project defaults) must be on one of the project's `allowed_origins`, otherwise create is rejected. Still confirm with the webhook or `GET /v1/payments/{id}`.

Iframe:

```html
<div id="pay"></div>
<script src="https://pay.example.com/gateway.js"></script>
<script>
  PaymentGateway.open({ gateway: "https://pay.example.com", paymentId: id, container: "#pay",
    onPaid: () => location.reload() });
</script>
```

The tenant's `allowed_origins` must include the embedding site. Card checkouts cannot be framed, so the embed sends the top window to them.

## Webhooks

`POST` to the tenant `webhook_url`, JSON `{id, type, created, data:{payment, detail}}`, with headers `X-Gateway-Event-Id` and `X-Gateway-Signature: t=<unix>,v1=<hex>` where `v1 = HMAC-SHA256(webhook_secret, "{t}.{body}")`. Reject old timestamps and dedupe on the event id. Failed deliveries retry with backoff for 3 days. Events: `payment.paid`, `payment.expired`, `payment.cancelled`, `payment.late_payment`, `attempt.failed`, `refund.completed`, `refund.failed`.

## Other endpoints

```
GET    /v1/payments/{id}                     GET /v1/payments?reference=
POST   /v1/payments/{id}/cancel
POST   /v1/payments/{id}/refund              {"amount":500,"destination":"<bolt11 or address>"}
GET    /v1/saved-methods?customer_ref=       DELETE /v1/saved-methods/{id}
POST   /v1/saved-methods/{id}/charge         a payment body, charged off session
GET    /v1/methods      GET /v1/rates
GET    /admin/audit?action=&actor=&tenant_id=&before=&limit=   (audit log, newest first)
GET    /admin/deliveries?undelivered=1       POST /admin/deliveries/{id}/replay
```

Lightning refunds need a bolt11 invoice with an amount, on-chain refunds an address. Card refunds go back to the card.

## Email

Mail is off unless both variables are set. With mail off, invite links are only shown in the console and password reset does nothing.

| Variable | Meaning |
| --- | --- |
| `SMTP_URL` | `smtps://user:pass@host:465` (implicit TLS) or `smtp://user:pass@host:587?tls=required` (STARTTLS) |
| `MAIL_FROM` | Sender, for example `Gateway <noreply@example.com>` |

With mail on:

- `POST /console/api/reset {"email"}` always answers `{"ok":true}` and is rate limited. A single use token valid for 1 hour is emailed (only its hash is stored). `POST /console/api/reset/confirm {"token","password"}` sets the password, signs out every session and voids other reset tokens. The console pages are `/console#/reset` and `/console/reset/<token>`.
- Operator and team invites are also emailed. The link is still in the API response.
- `POST /console/api/email {"password","new_email"}` emails a confirmation to the new address, `POST /console/api/email/confirm {"token"}` applies it.
- Project owners get an email for abandoned webhook deliveries, a provider that failed 3 times in a row and failed refunds, at most one per event type per project per hour. `POST /console/api/notifications {"enabled"}` turns them off for an account.

Mail is plain text plus simple HTML, with no tracking and no remote images.

## Develop

```
docker run -d --name pgw-test-db -e MARIADB_ROOT_PASSWORD=test -p 127.0.0.1:33907:3306 mariadb:11.4
TEST_DATABASE_URL=mysql://root:test@127.0.0.1:33907/ cargo test
```

Each test creates its own database. The build needs `protoc`.

### Extra suites

- `tests/chaos.rs`: mock provider fault injection with invariants (one `payment.paid` per settled payment, no over refund, receipts cover the amount).
- `tests/providers_e2e.rs`: Stripe and Revolut flows replayed from `tests/fixtures/` (Revolut fixtures are recorded from the Revolut sandbox, Stripe ones are built from the documented response shapes). With `STRIPE_TEST_KEY` (an `sk_test_` key) or `REVOLUT_SANDBOX_TOKEN` set, the live tests also run against the real test mode APIs.
- `tests/regtest_e2e.rs` and `tests/regtest_long.rs`: real bitcoind and LND. `regtest/scripts/up.sh`, then `REGTEST=1 cargo test --test regtest_e2e --test regtest_long -- --test-threads=1`, then `regtest/scripts/reset.sh`. The long test covers parallel lightning payments, underpayment plus top up, overpayment, an RBF replacement, a gateway kill mid payment and an LND restart. `mine.sh`, `fund.sh` and `rebalance.sh` are helpers. The `regtest` workflow runs this on a schedule.
- `scripts/load.sh`: load test, see `docs/performance.md`.
