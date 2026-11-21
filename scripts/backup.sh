#!/usr/bin/env bash
set -euo pipefail

: "${DB_NAME:?DB_NAME is required}"
DB_HOST="${DB_HOST:-127.0.0.1}"
DB_PORT="${DB_PORT:-3306}"
DB_USER="${DB_USER:-root}"
export MYSQL_PWD="${DB_PASSWORD:-${MYSQL_PWD:-}}"

dump_bin="${DUMP_BIN:-$(command -v mariadb-dump || command -v mysqldump || true)}"
[ -n "$dump_bin" ] || { echo "mariadb-dump or mysqldump not found" >&2; exit 1; }
dump_extra=()
dump_help="$("$dump_bin" --help 2>/dev/null || true)"
if [[ "$dump_help" == *--column-statistics* ]]; then
    dump_extra+=(--column-statistics=0)
fi

dump() {
    "$dump_bin" --host="$DB_HOST" --port="$DB_PORT" --user="$DB_USER" \
        --single-transaction --quick --routines --triggers --hex-blob \
        --default-character-set=utf8mb4 "${dump_extra[@]}" --databases "$DB_NAME"
}

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
restic_host="${RESTIC_HOST:-payment-gateway}"

if [ -n "${RESTIC_REPOSITORY:-}" ]; then
    restic cat config >/dev/null 2>&1 || restic init
    staging="$(mktemp "${TMPDIR:-/tmp}/${DB_NAME}-XXXXXX.sql")"
    trap 'rm -f "$staging"' EXIT
    dump > "$staging"
    restic backup --stdin --stdin-filename "${DB_NAME}.sql" --host "$restic_host" --tag payment-gateway --tag "$stamp" < "$staging"
    if [ -n "${RESTIC_FORGET_ARGS:-}" ]; then
        read -ra forget_args <<< "$RESTIC_FORGET_ARGS"
        restic forget --prune --host "$restic_host" --tag payment-gateway "${forget_args[@]}"
    fi
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
