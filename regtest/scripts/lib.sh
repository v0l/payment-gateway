#!/usr/bin/env bash
set -euo pipefail
COMPOSE_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/compose.yaml"
export REGTEST_DIR="${REGTEST_DIR:-/tmp/pgw-reg}"
dc() { docker compose -f "$COMPOSE_FILE" "$@"; }
btc() { dc exec -T bitcoind bitcoin-cli -regtest -rpcuser=regtest -rpcpassword=regtest "$@"; }
btcw() { btc -rpcwallet=scripts "$@"; }
lncli() {
  local node="$1"; shift
  dc exec -T "$node" lncli --network=regtest "$@"
}
ensure_wallet() {
  btc listwallets | grep -q '"scripts"' && return 0
  btc loadwallet scripts >/dev/null 2>&1 || btc createwallet scripts >/dev/null
}
