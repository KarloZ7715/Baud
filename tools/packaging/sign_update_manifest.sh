#!/usr/bin/env bash
# Signs the Baud update manifest for a release.
#
# Usage:
#   BAUD_UPDATE_SIGNING_KEY=<32-byte hex seed> \
#     ./tools/packaging/sign_update_manifest.sh <dist_dir> <tag>
#
# Requires a matching `baud_Linux_x86_64.tar.gz` entry in `<dist_dir>/SHA256SUMS`.
# Writes `update-manifest.json` and `update-manifest.sig` next to the release assets.

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
SIGNER_DIR="$REPO_ROOT/tools/packaging/update_signer"

dist_dir="${1:-}"
tag="${2:-}"

if [[ -z "$dist_dir" || -z "$tag" ]]; then
    echo "Usage: ${0##*/} <dist_dir> <tag>" >&2
    exit 1
fi

if [[ ! -d "$dist_dir" ]]; then
    echo "Error: distribution directory not found: $dist_dir" >&2
    exit 1
fi

if [[ -z "${BAUD_UPDATE_SIGNING_KEY:-}" ]]; then
    echo "Error: BAUD_UPDATE_SIGNING_KEY is not set" >&2
    exit 1
fi

# Build the helper signer once and reuse the binary for speed.
SIGNER_BIN="$REPO_ROOT/target/baud-update-signer"
if [[ ! -x "$SIGNER_BIN" ]]; then
    cargo build --manifest-path "$SIGNER_DIR/Cargo.toml" --release
    cp "$SIGNER_DIR/target/release/baud-update-signer" "$SIGNER_BIN"
fi

"$SIGNER_BIN" "$dist_dir" "$tag"
