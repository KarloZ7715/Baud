#!/usr/bin/env bash
# Capturas A/B reproducibles de Baud frente a foot (Linux/Hyprland).
#
# grim actual no tiene -w: recorta por geometria de hyprctl (at + size).
#
# Windows (manual, no hay grim):
#   1. Abrir Baud y Windows Terminal con la misma fuente, tamano y specimen
#      (JetBrainsMono Nerd Font 11, perfil grayscale en WT).
#   2. Win+Shift+S sobre la misma region de contenido de cada ventana.
#   3. Guardar como ab_baud.png / ab_wt.png en el directorio de artifacts.
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
SPECIMEN="$ROOT/tools/render_specimen.txt"
OUTDIR=""

FONT="${FONT:-JetBrainsMono Nerd Font}"
FONT_SIZE="${FONT_SIZE:-11}"
CONTRAST="${CONTRAST:-0.0}"
WIN_W="${WIN_W:-900}"
WIN_H="${WIN_H:-720}"
WAIT_S="${WAIT_S:-2}"
BAUD_BIN="${BAUD_BIN:-}"
CAPTURE_BAUD=1
CAPTURE_FOOT=1

usage() {
  cat <<EOF
Uso: tools/render_ab.sh [outdir] [opciones]

  --contrast N     font.text_contrast de Baud (default: ${CONTRAST})
  --baud-bin PATH  binario de baud (default: target/release/baud o debug)
  --baud-only      solo captura Baud
  --foot-only      solo captura foot
  -h               esta ayuda

Variables: FONT FONT_SIZE CONTRAST WIN_W WIN_H WAIT_S BAUD_BIN
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --contrast)
      CONTRAST="$2"
      shift 2
      ;;
    --baud-bin)
      BAUD_BIN="$2"
      shift 2
      ;;
    --baud-only) CAPTURE_FOOT=0; shift ;;
    --foot-only) CAPTURE_BAUD=0; shift ;;
    -h|--help) usage; exit 0 ;;
    --)
      shift
      break
      ;;
    -*)
      echo "argumento desconocido: $1" >&2
      usage >&2
      exit 2
      ;;
    *)
      if [[ -n "$OUTDIR" ]]; then
        echo "argumento desconocido: $1" >&2
        usage >&2
        exit 2
      fi
      OUTDIR="$1"
      shift
      ;;
  esac
done
OUTDIR="${OUTDIR:-$PWD}"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "falta el comando: $1" >&2
    exit 1
  }
}

need grim
need hyprctl
need python3
[[ -f "$SPECIMEN" ]] || {
  echo "no existe el specimen: $SPECIMEN" >&2
  exit 1
}

