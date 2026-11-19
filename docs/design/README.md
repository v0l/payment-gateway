# Design language

One language for the console, the hosted pay page and email. Tokens live in `web/tokens.css`. The living reference is `/console/styleguide` (static, CSP safe, shows every component in light and dark). Surface guides: [console](console.md), [pay page](pay-page.md), [email](email.md). The round 1 and round 2 directions behind the console are in [directions](directions/round2/README.md), with the product research in [research.md](directions/research.md).

## Console identity (v4)

Subject: a payment gateway console for merchant developers and the platform operator. They read amounts, statuses and ids all day, in sats, EUR and BTC, across Lightning, on-chain, card and bank.

Direction: a blend of the three round 2 directions, chosen by the user. The shell is D (a top bar with project tabs and search, no sidebar). Home leads with F's "Needs attention" queue, then D's today against last week chart and activity feed, with a right rail of E's funds, D's month figures and F's payment method health. Payments is F's views over D's table and side panel, with E's "available" column. Payment detail is F's sections, D's timeline and API object, and E's "where the money went". Balance is E's page.

### Palette

| Name | Light | Dark | Role |
| --- | --- | --- | --- |
| Page | `#fbfbfc` | `#12141b` | page |
| Surface | `#ffffff` | `#181b24` | top bar, tables, cards, inputs |
| Rail | `#f4f5f7` | `#15171f` | table heads, group rows |
| Ink | `#1e2130` | `#e6e8ef` | text |
| Blue | `#1f3a8a` | `#9db0f0` | primary action, links, focus, wallet ramp |

Colour otherwise means state only: green paid, amber waiting, red needs action, blue informational. Charts are ink and grey.

### Type

- IBM Plex Sans (self-hosted, 300, 400, 500, 600) for everything. The balance and payment amounts use 300 at 40 to 44 px with the cents set smaller, so a figure reads as a figure rather than a heading.
- IBM Plex Mono (self-hosted) only for ids, invoices and the raw API object.
- Scale: 12, 13, 14, 15, 18, 22, 28, 44.

### Layout

```
[project v] Live          [Find a payment, customer or reference  Ctrl K]  Operator v  (SA)
Home 6   Payments   Customers   Billing   Balance   Developers   Settings
--------------------------------------------------------------------------------------------
Needs attention                                            | Funds
+--------------------------------------------------------+ | 2,958,424 sats
| [ ] Disputes 1                                          | | Settling, latest payout
|     Dedicated host  Customer disputed  Opened  €149 Review| | wallets with what is settling
| [ ] Paid late or short 2                                | |
|     ...                                    Accept  Decide | | This month
| 1 for €18.00 waiting for customers. Show them           | | Paid €6,412  €2,545 +152%
+--------------------------------------------------------+ |
Today  Paid so far €405.00  €0.00 by this time last week   | Payment methods
~~~~~ today vs same day last week ~~~~~                    | • Lightning node, working
Activity                    [All|Payments|Refunds|Payouts|Webhooks]
01:42  • €18.00 paid           Lightning
```

Left aligned, amounts right aligned in tables. Under 900 px the rail drops below the main column, the tabs scroll sideways, the attention table and payments table become stacked lists, and the payment side panel gives way to the full detail page.

### Defaults avoided

- No sidebar, no navy rail, no stat tile row, no identical rounded cards. Structure is tables, ruled lists and one sticky rail.
- No all caps labels, eyebrows, middle dot joins, arrows on links or gradients.
- `--faint` is for dots and marks only, never text. Every text pair passes AA in both themes.

### Critique notes

Composites of v3, the round 2 mockups and the built page are in `docs/design/screenshots/console-v4/compare/`. Screenshots of every page at 1440 and 390 are in `docs/design/screenshots/console-v4/`, made by `web/scripts/console-shots.ts` against `examples/console_demo.rs`. Each run checks axe (WCAG 2.1 AA), horizontal scroll, console errors and stray "null" text on every page at both widths. Fixes from that review:

