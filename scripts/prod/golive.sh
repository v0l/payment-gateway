#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need kubectl
need curl
need jq
need npx

: "${WEBHOOK_TEST_URL:?set WEBHOOK_TEST_URL to an https endpoint you can read, for example a request catcher}"
payment_msat="${PAYMENT_MSAT:-10000}"
pay_timeout="${PAY_TIMEOUT_SECS:-120}"
revolut_token_secret="${REVOLUT_TOKEN_SECRET:-yalr/yalr-revolut}"
wallet_file="${NWC_WALLET_FILE:-$HOME/.agents/memory/wallet.json}"
check_file="$secrets_dir/tenant-golive-check.json"
backup_job="payment-gateway-golive-$(date -u +%Y%m%d%H%M%S)"
drill_job="payment-gateway-restore-drill"
k8s_dir="${K8S_DIR:-}/payment-gateway-prod"

if [ "${1:-}" != "--apply" ]; then
    cat <<EOF
dry run. With --apply this performs, against $public_url:
  1. GET /health and /ready on the public host
  2. creates tenant golive-check with webhook $WEBHOOK_TEST_URL and a lightning provider
  3. creates a ${payment_msat} msat mainnet invoice, pays it from the NWC wallet (spends real sats), waits for status paid
  4. waits for the payment.paid webhook delivery to be marked delivered
  5. adds a Revolut live provider on golive-check, creates a card attempt, cancels the payment
  6. runs a backup Job, then the restore drill Job into scratch database gateway_drill, then drops it
  7. disables the golive-check tenant
EOF
    exit 0
fi

step "1 health and ready"
code="$(curl -s -o /dev/null -m 15 -w '%{http_code}' "$public_url/health")"
[ "$code" = 200 ] || fail "$public_url/health returned $code"
code="$(curl -s -o /dev/null -m 15 -w '%{http_code}' "$public_url/ready")"
[ "$code" = 200 ] || fail "$public_url/ready returned $code"
ok "health and ready"
code="$(curl -s -o /dev/null -m 15 -w '%{http_code}' "$public_url/admin/tenants")"
[ "$code" = 401 ] || [ "$code" = 403 ] || [ "$code" = 404 ] || fail "$public_url/admin/tenants answered $code without a token"
ok "admin answers $code without a token"

start_port_forward

step "2 golive-check tenant"
umask 077
if [ ! -e "$check_file" ]; then
    admin POST /admin/tenants -d "$(jq -n --arg u "$WEBHOOK_TEST_URL" '{name:"golive-check", webhook_url:$u}')" > "$check_file"
    check_id="$(jq -r .id "$check_file")"
    admin POST "/admin/tenants/$check_id/providers" -d '{
        "method":"lightning","provider_type":"lnd","name":"cluster-lnd",
        "config":{"url":"https://lnd.bitcoin:10009","cert_path":"/lnd/tls.cert","macaroon_path":"/lnd/gateway.macaroon"},
        "fee_rate":0.0}' >/dev/null
fi
check_id="$(jq -r .id "$check_file")"
check_key="$(jq -r .api_key "$check_file")"
ok "tenant $check_id"

step "3 mainnet lightning payment of $payment_msat msat"
reference="golive-$(date -u +%Y%m%dT%H%M%SZ)"
payment="$(api "$check_key" POST /v1/payments -H "Idempotency-Key: $reference" \
    -d "$(jq -n --arg r "$reference" --argjson a "$payment_msat" '{reference:$r, amount:$a, currency:"BTC", description:"apexstrata-pay golive check", expires_in:900}')")"
payment_id="$(jq -r .id <<<"$payment")"
attempt="$(api "$check_key" POST "/v1/payments/$payment_id/attempts" -d '{"method":"lightning"}')"
bolt11="$(jq -r '.attempts[-1].details.bolt11' <<<"$attempt")"
case "$bolt11" in ln*) ;; *) fail "no bolt11 in attempt: $attempt" ;; esac
echo "invoice: $bolt11"
if [ -n "${PAY_CMD:-}" ]; then
    $PAY_CMD "$bolt11"
else
    NWC_URL="$(jq -r .uri "$wallet_file")" npx -y @getalby/cli pay-invoice "$bolt11"
fi
deadline=$((SECONDS + pay_timeout))
status=""
while [ $SECONDS -lt $deadline ]; do
    status="$(api "$check_key" GET "/v1/payments/$payment_id" | jq -r .status)"
    [ "$status" = paid ] && break
    sleep 2
done
[ "$status" = paid ] || fail "payment $payment_id is $status after ${pay_timeout}s"
ok "payment $payment_id paid"

step "4 webhook delivery"
delivered=""
deadline=$((SECONDS + 120))
while [ $SECONDS -lt $deadline ]; do
    delivered="$(admin GET "/admin/deliveries?tenant_id=$check_id" | jq -r '[.[] | select(.event_type=="payment.paid" and .delivered_at != null)] | length')"
    [ "$delivered" -ge 1 ] && break
    sleep 3
done
[ "${delivered:-0}" -ge 1 ] || fail "payment.paid webhook was not delivered to $WEBHOOK_TEST_URL"
ok "payment.paid delivered to $WEBHOOK_TEST_URL"

