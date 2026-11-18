# Runbooks

## Payments are paid at the provider but stay pending

1. Check `GET /status`: is a replica holding the lease, and is the database `ok`?
2. Check `gateway_watcher_up{provider}`. A watcher that is down means the LND stream or poller stopped. Look in the logs for the provider error. Restarting the leader makes watchers resume from their cursors.
3. For LND, confirm the macaroon still works and the node is synced.
4. After the watcher is back it reconciles pending attempts on its own.

## Webhooks are not reaching a customer

1. Open the project in the console, Webhooks tab, and read the recent delivery results.
2. A 4xx or 5xx is the customer's endpoint. Tell them.
3. Once they are fixed, replay abandoned deliveries from the console or with `POST /admin/deliveries/{id}/replay`.
4. Deliveries retry for three days before being abandoned.

## Rotate an admin or API key

Admin token: change `GATEWAY_ADMIN_TOKEN` and redeploy. Project API key: use the console API keys tab or `POST /admin/tenants/{id}/rotate-key`.

## Disable a project

`POST /admin/tenants/{id}/enabled` or the console operator page. Existing payments are kept.

## Ledger does not reconcile

Call `GET /admin/ledger/reconcile`. It lists cached balance mismatches and any unbalanced transaction. Do not edit balances by hand. Post an `adjustment` transaction.

## A payout is stuck in `sending`

Do not retry by hand. The payout worker looks the payment up in LND (`TrackPaymentV2`, `GetTransactions`) and only retries when LND does not know it. An unknown outcome keeps the funds reserved. Check LND, then let the worker decide.

## Restore from backup

See [backups](/operator/backups) and `docs/runbooks/restore.md`.

## Suspected encryption key leak

Follow the key rotation steps, then regenerate provider credentials and webhook secrets.
