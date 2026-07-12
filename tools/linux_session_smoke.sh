#!/usr/bin/env bash
# Smoke mínimo de sesión Linux para Baud.
# Sin display: sale 0 (skip). Con --xvfb: fuerza X11 vía xvfb-run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
Uso: tools/linux_session_smoke.sh [--xvfb] [--build]

  --xvfb   Ejecuta bajo xvfb-run (solo X11). Quita WAYLAND_*.
  --build  Compila release antes de lanzar.
  -h       Esta ayuda.

Sin DISPLAY/WAYLAND_DISPLAY y sin --xvfb: omite con exit 0.
EOF
}

USE_XVFB=0
DO_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --xvfb) USE_XVFB=1 ;;
    --build) DO_BUILD=1 ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "argumento desconocido: $arg" >&2
      usage >&2
      exit 2
      ;;
  esac
done

have_display() {
  [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]
}

if [[ "$USE_XVFB" -eq 0 ]] && ! have_display; then
  echo "linux_session_smoke: sin display; omitiendo (exit 0)"
  exit 0
fi

if [[ "$DO_BUILD" -eq 1 ]] || [[ ! -x "${CARGO_TARGET_DIR:-$ROOT/target}/release/baud" ]]; then
  echo "linux_session_smoke: compilando baud (release)..."
  cargo build --release --locked
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
BIN="${TARGET_DIR}/release/baud"
if [[ ! -x "$BIN" ]]; then
  echo "linux_session_smoke: no se encontró $BIN" >&2
  exit 1
fi
LOG="${TMPDIR:-/tmp}/baud-linux-session-smoke.log"

# Arranque breve: si el proceso vive ~1s, el smoke de crash pasó.
# Feeling completo (ASCII/resize/paste) es manual; ver docs/standards/linux-session-matrix.md.
run_baud_smoke() {
  local timeout_bin=""
  if command -v timeout >/dev/null 2>&1; then
    timeout_bin="timeout"
  elif command -v gtimeout >/dev/null 2>&1; then
    timeout_bin="gtimeout"
  fi

  if [[ -n "$timeout_bin" ]]; then
    "$timeout_bin" 3s "$BIN" >"$LOG" 2>&1 &
  else
    "$BIN" >"$LOG" 2>&1 &
  fi
  local pid=$!
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid" || true
    echo "linux_session_smoke: baud salió demasiado pronto" >&2
    tail -n 40 "$LOG" >&2 || true
    return 1
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "linux_session_smoke: ok (proceso vivo tras 1s)"
}

if [[ "$USE_XVFB" -eq 1 ]]; then
  if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "linux_session_smoke: xvfb-run no instalado; omitiendo (exit 0)"
    exit 0
  fi
  # Solo X11: no afirmar cobertura Wayland.
  exec env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    xvfb-run -a env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    bash -c "BIN='$BIN' LOG='$LOG'; $(declare -f run_baud_smoke); run_baud_smoke"
fi

run_baud_smoke
