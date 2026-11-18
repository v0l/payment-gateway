# Shared platform providers

A shared provider is a Lightning or on-chain LND wallet that belongs to the platform. Projects that have no node of their own enable it with one click and the platform holds their money until they withdraw it.

## Add one

Open Operator, Shared providers in the console, or `POST /admin/shared`. Give it a name, the method (`lightning` or `onchain`), the network, the LND url, `tls.cert` and a receive macaroon (see [LND and macaroons](/operator/lnd)). The config is sealed at rest and never returned. It is rotated with the other sealed columns by key rotation.

## Fee

Each provider has a default fee: a rate in basis points and a base in msats. Per received payment:

```text
fee = ceil(received * fee_bps / 10000) + fee_base_msat, capped at received
net = received - fee
```

The fee is rounded up and the project is never credited more than was received. The net goes to the project balance, the fee to the platform fees account, both in the same transaction as the settlement. Override the terms for one project with `POST /admin/shared/{id}/projects/{tenant}/fee`. Only that project sees the override. Send `null` for both values to remove it.

## Who can use which provider

Every provider has a network and two flags, `allow_untrusted` and `allow_trusted`.

| Project | Allowed providers |
| --- | --- |
| Test mode (untrusted) | Only providers on testnet, signet or regtest with `allow_untrusted` on |
| Trusted | Providers with `allow_trusted` on, including mainnet |

A mainnet provider cannot be allowed for test mode projects. The check runs when a project enables a method and again when it starts a payment, so changing a flag takes effect for existing projects.

## Health

A shared provider that fails repeatedly is never disabled automatically. Operators get an alert mail instead, because disabling it would stop every project that uses it. Customer providers are disabled after 10 failures in a row.

## Solvency

The platform wallet balance has to cover what the ledger says it owes projects. `ledger::reconcile` checks this, see [Monitoring and alerts](/operator/monitoring).
