#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
node="${1:-lnd-gateway}"
out="${REGTEST_DIR}"

receive_permissions=(
  uri:/lnrpc.Lightning/AddInvoice
  uri:/lnrpc.Lightning/LookupInvoice
  uri:/lnrpc.Lightning/SubscribeInvoices
  uri:/invoicesrpc.Invoices/CancelInvoice
  uri:/invoicesrpc.Invoices/SubscribeSingleInvoice
  uri:/lnrpc.Lightning/NewAddress
  uri:/lnrpc.Lightning/GetTransactions
  uri:/lnrpc.Lightning/SubscribeTransactions
)

payout_permissions=(
  uri:/routerrpc.Router/SendPaymentV2
  uri:/routerrpc.Router/TrackPaymentV2
  uri:/lnrpc.Lightning/SendCoins
  uri:/lnrpc.Lightning/GetTransactions
)

bake() {
  local name="$1"; shift
  lncli "$node" bakemacaroon --save_to="/tmp/$name.macaroon" "$@" >/dev/null
  dc cp "$node:/tmp/$name.macaroon" "$out/$name.macaroon"
}

bake gateway-receive "${receive_permissions[@]}"
bake gateway-payout "${payout_permissions[@]}"
echo "macaroons in $out"
