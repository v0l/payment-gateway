#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need kubectl
need curl
need jq

usage() {
    cat >&2 <<EOF
usage: bootstrap.sh tenant      [--apply]   create the yalr tenant and its Lightning provider
       bootstrap.sh revolut     [--apply]   add the Revolut live provider (registers a live webhook at Revolut)
       bootstrap.sh fixedfloat  [--apply]   add the FixedFloat provider from $secrets_dir/fixedfloat.json (needs an onchain provider on the tenant)
       bootstrap.sh webhook URL [--apply]   point the yalr tenant at its webhook endpoint
       bootstrap.sh operator EMAIL [--apply] create the first operator invite
EOF
    exit 2
}

[ $# -ge 1 ] || usage
cmd="$1"
shift

tenant_name="${TENANT_NAME:-yalr}"
tenant_file="$secrets_dir/tenant-$tenant_name.json"
revolut_api_version="${REVOLUT_API_VERSION:-2024-09-01}"
revolut_url="${REVOLUT_URL:-https://merchant.revolut.com}"

tenant_id() { jq -r .id "$tenant_file"; }

run_or_describe() {
    local apply="$1" text="$2"
    if [ "$apply" != "--apply" ]; then
        echo "dry run: $text"
        return 1
    fi
}

case "$cmd" in
tenant)
    run_or_describe "${1:-}" "create tenant $tenant_name and a mainnet lightning/lnd provider (cert_path /lnd/tls.cert, macaroon_path /lnd/gateway.macaroon), no webhook url yet" || exit 0
    [ ! -e "$tenant_file" ] || fail "$tenant_file exists, tenant already created"
    start_port_forward
    umask 077
    created="$(admin POST /admin/tenants -d "$(jq -n --arg n "$tenant_name" '{name: $n}')")"
    printf '%s\n' "$created" > "$tenant_file"
    id="$(jq -r .id "$tenant_file")"
    provider="$(admin POST "/admin/tenants/$id/providers" -d '{
        "method":"lightning","provider_type":"lnd","name":"cluster-lnd",
        "config":{"url":"https://lnd.bitcoin:10009","cert_path":"/lnd/tls.cert","macaroon_path":"/lnd/gateway.macaroon"},
        "fee_rate":0.0}')"
    echo "tenant $id created, credentials in $tenant_file (api_key and webhook_secret are shown once, hand them to YALR #854)"
    echo "lightning provider: $provider"
    ;;
revolut)
    run_or_describe "${1:-}" "create the revolut provider from secret yalr/yalr-revolut api-key at $revolut_url. The gateway registers https://pay.apexstrata.ie/hook/<tenant>/<provider> at Revolut on its next worker tick" || exit 0
    [ -e "$tenant_file" ] || fail "run tenant first"
    start_port_forward
    token="$(kubectl --context "$context" -n yalr get secret yalr-revolut -o jsonpath='{.data.api-key}' | base64 -d)"
    body="$(jq -n --arg t "$token" --arg v "$revolut_api_version" --arg u "$revolut_url" \
        '{method:"revolut",provider_type:"revolut",name:"revolut-live",config:{url:$u,token:$t,api_version:$v},fee_rate:0.0}')"
    admin POST "/admin/tenants/$(tenant_id)/providers" -d "$body"
    echo
    echo "webhook registration happens within one worker tick; confirm with: kubectl --context $context -n $namespace logs deploy/payment-gateway | grep -i webhook"
    ;;
fixedfloat)
    run_or_describe "${1:-}" "create the fixedfloat provider from $secrets_dir/fixedfloat.json (api_key, api_secret, coins)" || exit 0
    [ -e "$tenant_file" ] || fail "run tenant first"
    [ -r "$secrets_dir/fixedfloat.json" ] || fail "$secrets_dir/fixedfloat.json missing"
    start_port_forward
    body="$(jq '{method:"fixedfloat",provider_type:"fixedfloat",name:"fixedfloat",config:.,fee_rate:0.0}' "$secrets_dir/fixedfloat.json")"
    admin POST "/admin/tenants/$(tenant_id)/providers" -d "$body"
    ;;
webhook)
    [ $# -ge 1 ] || usage
    url="$1"
    run_or_describe "${2:-}" "set the $tenant_name tenant webhook_url to $url" || exit 0
    [ -e "$tenant_file" ] || fail "run tenant first"
    start_port_forward
    admin POST "/admin/tenants/$(tenant_id)/settings" -d "$(jq -n --arg u "$url" '{webhook_url: $u}')"
    ;;
operator)
    [ $# -ge 1 ] || usage
    email="$1"
    run_or_describe "${2:-}" "create an operator invite for $email" || exit 0
    start_port_forward
    admin POST /admin/invites -d "$(jq -n --arg e "$email" '{email: $e, operator: true}')"
    ;;
*)
    usage
    ;;
esac
