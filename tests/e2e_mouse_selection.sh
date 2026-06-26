#!/bin/bash
# Test E2E de seleccion con mouse para Baud
# Requiere: ydotool, grim, jq, hyprctl
#
# USO:
#   ./tests/e2e_mouse_selection.sh
#
# Esto lanza Baud, hace click y arrastra para seleccionar texto,
# copia al clipboard con Ctrl+Shift+C, y verifica el resultado.
# Las capturas de pantalla quedan en /tmp/baud-e2e/

set -euo pipefail

BAUD_DIR="/home/carloscc/Documentos/Dev/baud"
BAUD_BIN="$BAUD_DIR/target/release/baud"
BAUD_LOG="/tmp/baud-e2e.log"
SCREENSHOT_DIR="/tmp/baud-e2e"

mkdir -p "$SCREENSHOT_DIR"

echo "=== E2E: Seleccion con mouse en Baud ==="
echo ""

# ---- 1. Compilar ----
echo "[1/8] Compilando Baud..."
cd "$BAUD_DIR"
cargo build --release 2>&1 | tail -3
echo "  OK: compilacion completa"
echo ""

# ---- 2. Lanzar Baud ----
echo "[2/8] Lanzando Baud..."
$BAUD_BIN > "$BAUD_LOG" 2>&1 &
BAUD_PID=$!
sleep 2

if ! kill -0 "$BAUD_PID" 2>/dev/null; then
    echo "FAIL: Baud no se lanzo. Log:"
    cat "$BAUD_LOG"
    exit 1
fi
echo "  OK: Baud PID=$BAUD_PID"
echo ""

# ---- 3. Obtener geometria de la ventana ----
echo "[3/8] Obteniendo geometria de la ventana..."
BAUD_WIN=$(hyprctl clients -j | jq '.[] | select(.title == "baud")')
if [ -z "$BAUD_WIN" ]; then
    echo "FAIL: ventana baud no encontrada en hyprctl"
    kill $BAUD_PID 2>/dev/null
    exit 1
fi

WIN_X=$(echo "$BAUD_WIN" | jq '.at[0]')
WIN_Y=$(echo "$BAUD_WIN" | jq '.at[1]')
WIN_W=$(echo "$BAUD_WIN" | jq '.size[0]')
WIN_H=$(echo "$BAUD_WIN" | jq '.size[1]')
echo "  Ventana en: x=$WIN_X y=$WIN_Y w=$WIN_W h=$WIN_H"

# Validar geometria
if [ "$WIN_W" -le 0 ] || [ "$WIN_H" -le 0 ]; then
    echo "FAIL: geometria invalida: ${WIN_W}x${WIN_H}"
    kill $BAUD_PID 2>/dev/null
    exit 1
fi
echo ""

# ---- 4. Captura inicial (antes de seleccionar) ----
echo "[4/8] Captura inicial..."
if command -v grim &> /dev/null; then
    grim "$SCREENSHOT_DIR/01-antes.png"
    echo "  OK: captura guardada en $SCREENSHOT_DIR/01-antes.png"
else
    echo "  WARN: grim no disponible, no se toma screenshot"
fi
echo ""

# ---- 5. Click y arrastrar para seleccionar ----
echo "[5/8] Simulando click+arrastre con ydotool..."

# Asumiendo cell_w ~ 10px, cell_h ~ 20px (valores típicos de Baud con font-size=14)
# Click en 1/3 de la ventana -> arrastre a 2/3
CLICK_X=$((WIN_X + WIN_W/3))
CLICK_Y=$((WIN_Y + WIN_H/3))
DRAG_X=$((WIN_X + 2*WIN_W/3))
DRAG_Y=$((WIN_Y + 2*WIN_H/3))

echo "  Click en: ($CLICK_X, $CLICK_Y)"
echo "  Arrastre a: ($DRAG_X, $DRAG_Y)"

# Verificar ydotool
if ! command -v ydotool &> /dev/null; then
    echo "  WARN: ydotool no disponible, simulacion de teclado omitida"
    SKIP_DRAG=true
else
    SKIP_DRAG=false
    
    # Asegurar que el servicio ydotool esta corriendo
    if ! pgrep -x ydotool > /dev/null; then
        echo "  INFO: Iniciando ydotoold..."
        ydotoold &
        sleep 1
    fi
    
    # Mover a posicion inicial
    ydotool mousemove -x $CLICK_X -y $CLICK_Y
    sleep 0.2
    
    # Click + arrastre
    ydotool click 1 --down
    sleep 0.05
    ydotool mousemove -x $DRAG_X -y $DRAG_Y
    sleep 0.1
    ydotool click 1 --up
    sleep 0.5
    
    echo "  OK: click+arrastre completado"
