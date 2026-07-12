#!/usr/bin/env bash
# Exercises the release asset verifier without building Baud packages.

set -Eeuo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT

dist="$tmpdir/dist"
mkdir -p "$dist"

printf '#!/bin/sh\necho fixture\n' > "$tmpdir/baud"
chmod 755 "$tmpdir/baud"
tar czf "$dist/baud_Linux_x86_64.tar.gz" -C "$tmpdir" baud
touch "$dist/baud-0.0.6-x86_64.AppImage" "$dist/baud_0.0.6_amd64.deb" "$dist/baud-0.0.6-1.x86_64.rpm"
(cd "$dist" && sha256sum *.AppImage *.deb *.rpm *.tar.gz > SHA256SUMS)

"$repo_root/tools/packaging/verify_release_assets.sh" "$dist"

printf '0%.0s' {1..64} > "$dist/SHA256SUMS"
printf '  baud_Linux_x86_64.tar.gz\n' >> "$dist/SHA256SUMS"
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: invalid manifest unexpectedly passed" >&2
    exit 1
fi

echo "release asset verifier tests passed"
