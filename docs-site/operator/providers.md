# Stripe and Revolut

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

Copy the endpoint's signing secret (`whsec_...`) into the provider config. The gateway rejects hook calls whose signature does not verify. Use a restricted key where Stripe allows it.

## Revolut

```sh
curl -X POST "$GW/admin/tenants/1/providers" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"method":"revolut","provider_type":"revolut","name":"revolut","config":{"url":"https://merchant.revolut.com","token":"...","api_version":"...","public_key":"..."}}'
```

The gateway registers its webhook with Revolut itself and stores the signing secret, so there is nothing to copy. Use `https://sandbox-merchant.revolut.com` for the sandbox. If the public URL changes, save the provider again to register the new hook.

Both providers need the gateway to be reachable from the internet on `GATEWAY_PUBLIC_URL`. Hook routes are rate limited per IP.
