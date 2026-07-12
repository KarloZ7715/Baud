#!/usr/bin/env bash
# Construye un paquete .rpm de Baud usando cargo-generate-rpm.
# Requisitos: cargo-generate-rpm instalado (cargo install cargo-generate-rpm).
# Uso: ./tools/packaging/build_rpm.sh

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"

if ! cargo generate-rpm --version &>/dev/null 2>&1; then
    echo "Error: cargo-generate-rpm no está instalado." >&2
    echo "Instálalo con: cargo install cargo-generate-rpm" >&2
    exit 1
fi

if [[ ! -f "$REPO_ROOT/target/release/baud" ]]; then
    echo "Error: binario release no encontrado." >&2
    echo "Ejecuta 'cargo build --release' primero." >&2
    exit 1
fi

echo "Eliminando símbolos de depuración del binario..."
strip -s "$REPO_ROOT/target/release/baud"

echo "Construyendo paquete .rpm con cargo-generate-rpm..."
cd "$REPO_ROOT"
cargo generate-rpm

RPM_FILE="$(ls -1 target/generate-rpm/baud-*.rpm 2>/dev/null | tail -1 || true)"
if [[ -z "$RPM_FILE" ]]; then
    echo "Error: no se generó el .rpm." >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
cp "$RPM_FILE" "$DIST_DIR/"
echo "Paquete .rpm generado: $DIST_DIR/$(basename "$RPM_FILE")"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$(basename "$RPM_FILE")" >> SHA256SUMS)
fi

if command -v rpm &>/dev/null; then
    echo ""
    echo "Contenido del .rpm:"
    rpm -qlp "$DIST_DIR/$(basename "$RPM_FILE")"
fi
