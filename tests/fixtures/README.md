# Provider fixtures

Revolut fixtures are real sandbox recordings (18 Nov 2026): `order_created.json`, `order_pending.json`, `order_completed.json` (a paid order), `refund.json` and `webhook_order_completed.json` (a real `ORDER_COMPLETED` delivery body, signature verified against the signing secret). They hold no credentials. Order and payment ids are sandbox ids.

The Stripe fixtures are still built by hand from the documented response shapes, because no `STRIPE_TEST_KEY` exists yet. They prove the gateway handles the documented shapes, not that the live API still returns them.

With keys, `scripts/record-fixtures.sh` re-records what it can:

```
STRIPE_TEST_KEY=sk_test_... REVOLUT_SANDBOX_TOKEN=... scripts/record-fixtures.sh
```

Stripe: checkout session, session retrieve, off-session intent, refund and card declined are recorded. The `session_completed_event.json` webhook body stays hand-built (it needs a paid checkout). Revolut: the script records a created and a pending order. The completed order, refund and webhook bodies need a paid order: pay it through `POST /api/orders/{id}/payments` with the sandbox card 4929 4205 7359 5709 (the `environment` block belongs inside `payment_method`), register a webhook to a capture URL, and save the bodies. The live Revolut test in `tests/providers_e2e.rs` does the create, pay and refund steps. Review the diff and run `cargo test --test providers_e2e` after recording.
