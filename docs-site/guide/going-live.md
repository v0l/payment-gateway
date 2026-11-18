# Going live checklist

1. **Test mode first.** Build against a `pgw_test_` key. Test projects have low limits and only send webhooks to public https hosts.
2. **Limits.** Read your limits in the console: payments and volume per day, pending payments, amount per payment, providers, webhook endpoints, attempts per payment. A breach returns 403 `limit_exceeded` with the limit name, `current` and `max`. Ask the operator to lift them when you are ready.
3. **Providers.** Add each provider with its production credentials and make a small real payment through every method you plan to offer. Credentials are write-only: the console shows `set` and never returns them.
4. **Webhooks.** Add your https endpoint, store the signing secret, verify signatures and the timestamp, deduplicate on the event id and answer 2xx quickly. Send a test event from the console.
5. **Idempotency.** Send an `Idempotency-Key` on every `POST /v1/payments` and every refund call you may retry.
6. **Confirm server side.** Fulfil orders from the webhook or a `GET`, never from the browser redirect.
7. **Origins.** Set `allowed_origins` and the return URLs to your production site only.
8. **Keys.** Use scoped keys: a key that only creates payments does not need `refunds:write`. Rotate any key that was pasted anywhere it should not be.
9. **Rate limits.** Handle 429 by waiting for `Retry-After`. Bodies over 1 MiB get 413.
10. **Reconcile.** Poll `GET /v1/events` or list payments as a safety net for missed webhooks.
