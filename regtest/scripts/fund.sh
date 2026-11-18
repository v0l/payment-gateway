#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
node="${1:?usage: fund.sh <lnd-service> [btc]}"
amount="${2:-1}"
ensure_wallet
addr="$(lncli "$node" newaddress p2wkh | grep -o '"address": *"[^"]*"' | cut -d'"' -f4)"
btcw sendtoaddress "$addr" "$amount" >/dev/null
"$(dirname "$0")/mine.sh" 3
