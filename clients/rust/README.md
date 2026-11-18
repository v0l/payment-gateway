# payment-gateway-client (Rust)

Async Rust client (reqwest, rustls) for the payment gateway `/v1` API.

## Quick start

```rust
use payment_gateway_client::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let gw = Gateway::new("https://pay.example.com", "pgw_key");

    let input = CreatePayment::new(1999, "EUR").reference("order-1001");
    let payment = gw.create_payment_with_key(&input, "order-1001").await?;
    println!("{}", payment.pay_url);

    gw.create_attempt(&payment.id, &CreateAttempt::new("lightning")).await?;
    gw.refund_payment(&payment.id, &RefundInput { amount: Some(500), ..Default::default() }).await?;

    let mut pager = gw.list_payments(&ListPayments::default());
    while let Some(p) = pager.next().await? {
        println!("{}", p.id);
    }
    Ok(())
}
```

Amounts are integer minor units. `create_payment` generates an idempotency key, `create_payment_with_key` takes yours. The key is reused across retries.

Saved methods: `list_saved_methods`, `delete_saved_method`, `charge_saved_method`.

## Retries

429 and 5xx are retried with exponential backoff and jitter, honouring `Retry-After`. Without an idempotency key only `GET`, `DELETE` and 429 are retried. Configure with `Gateway::with_retries`.

## Errors

```rust
match gw.get_payment("missing").await {
    Err(Error::Api { status, code, field, .. }) => println!("{status} {code} {field:?}"),
    Err(other) => println!("{other}"),
    Ok(_) => {}
}
```

## Verify a webhook

```rust
let event = verify_webhook(raw_body, signature_header, &secret, 300)?;
```

`raw_body` is the unparsed request body, `signature_header` the `x-gateway-signature` value. The compare is constant time, and the timestamp must be within the tolerance in seconds (0 disables it). Failures are a `WebhookError`.

## Tests

```
cargo test
```

Set the variables printed by `scripts/start-test-gateway.sh` to also run the conformance scenario against a live gateway.
