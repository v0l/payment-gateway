# Stripe, Revolut and PayPal

## Stripe

Add the provider with a secret key and the webhook signing secret:

```sh
curl -X POST "$GW/admin/tenants/1/providers" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"method":"stripe","provider_type":"stripe","name":"cards","config":{"secret_key":"sk_live_...","webhook_secret":"whsec_..."}}'
```

The response holds a `hook_url` of the form `https://pay.example.com/hook/{tenant}/{provider}`. In the Stripe dashboard add a webhook endpoint with that URL and subscribe it to:

- `checkout.session.completed`
- `checkout.session.async_payment_succeeded`
- `checkout.session.async_payment_failed`

Also subscribe it to `charge.refunded`, `charge.refund.updated`, `charge.dispute.created` and `charge.dispute.closed` so refunds and disputes reach the payment.

Copy the endpoint's signing secret (`whsec_...`) into the provider config. The gateway rejects hook calls whose signature does not verify. Use a restricted key where Stripe allows it.

## Revolut

```sh
curl -X POST "$GW/admin/tenants/1/providers" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"method":"revolut","provider_type":"revolut","name":"revolut","config":{"url":"https://merchant.revolut.com","token":"...","api_version":"...","public_key":"..."}}'
```

The gateway registers its webhook with Revolut itself and stores the signing secret, so there is nothing to copy. Use `https://sandbox-merchant.revolut.com` for the sandbox. If the public URL changes, save the provider again to register the new hook.

## PayPal

```sh
curl -X POST "$GW/admin/tenants/1/providers" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"method":"paypal","provider_type":"paypal","name":"paypal","config":{"client_id":"...","client_secret":"...","mode":"sandbox","webhook_id":"..."}}'
```

`mode` is `sandbox` or `live` and defaults to `sandbox`. In the PayPal developer dashboard create a webhook that points at the provider's `hook_url` and subscribe it to `CHECKOUT.ORDER.APPROVED`, `PAYMENT.CAPTURE.COMPLETED`, `PAYMENT.CAPTURE.DENIED`, `PAYMENT.CAPTURE.REFUNDED` and `CUSTOMER.DISPUTE.*`. Store its id as `webhook_id`. The customer returns from PayPal to `/pay/{id}/return`, which captures the approved order.

## Disputes and provider health

Stripe, Revolut and PayPal disputes show on the payment as `disputed` and `dispute_status` and fire `payment.disputed` and `payment.dispute_closed` webhooks. Revolut registers its dispute events itself.

Each provider keeps a rolling 24 hour success and error count and its last error, shown in the console. In the console, Test connection checks the credentials without creating a payment. After 10 failures in a row a customer provider is disabled and the project owners are mailed. Providers of trusted projects are never disabled, the operators are mailed instead.

All providers need the gateway to be reachable from the internet on `GATEWAY_PUBLIC_URL`. Hook routes are rate limited per IP.
