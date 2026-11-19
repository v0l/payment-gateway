# Treasury and reconciliation

The Treasury page, under the operator section of the console, answers one question: can the gateway pay everyone it owes? It is read only. Nothing on it moves money or rewrites a balance.

## What a run checks

A run adds up what the gateway owes (project balances, pending payouts and platform fees) and compares it with what the shared platform providers report holding: the local channel balance of each Lightning node and the confirmed balance of each on-chain wallet. It also checks the ledger itself, that every cached balance equals the sum of its entries and that every transaction sums to zero.

The lead worker runs it every hour. **Run now** on the page starts one on demand. Every run is kept with its assets, liabilities, difference, drift, unbalanced count, stuck count and status, so you can see when a problem began.

## Alerts

A run raises an alert when:

- liabilities exceed assets by more than `GATEWAY_SOLVENCY_TOLERANCE_SATS` (default 0)
- a wallet lookup fails, so solvency could not be verified
- a cached balance differs from its entries
- a transaction does not sum to zero

Alerts are emailed to operators, at most once an hour, and shown as a banner on every console page for operators. They clear on the next run that finds nothing.

## Stuck money

The page lists things that look finished but did not complete:

| Problem | Meaning |
| --- | --- |
| Settled, no ledger credit | A payment settled on a shared provider but the project was not credited. |
| Withdrawal stuck sending | A withdrawal has been `sending` for over an hour. The node outcome is unknown, so the funds stay reserved. |
| On-chain receipt unconfirmed | A receipt has waited more than 24 hours for confirmations. |
| Paid, webhook not delivered | Every webhook for a paid attempt is still undelivered after an hour. |

The same figures are on `/metrics`: `gateway_liabilities_msat`, `gateway_assets_msat`, `gateway_solvency_difference_msat`, `gateway_ledger_drift_accounts`, `gateway_ledger_unbalanced_transactions`, `gateway_stuck_items`, `gateway_withdrawals_pending` and `gateway_fees_earned_msat`. See [Monitoring and alerts](/operator/monitoring) for suggested rules and the [runbooks](/operator/runbooks) for what to do when the ledger does not reconcile.

## Endpoints

| Route | What it does |
| --- | --- |
| `GET /console/api/admin/treasury` | The page: totals, wallets, balances per project, stuck items and the last 50 runs. |
| `POST /console/api/admin/treasury/run` | Runs a reconciliation now and returns it. |
| `GET /console/api/admin/treasury/banner` | The current alerts, used by the banner. |
| `GET /admin/ledger/reconcile` | Admin token API. Lists cached balance mismatches and unbalanced transactions. |
