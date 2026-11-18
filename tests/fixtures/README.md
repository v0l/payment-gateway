# Provider fixtures

Revolut fixtures are real sandbox recordings (18 Nov 2026): `order_created.json`, `order_pending.json`, `order_completed.json` (a paid order), `refund.json` and `webhook_order_completed.json` (a real `ORDER_COMPLETED` delivery body, signature verified against the signing secret). They hold no credentials. Order and payment ids are sandbox ids.

Also recorded live from the Revolut sandbox (18 Nov 2026, #744): `order_payment_declined.json` (an order whose payment was declined with the sandbox card 4929 5736 3812 5985, paid through the checkout widget with Playwright `frameLocator`), `webhook_order_payment_declined.json` (the real `ORDER_PAYMENT_DECLINED` delivery body, signature checked against the signing secret) and `payment_methods.json` (`GET /api/1.0/customers/{id}/payment-methods`). The live test `revolut_sandbox_live_test_connection_saved_methods_and_removal` runs test connection, the saved method list and a removal of an unknown method (Revolut answers 404, which is treated as removed). A real removal was not run: saving a card needs the embedded widget (#775), so the only saved cards belong to earlier sandbox customers.

Doc-shaped, not recorded: `revolut/webhook_dispute_action_required.json` (the sandbox cannot raise a dispute), all Stripe fixtures including `stripe/dispute_created_event.json` and `stripe/dispute_closed_event.json`, and `paypal/webhook_dispute_created.json`. There are no Stripe or PayPal sandbox keys, so those prove the gateway handles the documented shapes, not that the live APIs still return them.

The Stripe fixtures are still built by hand from the documented response shapes, because no `STRIPE_TEST_KEY` exists yet. They prove the gateway handles the documented shapes, not that the live API still returns them.

With keys, `scripts/record-fixtures.sh` re-records what it can:

```
STRIPE_TEST_KEY=sk_test_... REVOLUT_SANDBOX_TOKEN=... scripts/record-fixtures.sh
```

Stripe: checkout session, session retrieve, off-session intent, refund and card declined are recorded. The `session_completed_event.json` webhook body stays hand-built (it needs a paid checkout). Revolut: the script records a created and a pending order. The completed order, refund and webhook bodies need a paid order: pay it through `POST /api/orders/{id}/payments` with the sandbox card 4929 4205 7359 5709 (the `environment` block belongs inside `payment_method`), register a webhook to a capture URL, and save the bodies. The live Revolut test in `tests/providers_e2e.rs` does the create, pay and refund steps. Review the diff and run `cargo test --test providers_e2e` after recording.
