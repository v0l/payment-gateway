# Quickstart: hosted page

Send the customer to the gateway pay page. It shows the amount, a method picker, the QR code or the card form, updates live, and redirects back to your site when the payment settles.

First allow your site in the project settings: `success_url` and `cancel_url` must be on one of the project's `allowed_origins`, otherwise create is rejected. Use the console, or the admin API:

```sh
curl -X POST "$GW/admin/tenants/1/settings" \
  -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"allowed_origins":["https://shop.example.com"],"display_name":"Example Shop"}'
```

Then create the payment with the return URLs and redirect to `pay_url`:

```sh run
ORIGIN=${SHOP_ORIGIN:-https://shop.example.com}
PAYMENT=$(curl -sf -X POST "$GW/v1/payments" \
  -H "Authorization: Bearer $KEY" \
  -H "Idempotency-Key: quickstart-hosted-1" \
  -H "Content-Type: application/json" \
  -d "{\"reference\":\"quickstart-hosted-1\",\"amount\":2500,\"currency\":\"EUR\",\"success_url\":\"$ORIGIN/thanks\",\"cancel_url\":\"$ORIGIN/cart\"}")
echo "$PAYMENT" | jq -r .pay_url
```

After payment the customer lands on `success_url` with `payment_id`, `reference`, `status` and `sig`. The signature is `HMAC-SHA256(webhook_secret, "{payment_id}.{reference}.{status}")` in hex. Check it, but still confirm with the webhook or `GET /v1/payments/{id}`. The redirect alone is not proof of payment.

## Languages and branding

The pay page is available in English, Spanish, French, German, Portuguese, Italian and Irish. The language comes from `?lang=` on the URL, then the browser language list, then the project default, then English. The customer can switch it from a selector at the bottom of the page.

Branding is set per project in the console, or with `GET` and `POST /admin/tenants/{id}/branding`. You can set a display name, a logo and accent colour (with dark variants), `theme` (`auto`, `light` or `dark`), corner `radius`, `font`, a `default_lang`, a short `thanks_text` for the success screen and up to three https footer links. Only these safe values are accepted, so a project cannot inject its own CSS or script. The "powered by" line is on by default and only the operator can turn it off.

## Receipts

Once a payment is paid, its `receipt_url` (also `GET /receipt/{payment_id}`) is a public, print friendly receipt with the seller details from the project settings, the lines, tax and total. Like the pay page it works from the payment id alone, needs no API key, and returns 404 until the payment is paid. Link to it from your thank-you page or email.
