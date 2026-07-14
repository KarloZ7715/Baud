#!/bin/bash
# Harness manual para verificar el trafico de bytes de mouse reporting en Baud.
# Uso: ejecutar dentro de una sesion de Baud (por ejemplo, `bash tests/mouse_report_harness.sh`).
# Activa el modo de mouse solicitado, muestra los bytes recibidos con `cat -v`,
# y desactiva los modos al salir para no dejar el terminal en estado extraño.

set -euo pipefail

MODE="${1:-drag}"

reset_modes() {
    printf '\e[?1000l\e[?1002l\e[?1003l\e[?1004l\e[?1006l'
}
trap reset_modes EXIT INT TERM

case "$MODE" in
    click)
        printf 'Modo: click (1000 + SGR 1006)\n'
        printf '\e[?1000h\e[?1006h'
        ;;
    drag)
        printf 'Modo: drag (1002 + SGR 1006)\n'
        printf '\e[?1002h\e[?1006h'
        ;;
    anymotion)
        printf 'Modo: any motion (1003 + SGR 1006)\n'
        printf '\e[?1003h\e[?1006h'
        ;;
    focus)
        printf 'Modo: focus events (1004)\n'
        printf '\e[?1004h'
        printf 'Alterna el foco de la ventana (Alt-Tab). Se mostraran CSI O / CSI I.\n'
        ;;
    *)
        printf 'Uso: %s [click|drag|anymotion|focus]\n' "$0" >&2
        exit 1
        ;;
esac

printf 'Haz click / arrastra / alterna foco. Pulsa Ctrl-D para terminar.\n'
cat -v
