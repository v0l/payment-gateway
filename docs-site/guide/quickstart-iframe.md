# Quickstart: iframe

The same pay page can run inside your own page. The gateway sends `frame-ancestors` limited to the project's `allowed_origins`, so add the embedding site there first (see the [hosted quickstart](/guide/quickstart-hosted)).

Create the payment on your server as in the [API quickstart](/guide/quickstart-api), pass the payment id to the page, then:

```html
<div id="pay"></div>
<script src="https://pay.example.com/gateway.js"></script>
<script src="/checkout.js"></script>
```

And in `checkout.js`, which keeps your own page free of inline scripts:

```js
PaymentGateway.open({
  gateway: "https://pay.example.com",
  paymentId: document.getElementById("pay").dataset.paymentId,
  container: "#pay",
  onPaid: function () { window.location = "/thanks"; }
});
```

The frame posts `pgw:ready`, `pgw:resize`, `pgw:paid`, `pgw:expired` and `pgw:cancelled`. `gateway.js` turns them into the `onReady`, `onPaid` and similar callbacks and resizes the frame. Card checkouts from Stripe and Revolut refuse to be framed, so for those methods the embed redirects the top window.

As with every integration mode, confirm payment on your server from the webhook.
