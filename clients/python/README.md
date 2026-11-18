# payment-gateway-client (Python)

Python client for the payment gateway `/v1` API. Standard library only. Install the `httpx` extra to use httpx as the transport.

## Quick start

```python
from payment_gateway import Gateway

gw = Gateway("https://pay.example.com", "pgw_key")

payment = gw.create_payment(idempotency_key="order-1001", reference="order-1001", amount=1999, currency="EUR")
print(payment["pay_url"])

gw.create_attempt(payment["id"], "lightning")
gw.get_payment(payment["id"])
gw.refund_payment(payment["id"], amount=500)
gw.cancel_payment(other_id)

for p in gw.list_payments(status="paid", limit=50):
    print(p["id"])
```

Amounts are integer minor units. `create_payment` and `charge_saved_method` always send an `Idempotency-Key`, generated when you do not pass one and reused across retries.

Saved methods: `list_saved_methods()`, `delete_saved_method(id)`, `charge_saved_method(id, ...)`.

To use httpx: `pip install "payment-gateway-client[httpx]"` and `Gateway(url, key, transport=httpx_transport)`.

## Retries

429 and 5xx are retried with exponential backoff and jitter, honouring `Retry-After`. Calls without an idempotency key are only retried for `GET`, `DELETE` and 429. Tune with `max_retries`, `retry_base_delay`, `retry_max_delay` and `timeout`.

## Errors

```python
from payment_gateway import GatewayError

try:
    gw.create_payment(amount=0, currency="EUR")
except GatewayError as e:
    print(e.status, e.code, e.field, str(e))
```

`GatewayError` has `status`, `code`, `field`, `retry_after` and `body`. Transport failures raise `NetworkError`.

## Verify a webhook

Use the raw request body.

```python
from payment_gateway import verify_webhook, WebhookVerificationError

try:
    event = verify_webhook(request.get_data(), request.headers, WEBHOOK_SECRET, 300)
except WebhookVerificationError:
    abort(400)
```

Signatures are compared with `hmac.compare_digest`. The tolerance is in seconds (default 300, 0 disables the timestamp check).

## Tests

```
python -m unittest discover -s tests
```

Set the variables printed by `scripts/start-test-gateway.sh` to also run the conformance scenario against a live gateway.
