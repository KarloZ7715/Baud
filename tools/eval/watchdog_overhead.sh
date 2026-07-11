#!/usr/bin/env bash
# Harness JSON (watchdog overhead).
# Corre tests unitarios del módulo + microbench Criterion en release.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

cargo test --lib --quiet -- watchdog:: >/dev/null

# Solo la línea JSON (Criterion/cargo pueden escribir a stderr).
if ! OUT="$(cargo bench --bench watchdog_overhead -- --json 2>/dev/null | tail -n 1)"; then
  echo '{"hot_path_ns":999999,"ping_ns":999999,"enter_leave_ns":999999,"busy_ns":999999,"tests_passed":0}'
  exit 1
fi

if [[ "$OUT" != *'"hot_path_ns"'* ]]; then
  echo '{"hot_path_ns":999999,"ping_ns":999999,"enter_leave_ns":999999,"busy_ns":999999,"tests_passed":0}'
  exit 1
fi
printf '%s\n' "$OUT"
