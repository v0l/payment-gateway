# Quickstart: API only

You create the payment, ask for an attempt, show the invoice or address yourself, and learn the result from a [webhook](/guide/webhooks) or by polling.

Create a project and an API key in the [console](/console), add a provider, and export the key:

```sh
export GW=https://pay.example.com
export KEY=pgw_test_replace_me
```

## 1. Create a payment

Send an `Idempotency-Key` so a retry never creates a second payment. Amounts are minor units: 1500 is 15.00 EUR.

```sh run
PAYMENT=$(curl -sf -X POST "$GW/v1/payments" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: quickstart-api-1" \
  -H "Content-Type: application/json" \
  -d '{"reference":"quickstart-api-1","amount":1500,"currency":"EUR","expires_in":3600,"customer_ref":"user-9"}')
ID=$(echo "$PAYMENT" | jq -r .id)
echo "$PAYMENT" | jq '{id, status, pay_url}'
```

## 2. Create an attempt

```sh run
ATTEMPT=$(curl -sf -X POST "$GW/v1/payments/$ID/attempts" \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -d '{"method":"lightning"}')
echo "$ATTEMPT" | jq -r '.attempts[0].details.bolt11'
```

Show the `bolt11` as a QR code. Other methods return their own `details`, see [concepts](/guide/concepts).

## 3. Learn the result

Wait for the `payment.paid` webhook, or poll:

```sh run
curl -sf "$GW/v1/payments/$ID" -H "Authorization: Bearer $KEY" | jq -r .status
```

The status is `pending` until the payment settles, then `paid`. Do not fulfil an order on anything but the webhook or a `GET` that says `paid`.

## Using the JavaScript client

The same flow with the JavaScript client (`clients/js` in the repository), which also retries 429 and 5xx responses and reuses the idempotency key across retries. There are matching Rust (`clients/rust`) and Python (`clients/python`) clients.

```js run
import { Gateway } from "@payment-gateway/client";

const gw = new Gateway({ baseUrl: process.env.GW, apiKey: process.env.KEY });
const payment = await gw.createPayment(
  { reference: "quickstart-js-1", amount: 1500, currency: "EUR" },
  { idempotencyKey: "quickstart-js-1" },
);
const withAttempt = await gw.createAttempt(payment.id, { method: "lightning" });
console.log(withAttempt.attempts[0].details.bolt11);
const current = await gw.getPayment(payment.id);
if (current.status !== "pending") throw new Error(current.status);
```

## 4. Cancel

A pending payment can be cancelled:

```sh run
curl -sf -X POST "$GW/v1/payments/$ID/cancel" -H "Authorization: Bearer $KEY" | jq -r .status
```
