# Provider fixtures

The Stripe and Revolut fixtures here are built by hand from the documented response shapes. They are not recordings of real API traffic, so they prove the gateway handles the documented shapes, not that the live APIs still return them.

With a key, `scripts/record-fixtures.sh` replaces the fixtures it can with real responses:

```
STRIPE_TEST_KEY=sk_test_... REVOLUT_SANDBOX_TOKEN=... scripts/record-fixtures.sh
```

Stripe: checkout session, session retrieve, off-session intent, refund and card declined are recorded. The `session_completed_event.json` webhook body stays hand-built (it needs a paid checkout). Revolut: a created order is recorded. `order_completed.json`, `refund.json` and `webhook_order_completed.json` need a paid sandbox order and stay hand-built. Review the diff and run `cargo test --test providers_e2e` after recording.
