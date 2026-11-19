# Using the console

The console at `/console` is where you run a project day to day. It has a top bar with a project switcher, a search box (press Ctrl K) and your account menu, and tabs underneath. Everything the console shows is also reachable by API, and every change you make is written to the project's Activity log.

## Home

Home opens on **Needs attention**: the things that need a person, grouped by what to do about them.

- Disputes the customer has opened with their bank.
- Payments that arrived late or short. Accept a late payment, or decide what to do about a short on-chain one.
- On-chain payments waiting for confirmations.
- Payments whose last attempt was declined and where the customer has not tried again.
- Webhook endpoints that are failing, payment methods the gateway switched off after repeated errors, subscriptions that are past due and failed payouts.

Below the list, a single line says how much is only waiting for customers to pay, so you know nothing is hiding there. Select rows to cancel several payments at once, or to mark late payments as paid. The second action needs manual payment handling turned on in Settings and a reason of 3 to 500 characters.

The rest of Home is today's paid total against the same hour last week, an activity feed you can filter by payments, refunds, payouts or webhooks, and a side rail with your funds, this month against the same days last month, and the health of each payment method.

## Payments and saved views

Payments has ready-made views: all, pending, paid, refunded, disputed, expired, cancelled and the last 7 days, each with a count. Filter by status, method, customer, date range, amount or a reference prefix. When you have a filter you use often, choose **Save view** to keep it under a name. Saved views belong to your account and to the project, and you can keep up to 20 of them. Export the current filter as CSV from the same page.

The payment detail page shows the timeline, the attempts, the webhook deliveries with what your server answered, internal notes, and where the money went. From it you can refund, resend the webhook or, with manual handling on, mark the payment as paid.

## Balance

Balance is for projects that use platform payment methods, where the gateway holds the money until you withdraw it. It shows what is held for you, what is still settling and your latest payout, with a statement underneath. Each statement line is a payment, a refund, a fee or a payout, with the balance after it and a link to the payment behind it. Payouts, destinations and withdrawal requests are on the Payouts tab next to it. See [Custody and payouts](/guide/custody).

## Endpoints behind these pages

These routes are under `/console/api` and use your signed-in session, not an API key. They are listed here for people scripting against the console. The stable integration surface is [`/v1`](/guide/reference).

| Route | What it returns or does |
| --- | --- |
| `GET /projects/{id}/home?tz=` | Today against last week by hour, this month against last month, funds and method health. `tz` is minutes east of UTC. |
| `GET /projects/{id}/attention` | The Needs attention groups and the count of payments only waiting for customers. |
| `POST /projects/{id}/attention/bulk` | `{action: "cancel" or "mark_paid", payments: [...], reason}`, 1 to 50 payments, admin role. Returns what was done and what failed. |
| `GET /projects/{id}/feed?kind=&before=` | The activity feed. `kind` is `all`, `payments`, `refunds`, `payouts` or `webhooks`. |
| `GET /projects/{id}/payment-view-counts` | Counts for the ready-made payment views. |
| `GET` and `POST /projects/{id}/saved-views`, `DELETE /projects/{id}/saved-views/{view}` | Saved payment filters, 20 per account and project. |
| `POST /projects/{id}/payments-bulk-cancel` | `{older_than_hours}` cancels stale pending payments, 200 per call. |
| `GET /projects/{id}/payments-export` | The current payment filter as CSV. |
| `GET /projects/{id}/balance` and `GET /projects/{id}/statement?before=&limit=` | The balance and the statement, 1 to 200 lines per page. |

Viewers can read, admins can change, and only members of the project can reach its routes.
