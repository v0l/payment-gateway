#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need openssl
need kubectl

apply=false
apply_enabled "${1:-}" && apply=true

generate() {
    local name="$1" cmd="$2"
    local file="$secrets_dir/$name"
    if [ -e "$file" ]; then
        echo "keep    $file"
        return
    fi
    if $apply; then
        umask 077
        mkdir -p "$secrets_dir"
        chmod 700 "$secrets_dir"
        eval "$cmd" > "$file"
        chmod 600 "$file"
        echo "created $file"
    else
        echo "would create $file"
    fi
}

step "local secret files in $secrets_dir"
generate encryption-key 'openssl rand -hex 32'
generate admin-token 'openssl rand -hex 24'
generate metrics-token 'openssl rand -hex 24'
generate mysql-root-password 'openssl rand -hex 16'
generate mysql-gateway-password 'openssl rand -hex 16'
generate restic-password 'openssl rand -hex 24'

if ! $apply; then
    cat <<EOF

Dry run. With --apply this also creates, in namespace $namespace:
  secret/mysql                    root-password, gateway-password
  secret/gateway                  database-url, admin-token, encryption-key, metrics-token, smtp-url
  secret/payment-gateway-backup   db-user, db-password, restic-repository, restic-password, aws-*
  secret/ghcr                     copied from payment-gateway/ghcr (image pull)
  secret/lnd                      NOT here, see bake-lnd-macaroon.sh
EOF
    exit 0
fi

need jq
read_file() { tr -d '\n' < "$secrets_dir/$1"; }
sandbox_ctx=(kubectl --context "$context" -n payment-gateway)
b2="${B2_ENV_FILE:-$HOME/.config/backup/b2.env}"
[ -r "$b2" ] || { echo "B2 credentials file $b2 not readable" >&2; exit 1; }
set -a
# shellcheck disable=SC1090
source "$b2"
set +a

step "namespace"
kubectl --context "$context" get ns "$namespace" >/dev/null 2>&1 \
    || { echo "namespace $namespace does not exist: apply 00-namespace.yml first" >&2; exit 1; }

step "secret/mysql"
kc create secret generic mysql \
    --from-file=root-password="$secrets_dir/mysql-root-password" \
    --from-file=gateway-password="$secrets_dir/mysql-gateway-password"

step "secret/gateway"
smtp_url="$("${sandbox_ctx[@]}" get secret gateway -o jsonpath='{.data.smtp-url}' | base64 -d)"
db_url="mysql://gateway:$(read_file mysql-gateway-password)@mysql:3306/gateway"
kc create secret generic gateway \
    --from-literal=database-url="$db_url" \
    --from-literal=smtp-url="$smtp_url" \
    --from-file=admin-token="$secrets_dir/admin-token" \
    --from-file=encryption-key="$secrets_dir/encryption-key" \
    --from-file=metrics-token="$secrets_dir/metrics-token"

step "secret/payment-gateway-backup"
kc create secret generic payment-gateway-backup \
    --from-literal=db-user=gateway \
    --from-literal=db-password="$(read_file mysql-gateway-password)" \
    --from-literal=restic-repository="s3:${B2_S3_ENDPOINT}/${B2_BUCKET}/payment-gateway-prod" \
    --from-file=restic-password="$secrets_dir/restic-password" \
    --from-literal=aws-access-key-id="$AWS_ACCESS_KEY_ID" \
    --from-literal=aws-secret-access-key="$AWS_SECRET_ACCESS_KEY"

step "secret/ghcr"
"${sandbox_ctx[@]}" get secret ghcr -o json \
    | jq '{apiVersion, kind, type, metadata: {name: .metadata.name, namespace: "'"$namespace"'"}, data}' \
    | kubectl --context "$context" create -f -

echo
echo "Next: escrow $secrets_dir/encryption-key off the cluster (docs/runbooks/production.md, section Escrow)."