fi
echo ""

# ---- 5b. Captura post-seleccion ----
echo "[5b] Captura despues de seleccionar..."
if command -v grim &> /dev/null; then
    grim "$SCREENSHOT_DIR/02-despues-seleccion.png"
    echo "  OK: captura guardada en $SCREENSHOT_DIR/02-despues-seleccion.png"
    # Comparar con la captura inicial para detectar cambios visuales
    if [ -f "$SCREENSHOT_DIR/01-antes.png" ]; then
        PX_DIFF=$(compare -metric AE "$SCREENSHOT_DIR/01-antes.png" "$SCREENSHOT_DIR/02-despues-seleccion.png" /dev/null 2>&1 || true)
        if [ -n "$PX_DIFF" ] && [ "$PX_DIFF" -gt 0 ] 2>/dev/null; then
            echo "  Diferencia: $PX_DIFF pixeles cambiaron (la seleccion modifico el render)"
        else
            echo "  WARN: No se detectaron cambios visuales entre antes y despues de seleccionar"
            echo "  Esto podria indicar que la seleccion es INVISIBLE"
        fi
    fi
else
    echo "  WARN: grim no disponible"
fi
echo ""

# ---- 6. Copiar al clipboard (Ctrl+Shift+C) ----
echo "[6/8] Copiando seleccion al clipboard (Ctrl+Shift+C)..."

if [ "$SKIP_DRAG" = false ]; then
    # ydotool key: 29=Ctrl, 42=Shift, 46=C
    ydotool key 29 42 46
    sleep 0.3
    echo "  OK: Ctrl+Shift+C enviado"
else
    echo "  SKIP: ydotoold no disponible"
fi
echo ""

# ---- 7. Verificar clipboard ----
echo "[7/8] Verificando clipboard..."
if command -v wl-paste &> /dev/null; then
    CLIPBOARD=$(wl-paste 2>/dev/null || echo "")
    if [ -z "$CLIPBOARD" ]; then
        echo "  WARN: clipboard vacio (puede ser normal si la terminal esta limpia)"
    else
        CLIP_LEN=$(echo "$CLIPBOARD" | wc -c)
        CLIP_LINES=$(echo "$CLIPBOARD" | wc -l)
        echo "  OK: clipboard contiene $CLIP_LEN bytes en $CLIP_LINES lineas"
        echo "  Primeros 200 chars:"
        echo "    ${CLIPBOARD:0:200}"
    fi
else
    echo "  WARN: wl-paste no disponible"
fi
echo ""

# ---- 8. Cerrar Baud ----
echo "[8/8] Limpiando..."

# Extra: intentar copiar segunda vez (despues de que el grid se actualice)
# para probar que la seleccion persiste
if [ "$SKIP_DRAG" = false ]; then
    echo "  Info: esperando 1s y copiando de nuevo..."
    sleep 1
    ydotool key 29 42 46
    sleep 0.3
    if command -v wl-paste &> /dev/null; then
        CLIPBOARD2=$(wl-paste 2>/dev/null || echo "")
        if [ "$CLIPBOARD2" != "$CLIPBOARD" ]; then
            echo "  WARN: el clipboard cambio entre la primera y segunda copia"
        fi
    fi
fi

kill $BAUD_PID 2>/dev/null
wait $BAUD_PID 2>/dev/null || true
echo "  OK: Baud terminado (PID $BAUD_PID)"
echo ""

# ---- Resumen ----
echo "=== E2E test completo ==="
echo "Capturas en: $SCREENSHOT_DIR/"
echo "Log en: $BAUD_LOG"
echo ""

# Verificar si se detectaron bugs
BUGS=0
[ -f "$SCREENSHOT_DIR/02-despues-seleccion.png" ] || { echo "BUG: No se pudo capturar post-seleccion"; BUGS=$((BUGS+1)); }
if [ -z "$CLIPBOARD" ] && command -v wl-paste &> /dev/null; then
    echo "BUG: El clipboard quedo vacio despues de Ctrl+Shift+C"
    BUGS=$((BUGS+1))
fi

if [ $BUGS -gt 0 ]; then
    echo "Se detectaron $BUGS posible(s) bug(s)."
    exit 1
fi

echo "Sin fallos criticos detectados (revisar capturas visualmente)."