- Home: the queue had a "Needs attention" section title under a "Home" page title, so the queue now owns the page head. The today chart got its own "Today" heading. Webhook failures for one endpoint were 30 identical feed rows, now one row per endpoint.
- Attention times were formatted in UTC on the server. The API now sends a label and a timestamp, and the browser formats it in local time ("Paid yesterday 17:18").
- Balance: providers that pay you directly showed "Paid to you directly" wrapped over three lines in the wallet table. They now show what is settling and where it goes.
- Payment detail: refund and webhook rows sorted by time alone, so equal timestamps flipped. Ties keep the order they happened in. The "where the money went" table no longer overflows at 390.
- Form labels were not tied to their inputs. `field()` now sets `for`.
- The Operator label crowded the search box at 390; on phones only its chevron shows.
- Endpoint health returned 500 once any delivery had a duration (a DECIMAL average read as a float). Fixed with a regression test.

## Principles

The product handles other people's money, so it should feel like a good bank statement: calm, exact and quiet.

1. **Numbers first.** Amounts, statuses and ids are the content. They use tabular figures, never wrap mid-value and never share a line with decoration.
2. **What needs someone comes first.** Home opens on the queue of things a person must decide, and collapses to one line when nothing is open.
3. **Say what happened.** Every state names its cause and the next step. Errors do not apologise.
4. **No surprises with money.** Destructive and irreversible actions ask first, name what will be lost, and use the same verb on the button as in the dialog title.
5. **Works everywhere.** 390 px wide, keyboard only, light or dark, reduced motion, no external fonts, scripts or images.
6. **Borders over shadows.** Structure comes from 1 px lines. Shadow is reserved for things that float (menus, dialogs, toasts).

## Tokens

All tokens are CSS custom properties in `web/tokens.css`. Dark values apply with `prefers-color-scheme: dark`, or with `data-theme="dark"` / `data-theme="light"` on any element to force one (the styleguide uses this). Never write a colour literal in `console.css`, `pay.css` or `components.css`; a test enforces it.

### Colour

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--bg` | `#fbfbfc` | `#12141b` | page |
| `--card` | `#ffffff` | `#181b24` | top bar, tables, cards, inputs, dialogs |
| `--rail` | `#f4f5f7` | `#15171f` | table heads, group rows |
| `--code` | `#f1f2f5` | `#20232e` | code, pills, skeletons |
| `--hover` | `#f4f5f8` | `#1e212b` | hovered rows and menu items |
| `--text` | `#1e2130` | `#e6e8ef` | body |
| `--muted` | `#5d6375` | `#a4a9b8` | hints, metadata, table heads |
| `--faint` | `#8a8fa0` | `#7c8293` | dots and marks only, never text |
| `--line` | `#e3e5ea` | `#2a2e3a` | dividers |
| `--line-strong` | `#767c8f` | `#7a8092` | borders of inputs (3:1 needed) |
| `--accent` / `--accent-text` | `#1f3a8a` / `#ffffff` | `#9db0f0` / `#10152a` | primary action, links |
| `--accent-soft` | `#e8ecf8` | `#232b45` | selected row |
| `--focus` | `#2f5bd3` | `#9db0f0` | focus ring |
| `--ok`, `--ok-bg` | `#16794a`, `#e7f3ec` | `#6cc897`, `#16291f` | paid, completed, delivered |
| `--warn`, `--warn-bg` | `#8f5300`, `#fbf1dd` | `#e6b45c`, `#2f2512` | waiting, needs a look, one time secrets |
| `--bad`, `--bad-bg` | `#b42318`, `#fbe9e7` | `#f08a80`, `#351a17` | failed, disputed, destructive |
| `--info`, `--info-bg` | `#1f5b8a`, `#e5eff7` | `#7fb4e6`, `#15263a` | processing, approved |
| `--series-a`, `--series-b` | ink, grey | ink, grey | today and last week lines |
| `--wallet-1` to `--wallet-5` | blue ramp | blue ramp | wallet shares on Balance |

Rule: never convey state by colour alone. A pill always carries its status word.

### Contrast (WCAG AA)

`bun web/scripts/contrast.ts` reads `web/src/tokens.css`, checks every text and control pair in both themes and exits non zero on failure. Text pairs need 4.5:1, control borders, focus ring and `--faint` marks 3:1. Every pair passes in both themes.

### Type

Self-hosted fonts served from `/console/fonts/` (CSP `font-src 'self'`, no CDNs), with system fallbacks.

- `--font-sans`: IBM Plex Sans, then `system-ui`.
- `--font-mono`: IBM Plex Mono, then `ui-monospace`, for ids, invoices, codes, addresses and the API object only.

