#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
state="${GATEWAY_TEST_STATE:-${TMPDIR:-/tmp}/pgw-test-gateway}"
port="${GATEWAY_TEST_PORT:-18747}"
admin_token="${GATEWAY_TEST_ADMIN_TOKEN:-test-admin-token-0123456789abcdef}"

stop() {
  if [ -f "$state/pid" ]; then
    kill "$(cat "$state/pid")" 2>/dev/null || true
  fi
  rm -rf "$state"
}

if [ "${1:-}" = "--stop" ]; then
  stop
  exit 0
fi

: "${TEST_DATABASE_URL:?set TEST_DATABASE_URL, for example mysql://root:test@127.0.0.1:33907/}"

stop
mkdir -p "$state"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
(cd "$root" && cargo build --quiet --bin payment-gateway --example test_database >&2)
bin_dir="${CARGO_TARGET_DIR:-$root/target}/debug"

database_url="$("$bin_dir/examples/test_database")"

GATEWAY_LISTEN="127.0.0.1:$port" \
GATEWAY_PUBLIC_URL="http://127.0.0.1:$port" \
GATEWAY_ADMIN_TOKEN="$admin_token" \
GATEWAY_ENCRYPTION_KEY="$(printf '07%.0s' $(seq 32))" \
GATEWAY_ALLOW_MOCK=1 \
DATABASE_URL="$database_url" \
WORKER_TICK_SECS=1 \
LOG_FORMAT=text \
LOG_LEVEL=warn \
  "$bin_dir/payment-gateway" serve >"$state/gateway.log" 2>&1 &
echo $! >"$state/pid"

url="http://127.0.0.1:$port"
for _ in $(seq 1 100); do
  if curl -fs "$url/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$(cat "$state/pid")" 2>/dev/null; then
    cat "$state/gateway.log" >&2
    echo "gateway exited" >&2
    exit 1
  fi
  sleep 0.2
done
curl -fs "$url/health" >/dev/null

tenant="$(curl -fs -X POST "$url/admin/tenants" \
  -H "authorization: Bearer $admin_token" \
  -H 'content-type: application/json' \
  -d '{"name":"test tenant"}')"
tenant_id="$(printf '%s' "$tenant" | sed -E 's/.*"id":([0-9]+).*/\1/')"
api_key="$(printf '%s' "$tenant" | sed -E 's/.*"api_key":"([^"]+)".*/\1/')"
webhook_secret="$(printf '%s' "$tenant" | sed -E 's/.*"webhook_secret":"([^"]+)".*/\1/')"

provider="$(curl -fs -X POST "$url/admin/tenants/$tenant_id/providers" \
  -H "authorization: Bearer $admin_token" \
  -H 'content-type: application/json' \
  -d '{"method":"lightning","provider_type":"mock","name":"mock lightning","config":{"secret":"mock-secret"}}')"
provider_id="$(printf '%s' "$provider" | sed -E 's/.*"id":([0-9]+).*/\1/')"

cat >"$state/env" <<ENV
GATEWAY_URL=$url
GATEWAY_ADMIN_TOKEN=$admin_token
GATEWAY_API_KEY=$api_key
GATEWAY_TENANT_ID=$tenant_id
GATEWAY_PROVIDER_ID=$provider_id
GATEWAY_WEBHOOK_SECRET=$webhook_secret
GATEWAY_MOCK_SECRET=mock-secret
ENV
cat "$state/env"
