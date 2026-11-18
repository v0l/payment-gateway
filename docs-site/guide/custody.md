# Custody and payouts

By default the gateway does not hold your money. Each project connects its own providers: your LND node, your Stripe account, your Revolut merchant account. Funds go straight to you.

Projects that have no node of their own can use shared platform providers. Then the operator receives the money and the gateway keeps a double-entry ledger of what it owes each project. The ledger has accounts for the project balance, platform fees, the platform wallet, pending payouts and refund reserves. Entries are append only and every transaction sums to zero.

## Platform managed methods

A project enables a platform managed method on its Providers tab. It then appears in `GET /v1/methods` with `platform_managed: true` and works like any other Lightning or on-chain method. When a payment is paid, the received amount is split:

- the platform fee is `ceil(received * fee_bps / 10000) + fee_base_msat`, never more than the amount received
- the net, received minus the fee, is added to the project balance

The paid attempt shows both as `platform_fee` and `net_credited` (msats), and the `payment.paid` webhook carries them in `data.detail`. You withdraw the balance as described below.

Refunds of these payments are debited from the project balance and paid from the platform wallet. The fee is not returned. A refund the balance cannot cover is refused until the balance is topped up.

The fee shown next to a method is the one that applies to your project. An operator can set a different fee for a single project, and only that project sees it.

## Withdrawals

A project owner withdraws its balance (BTC, in msats) in the console to a destination: an on-chain address, a Lightning address or an LNURL-pay. Rules:

- A destination is verified with a small test payout (or a signed message for a legacy P2PKH address) and waits a delay, 24 hours by default, before it can be used.
- A request reserves the amount plus a fee reserve in the same transaction, so parallel requests cannot overdraw the balance.
- Trusted projects under the operator's auto threshold are approved at once. Others wait for operator review.
- Daily maximums apply, lower for untrusted projects.
- Outcomes are sent as `withdrawal.requested`, `withdrawal.completed` and `withdrawal.failed` events. They never include the destination.

See the [custodial and non-custodial statement](/legal/custody) for what that means legally. It is a draft and needs review.
