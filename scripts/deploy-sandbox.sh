#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
ref="${1:-master}"
commit="$(git -C "$repo" rev-parse --short "$ref")"
registry_image="${REGISTRY_IMAGE:-registry.v0l.io/payment-gateway}"
context="${KUBE_CONTEXT:-axst-k8s}"
namespace="${KUBE_NAMESPACE:-payment-gateway}"
backup_dir="${BACKUP_DIR:-$HOME/backups/payment-gateway}"
kubectl_ns=(kubectl --context "$context" -n "$namespace")

mkdir -p "$backup_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
dump="$backup_dir/gateway-$stamp.sql"
"${kubectl_ns[@]}" exec deploy/mysql -- sh -c \
    'MYSQL_PWD=$MARIADB_ROOT_PASSWORD mariadb-dump -uroot --single-transaction --routines --triggers --hex-blob --databases gateway' \
    > "$dump.partial"
grep -q "Dump completed" "$dump.partial" || { echo "backup incomplete" >&2; exit 1; }
mv "$dump.partial" "$dump"
echo "backup $dump"

worktree="$(mktemp -d /tmp/pgw-deploy.XXXXXX)"
trap 'git -C "$repo" worktree remove --force "$worktree" >/dev/null 2>&1 || true' EXIT
git -C "$repo" worktree add --detach "$worktree" "$commit" >/dev/null
docker build -t "$registry_image:sandbox" -t "$registry_image:$commit" "$worktree"
docker push "$registry_image:sandbox"
docker push "$registry_image:$commit"

"${kubectl_ns[@]}" rollout restart deploy/payment-gateway
"${kubectl_ns[@]}" rollout status deploy/payment-gateway --timeout=300s
echo "deployed $commit"
