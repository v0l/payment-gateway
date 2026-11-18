#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
ensure_wallet
btcw generatetoaddress "${1:-1}" "$(btcw getnewaddress)" >/dev/null
btc getblockcount
