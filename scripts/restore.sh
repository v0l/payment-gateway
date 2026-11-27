#!/usr/bin/env bash
set -euo pipefail

usage() { echo "usage: restore.sh <dump.sql.gz | restic:latest | restic:<snapshot>> <target database>" >&2; exit 2; }
[ $# -eq 2 ] || usage
source_ref="$1"
target="$2"

DB_HOST="${DB_HOST:-127.0.0.1}"
DB_PORT="${DB_PORT:-3306}"
DB_USER="${DB_USER:-root}"
export MYSQL_PWD="${DB_PASSWORD:-${MYSQL_PWD:-}}"

client="$(command -v mariadb || command -v mysql || true)"
[ -n "$client" ] || { echo "mariadb or mysql client not found" >&2; exit 1; }
sql() { "$client" --host="$DB_HOST" --port="$DB_PORT" --user="$DB_USER" "$@"; }

case "$target" in
    *[!A-Za-z0-9_]*|"") echo "target database name must be letters, digits and underscores" >&2; exit 2 ;;
esac

existing="$(sql -N -e "select count(*) from information_schema.schemata where schema_name = '$target'")"
if [ "$existing" != "0" ] && [ "${RESTORE_OVERWRITE:-}" != "1" ]; then
    echo "database $target already exists, set RESTORE_OVERWRITE=1 to replace it" >&2
    exit 1
fi

read_dump() {
    case "$source_ref" in
        restic:*) restic dump "${source_ref#restic:}" "${DB_SOURCE_NAME:?DB_SOURCE_NAME is required for restic}.sql" ;;
        *.gz) gzip -dc "$source_ref" ;;
        *) cat "$source_ref" ;;
    esac
}

sql -e "drop database if exists \`$target\`; create database \`$target\` character set utf8mb4"
read_dump \
    | sed -E 's/^(CREATE DATABASE|USE) .*$//I; s/^\/\*!40000 ALTER DATABASE.*$//' \
    | sql --database="$target"

if [ "${RESTORE_REQUIRE_INTEGRITY:-}" = "1" ]; then
    [ -n "${GATEWAY_BIN:-}" ] || { echo "RESTORE_REQUIRE_INTEGRITY is set but GATEWAY_BIN is not" >&2; exit 1; }
    [ -n "${GATEWAY_ENCRYPTION_KEYS:-}${GATEWAY_ENCRYPTION_KEY:-}" ] || { echo "RESTORE_REQUIRE_INTEGRITY is set but no encryption key is available" >&2; exit 1; }
fi

print_counts() {
    for table in tenant payment payment_attempt refund webhook_delivery; do
        echo "$table: $(sql -N --database="$target" -e "select count(*) from \`$table\`")"
    done
    echo "migrations: $(sql -N --database="$target" -e 'select count(*) from _sqlx_migrations')"
    echo "tables: $(sql -N -e "select count(*) from information_schema.tables where table_schema = '$target'")"
}
print_counts

if [ -n "${GATEWAY_BIN:-}" ]; then
    DATABASE_URL="mysql://${DB_USER}:${MYSQL_PWD}@${DB_HOST}:${DB_PORT}/${target}" "$GATEWAY_BIN" check-integrity
fi
echo "restored into $target"
