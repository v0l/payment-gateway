# @payment-gateway/client

TypeScript client for the payment gateway `/v1` API. ESM and CommonJS builds, bundled types, no runtime dependencies. Needs Node 18+ or any runtime with `fetch`.

## Quick start

```ts
import { Gateway } from "@payment-gateway/client";

const gw = new Gateway({ baseUrl: "https://pay.example.com", apiKey: process.env.GATEWAY_API_KEY! });

const payment = await gw.createPayment(
  { reference: "order-1001", amount: 1999, currency: "EUR" },
  { idempotencyKey: "order-1001" },
);
console.log(payment.pay_url);

const withAttempt = await gw.createAttempt(payment.id, { method: "lightning" });
const paid = await gw.getPayment(payment.id);
await gw.refundPayment(payment.id, { amount: 500 });
await gw.cancelPayment(otherId);

for await (const p of gw.listPayments({ status: "paid", limit: 50 })) {
  console.log(p.id);
}
```

Amounts are integer minor units. `createPayment` and `chargeSavedMethod` always send an `Idempotency-Key`; one is generated if you do not pass it, and the same key is reused across retries.

Saved methods: `listSavedMethods()`, `deleteSavedMethod(id)`, `chargeSavedMethod(id, input)`.

## Retries

429 and 5xx responses are retried with exponential backoff and jitter, honouring `Retry-After`. Calls without an idempotency key are only retried when they are a `GET` or `DELETE`, or on 429. Tune with `maxRetries`, `retryBaseDelayMs`, `retryMaxDelayMs` and `timeoutMs`.

## Errors

```ts
import { GatewayError } from "@payment-gateway/client";

try {
  await gw.createPayment({ amount: 0, currency: "EUR" });
} catch (e) {
  if (e instanceof GatewayError) console.log(e.status, e.code, e.field, e.message);
}
```

`GatewayError` carries `status`, `code` (`invalid_request`, `not_found`, `conflict`, `rate_limited`, ...), `field`, `retryAfter` and the raw `body`. Transport failures throw `NetworkError`.

## Verify a webhook

Pass the raw request body, not a re-serialised object.

```ts
import express from "express";
import { verifyWebhook, WebhookVerificationError } from "@payment-gateway/client";

app.post("/webhook", express.raw({ type: "*/*" }), (req, res) => {
  try {
    const event = verifyWebhook(req.body, req.headers, process.env.WEBHOOK_SECRET!, 300);
    res.sendStatus(200);
  } catch (e) {
    if (e instanceof WebhookVerificationError) return res.sendStatus(400);
    throw e;
  }
});
```

The signature is compared in constant time and the timestamp must be within the tolerance (seconds, default 300, 0 disables the check).

## Tests

```
npm install && npm test
```

Set `GATEWAY_URL` and the other variables printed by `scripts/start-test-gateway.sh` to also run the conformance scenario against a live gateway.
