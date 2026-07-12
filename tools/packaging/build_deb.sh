#!/usr/bin/env bash
# Construye un paquete .deb de Baud usando cargo-deb.
# Requisitos: cargo-deb instalado (cargo install cargo-deb).
# Uso: ./tools/packaging/build_deb.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$REPO_ROOT/dist"

if ! command -v cargo-deb &>/dev/null && ! cargo deb --version &>/dev/null 2>&1; then
    echo "Error: cargo-deb no está instalado." >&2
    echo "Instálalo con: cargo install cargo-deb" >&2
    exit 1
fi

if [[ ! -f "$REPO_ROOT/target/release/baud" ]]; then
    echo "Error: binario release no encontrado." >&2
    echo "Ejecuta 'cargo build --release' primero." >&2
    exit 1
fi

echo "Construyendo paquete .deb con cargo-deb..."
cd "$REPO_ROOT"
cargo deb --no-build

DEB_FILE="$(ls -1 target/debian/baud_*.deb 2>/dev/null | tail -1 || true)"
if [[ -z "$DEB_FILE" ]]; then
    echo "Error: no se generó el .deb." >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
cp "$DEB_FILE" "$DIST_DIR/"
echo "Paquete .deb generado: $DIST_DIR/$(basename "$DEB_FILE")"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$(basename "$DEB_FILE")" >> SHA256SUMS)
fi

if command -v dpkg-deb &>/dev/null; then
    echo ""
    echo "Contenido del .deb:"
    dpkg-deb -c "$DIST_DIR/$(basename "$DEB_FILE")"
fi
