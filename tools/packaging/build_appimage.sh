#!/usr/bin/env bash
# Builds a Baud AppImage using linuxdeploy.
# Prerequisites: cargo build --release, linuxdeploy (auto-downloaded if missing).
# Usage: ./tools/packaging/build_appimage.sh [--version X.Y.Z]

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"
BINARY="$REPO_ROOT/target/release/baud"
DESKTOP_FILE="$REPO_ROOT/assets/packaging/baud.desktop"
ICON_256="$REPO_ROOT/assets/icons/hicolor/256x256/apps/baud.png"

LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_CHECKSUM="sha256:c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"

VERSION="${BAUD_VERSION:-}"
if [[ -z "$VERSION" ]]; then
    if [[ "${1:-}" == "--version" ]]; then
        VERSION="$2"
    else
        VERSION="$(grep -m1 '^version' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
    fi
fi

if [[ ! -f "$BINARY" ]]; then
    echo "Error: release binary not found at $BINARY" >&2
    echo "Run 'cargo build --release' first." >&2
    exit 1
fi

if [[ ! -f "$DESKTOP_FILE" ]]; then
    echo "Error: .desktop file not found at $DESKTOP_FILE" >&2
    exit 1
fi

if [[ ! -f "$ICON_256" ]]; then
    echo "Error: 256x256 icon not found at $ICON_256" >&2
    exit 1
fi

APPDIR="$(mktemp -d)"
trap 'rm -rf -- "$APPDIR"' EXIT

ICON_48="$REPO_ROOT/assets/icons/hicolor/48x48/apps/baud.png"
LICENSE_FILE="$REPO_ROOT/LICENSE"

if [[ ! -f "$ICON_48" ]]; then
    echo "Error: 48x48 icon not found at $ICON_48" >&2
    exit 1
fi

if [[ ! -f "$LICENSE_FILE" ]]; then
    echo "Error: LICENSE file not found at $LICENSE_FILE" >&2
    exit 1
fi

echo "Creating AppDir at $APPDIR..."

mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/48x48/apps"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/doc/baud"

cp "$BINARY" "$APPDIR/usr/bin/baud"
cp "$DESKTOP_FILE" "$APPDIR/usr/share/applications/baud.desktop"
cp "$ICON_48" "$APPDIR/usr/share/icons/hicolor/48x48/apps/baud.png"
cp "$ICON_256" "$APPDIR/usr/share/icons/hicolor/256x256/apps/baud.png"
cp "$LICENSE_FILE" "$APPDIR/usr/share/doc/baud/LICENSE"

LINUXDEPLOY="$REPO_ROOT/tools/packaging/.cache/linuxdeploy-x86_64.AppImage"
if [[ ! -f "$LINUXDEPLOY" ]]; then
    echo "Downloading linuxdeploy..."
    mkdir -p "$(dirname "$LINUXDEPLOY")"
    curl -fSL --retry 3 -o "$LINUXDEPLOY" "$LINUXDEPLOY_URL"
    if ! echo "${LINUXDEPLOY_CHECKSUM#sha256:}  $LINUXDEPLOY" | sha256sum -c --status; then
        echo "Error: linuxdeploy checksum mismatch" >&2
        rm -f "$LINUXDEPLOY"
        exit 1
    fi
    chmod +x "$LINUXDEPLOY"
fi

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)
        echo "Error: unsupported architecture for AppImage: $ARCH" >&2
        exit 1
        ;;
esac
export ARCH="$ARCH"
export LINUXDEPLOY_OUTPUT_PREFIX="baud"
export LINUXDEPLOY_OUTPUT_VERSION="$VERSION"

echo "Running linuxdeploy..."
env APPIMAGE_EXTRACT_AND_RUN=1 "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/baud" \
    --desktop-file "$APPDIR/usr/share/applications/baud.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/baud.png" \
    --output appimage

shopt -s nullglob
generated_appimages=(./*-${VERSION}-${ARCH}.AppImage)
if (( ${#generated_appimages[@]} == 1 )); then
    APPIMAGE_NAME="baud-${VERSION}-${ARCH}.AppImage"
    mkdir -p "$DIST_DIR"
    mv -f -- "${generated_appimages[0]}" "$DIST_DIR/$APPIMAGE_NAME"
    echo "AppImage created: $DIST_DIR/$APPIMAGE_NAME"

    if command -v sha256sum &>/dev/null; then
        (cd "$DIST_DIR" && sha256sum "$APPIMAGE_NAME" > SHA256SUMS)
    fi

    echo "Checking for bundled GPU libraries in AppDir..."
    if find "$APPDIR" \( -name 'libGL*' -o -name 'libEGL*' -o -name 'libvulkan*' \) | grep -q .; then
        echo "WARNING: GPU libraries found in AppDir that should not be bundled:" >&2
        find "$APPDIR" \( -name 'libGL*' -o -name 'libEGL*' -o -name 'libvulkan*' \) >&2
    else
        echo "OK: no GL/Vulkan libraries bundled."
    fi
else
    echo "Error: AppImage was not created." >&2
    exit 1
fi
