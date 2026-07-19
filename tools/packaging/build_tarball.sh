#!/usr/bin/env bash
# Packages the Baud release binary, desktop entry, and icons into a tarball.
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

DESKTOP_SRC="$REPO_ROOT/packaging/linux/baud.desktop"
ICON_48_SRC="$REPO_ROOT/assets/icons/hicolor/48x48/apps/baud.png"
ICON_256_SRC="$REPO_ROOT/assets/icons/hicolor/256x256/apps/baud.png"

for src in "$DESKTOP_SRC" "$ICON_48_SRC" "$ICON_256_SRC"; do
    if [[ ! -f "$src" ]]; then
        echo "Error: packaging resource not found: $src" >&2
        exit 1
    fi
done

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

echo "Creating tarball ${TARBALL_NAME} with desktop bundle profile..."

staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT

cp "$BINARY" "$staging/baud"

mkdir -p "$staging/share/applications"
cp "$DESKTOP_SRC" "$staging/share/applications/baud.desktop"

mkdir -p "$staging/share/icons/hicolor/48x48/apps"
cp "$ICON_48_SRC" "$staging/share/icons/hicolor/48x48/apps/baud.png"

mkdir -p "$staging/share/icons/hicolor/256x256/apps"
cp "$ICON_256_SRC" "$staging/share/icons/hicolor/256x256/apps/baud.png"

mkdir -p "$DIST_DIR"
tar czf "$DIST_DIR/$TARBALL_NAME" -C "$staging" baud share

echo "Tarball created: $DIST_DIR/$TARBALL_NAME"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$TARBALL_NAME" > SHA256SUMS)
fi
