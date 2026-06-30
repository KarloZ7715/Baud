#!/usr/bin/env bash
# Cargas reproducibles para medir el pipeline PTY de Baud (manual).
# Ejecutar DENTRO de una sesion baud.
#
# Uso:
#   bash tests/pty_pipeline_load.sh              # perfil smoke (no congela baud)
#   LOAD_PROFILE=stress bash tests/pty_pipeline_load.sh  # carga del plan (agresiva)
#
# Metricas: RUST_LOG=baud::pipeline=debug cargo run
set -Eeuo pipefail

: "${LOAD_SECS:=5}"
: "${LOAD_PROFILE:=smoke}"

log_info() {
    printf '[%s] INFO: %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "$*" >&2
}

log_warn() {
    printf '[%s] WARN: %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "$*" >&2
}

usage() {
    cat <<'EOF'
Uso: tests/pty_pipeline_load.sh

Variables:
  LOAD_PROFILE   smoke (defecto) | stress
  LOAD_SECS        segundos por caso (defecto: 5)

smoke: cargas acotadas; baud debe seguir respondiendo.
stress: yes 50MB + seq 1M; puede saturar baud (solo medicion).
EOF
}

resolve_profile() {
    case "$LOAD_PROFILE" in
        smoke)
            YES_BYTES=524288
            SEQ_MAX=50000
            BURST_BYTES=65536
            ;;
        stress)
            YES_BYTES=50000000
            SEQ_MAX=1000000
            BURST_BYTES=0
            log_warn "LOAD_PROFILE=stress: carga agresiva; baud puede dejar de responder"
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'ERROR: LOAD_PROFILE invalido: %s (use smoke o stress)\n' "$LOAD_PROFILE" >&2
            exit 1
            ;;
    esac
}

run_load() {
    local -r label="$1"
    shift
    log_info "=== $label (~${LOAD_SECS}s) ==="
    if timeout "$LOAD_SECS" "$@"; then
        return 0
    fi
    local -r rc=$?
    if [[ "$rc" -eq 124 ]]; then
        log_info "timeout esperado (${LOAD_SECS}s)"
        return 0
    fi
    log_warn "comando termino con codigo $rc"
    return 0
}

pick_burst_source() {
    local -a candidates=(
        /var/log/syslog
        /var/log/messages
        /proc/cpuinfo
    )
    local path
    for path in "${candidates[@]}"; do
        if [[ -r "$path" ]]; then
            printf '%s\n' "$path"
            return 0
        fi
    done
    return 1
}

run_burst_load() {
    local -r source="$1"
    if [[ "$BURST_BYTES" -gt 0 ]]; then
        run_load "head de ${source} (${BURST_BYTES} bytes)" \
            head -c "$BURST_BYTES" "$source"
    else
        run_load "cat ${source} (rafaga)" \
            cat "$source"
    fi
}

main() {
    if ! command -v timeout &>/dev/null; then
        printf 'ERROR: se requiere el comando timeout\n' >&2
        exit 1
    fi

    if ! [[ "$LOAD_SECS" =~ ^[0-9]+$ ]] || [[ "$LOAD_SECS" -lt 1 ]]; then
        printf 'ERROR: LOAD_SECS debe ser un entero >= 1\n' >&2
        exit 1
    fi

    resolve_profile

    log_info "Iniciando cargas PTY (perfil=${LOAD_PROFILE}, ${LOAD_SECS}s por caso)"

    run_load "yes (throughput)" \
        sh -c "yes | head -c ${YES_BYTES}"

    run_load "seq (lineas)" \
        sh -c "seq 1 ${SEQ_MAX}"

    local burst_source
    if burst_source="$(pick_burst_source)"; then
        run_burst_load "$burst_source"
    else
        log_warn "sin fuente de rafaga legible; omitiendo caso cat"
    fi

    log_info "=== idle (sin output, medir CPU de baud en otra terminal) ==="
    sleep "$LOAD_SECS"

    log_info "Cargas completadas"
}

main "$@"
