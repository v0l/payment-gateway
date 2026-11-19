#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
state="${TMPDIR:-/tmp}/pgw-console-demo"
port="${DEMO_PORT:-18760}"
if [ "${1:-}" = "--stop" ]; then
  [ -f "$state/pid" ] && kill "$(cat "$state/pid")" 2>/dev/null || true
  [ -f "$state/db" ] && docker exec pgw-test-db mariadb -uroot -ptest -e "drop database if exists $(cat "$state/db")" || true
  rm -rf "$state"
  exit 0
fi
mkdir -p "$state"
db="pgw_demo_$(date +%s)"
docker exec pgw-test-db mariadb -uroot -ptest -e "create database $db"
echo "$db" > "$state/db"
(cd "$root" && CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo build --quiet --example console_demo)
DEMO_DATABASE_URL="mysql://root:test@127.0.0.1:33907/$db" DEMO_LISTEN="127.0.0.1:$port" \
  "${CARGO_TARGET_DIR:-$root/target}/debug/examples/console_demo" > "$state/log" 2>&1 &
echo $! > "$state/pid"
for _ in $(seq 1 300); do
  if grep -q "^LISTEN=" "$state/log" 2>/dev/null; then break; fi
  if ! kill -0 "$(cat "$state/pid")" 2>/dev/null; then cat "$state/log"; exit 1; fi
  sleep 0.5
done
cat "$state/log"
