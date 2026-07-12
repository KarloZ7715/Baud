#!/usr/bin/env bash
# Construye un AppImage de Baud usando linuxdeploy.
# Requisitos: cargo build --release previo, linuxdeploy descargable.
# Uso: ./tools/packaging/build_appimage.sh [--version X.Y.Z]

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"
BINARY="$REPO_ROOT/target/release/baud"
DESKTOP_FILE="$REPO_ROOT/assets/packaging/baud.desktop"
ICON_256="$REPO_ROOT/assets/icons/hicolor/256x256/apps/baud.png"

LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/2.0.0-alpha-1-20241106/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_CHECKSUM="sha256:93be974999444d69b27f37bb81a38033acf2e4e0b28b15210ff8a7e4c96a05c8"

VERSION="${BAUD_VERSION:-}"
if [[ -z "$VERSION" ]]; then
    if [[ "${1:-}" == "--version" ]]; then
        VERSION="$2"
    else
        VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
    fi
fi

if [[ ! -f "$BINARY" ]]; then
    echo "Error: binario no encontrado en $BINARY" >&2
    echo "Ejecuta 'cargo build --release' primero." >&2
    exit 1
fi

if [[ ! -f "$DESKTOP_FILE" ]]; then
    echo "Error: archivo .desktop no encontrado en $DESKTOP_FILE" >&2
    exit 1
fi

if [[ ! -f "$ICON_256" ]]; then
    echo "Error: icono 256x256 no encontrado en $ICON_256" >&2
    exit 1
fi

APPDIR="$(mktemp -d)"
trap 'rm -rf "$APPDIR"' EXIT

echo "Creando AppDir en $APPDIR..."

mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/48x48/apps"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/doc/baud"

cp "$BINARY" "$APPDIR/usr/bin/baud"
cp "$DESKTOP_FILE" "$APPDIR/usr/share/applications/baud.desktop"
cp "$REPO_ROOT/assets/icons/hicolor/48x48/apps/baud.png" "$APPDIR/usr/share/icons/hicolor/48x48/apps/baud.png"
cp "$ICON_256" "$APPDIR/usr/share/icons/hicolor/256x256/apps/baud.png"
cp "$REPO_ROOT/LICENSE" "$APPDIR/usr/share/doc/baud/LICENSE"

LINUXDEPLOY="$REPO_ROOT/tools/packaging/.cache/linuxdeploy-x86_64.AppImage"
if [[ ! -f "$LINUXDEPLOY" ]]; then
    echo "Descargando linuxdeploy..."
    mkdir -p "$(dirname "$LINUXDEPLOY")"
    curl -fSL --retry 3 -o "$LINUXDEPLOY" "$LINUXDEPLOY_URL"
    chmod +x "$LINUXDEPLOY"
fi

export ARCH=x86_64
export LINUXDEPLOY_OUTPUT_PREFIX="baud"
export LINUXDEPLOY_OUTPUT_VERSION="$VERSION"

echo "Ejecutando linuxdeploy..."
env APPIMAGE_EXTRACT_AND_RUN=1 "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/baud" \
    --desktop-file "$APPDIR/usr/share/applications/baud.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/baud.png" \
    --output appimage

APPIMAGE_NAME="baud-${VERSION}-x86_64.AppImage"
if [[ -f "$APPIMAGE_NAME" ]]; then
    mkdir -p "$DIST_DIR"
    mv -f "$APPIMAGE_NAME" "$DIST_DIR/$APPIMAGE_NAME"
    echo "AppImage generado: $DIST_DIR/$APPIMAGE_NAME"

    if command -v sha256sum &>/dev/null; then
        (cd "$DIST_DIR" && sha256sum "$APPIMAGE_NAME" >> SHA256SUMS)
    fi

    echo "Verificando ausencia de libGL/libEGL/libvulkan en AppDir..."
    if find "$APPDIR" \( -name 'libGL*' -o -name 'libEGL*' -o -name 'libvulkan*' \) | grep -q .; then
        echo "ADVERTENCIA: se encontraron bibliotecas GPU en AppDir que no deberían estar empaquetadas:" >&2
        find "$APPDIR" \( -name 'libGL*' -o -name 'libEGL*' -o -name 'libvulkan*' \) >&2
    else
        echo "OK: sin bibliotecas GL/Vulkan empaquetadas."
    fi
else
    echo "Error: no se generó el AppImage." >&2
    exit 1
fi
