#!/usr/bin/env bash
# Builds a .rpm package of Baud using cargo-generate-rpm.
# Prerequisites: cargo-generate-rpm installed (cargo install cargo-generate-rpm).
# Usage: ./tools/packaging/build_rpm.sh

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
DIST_DIR="$REPO_ROOT/dist"

if ! cargo generate-rpm --version &>/dev/null; then
    echo "Error: cargo-generate-rpm is not installed." >&2
    echo "Install it with: cargo install cargo-generate-rpm" >&2
    exit 1
fi

if [[ ! -f "$REPO_ROOT/target/release/baud" ]]; then
    echo "Error: release binary not found." >&2
    echo "Run 'cargo build --release' first." >&2
    exit 1
fi

if ! command -v strip &>/dev/null; then
    echo "Error: strip not found. Install binutils." >&2
    exit 1
fi

echo "Stripping debug symbols from binary..."
strip -s "$REPO_ROOT/target/release/baud"

echo "Building .rpm package with cargo-generate-rpm..."
cd "$REPO_ROOT"
cargo generate-rpm

RPM_FILE="$(ls -1t target/generate-rpm/baud-*.rpm 2>/dev/null | head -1 || true)"
if [[ -z "$RPM_FILE" ]]; then
    echo "Error: .rpm was not created." >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
cp "$RPM_FILE" "$DIST_DIR/"
echo ".rpm package created: $DIST_DIR/$(basename "$RPM_FILE")"

if command -v sha256sum &>/dev/null; then
    (cd "$DIST_DIR" && sha256sum "$(basename "$RPM_FILE")" > SHA256SUMS)
fi

if command -v rpm &>/dev/null; then
    echo ""
    echo "Package contents:"
    rpm -qlp "$DIST_DIR/$(basename "$RPM_FILE")"
fi
