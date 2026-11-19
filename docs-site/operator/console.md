# Console tools

Operators see an extra Operator menu in the console top bar. These pages and endpoints need an operator account with two factor authentication (or `GATEWAY_OPERATOR_2FA_GRACE=1` while bootstrapping).

## Projects

The Projects page lists every project with its status (test, live or disabled), payment count, paid volume over 30 days, the daily payment limit and the time of the last payment. Use it to spot idle or noisy projects. The same data is at `GET /console/api/operator/tenants`.

## Support view

Open a project from the list with "support view" for a read only look at its dashboard numbers, recent payments, providers and webhook endpoint health. No secrets are shown and nothing can be changed from this page. Every open is written to the audit log as `support.viewed` with the project id, so ask the customer first.

## Global search

Paste a payment id (or its first characters), a reference, an invoice hash or a transaction id into the search box. Each hit shows the project, the payment and which field matched, and opens the payment detail. Search needs at least 4 characters. Endpoint: `GET /console/api/operator/search?q=`.

## Platform health

`GET /console/api/operator/health` and the cards at the top of the Projects page show the number of projects, disabled projects, pending payments, webhooks waiting, abandoned webhooks in the last 24 hours and uptime. It is a quick check before looking at metrics, see [Monitoring and alerts](/operator/monitoring).

## Project deletion

Owners can disable a project, or request deletion in the Settings danger zone. The request returns a token once. After 24 hours the owner confirms with the token, which disables the project, revokes its keys and removes its members. Deletion is refused while payments are pending or a ledger balance is not zero. Both steps are audit logged.
