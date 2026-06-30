#!/usr/bin/env bash
# Verificacion automatizada del pipeline PTY
#
# Uso:  ./tests/pty_pipeline_verify.sh
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd -P)"

log_info() {
    printf '[%s] INFO: %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "$*" >&2
}

log_error() {
    printf '[%s] ERROR: %s\n' "$(date +'%Y-%m-%d %H:%M:%S')" "$*" >&2
}

check_dependencies() {
    local -a missing=()
    local -a required=("cargo")

    for cmd in "${required[@]}"; do
        if ! command -v "$cmd" &>/dev/null; then
            missing+=("$cmd")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Faltan dependencias: ${missing[*]}"
        return 1
    fi
}

run_step() {
    local -r label="$1"
    shift
    log_info "$label"
    if "$@"; then
        log_info "OK: $label"
        return 0
    fi
    log_error "FAIL: $label"
    return 1
}

main() {
    local failures=0

    check_dependencies || exit 1
    cd "$REPO_ROOT" || exit 1

    run_step "cargo test event_loop::" \
        cargo test event_loop:: || failures=$((failures + 1))

    run_step "cargo test (suite completa)" \
        cargo test || failures=$((failures + 1))

    run_step "cargo clippy" \
        cargo clippy -- -D warnings || failures=$((failures + 1))

    if [[ "$failures" -gt 0 ]]; then
        log_error "pty_pipeline_verify: $failures paso(s) fallaron"
        exit 1
    fi

    log_info "pty_pipeline_verify: todos los checks pasaron"
    log_info "Carga manual (smoke): bash tests/pty_pipeline_load.sh dentro de baud"
    log_info "Carga agresiva: LOAD_PROFILE=stress bash tests/pty_pipeline_load.sh"
}

main "$@"
