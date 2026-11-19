#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need ssh
need kubectl

bitcoin_ssh="${BITCOIN_SSH:-debian@10.97.0.40}"
ssh_key="${BITCOIN_SSH_KEY:-$HOME/.ssh/agents}"
lnd_container="${LND_CONTAINER:-bitcoin-lnd-1}"
root_key_id="${MACAROON_ROOT_KEY_ID:-853}"
source_ns="${LND_CERT_NAMESPACE:-yalr}"
permissions=(invoices:read invoices:write offchain:read offchain:write)

apply=false
apply_enabled "${1:-}" && apply=true

echo "macaroon permissions: ${permissions[*]}"
echo "root key id: $root_key_id (revoke later with: lncli deletemacaroonid $root_key_id)"
echo "tls.cert copied from secret/lnd in namespace $source_ns (never regenerate it on the LND side)"
if ! $apply; then
    echo "dry run: --apply bakes the macaroon on $bitcoin_ssh and creates secret/lnd in $namespace"
    exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
chmod 700 "$work"

ssh -i "$ssh_key" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new "$bitcoin_ssh" \
    "sudo docker exec $lnd_container lncli --network=mainnet bakemacaroon --root_key_id $root_key_id ${permissions[*]}" \
    | tr -d '\r\n' > "$work/macaroon.hex"
[ -s "$work/macaroon.hex" ] || fail "LND returned no macaroon"
grep -Eq '^[0-9a-f]+$' "$work/macaroon.hex" || fail "macaroon is not hex"
xxd -r -p "$work/macaroon.hex" > "$work/gateway.macaroon"

kubectl --context "$context" -n "$source_ns" get secret lnd -o jsonpath='{.data.tls\.cert}' | base64 -d > "$work/tls.cert"
grep -q 'BEGIN CERTIFICATE' "$work/tls.cert" || fail "tls.cert not found in $source_ns/lnd"

mkdir -p "$secrets_dir"
chmod 700 "$secrets_dir"
install -m 600 "$work/macaroon.hex" "$secrets_dir/lnd-gateway-macaroon.hex"

kc create secret generic lnd \
    --from-file=tls.cert="$work/tls.cert" \
    --from-file=gateway.macaroon="$work/gateway.macaroon"
echo "created secret/lnd in $namespace. Restart the gateway pod so it mounts it: kubectl -n $namespace rollout restart deploy/payment-gateway"
