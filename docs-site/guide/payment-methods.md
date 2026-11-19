# Payment methods

A customer sees the methods you have turned on for the project, and nothing else. Turn them on from Developers, Payment methods in the console, or list what is live with `GET /v1/methods`.

| Method | Provider | What the customer gets | Notes |
| --- | --- | --- | --- |
| `lightning` | Your LND node, or the platform node | A bolt11 invoice and a QR code | Settles instantly. |
| `onchain` | Your LND node, or the platform wallet | A fresh address and a bitcoin: link | Counts confirmations, shows "0 of N" and keeps the same address if the customer pays short. |
| `stripe` | Your Stripe account | A hosted Stripe checkout | Refunds and disputes are tracked. Can save a card. |
| `revolut` | Your Revolut merchant account | A hosted card form on the pay page | Refunds and disputes are tracked. Can save a card. |
| `paypal` | Your PayPal account | PayPal approval, captured when the customer returns | Disputes are tracked. |
| `fixedfloat` | FixedFloat, into your on-chain wallet | A deposit address for an altcoin such as XMR or LTC | Needs an on-chain method on the same project. |

## Your own providers or the platform's

With your own provider the money goes straight to you and the gateway only watches for it. Without a node, enable a platform managed Lightning or on-chain method instead. The operator receives the money and the gateway keeps a ledger of what it owes you, minus a fee that is shown next to the method. See [Custody and payouts](/guide/custody).

## Fees and testing a method

A method can add a processing fee to what the customer pays (a percent, a base amount or both). The pay page shows it before they choose. Use **Test** on a provider to check its credentials without creating a payment. Secrets are write-only and are never shown again.

## Failures

The console shows each method's recent success and error count and its last error. After 10 failures in a row a method on a customer project is switched off and the project owners are emailed, so the pay page does not keep offering something that is broken. Turn it back on once the cause is fixed.