if [[ -z "${WAYLAND_DISPLAY:-}" || -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
  echo "hace falta una sesion Hyprland (WAYLAND_DISPLAY + HYPRLAND_INSTANCE_SIGNATURE)" >&2
  exit 1
fi

mkdir -p -- "$OUTDIR"
OUTDIR="$(cd -- "$OUTDIR" && pwd -P)"

resolve_baud() {
  if [[ -n "$BAUD_BIN" ]]; then
    printf '%s\n' "$BAUD_BIN"
    return
  fi
  local cand
  for cand in \
    "${CARGO_TARGET_DIR:-$ROOT/target}/release/baud" \
    "$ROOT/target/release/baud" \
    "${CARGO_TARGET_DIR:-$ROOT/target}/debug/baud" \
    "$ROOT/target/debug/baud"; do
    if [[ -x "$cand" ]]; then
      printf '%s\n' "$cand"
      return
    fi
  done
  echo "no se encontro el binario de baud; pasa --baud-bin o compila" >&2
  exit 1
}

# Espera a que hyprctl vea una ventana mapped de esa class y la deja
# flotante al tamano pedido. Imprime address\tpid.
wait_client() {
  local class="$1"
  python3 - "$class" "$WIN_W" "$WIN_H" <<'PY'
import json, subprocess, sys, time

class_name, win_w, win_h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
deadline = time.monotonic() + 12.0
address = None
pid = None
while time.monotonic() < deadline:
    clients = json.loads(subprocess.check_output(["hyprctl", "clients", "-j"]))
    for c in clients:
        if c.get("class") == class_name and c.get("mapped") and not c.get("hidden"):
            address = c["address"]
            pid = c["pid"]
            break
    if address:
        break
    time.sleep(0.1)
if not address:
    sys.stderr.write(f"timeout esperando ventana class={class_name}\n")
    sys.exit(1)

def dispatch(lua):
    subprocess.check_call(["hyprctl", "dispatch", lua], stdout=subprocess.DEVNULL)

win = f'window = "address:{address}"'
dispatch(f'hl.dsp.window.float({{ action = "enable", {win} }})')
dispatch(f'hl.dsp.window.alter_zorder({{ mode = "top", {win} }})')
dispatch(f'hl.dsp.focus({{ {win} }})')
dispatch(f'hl.dsp.window.resize({{ x = {win_w}, y = {win_h}, relative = false, {win} }})')
dispatch(f'hl.dsp.window.move({{ x = 80, y = 80, relative = false, {win} }})')
print(f"{address}\t{pid}")
PY
}

client_geometry() {
  local address="$1"
  python3 - "$address" <<'PY'
import json, subprocess, sys
address = sys.argv[1]
clients = json.loads(subprocess.check_output(["hyprctl", "clients", "-j"]))
for c in clients:
    if c.get("address") == address:
        x, y = c["at"]
        w, h = c["size"]
        print(f"{x},{y} {w}x{h}")
        sys.exit(0)
sys.stderr.write(f"ventana {address} desaparecio\n")
sys.exit(1)
PY
}

capture_window() {
  local address="$1"
  local dest="$2"
  local geom
  # Enfocar justo antes de grim: si la ventana queda inactiva Hyprland
  # la atenua y el A/B compara peso de trazo con brillo de compositor.
  hyprctl dispatch "hl.dsp.focus({ window = \"address:${address}\" })" >/dev/null
  hyprctl dispatch "hl.dsp.window.alter_zorder({ mode = \"top\", window = \"address:${address}\" })" >/dev/null
  hyprctl dispatch "hl.dsp.window.pin({ action = \"enable\", window = \"address:${address}\" })" >/dev/null
  sleep 0.5
  geom="$(client_geometry "$address")"
  echo "captura $address geom=$geom -> $dest"
  grim -g "$geom" "$dest"
}

# Pega el specimen cuando la ventana ya tiene tamano, para que no se vaya al scrollback.
feed_specimen() {
  local pid="$1"
  python3 - "$pid" "Sphinx" "$SPECIMEN" <<'PY'
import json, os, socket, sys, time
from pathlib import Path

pid, pattern, specimen_path = sys.argv[1], sys.argv[2], sys.argv[3]
runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
sock_path = os.path.join(runtime, "baud", f"{pid}.sock")
token_path = os.path.join(runtime, "baud", f"{pid}.token")
deadline = time.monotonic() + 12.0
while time.monotonic() < deadline:
    if os.path.exists(sock_path) and os.path.exists(token_path):
        break
    time.sleep(0.05)
else:
    sys.stderr.write(f"timeout esperando socket {sock_path}\n")
    sys.exit(1)

token = open(token_path, encoding="utf-8").read().strip()
specimen = Path(specimen_path).read_text(encoding="utf-8")
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(12)
sock.connect(sock_path)
buf = sock.makefile("rwb")

def rpc(req):
    buf.write((json.dumps(req) + "\n").encode())
    buf.flush()
    line = buf.readline()
    if not line:
        raise SystemExit("socket cerrado")
    return json.loads(line)

hello = rpc({"id": 1, "method": "hello", "params": {"token": token}})
if "err" in hello:
    raise SystemExit(f"hello: {hello}")
sent = rpc({"id": 2, "method": "send_text", "params": {"text": specimen}})
if "err" in sent:
    raise SystemExit(f"send_text: {sent}")
waited = rpc({
    "id": 3,
    "method": "wait_for",
    "params": {"pattern": pattern, "timeout_ms": 8000},
})
if "err" in waited:
    screen = rpc({"id": 4, "method": "screen_text", "params": {}})
    sys.stderr.write(f"wait_for fallo: {waited}\n")
    sys.stderr.write(f"pantalla: {screen}\n")
    sys.exit(1)
screen = rpc({"id": 4, "method": "screen_text", "params": {}})
lines = screen.get("ok", {}).get("lines", [])
preview = next((ln for ln in lines if ln.strip()), "")
print(f"pantalla pid={pid}: {preview[:80]}", file=sys.stderr)
PY
}

TMPDIR=""
PIDS=()
cleanup() {
  local pid
  for pid in "${PIDS[@]+"${PIDS[@]}"}"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  if [[ -n "$TMPDIR" && -d "$TMPDIR" ]]; then
    rm -rf -- "$TMPDIR"
  fi
}
trap cleanup EXIT

TMPDIR="$(mktemp -d "${TMPDIR:-/tmp}/baud-render-ab.XXXXXX")"
mkdir -p -- "$TMPDIR/config/baud" "$TMPDIR/state/baud/logs" "$TMPDIR/foot"

if [[ "$CAPTURE_BAUD" -eq 1 ]]; then
  BAUD_BIN="$(resolve_baud)"
  cat >"$TMPDIR/config/baud/config.toml" <<EOF
remote_control = true

[theme]
import = false

[font]
family = "$FONT"
size = $FONT_SIZE
text_contrast = $CONTRAST
builtin_box_drawing = false

[window]
padding_x = 0
padding_y = 0
decorations = "none"
startup = "windowed"
width = $WIN_W
height = $WIN_H
EOF

  echo "usando $BAUD_BIN"
  # No tocar XDG_RUNTIME_DIR: ahi vive el socket de Wayland del compositor.
  XDG_CONFIG_HOME="$TMPDIR/config" \
    XDG_STATE_HOME="$TMPDIR/state" \
    BAUD_SKIP_CONSENT_UI=1 \
    "$BAUD_BIN" \
    --app-id baud-render-ab \
    --title baud-render-ab \
    -e cat \
    >/dev/null 2>"$TMPDIR/baud.err" &
  PIDS+=("$!")
  if ! baud_info="$(wait_client baud-render-ab)"; then
    echo "baud no abrio ventana; stderr:" >&2
    cat "$TMPDIR/baud.err" >&2 || true
    exit 1
  fi
  baud_addr="${baud_info%%$'\t'*}"
  baud_pid="${baud_info##*$'\t'}"
  if ! feed_specimen "$baud_pid"; then
    echo "el specimen no aparecio en la pantalla de baud" >&2
    cat "$TMPDIR/baud.err" >&2 || true
    exit 1
  fi
  sleep "$WAIT_S"
  capture_window "$baud_addr" "$OUTDIR/ab_baud.png"
  kill -TERM "${PIDS[-1]}" 2>/dev/null || true
  wait "${PIDS[-1]}" 2>/dev/null || true
  PIDS=()
  echo "baud -> $OUTDIR/ab_baud.png"
fi

if [[ "$CAPTURE_FOOT" -eq 1 ]]; then
  need foot
  foot \
    --app-id foot-render-ab \
    --title foot-render-ab \
    --hold \
    --window-size-pixels="${WIN_W}x${WIN_H}" \
    --font="${FONT}:size=${FONT_SIZE}" \
    --override=pad=0x0 \
    --override=csd.preferred=none \
    --override=colors.background=0a0a0a \
    --override=colors.foreground=ececec \
    cat "$SPECIMEN" \
    >/dev/null 2>"$TMPDIR/foot.err" &
  PIDS+=("$!")
  if ! foot_info="$(wait_client foot-render-ab)"; then
    echo "foot no abrio ventana; stderr:" >&2
    cat "$TMPDIR/foot.err" >&2 || true
    exit 1
  fi
  foot_addr="${foot_info%%$'\t'*}"
  sleep "$WAIT_S"
  capture_window "$foot_addr" "$OUTDIR/ab_foot.png"
  kill -TERM "${PIDS[-1]}" 2>/dev/null || true
  wait "${PIDS[-1]}" 2>/dev/null || true
  PIDS=()
  echo "foot -> $OUTDIR/ab_foot.png"
fi
