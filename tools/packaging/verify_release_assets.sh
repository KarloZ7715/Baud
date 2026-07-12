#!/usr/bin/env bash
# Validates the complete Linux asset set before it is uploaded to a release.

set -Eeuo pipefail

dist_dir="${1:-dist}"
required_tarball="baud_Linux_x86_64.tar.gz"

if [[ ! -d "$dist_dir" ]]; then
    echo "Error: distribution directory not found: $dist_dir" >&2
    exit 1
fi

require_one() {
    local pattern=$1
    local matches=()

    shopt -s nullglob
    matches=("$dist_dir"/$pattern)
    shopt -u nullglob

    if (( ${#matches[@]} != 1 )); then
        printf 'Error: expected one asset matching %s, found %d\n' "$pattern" "${#matches[@]}" >&2
        exit 1
    fi
}

require_one '*.AppImage'
require_one '*.deb'
require_one '*.rpm'

if [[ ! -f "$dist_dir/$required_tarball" ]]; then
    echo "Error: missing $required_tarball" >&2
    exit 1
fi

manifest="$dist_dir/SHA256SUMS"
if [[ ! -s "$manifest" ]]; then
    echo "Error: missing SHA256SUMS" >&2
    exit 1
fi

if ! awk 'NF != 2 || ++seen[$2] > 1 { exit 1 }' "$manifest"; then
    echo "Error: SHA256SUMS must contain one checksum per asset" >&2
    exit 1
fi

expected_assets="$(find "$dist_dir" -maxdepth 1 -type f \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' -o -name '*.tar.gz' \) -printf '%f\n' | sort)"
manifest_assets="$(awk '{ print $2 }' "$manifest" | sort)"
if [[ "$expected_assets" != "$manifest_assets" ]]; then
    echo "Error: SHA256SUMS must cover exactly the release assets" >&2
    exit 1
fi

(cd "$dist_dir" && sha256sum -c --status SHA256SUMS)

tar_contents="$(tar tzf "$dist_dir/$required_tarball")"
if [[ "$tar_contents" != "baud" ]]; then
    echo "Error: $required_tarball must contain only a root-level baud binary" >&2
    exit 1
fi

echo "Release assets verified in $dist_dir"
