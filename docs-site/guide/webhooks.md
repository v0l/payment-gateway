# Webhooks

The gateway sends an HTTP `POST` with a JSON body to each of the project's webhook endpoints. Create endpoints, choose which events each receives and send test events in the console.

```json
{
  "id": "evt_...",
  "type": "payment.paid",
  "created": 1767225600,
  "data": {"payment": {"id": "...", "status": "paid", "reference": "order-1"}, "detail": {}}
}
```

Withdrawal events carry `data.withdrawal` instead of `data.payment`.

## Headers

- `X-Gateway-Event-Id`: the event id. Deduplicate on it, deliveries are at least once.
- `X-Gateway-Signature`: `t=<unix seconds>,v1=<hex>` where `v1` is `HMAC-SHA256(secret, "{t}.{body}")` over the raw request body.

## Events

`payment.paid`, `payment.expired`, `payment.cancelled`, `payment.late_payment`, `attempt.failed`, `refund.completed`, `refund.failed`, `withdrawal.requested`, `withdrawal.completed`, `withdrawal.failed`. Ignore event types you do not know, new ones can be added.

## Delivery and retries

Answer with any 2xx status to accept. Anything else, or a timeout, is retried with growing delays for three days, always with the same event id. After that the delivery is abandoned and an operator can replay it. You can also pull the same events from `GET /v1/events`, or poll `GET /v1/payments/{id}`.

## Verifying a signature

Verify against the raw body, not a re-serialised copy. Reject timestamps more than five minutes from now, and compare in constant time.

The JavaScript (`clients/js`), Rust (`clients/rust`) and Python (`clients/python`) client libraries in `clients/` ship a `verifyWebhook` helper, so you can use one of them instead of the samples below.

### JavaScript (Node)

@include samples/verify.js js

### Python

@include samples/verify.py python

### Rust

The Rust sample needs the `hmac`, `sha2` and `hex` crates.

@include samples/verify.rs rust
