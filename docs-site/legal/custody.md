# Custodial and non-custodial

This page describes what the software does. Whether that makes an operator subject to any licence or regulation depends on where and how it is run and must be checked with a lawyer. This page makes no claim about that.

## Not custodial

When a project connects its own providers (its own Lightning or on-chain node, its own Stripe, Revolut or PayPal account), customer funds go straight to the project's accounts. The gateway sees payment status and sends it to your application. It does not hold, pool or move that money.

## Custodial

When a project uses shared platform providers, or the operator's wallet receives the funds, the operator holds money on the project's behalf. In that case:

- the gateway keeps a double-entry ledger of what the operator owes the project;
- the project can withdraw its balance to a verified destination, with limits, delays and operator review;
- fees the operator earns are recorded in a separate account;
- a pending withdrawal reserves funds until it completes, is rejected or is cancelled.

The ledger is a record kept by software. It is not a bank account, a deposit or insurance, and nothing here promises that funds are safe or recoverable.

[Operator to state which of the two modes it offers, who holds the funds, any licence it relies on, and what happens to balances on closure.]
