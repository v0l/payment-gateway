#!/usr/bin/env bash
set -euo pipefail

: "${DB_NAME:?DB_NAME is required}"
DB_HOST="${DB_HOST:-127.0.0.1}"
DB_PORT="${DB_PORT:-3306}"
DB_USER="${DB_USER:-root}"
export MYSQL_PWD="${DB_PASSWORD:-${MYSQL_PWD:-}}"

dump_bin="$(command -v mariadb-dump || command -v mysqldump || true)"
[ -n "$dump_bin" ] || { echo "mariadb-dump or mysqldump not found" >&2; exit 1; }

dump() {
    "$dump_bin" --host="$DB_HOST" --port="$DB_PORT" --user="$DB_USER" \
        --single-transaction --quick --routines --triggers --hex-blob \
        --default-character-set=utf8mb4 --databases "$DB_NAME"
}

stamp="$(date -u +%Y%m%dT%H%M%SZ)"

if [ -n "${RESTIC_REPOSITORY:-}" ]; then
    dump | restic backup --stdin --stdin-filename "${DB_NAME}.sql" --tag payment-gateway --tag "$stamp"
    echo "restic:${RESTIC_REPOSITORY}"
    exit 0
fi

: "${BACKUP_DIR:?BACKUP_DIR or RESTIC_REPOSITORY is required}"
mkdir -p "$BACKUP_DIR"
out="$BACKUP_DIR/${DB_NAME}-${stamp}.sql.gz"
tmp="$out.partial"
trap 'rm -f "$tmp"' EXIT
dump | gzip -9 > "$tmp"
gzip -t "$tmp"
mv "$tmp" "$out"
trap - EXIT

if [ -n "${BACKUP_KEEP:-}" ]; then
    ls -1t "$BACKUP_DIR"/"${DB_NAME}"-*.sql.gz | tail -n +"$((BACKUP_KEEP + 1))" | xargs -r rm -f
fi
echo "$out"