step "5 revolut live order created and cancelled"
revolut_ns="${revolut_token_secret%%/*}"
revolut_name="${revolut_token_secret##*/}"
revolut_token="$(kubectl --context "$context" -n "$revolut_ns" get secret "$revolut_name" -o jsonpath='{.data.api-key}' | base64 -d)"
revolut_url="${REVOLUT_URL:-https://merchant.revolut.com}"
revolut_version="${REVOLUT_API_VERSION:-2024-09-01}"
revolut_provider="$(admin GET "/admin/tenants/$check_id/providers" | jq -r '[.[] | select(.method=="revolut")][0].id // empty')"
if [ -z "$revolut_provider" ]; then
    revolut_provider="$(admin POST "/admin/tenants/$check_id/providers" -d "$(jq -n --arg t "$revolut_token" --arg u "$revolut_url" --arg v "$revolut_version" \
        '{method:"revolut",provider_type:"revolut",name:"revolut-live",config:{url:$u,token:$t,api_version:$v},fee_rate:0.0}')" | jq -r .id)"
fi
admin POST "/admin/tenants/$check_id/providers/$revolut_provider/enabled" -d '{"enabled":true}' >/dev/null
card_ref="golive-card-$(date -u +%Y%m%dT%H%M%SZ)"
card="$(api "$check_key" POST /v1/payments -H "Idempotency-Key: $card_ref" \
    -d "$(jq -n --arg r "$card_ref" '{reference:$r, amount:100, currency:"EUR", description:"apexstrata-pay golive check", expires_in:900}')")"
card_id="$(jq -r .id <<<"$card")"
card_attempt="$(api "$check_key" POST "/v1/payments/$card_id/attempts" -d '{"method":"revolut"}')"
checkout="$(jq -r '.attempts[-1].details.checkout_url' <<<"$card_attempt")"
case "$checkout" in https://*) ok "revolut order created, checkout $checkout" ;; *) fail "no checkout_url: $card_attempt" ;; esac
cancelled="$(api "$check_key" POST "/v1/payments/$card_id/cancel" | jq -r .status)"
[ "$cancelled" = cancelled ] || fail "card payment is $cancelled after cancel"
ok "revolut order cancelled"

admin POST "/admin/tenants/$check_id/providers/$revolut_provider/enabled" -d '{"enabled":false}' >/dev/null
ok "golive-check revolut provider disabled so the worker does not register its webhook again"
hooks="$(curl -fsS "$revolut_url/api/1.0/webhooks" -H "Authorization: Bearer $revolut_token" -H "Revolut-Api-Version: $revolut_version")"
mine="$(jq -r --arg p "$public_url/hook/$check_id/" '[.[] | select(.url | startswith($p))] | .[].id' <<<"$hooks")"
for id in $mine; do
    curl -fsS -X DELETE "$revolut_url/api/1.0/webhooks/$id" -H "Authorization: Bearer $revolut_token" -H "Revolut-Api-Version: $revolut_version" >/dev/null
    ok "deleted golive-check webhook $id at Revolut"
done

step "6 backup and restore drill"
kc create job "$backup_job" --from=cronjob/payment-gateway-backup
kc wait --for=condition=complete "job/$backup_job" --timeout=900s || { kc logs "job/$backup_job" || true; fail "backup job failed"; }
ok "backup job $backup_job"
kc delete job "$drill_job" --ignore-not-found
[ -n "${K8S_DIR:-}" ] || fail "set K8S_DIR to the manifests checkout"
kc apply -f "$k8s_dir/restore-drill-job.yml"
kc wait --for=condition=complete "job/$drill_job" --timeout=900s || { kc logs "job/$drill_job" || true; fail "restore drill failed"; }
kc logs "job/$drill_job" | tail -3
db_scalar() { kc exec deploy/mysql -- sh -c "MYSQL_PWD=\$MARIADB_ROOT_PASSWORD mariadb -uroot -N -e \"$1\""; }
tables="$(db_scalar "select count(*) from information_schema.tables where table_schema = 'gateway_drill'")"
live="$(db_scalar "select count(*) from information_schema.tables where table_schema = 'gateway'")"
[ "$tables" = "$live" ] || fail "restored $tables tables, live has $live"
[ "$tables" -gt 0 ] || fail "restored database is empty"
restored_payments="$(db_scalar "select count(*) from gateway_drill.payment")"
[ "$restored_payments" -ge 1 ] || fail "restored database holds no payments"
ok "restored $tables tables and $restored_payments payments into gateway_drill"
kc exec deploy/payment-gateway -- env "DATABASE_URL=mysql://root:$(tr -d '\n' < "$secrets_dir/mysql-root-password")@mysql:3306/gateway_drill" /usr/local/bin/payment-gateway check-integrity >/dev/null \
    || fail "check-integrity failed on the restored database"
ok "check-integrity passed on the restored database"
db_scalar "drop database gateway_drill" >/dev/null
ok "scratch database dropped"

step "7 cleanup"
admin POST "/admin/tenants/$check_id/enabled" -d '{"enabled":false}' >/dev/null
ok "golive-check tenant disabled"
echo
echo "ALL GO-LIVE CHECKS PASSED"
