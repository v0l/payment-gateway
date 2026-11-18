# Refunds and saved methods

## Refunds

Refund a paid payment with `POST /v1/payments/{id}/refund`. The key needs the `refunds:write` scope. With no body the remaining amount is refunded. A partial refund sends `amount` in minor units of the payment currency, and several partial refunds can add up to the payment amount but not beyond it.

```sh
curl -X POST "$GW/v1/payments/$ID/refund" \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"amount":500,"destination":"lnbc..."}'
```

Lightning and on-chain refunds need a `destination` (an invoice or an address) because the gateway cannot know where the payer's funds should go. Card refunds go back to the card. The payment becomes `partially_refunded` and then `refunded`. A refund that the provider rejects is recorded as failed and can be retried. You receive `refund.completed` or `refund.failed`. List refunds with `GET /v1/refunds`.

## Saved methods

A customer can save a card so later payments are charged without them present.

1. Create the payment with a `customer_ref`.
2. Create the attempt with `"save_method": true`. The customer completes checkout once.
3. The saved method appears in `GET /v1/saved-methods?customer_ref=...`.
4. Charge it with `POST /v1/saved-methods/{id}/charge` and a normal create-payment body. The payment settles at once, or is cancelled when the charge fails.
5. Remove it with `DELETE /v1/saved-methods/{id}`.

Saving and charging need the `saved:write` scope. A saved method can only be used for the customer it was saved for.
