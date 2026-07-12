#!/usr/bin/env bash
# Packages the Baud release binary into a tarball for the install script.
# Usage: ./tools/packaging/build_tarball.sh

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"
BINARY="$REPO_ROOT/target/release/baud"

if [[ ! -f "$BINARY" ]]; then
    echo "Error: release binary not found at $BINARY" >&2
    echo "Run 'cargo build --release' first." >&2
    exit 1
fi

ARCH="${BAUD_ARCH:-$(uname -m)}"
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    i386|i686)     ARCH="i386" ;;
    *)
        echo "Error: unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

OS="${BAUD_OS:-$(uname -s)}"
case "$OS" in
    Linux|Darwin) ;;
    *)
        echo "Error: unsupported operating system: $OS" >&2
        exit 1
        ;;
esac
TARBALL_NAME="baud_${OS}_${ARCH}.tar.gz"

echo "Creating tarball ${TARBALL_NAME}..."

mkdir -p "$DIST_DIR"
tar czf "$DIST_DIR/$TARBALL_NAME" -C "$(dirname "$BINARY")" "$(basename "$BINARY")"

echo "Tarball created: $DIST_DIR/$TARBALL_NAME"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$TARBALL_NAME" > SHA256SUMS)
fi
