#!/usr/bin/env bash
set -euo pipefail

context="${KUBE_CONTEXT:-axst-k8s}"
namespace="${KUBE_NAMESPACE:-payment-gateway-prod}"
export public_url="${GATEWAY_PUBLIC_URL:-https://pay.apexstrata.ie}"
secrets_dir="${PGW_SECRETS_DIR:-$HOME/.config/payment-gateway-prod}"
local_port="${GATEWAY_LOCAL_PORT:-18081}"

kc() { kubectl --context "$context" -n "$namespace" "$@"; }
need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 1; }; }
apply_enabled() { [ "${1:-}" = "--apply" ]; }
step() { printf '\n== %s\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "ok: $*"; }

start_port_forward() {
    kc port-forward svc/payment-gateway "$local_port:8080" >/dev/null 2>&1 &
    forward_pid=$!
    trap 'kill "$forward_pid" 2>/dev/null || true' EXIT
    for _ in $(seq 1 30); do
        curl -fs "http://127.0.0.1:$local_port/health" >/dev/null 2>&1 && return 0
        sleep 1
    done
    fail "port-forward to svc/payment-gateway did not come up"
}

admin() {
    local method="$1" path="$2"
    shift 2
    curl -fsS -X "$method" "http://127.0.0.1:$local_port$path" \
        -H "Authorization: Bearer $(tr -d '\n' < "$secrets_dir/admin-token")" \
        -H 'content-type: application/json' "$@"
}

api() {
    local key="$1" method="$2" path="$3"
    shift 3
    curl -fsS -X "$method" "http://127.0.0.1:$local_port$path" \
        -H "Authorization: Bearer $key" -H 'content-type: application/json' "$@"
}
