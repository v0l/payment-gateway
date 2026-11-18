# Console directions, round 2

Round 1 (A statement, B terminal, C lanes) was rejected as themed. These three are plain product consoles built from what well regarded payment and fintech dashboards do; see [research.md](../research.md). Same Northwind Hosting data and the same three pages in each. Regenerate with `node gen/d.mjs` (and `e`, `f`) from `docs/design/directions`, then `sh gen/shoot2.sh`. Fonts are self hosted in each folder.

Screenshots: `{d,e,f}-{overview,payments,payment}-{1440,390}.png` in this folder.

## D, Activity

What happened today, as it happens. For the developer who opens the console to check that payments are flowing.

Borrows from Stripe (home figures with the previous period beside each, colour only for status, raw API object on the detail page), Vercel (project tabs under a top bar, neutral scale with fixed roles) and Mercury (the list stays visible while a row opens in a side panel).

- Palette: white `#ffffff`, wash `#f7f7f8`, line `#e4e4e7`, ink `#1f1f24`, muted `#6b6b76`. Colour only for status: green `#1d7a46`, amber `#a35a00`, red `#c3302f`.
- Type: Geist 400, 500 and 600. 14 px body, 24 px titles, figures in 600 with tabular numbers.
- Navigation: project bar with search, then a row of tabs. No sidebar.

```
[project  Live]                     [search /] [SK]
Overview  Payments  Customers  Balance  Payouts ...
---------------------------------------------------
Today                                [Create link]
Paid so far €618.00  vs €590.00 last Wed  +4.7%  | This month
~~~~~~ today vs last Wednesday, by hour ~~~~~~~~  | Paid       €14,236.40
                                                  |            €11,864.00 +20%
Activity            [All|Payments|Refunds|Hooks]  | Success rate  93.1%
21:46  • €18.00 paid               Lightning      | ...
21:31  • €192.00 paid              On-chain       | Needs action
20:41  • Webhook failed            Webhook        | Dispute, respond by 24 Nov
```

Payments: amount first table with filters in one row, the selected row opens a side panel (status steps, key facts). Detail: amount and status, a strip of five facts, then timeline, breakdown and webhook deliveries, with properties and the API object to the right.

## E, Balance

Where the money is and when it can leave. For the merchant who thinks in balances and payouts.

Borrows from Mercury (balance as the hero with cents set smaller, Move money in reach, wallets in the sidebar), Brex (available versus total balance, a running balance column), Lemon Squeezy and BTCPay (payout schedule in plain dates, BTC and fiat side by side).

- Palette: page `#fbfbfc`, sidebar `#f1f2f5`, ink `#1e2130`, muted `#646a7d`, line `#e3e5ea`, primary `#1f3a8a` with a four step blue ramp for wallets. Green `#16794a` for money in, red `#b42318` for money out.
- Type: IBM Plex Sans. The balance is 56 px at weight 300, so the figure reads as a figure rather than a heading. 15 px body.
- Navigation: light left sidebar with Move money, the sections, then each wallet with its balance.

```
| Northwind     | Wed 18 Nov, live              [Add funds][Withdraw]
| [Move money]  | Available to withdraw       | Wallet          Available
| Home          | €25,569.40                  | ■ Lightning      €9,842.10
| Payments 291  | €3,120 settling  €28,689 tot| ■ On-chain       €7,310.60
| ...           | [=====|====|===|=|=]        | ■ Card           €4,906.30
| Wallets       |                                               
| Lightning €9k | Today's balance changes     | Next payout
| On-chain  €7k | 21:46 pay_7Q4M2X  +€18  €25,569 | €20,000.00 Fri 20 Nov
| Card      €4k | 19:20 Refund     −€149  €25,259 | Recent payouts
```

Payments: grouped by day, with an "In your balance" column saying when each paid payment can be withdrawn. Detail: a narrow page that leads with "where the money went" (paid, fees, added to which wallet), then history and details side by side.

## F, Triage

What needs someone. For the operator who handles disputes, partial payments and failing webhooks.

Borrows from Ramp (home as a task feed with grouped items and bulk actions, a right rail of funds), Linear (chrome that recedes, compact header, dense rows), Adyen (detail page in sections that answer operator questions, masked customer data), Paddle (data tables with a pinned row menu) and BTCPay (statuses with a typical action, an Unusual view).

- Palette: canvas `#f6f7f8`, surface `#ffffff`, ink `#23252b`, muted `#6c707a`, line `#e3e5e8`, accent `#2f5bd3` for actions. Status red `#c42b1c`, amber `#9a6700`, green `#1a7f37`.
- Type: Source Sans 3 at 14 px with 36 px rows. Monospace only for API event names.
- Navigation: one 48 px bar with text links (Home carries the count of open items) and Ctrl K search. A right rail on the overview.

```
Northwind Live  Home 6  Payments  Customers ...         [Find   Ctrl K]
Needs attention  6 items                        [Assign][Mark handled] | Funds
[x] 2 selected                     [Accept both][Request the rest]     | Available €25,569
   Payment            Method  Why it is here      Due      Amt  Next   | Settling   €3,120
 | Disputes 1                                                         | Today
[ ] Dedicated host    Card    Not received   Respond by 24 Nov  Respond | Paid €618
 | Paid the wrong amount or late 2                                    | Providers
[x] VPS Frankfurt-2   On-chain  Paid partial  Expired 16:02  Request  | • Lightning node
 9 payments waiting for the customer, €1,420.00. Nothing to do. Show   | • Revolut token
For your information                                                   |
```

Payments: saved views with counts (All, Waiting, Unusual, Refunded, Disputed), filter chips, a dense table with checkboxes and a row menu pinned to the right. Detail: a section index on the left, then lifecycle, attempts, events, webhooks, cost, customer (masked) and metadata, with properties and related records on the right.

## Critique against the skill's generic defaults

- No identical rounded card grid and no stat tile row in any of them. D puts monthly figures in a ruled list beside the chart, E puts wallets in a table beside the balance, F puts counts on the tabs.
- No all caps labels, no eyebrows above headings, no middle dot joins, no arrows on links, no gradients.
- Monospace appears only where it is literally code: D's API object, F's event names.
- Each one spends its boldness once: D the today against last week line, E the light 56 px balance, F the grouped queue.
- Swap test: D and F could not trade palettes and look alike, because one is a tabbed, airy activity page and the other a compact table tool with a rail; E is the only one with a sidebar and the only one that leads with money held rather than money moving.
- Fixed during review: E's wallet breakdown first read as five equal tiles and became a table; F's event names and IDs in monospace read as a template tell and were reduced to the event names; mobile layouts for D's search, E's history and F's detail tables no longer overflow.
