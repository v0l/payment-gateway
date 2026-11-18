# Console guide

The operator and project console at `/console`. Components and tokens are in the [design language](README.md); the styleguide is at `/console/styleguide`.

## Layout

- Two columns: a 220 px navigation and the main area (max 1100 px, padded 22/26). Under 800 px the navigation stacks above the content. The page never scrolls sideways at 360 px; a wide table scrolls inside `main`.
- The navigation lists every project the account belongs to. The open project expands into its areas; others show "Open". Below it: "+ New project", the Operator section (operators only), then the account email, Account and Sign out.
- The main area starts with the project name (`h2`), a one line `sub` giving role and the "test limits" pill for untrusted projects, then the tab bar, then content.
- Content is a vertical stack of `card`s and `grid` stat tiles. Use `h3` for section headings inside a tab (small, muted, uppercase is the only place all caps is allowed).

## Tab pattern

Tabs are links to `#/p/{id}/{tab}`, so reload and back keep your place. Order is fixed: Overview, Payments, Providers, Webhooks, Balance, Payouts, Team, Activity, Settings. Order by how often it is used, with Settings last. Do not add a tenth tab; group instead. The current tab is `on` and mirrored in the navigation. A tab never changes the project.

## Operator and tenant areas

- Tenant areas are everything under a project. They are what a customer sees and must work for untrusted (test mode) projects.
- Operator areas (Accounts, Audit, limits, trusted flag) sit under the Operator heading in the navigation only when `is_operator`. They never appear inside a project tab for a non operator. Operator only controls inside a project (limits, trusted) go in Settings under a clearly labelled "Operator" card.
- Server side checks gate every operator action. Hiding a control is a courtesy, not security.

## Density

Console density is compact: 14 px text, 8 px cell padding, 12 px card radius, 14 px gaps. Tables show 25 rows. Do not go below 12 px except pills (11 px). Touch targets reach 32 px high on mobile via control padding. Increase spacing, not font size, for emphasis.

## States and behaviour

- First paint is the shell. Fetch failures show `err` text with the reason from the API.
- Destructive actions (delete provider, rotate key, remove member, disable 2FA) use the confirm dialog pattern. Secrets returned once use the `secret` box.
- Polling views stop their timer when the route changes.
- Amounts use `fmtAmount` (Intl currency, sats for BTC). Do not format money by hand.
- Payments list: 50 rows per page. While the API returns a cursor the control is a "Load more" button; at the end it is the muted text "No more payments". Never pass a possibly null value to `replaceChildren`, it renders as the text "null".
- The Method column shows the method of the paid attempt, else of the latest attempt (the list API returns attempts), and "-" when the payment has none yet.
