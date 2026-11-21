#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
registry_image="${BACKUP_IMAGE:-registry.v0l.io/payment-gateway-backup}"
tag="${BACKUP_TAG:-prod}"

if [ -n "${GATEWAY_IMAGE:-}" ]; then
    gateway_image="$GATEWAY_IMAGE"
else
    commit="$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || true)"
    [ -n "$commit" ] || { echo "set GATEWAY_IMAGE, for example registry.v0l.io/payment-gateway:<commit>" >&2; exit 1; }
    gateway_image="registry.v0l.io/payment-gateway:$commit"
fi

if [ "${1:-}" != "--apply" ]; then
    echo "dry run: --apply builds $registry_image:$tag from deploy/Dockerfile.backup and pushes it"
    echo "gateway image: $gateway_image"
    exit 0
fi

docker build --build-arg "GATEWAY_IMAGE=$gateway_image" -t "$registry_image:$tag" -f "$repo/deploy/Dockerfile.backup" "$repo"
docker push "$registry_image:$tag"