Scale: `--fs-xs` 12, `--fs-sm` 13, `--fs-base` 14, `--fs-md` 15, `--fs-lg` 18, `--fs-xl` 22, `--fs-figure` 28, `--fs-display` 44. Line height 1.5 for text, 1.2 for headings. Weights: 300 big figures, 400 body, 500 tabs and buttons, 600 headings and amounts in tables. Amounts use tabular figures. The pay page keeps its 16 px body.

### Space, radius, shadow

- Space: 4, 8, 12, 16, 24, 32, 48 (`--space-1` to `--space-7`).
- Radius: 4 pills and kbd, 6 controls, 8 tables, cards and menus, 12 dialogs, pill for chips and the count badge.
- Shadow: `--shadow-2` menus and search results, `--shadow-3` dialogs. Nothing else floats.

### Motion

`--dur-fast` 120 ms for hover and focus, `--dur` 200 ms for open and close, easing `--ease`. Motion answers an action; nothing animates on load. The loading skeleton is the only looping animation. `tokens.css` makes all motion instant under `prefers-reduced-motion: reduce`.

### Breakpoints

- 390 px: the phone target. Nothing may scroll the page sideways. Wide tables scroll inside their own region, which is focusable and labelled.
- 900 px: the rail and side panels drop below or away, tables with many columns become stacked lists.
- 1180 px: the payment detail section index hides.
- 1440 px: console content max width.
- Pay page is a single 460 px column at every width.

Focus: every interactive element shows a 2 px `--focus` outline with 2 px offset on `:focus-visible`. Never remove it.

## Components

Classes are in `console.css` and `components.css`; helpers in `web/console/core.js` and `shell.js`. See the styleguide for each.

- **Shell.** `layout(active, projectId, content)` builds the top bar (project switcher, Live or Test mode, search, Operator menu, account menu) and the tab row. `projectShell(pid, tab, build, opts)` adds the page head. `operatorPage(active, title, sub, content)` for pages outside a project.
- **Search.** Ctrl K or `/` focuses it. Inside a project it finds payments by reference or id and customers by ref; operators also search every project once the term is 6 characters or more. Arrow keys move through results, Enter opens the first.
- **Page head.** `pageHead(title, sub, actions)`: 22 px title, one muted line, actions on the right. One primary action per page.
- **Buttons.** `primary` for the thing most people do next, default for secondary, `danger` for destructive, `link` for row actions. Disabled for viewers instead of hidden.
- **Inputs.** Built with `field(label, input)`, which ties the label to the input. Label above, hint below.
- **Tables.** `table(head, rows, opts)`: header row on `--rail`, 40 px rows, amounts right aligned with tabular figures, row dividers only. Clickable rows use `clickRow`, which opens on click and Enter. Group rows (`tr.group`) carry a coloured tone bar and a count.
- **Status pills.** `status(apiStatus, word)` maps the API status to a word ("Waiting", "Part refunded") and a tone. Red only for something broken or needing action; expired and cancelled stay neutral.
- **Money.** Integer minor units everywhere, BTC in msats. `fmtAmount` for display, `bigMoney` for the large figures with smaller cents, `signedMoney` for ledger lines (green in, red out with a real minus sign), `sats` for BTC amounts.
- **Chips.** `chips(options, current, onpick, label)`: pill toggles with `aria-pressed`, for feed filters, subscription status and webhook views.
- **Empty states.** A bold sentence saying what is missing and one line on how to change it.
- **Confirm dialogs.** `confirmDialog({title, body, keep, confirm, danger, input})` for anything destructive or irreversible. Focus starts on the safe button, Escape cancels. Browser `confirm()` is not used anywhere.

## Voice and tone

- Plain words. "Payment methods", not "providers", in what customers read. "Waiting", not "pending". "Turn off", not "disable".
- Sentence case everywhere, including buttons and headings. No exclamation marks.
- Active voice. Buttons are verb plus object. The same action keeps its name through button, dialog and toast.
- Errors state the cause and the fix and do not apologise.
- No em dashes or en dashes anywhere a person reads.
- Times: today shows the time, yesterday "Yesterday 17:18", older "6 Nov, 17:18", always in the browser's time zone.
