#!/usr/bin/env bash
# Harness JSON para /ce-optimize (watchdog overhead).
# Corre tests unitarios del módulo + microbench release.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

cargo test --lib --quiet -- watchdog:: >/dev/null

# El example imprime JSON; si el bench falla, tests_passed=0.
if ! OUT="$(cargo run --release --quiet --example watchdog_overhead 2>/dev/null)"; then
  echo '{"hot_path_ns":999999,"ping_ns":999999,"enter_leave_ns":999999,"busy_ns":999999,"tests_passed":0}'
  exit 1
fi

# Asegurar tests_passed=1 (el example ya lo pone; revalidamos que hay JSON).
if [[ "$OUT" != *'"hot_path_ns"'* ]]; then
  echo '{"hot_path_ns":999999,"ping_ns":999999,"enter_leave_ns":999999,"busy_ns":999999,"tests_passed":0}'
  exit 1
fi
printf '%s\n' "$OUT"
