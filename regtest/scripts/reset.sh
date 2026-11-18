#!/usr/bin/env bash
source "$(dirname "$0")/lib.sh"
dc down -v
rm -rf "$REGTEST_DIR"
