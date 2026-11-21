#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
registry_image="${BACKUP_IMAGE:-registry.v0l.io/payment-gateway-backup}"
tag="${BACKUP_TAG:-prod}"
gateway_image="${GATEWAY_IMAGE:-registry.v0l.io/payment-gateway:prod}"

if [ "${1:-}" != "--apply" ]; then
    echo "dry run: --apply builds $registry_image:$tag from deploy/Dockerfile.backup and pushes it"
    exit 0
fi

docker build --build-arg "GATEWAY_IMAGE=$gateway_image" -t "$registry_image:$tag" -f "$repo/deploy/Dockerfile.backup" "$repo"
docker push "$registry_image:$tag"
