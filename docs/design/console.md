# Console guide

The operator and project console at `/console`. Components and tokens are in the [design language](README.md); the styleguide is at `/console/styleguide`.

## Layout

- No sidebar. A 56 px top bar holds the project switcher, the Live or Test mode tag, search (Ctrl K or `/`), the Operator menu (operators only) and the account menu. Under it, the project tabs: Home, Payments, Customers, Billing, Balance, Developers, Settings. Tabs with more than one area show a second row: Billing (Subscriptions, Plans, Platform bill), Balance (Balance, Payouts), Developers (Payment methods, API keys, Webhooks), Settings (Settings, Team, Activity).
- Home carries a red count of open attention items. It refreshes when Home loads and when an action on the queue finishes.
- Content is at most 1440 px wide, padded 28 px. Under 900 px the tabs scroll sideways, the right rail and the payment side panel drop away or below, and wide tables become stacked lists. Nothing scrolls the page sideways at 390 px.
- Operator pages (Projects, Accounts, Shared providers, Platform plans, Treasury, Payout queue, Audit) use the same bar without project tabs.

## Home

1. **Needs attention.** `GET /console/api/projects/{id}/attention`. Groups, each with a tone bar and count: Disputes, Paid late or short, Waiting for confirmations, Declined, Refunds that failed, Webhook endpoint failing, Payment method switched off, Subscriptions past due, Payouts that failed. Every row has a reason, a when, an amount and one next step. Rows whose action the API supports in bulk have a checkbox: Paid late can be accepted as paid (only when manual payment handling is on in Settings, with a reason), Declined can be cancelled (`POST .../attention/bulk`). Payments that only wait for the customer are one quiet line under the table. With nothing open the queue is a single line.
2. **Today.** Paid so far today against the same time on the same day last week, as a cumulative line by hour (`GET .../home?tz=` with the browser's offset in minutes).
3. **Activity.** `GET .../feed?kind=all|payments|refunds|payouts|webhooks&before=`. Paid, created, declined, refunds, disputes, payouts and failed webhooks, newest first. Failed webhooks to one endpoint are one row.
4. **Right rail.** Funds (balance held by the platform, settling, in payouts, latest payout, each wallet and what is still settling), this month against the same days last month, and payment method health from `provider_health`.

## Payments

Views with counts (`GET .../payment-view-counts`): All, Waiting, Paid, Refunded, Expired, Last 7 days, then saved views. A filter row (search, status, method, dates), more filters in a disclosure. The table shows amount, status, description, customer, method, when the money is available, and created. Selecting a row fills the side panel; Open payment goes to the detail page. Export CSV, Cancel stale and New payment sit in the page head. 50 rows per page, then Load more.

## Payment detail

Header with the amount, status, dispute state and actions (Copy pay link, Cancel, Refund). Then: where the money went, timeline (attempts, events from `gateway_event`, webhook deliveries with their response), line items, attempts, network receipts, webhooks, cost, customer (email masked as `am***@domain`), metadata, notes, and the payment as the API returns it. A section index on the left at 1180 px and wider, properties on the right.

## Balance

Available to withdraw, settling and in payouts, a bar by wallet and the wallet table. Balance changes come from `GET .../statement`, one line per ledger entry on the project's platform balance with the balance after it and the payment it belongs to. The right column has payouts on the way, recent payouts, limits and when each method's money becomes available. Payouts (destinations, requests, history) is the second sub-tab.

## Operator and tenant areas

- Tenant areas are everything under a project. They are what a customer sees and must work for untrusted (test mode) projects.
- Operator areas sit in the Operator menu only when `is_operator`. Operator only controls inside a project (limits, trusted) are in Settings under "Operator".
- Server side checks gate every operator action. Hiding a control is a courtesy, not security.

## Density

Body 14 px, table rows 40 px, header rows 34 px, 8 px radius on tables, 6 px on controls. Increase spacing, not font size, for emphasis.

## States and behaviour

- First paint is the shell. Lists show skeleton rows while they load. Fetch failures show `err` text with the reason from the API.
- Destructive actions (delete payment method, rotate key, remove member, disable 2FA, cancel payments) use `confirmDialog`. Secrets returned once use the `secret` box.
- Polling views stop their timer when the route changes.
- Amounts use `fmtAmount` (Intl currency, sats for BTC). Do not format money by hand.
- Never pass a possibly null value to `replaceChildren`, it renders as the text "null". Filter the list with `.filter(Boolean)`; a test checks every script.
- Every function a script calls must be defined in a console script; a test checks this.

## Screenshots

`scripts/e2e/console_demo.sh` seeds a demo project through the service and serves it on port 18760. `node scripts/e2e/console_shots.mjs` (with `CHROME` pointing at a Chromium) signs in and saves every page at 1440 and 390 to `docs/design/screenshots/console-v4/`, failing on axe serious issues, horizontal scroll, console errors or "null" text. `python3 scripts/e2e/compare.py . docs/design/screenshots/console-v4/compare` builds the side by side sheets. `scripts/e2e/console_demo.sh --stop` removes the demo database.
