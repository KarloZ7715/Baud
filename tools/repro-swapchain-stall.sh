#!/usr/bin/env bash
# Repro del stall de adquisición del swapchain (plan 2026-08-02-001).
#
# Deja la ventana de Baud en un workspace oculto durante 60s con el cursor
# parpadeando, que es lo que dispara los redraws periodicos, y cuenta los
# stalls que quedaron en el log.
#
# Requiere Hyprland. Ejecutar desde la raiz del repo tras `cargo build --release`.
set -euo pipefail

LOG="$HOME/.local/state/baud/logs/baud.log.$(date +%F)"
HIDDEN_WS=9

if ! command -v hyprctl >/dev/null; then
  echo "requiere Hyprland (hyprctl no encontrado)" >&2
  exit 1
fi

before=$(grep -c "render lento" "$LOG" 2>/dev/null || echo 0)
echo "stalls antes: $before"

./target/release/baud &
baud_pid=$!
sleep 3

addr=$(hyprctl -j clients | jq -r ".[] | select(.pid == $baud_pid) | .address")
if [ -z "$addr" ]; then
  echo "no se encontro la ventana de baud (pid $baud_pid)" >&2
  kill "$baud_pid"
  exit 1
fi

echo "ocultando la ventana ($addr) en el workspace $HIDDEN_WS durante 60s..."
hyprctl dispatch movetoworkspacesilent "$HIDDEN_WS,address:$addr"
sleep 60
hyprctl dispatch movetoworkspace "1,address:$addr"
sleep 2

kill "$baud_pid" 2>/dev/null || true
sleep 1

after=$(grep -c "render lento" "$LOG" 2>/dev/null || echo 0)
echo "stalls despues: $after (delta: $((after - before)))"
echo
echo "--- desglose por espera ---"
grep "swapchain: no se pudo adquirir" "$LOG" 2>/dev/null | tail -20 || echo "ninguno"
