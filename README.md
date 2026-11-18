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

Migrations run on start. One process runs the API, the provider watchers (LND streams), the expiry sweep and the webhook sender, so run a single replica.

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

Hosted page: send the customer to the `pay_url` in the create response. It lets them pick a method, shows the QR or hands off to the card form, updates live, then redirects to `success_url` with `payment_id`, `reference`, `status` and `sig`. `sig` is `HMAC-SHA256(webhook_secret, "{payment_id}.{reference}.{status}")`. Still confirm with the webhook or `GET /v1/payments/{id}`.

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
