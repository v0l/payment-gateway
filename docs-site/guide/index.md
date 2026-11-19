# Documentation

The gateway collects money for your app and tells you when it has been paid. You create a payment, the customer pays it by one of the enabled methods, and the gateway sends a signed webhook.

Start with [concepts](/guide/concepts), then pick a quickstart.

- [Quickstart: API only](/guide/quickstart-api): you draw the payment screen.
- [Quickstart: hosted page](/guide/quickstart-hosted): you redirect the customer to the gateway pay page.
- [Quickstart: iframe](/guide/quickstart-iframe): the pay page inside your own page.
- [Webhooks](/guide/webhooks): events, signatures and verification code in JavaScript, Rust and Python.
- [Payment methods](/guide/payment-methods): what each method gives the customer.
- [Going live checklist](/guide/going-live)
- [Refunds and saved methods](/guide/refunds-saved), [subscriptions](/guide/subscriptions), [custody and payouts](/guide/custody)
- [Using the console](/guide/console): Home, saved views and the Balance page.
- [Error reference](/guide/errors), [FAQ](/guide/faq), [API reference](/guide/reference)

Running your own gateway? Read the [operator guide](/operator/).

Every `sh` example marked as runnable in these pages is extracted and executed against a real gateway by the test suite, and the verification samples are run against signatures the gateway produced. The shell examples use `$GW` for the gateway address (for example `https://pay.example.com`) and `$KEY` for a project API key, and need `curl` and `jq`.
