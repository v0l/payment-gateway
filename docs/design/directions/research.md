# Research: payment and fintech consoles

What well regarded money products do, read from their own docs, help centres, design write-ups and (for Mercury) the public demo app. For each, the patterns worth taking for our console and why. Round 2 directions D, E and F each name the products they borrow from.

## Stripe Dashboard

Sources: [Dashboard basics](https://docs.stripe.com/dashboard/basics), [Payment status updates](https://docs.stripe.com/payments/payment-intents/verifying-status), [Design your app](https://docs.stripe.com/stripe-apps/design), [925 Studios breakdown](https://www.925studios.co/blog/stripe-dashboard-design-breakdown).

- **Home is a short, opinionated set of figures, each with the previous period beside it.** Gross volume, net volume, new customers and successful payments, with a smaller comparison figure under each. It answers "is this good?" without a trip to reports. Home also surfaces unresolved disputes and verifications as notifications, so the things that cost money come first.
- **Colour is reserved for status.** Green succeeded, red failed, amber pending; charts and figures stay monochrome. Hierarchy comes from type size and weight, not colour. This is the cheapest way to make a red row mean "act now".
- **Navigation names jobs, and search spans every object.** Balances, Transactions, Customers; one search box finds a payment, customer or payout without knowing where it lives. `?` lists keyboard shortcuts. Detail pages, list pages and home are distinct surfaces, and the raw object (PaymentIntent JSON) is one click away for developers.

## Mercury

Sources: [mercury.com](https://mercury.com/), the public demo at [demo.mercury.com/dashboard](https://demo.mercury.com/dashboard) (inspected in a headless browser).

- **The balance is the hero, with money in and out for the period beside it.** Home opens on "Mercury balance $5,216,471.18" with "Last 30 days $1.8M − $490K", a graph or table toggle, and each account with its own balance in a list to the right. Cents are set smaller than the whole units, so large figures read at a glance.
- **Actions sit next to the money.** Send, Transfer, Deposit, Request and a global "Move money" button live on home and in the top bar, not buried in a section.
- **The list stays put while you inspect a row.** On Transactions a one line summary (net change, money in, money out) sits above the table, filters are inline buttons (Date, Keyword, Amount), and clicking a row opens a detail panel on the right with the list still visible. Status such as "Failed" is small coloured text beside the counterparty, not a pill. Type is a custom grotesk at a light display weight (380) with near-black ink `rgb(30,30,42)`.

## Ramp

Sources: [Introduction to Homepage](https://support.ramp.com/introduction-to-homepage), [Reviewing transactions](https://support.ramp.com/reviewing-transactions-from-ramp-cards).

- **Home is a task feed.** The homepage replaced the old Inbox: approvals, missing items and drafts first, then spending insights, then FYIs (payment received, balance updates). Sections collapse and stay collapsed.
- **A right hand rail keeps funds and pending objects in reach.** Accessible funds (up to five) and pending reimbursements or POs sit in a persistent side column, so the feed can stay about work.
- **Review in bulk, grouped by who or what.** Items are grouped (by trip, card, cardholder) with checkboxes per row and per group for bulk approve; flagged items move to their own sub tab. Grouping disappears once you filter or sort.

## Brex

Sources: [Manage your business account balance](https://www.brex.com/support/manage-business-account-balance), [Expense management](https://www.brex.com/product/expense-management).

- **Two balances, named precisely.** "Total balance" includes processing transfers; "available balance" is what can actually be moved now. For a gateway this maps to settled versus waiting for card settlement or confirmations.
- **Transaction tables carry a running balance column.** Each account's history shows the balance after each line, which makes reconciliation a reading task instead of arithmetic.
- **Managers only see what needs a look.** In-policy spend is approved automatically and only flagged items reach a person, with reasons attached.

## Linear

Sources: [How we redesigned the Linear UI](https://linear.app/now/how-we-redesigned-the-linear-ui), [A calmer interface for a product in motion](https://linear.app/now/behind-the-latest-design-refresh).

- **Chrome recedes, content leads.** The "inverted L" of sidebar and header is dimmed a few notches so the working area wins; icons were reduced and shrunk; tabs made compact.
- **"Structure should be felt, not seen."** Fewer separators, softer border contrast. Density stays high without looking crowded.
- **One layout system, several views.** List, board, split and full screen share a header for filters and display options and a side panel for properties. Themes are generated from three values (base, accent, contrast) in LCH so light, dark and high contrast stay consistent.

## Vercel

Sources: [Geist colours](https://vercel.com/geist/colors), [Managing deployments](https://vercel.com/docs/deployments/managing-deployments), [Observability](https://vercel.com/docs/observability).

- **A ten step neutral scale with fixed roles.** Steps 1 to 3 are component backgrounds (default, hover, active), 4 to 6 borders, 7 to 8 high contrast fills, 9 to 10 text. Two page backgrounds only, the second used sparingly. "Draw each border once."
- **Project home is the latest activity.** A project opens on its production state and recent deployments; lists filter by branch, date range, environment and status from dropdowns in one row.
- **Charts are investigative.** Drag to zoom a time range, then re-sort the list below by error rate or duration and click through to the logs for that row.

## Adyen Customer Area

Source: [Manage payments](https://docs.adyen.com/account/manage-payments).

- **The payment list is a reference table.** PSP reference, merchant reference, account, date, amount, method, status and risk score; search takes field syntax such as `cardNumberSummary:1111`.
- **The payment detail page is in sections that answer operator questions.** Payment lifecycle (status history), payment events (with the network reference to trace a refund), processing (fraud score, 3D Secure, liability shift) and transaction cost (fees). Shopper details are masked unless the role allows.

## Paddle

Sources: [Reimagining data tables](https://paddle.engineering/blog/data-tables-paddle-design-system/), [developer.paddle.com](https://developer.paddle.com/).

- **Tables are the product.** Transactions columns: product, customer email, amount, method, date paid, status, then an action menu pinned to the end of the row so it is reachable however many columns there are.
- **Hover spans the full page width and a fade shows more columns.** Small cues that make dense tables usable on a laptop, a tablet and a phone; keyboard arrow navigation between cells.

## Lemon Squeezy

Sources: [Getting paid](https://docs.lemonsqueezy.com/help/getting-started/getting-paid), [Orders](https://docs.lemonsqueezy.com/help/orders).

- **The payout schedule is stated in plain dates.** Payouts are created on the 1st and 15th, sales are held 13 days, and a payout under the threshold shows as Pending and rolls over. A merchant can predict when money lands.
- **Order actions are few and concrete.** Refund, resend receipt, generate invoice, export.

## BTCPay Server

Sources: [Dashboard](https://docs.btcpayserver.org/Dashboard/), [Invoices](https://docs.btcpayserver.org/Users/invoices/), [1.6.0 release](https://blog.btcpayserver.org/btcpay-server-1-6-0/).

- **Balance with a BTC and fiat toggle, and Lightning liquidity shown as local, remote and closing.** The dashboard leads with wallet balance over week, month or year, then recent invoices with status.
- **Statuses say what to do.** New, Processing (paid, waiting for confirmations), Settled, Expired, Invalid, each with a "typical action"; exceptions (paid partial, paid over, paid late, marked) sit beside the status. An "Unusual" filter finds every exception at once.
- **Invoice detail keeps status history, payments, metadata and refunds together**, and warns that a manual mark does not prove a payment confirmed.

## Zaprite

Source: [zaprite.com](https://zaprite.com/) (product screens on the site).

- **One order view with a dated event list.** "Invoice created", "sent to customer", "payment received", "Bitcoin deposit confirmed", each with a timestamp, under the amount and status.
- **Payment methods are connections you switch on per checkout**, and webhooks show delivery counts such as "98/100" next to each endpoint.

## Strike

Sources: [Strike API docs](https://docs.strike.me/), [Strike for business](https://strike.me/business/).

- **Settlement time is part of the method.** Lightning settles instantly; on-chain takes about 10 to 60 minutes; no chargebacks once delivered. The console should say when money will be final per method, not one generic "pending".
- I could not find public screens of Strike's business dashboard, so nothing visual is borrowed from it.

## What the three directions take

| Direction | Leads with | Borrows from |
|---|---|---|
| D, Activity | Today's volume against last week, then the live activity feed | Stripe home, Vercel project home and tabs, Mercury list with side panel |
| E, Balance | Available balance by wallet, next payout, ledger with running balance | Mercury home, Brex balances, Lemon Squeezy and BTCPay payouts |
| F, Triage | A queue of payments that need someone, in dense grouped tables | Ramp homepage and inbox, Linear chrome and density, Adyen and Paddle tables, BTCPay statuses |
