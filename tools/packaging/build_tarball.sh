#!/usr/bin/env bash
# Empaqueta el binario release de Baud en un tarball para el install script.
# Uso: ./tools/packaging/build_tarball.sh

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"
BINARY="$REPO_ROOT/target/release/baud"

if [[ ! -f "$BINARY" ]]; then
    echo "Error: binario release no encontrado en $BINARY" >&2
    echo "Ejecuta 'cargo build --release' primero." >&2
    exit 1
fi

ARCH="${BAUD_ARCH:-$(uname -m)}"
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    i386|i686)     ARCH="i386" ;;
    *) echo "Advertencia: arquitectura desconocida '$ARCH', usando nombre crudo." >&2 ;;
esac

OS="${BAUD_OS:-$(uname -s)}"
TARBALL_NAME="baud_${OS}_${ARCH}.tar.gz"

echo "Creando tarball ${TARBALL_NAME}..."

mkdir -p "$DIST_DIR"
tar czf "$DIST_DIR/$TARBALL_NAME" -C "$(dirname "$BINARY")" "$(basename "$BINARY")"

echo "Tarball generado: $DIST_DIR/$TARBALL_NAME"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$TARBALL_NAME" >> SHA256SUMS)
fi
