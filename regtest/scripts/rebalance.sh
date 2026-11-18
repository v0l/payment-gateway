#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
sats="${1:-500000}"
gw="$(lncli lnd-gateway getinfo | grep -o '"identity_pubkey": *"[^"]*"' | cut -d'"' -f4)"
lncli lnd-gateway sendpayment --dest "$(lncli lnd-customer getinfo | grep -o '"identity_pubkey": *"[^"]*"' | cut -d'"' -f4)" \
  --amt "$sats" --keysend --force >/dev/null
echo "moved $sats sats from gateway side to customer side (gateway $gw)"
