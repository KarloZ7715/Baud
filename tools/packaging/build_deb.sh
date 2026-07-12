#!/usr/bin/env bash
# Builds a .deb package of Baud using cargo-deb.
# Prerequisites: cargo-deb installed (cargo install cargo-deb).
# Usage: ./tools/packaging/build_deb.sh

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"

if ! cargo deb --version &>/dev/null; then
    echo "Error: cargo-deb is not installed." >&2
    echo "Install it with: cargo install cargo-deb" >&2
    exit 1
fi

if [[ ! -f "$REPO_ROOT/target/release/baud" ]]; then
    echo "Error: release binary not found." >&2
    echo "Run 'cargo build --release' first." >&2
    exit 1
fi

echo "Building .deb package with cargo-deb..."
cd "$REPO_ROOT"
cargo deb --no-build

DEB_FILE="$(ls -1t target/debian/baud_*.deb 2>/dev/null | head -1 || true)"
if [[ -z "$DEB_FILE" ]]; then
    echo "Error: .deb was not created." >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
cp "$DEB_FILE" "$DIST_DIR/"
echo ".deb package created: $DIST_DIR/$(basename "$DEB_FILE")"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$(basename "$DEB_FILE")" > SHA256SUMS)
fi

if command -v dpkg-deb &>/dev/null; then
    echo ""
    echo "Package contents:"
    dpkg-deb -c "$DIST_DIR/$(basename "$DEB_FILE")" || echo "Warning: could not list .deb contents" >&2
fi
