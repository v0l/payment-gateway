#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
dc up -d
for i in $(seq 1 60); do btc getblockcount >/dev/null 2>&1 && break; sleep 1; done
ensure_wallet
if [ "$(btc getblockcount)" -lt 101 ]; then
  btcw generatetoaddress 101 "$(btcw getnewaddress)" >/dev/null
fi
mkdir -p "$REGTEST_DIR"
for entry in "lnd-gateway:gateway" "lnd-customer:customer" "lnd-onchain:onchain"; do
  svc="${entry%%:*}"; name="${entry##*:}"
  for i in $(seq 1 90); do
    dc exec -T "$svc" test -f /root/.lnd/data/chain/bitcoin/regtest/admin.macaroon 2>/dev/null && break
    sleep 1
  done
  cid="$(dc ps -q "$svc")"
  docker cp "$cid:/root/.lnd/tls.cert" "$REGTEST_DIR/$name.cert"
  docker cp "$cid:/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon" "$REGTEST_DIR/$name.macaroon"
done
echo "certs and macaroons in $REGTEST_DIR"
