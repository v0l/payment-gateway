# FAQ

## Which amounts format does the API use?

Integer minor units: cents for EUR and USD, msats for BTC. There are no floats in amounts.

## Why did I get a 409 when I retried a create?

You reused an `Idempotency-Key` with different parameters. The same key with the same body returns the original payment.

## A webhook arrived twice. Is that a bug?

No. Delivery is at least once. Deduplicate on `X-Gateway-Event-Id`.

## Can I trust the redirect to my success page?

No. Confirm with the webhook or `GET /v1/payments/{id}`.

## Why is my webhook not delivered in test mode?

Test projects only deliver to public https hosts. Use a tunnel, not `localhost`.

## How long are webhooks retried?

Three days, with growing delays. An operator can replay an abandoned delivery.

## Can I take payments in a currency I do not hold?

Yes for fiat and Lightning. The gateway converts the fiat amount at the current rate for Bitcoin methods and shows the fee in the attempt.

## Where is the API description?

`GET /v1/openapi.json` returns the OpenAPI document with no authentication, and the [API reference](/guide/reference) is generated from it.
