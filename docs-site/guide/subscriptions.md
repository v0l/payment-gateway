# Subscriptions and recurring billing

The gateway does not run subscriptions. Plans, proration, invoices and renewal dates stay in your app, because they are app logic.

What the gateway gives you for recurring billing:

- Saved methods. Save a card on the first payment, then create a payment on each renewal date and charge the saved method with `POST /v1/saved-methods/{id}/charge`. See [refunds and saved methods](/guide/refunds-saved).
- Lightning and on-chain, which cannot be pulled. For these, create a payment each period and send the customer its `pay_url`, for example in an email.
- Idempotency keys. Use one per billing period, such as `sub-42-2026-01`, so a retried job never bills twice.
- Webhooks. `payment.paid` tells you to extend the subscription, and `attempt.failed` or `payment.expired` tells you to start your dunning process.

A first-class recurring billing feature is not part of the current API.
