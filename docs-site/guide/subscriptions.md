# Subscriptions and recurring billing

The gateway runs the billing cycle for you. You define plans, subscribe customers to them, and listen to webhooks. The gateway creates one invoice (a payment) per period, charges the customer's saved card, retries failures and tells you about every change. What a subscription unlocks in your product stays in your app.

All endpoints are under `/v1` and use the `payments:read` and `payments:write` scopes. The same screens exist in the console under Plans and Subscriptions.

## Plans

```json
{
  "name": "Pro",
  "amount": 1500,
  "currency": "EUR",
  "interval": "month",
  "interval_count": 1,
  "trial_days": 14,
  "grace_days": 3
}
```

`POST /v1/plans` creates a plan. `amount` is in minor units. `interval` is `day`, `week`, `month` or `year` and defaults to `month`, `interval_count` defaults to 1, `trial_days` to 0 and `grace_days` to 3. Plans are listed with `GET /v1/plans`, read with `GET /v1/plans/{id}` and retired with `POST /v1/plans/{id}/archive`. An archived plan keeps its existing subscribers but cannot be chosen for new ones.

## Subscribing a customer

`POST /v1/subscriptions` with `customer_ref`, `plan_id` and optionally `saved_method_id`.

- With a saved method, the first period is charged at once, or when the trial ends. The subscription is `active` (or `trialing`).
- Without one, the response is `incomplete` and carries `payment_id` and `pay_url`. Send the customer there. If they pay with a card and the payment saves it, the subscription becomes `active` and later periods are charged automatically. If they pay with Lightning or on-chain, it becomes `active` too, but each later period needs a new payment (see below). An unpaid first payment cancels the subscription after a day.

Read one with `GET /v1/subscriptions/{id}`, which includes the billing history in `periods`. List with `GET /v1/subscriptions`, filtered by `customer_ref` or `status`.

## Statuses and transitions

| Status | Meaning |
| --- | --- |
| `incomplete` | Waiting for the first payment |
| `trialing` | In the free trial, first charge at `trial_end` |
| `active` | Paid up, next charge at `next_charge` |
| `past_due` | All retries failed, cancels after a week unless you intervene |
| `paused` | No charges until resumed |
| `cancelled` | Ended, `cancel_reason` says why |

Actions, all `POST`:

- `/v1/subscriptions/{id}/pause` and `/resume`. A resumed subscription whose period already ended is billed again at once.
- `/v1/subscriptions/{id}/cancel` with `{"at_period_end": true}` keeps it running until the period ends, then cancels without charging. Without that field it cancels now and voids any unpaid invoice.
- `/v1/subscriptions/{id}/change-plan` with `plan_id` and `proration` (`none` or `prorate`). The new plan must use the same currency.

## How a period is billed

The billing worker creates one invoice per subscription and period. The invoice is a normal payment with reference `sub_<subscription id>_<period>`, so it shows up in payments, refunds and exports like any other. A database constraint on subscription and period guarantees it is created once, even with several workers running.

Period ends are counted from the start date, so a monthly subscription that starts on 31 January bills on 28 February (29 in a leap year) and on 31 March again.

If charging the saved card fails, the gateway retries 1, 3 and 5 days after the period start. Each retry is a new payment, and a failed one is cancelled, so a customer is never charged twice for a period. When the third retry fails the subscription is `past_due`, and a week later it is cancelled with `cancel_reason` `payment_failed`.

## Methods that cannot be charged automatically

Lightning and on-chain payments cannot be pulled from the customer. When a subscription has no saved card, the gateway creates the period's payment and sends an `invoice.payment_required` event carrying `pay_url` and `grace_ends`. Show the link to the customer, for example in an email. They have the plan's `grace_days` to pay. If they do not, the period fails the same way as a declined card.

## Changing plan and proration

With `proration: none` the new price starts with the next period. With `proration: prorate` the difference for the rest of the current period is settled now:

```
difference = (new_amount - old_amount) * remaining_days / period_days
```

Amounts are integer minor units and the division truncates toward zero. `remaining_days` is the number of days left in the current period, rounded up. `period_days` is the length of the period in whole days. For example, upgrading from 3100 to 6200 with 21 of 31 days left gives 3100 * 21 / 31 = 2100.

A positive difference is charged at once as a separate payment (reference ending in `_change`). A negative difference becomes a credit, shown as `credit` on the subscription and taken off the next invoice. The billing dates do not move.

## Webhooks

Subscription events use the normal [webhook](/guide/webhooks) envelope. `data.subscription` is always present and `data.payment` is the period's payment, or null when there is none.

| Event | When |
| --- | --- |
| `subscription.created` | A subscription was created |
| `invoice.paid` | A period was paid, or a zero amount period was covered by credit |
| `invoice.failed` | A charge failed. `detail` has `failures`, `reason` and `next_retry` |
| `invoice.payment_required` | A period needs the customer to pay `pay_url` |
| `subscription.past_due` | Every retry failed |
| `subscription.cancelled` | Cancelled, with `detail.reason` such as `requested`, `payment_failed` or `setup_expired` |
| `subscription.paused`, `subscription.resumed`, `subscription.updated` | The matching action ran. `updated` carries the plan change and `proration` |

Grant access on `invoice.paid`, show a payment prompt on `invoice.payment_required` and `invoice.failed`, and revoke on `subscription.cancelled`. Endpoint filters such as `invoice.*` and `subscription.*` work. The same events can be pulled from `GET /v1/events`.
