# LND and macaroons

The gateway talks to LND over gRPC with a TLS certificate and a macaroon. Do not give it `admin.macaroon`. Bake two macaroons that carry only the calls the gateway makes, so a deployment that only receives money holds no right to spend it.

## Receive macaroon

Used by the Lightning and on-chain providers (`macaroon_path`). It can create and look up invoices, cancel them, watch settlements, create addresses and read wallet transactions. It cannot send coins or pay invoices.

```sh
lncli bakemacaroon \
  uri:/lnrpc.Lightning/AddInvoice \
  uri:/lnrpc.Lightning/LookupInvoice \
  uri:/lnrpc.Lightning/SubscribeInvoices \
  uri:/invoicesrpc.Invoices/CancelInvoice \
  uri:/invoicesrpc.Invoices/SubscribeSingleInvoice \
  uri:/lnrpc.Lightning/NewAddress \
  uri:/lnrpc.Lightning/GetTransactions \
  uri:/lnrpc.Lightning/SubscribeTransactions \
  --save_to=gateway-receive.macaroon
```

## Payout macaroon

Used only by the payout worker (`GATEWAY_PAYOUT_LND_MACAROON`), and only needed when withdrawals or refunds are enabled. It can pay invoices, track payments and send on-chain.

```sh
lncli bakemacaroon \
  uri:/routerrpc.Router/SendPaymentV2 \
  uri:/routerrpc.Router/TrackPaymentV2 \
  uri:/lnrpc.Lightning/SendCoins \
  uri:/lnrpc.Lightning/GetTransactions \
  --save_to=gateway-payout.macaroon
```

Lightning and on-chain refunds send funds from the provider's node, so a provider that must issue refunds needs the payout permissions on its own macaroon too. If you want refunds, bake a third macaroon with both sets, and keep it off any host that does not need it.

## Tested on regtest

The script `regtest/scripts/macaroons.sh` bakes both macaroons with exactly these permissions inside the regtest LND container. The test `tests/regtest_macaroon.rs` points the Lightning and on-chain providers at the receive macaroon, takes a real Lightning payment through the gateway and checks that it settles, then checks that the receive macaroon is denied `SendCoins` and the payout macaroon is denied `AddInvoice`. Run it with:

```sh
regtest/scripts/up.sh
regtest/scripts/macaroons.sh
REGTEST=1 CARGO_BUILD_JOBS=2 cargo test --test regtest_macaroon
regtest/scripts/reset.sh
```

The script it runs:

@include ../regtest/scripts/macaroons.sh sh

## Add the provider

```sh
curl -X POST "$GW/admin/tenants/1/providers" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"method":"lightning","provider_type":"lnd","name":"main",
       "config":{"url":"https://lnd:10009","cert_path":"/lnd/tls.cert","macaroon_path":"/lnd/gateway-receive.macaroon"}}'
```

Add `"method":"onchain"` the same way, with optional `address_type` (`p2tr`, `p2wkh`, `np2wkh`) and `min_confirmations`. Mount the certificate and macaroon as read-only files. Rotating a macaroon means baking a new one and updating the file, and the old one stays valid until you change the LND root key or bake with a different `--root_key_id`.
