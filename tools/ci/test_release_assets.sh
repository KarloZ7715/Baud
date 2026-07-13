#!/usr/bin/env bash
# Exercises the release asset verifier with the desktop bundle profile.

set -Eeuo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT

dist="$tmpdir/dist"
mkdir -p "$dist"

setup_staging() {
    rm -rf "$tmpdir/staging"
    mkdir -p "$tmpdir/staging/share/applications"
    mkdir -p "$tmpdir/staging/share/icons/hicolor/48x48/apps"
    mkdir -p "$tmpdir/staging/share/icons/hicolor/256x256/apps"
    printf '#!/bin/sh\necho fixture\n' > "$tmpdir/staging/baud"
    chmod 755 "$tmpdir/staging/baud"
    printf '[Desktop Entry]\nName=Fixture\nExec=baud\nIcon=baud\n' > "$tmpdir/staging/share/applications/baud.desktop"
    printf 'fake48png' > "$tmpdir/staging/share/icons/hicolor/48x48/apps/baud.png"
    printf 'fake256png' > "$tmpdir/staging/share/icons/hicolor/256x256/apps/baud.png"
}

make_bundle() {
    tar czf "$dist/baud_Linux_x86_64.tar.gz" -C "$tmpdir/staging" baud share
}

make_manifest() {
    (cd "$dist" && sha256sum *.AppImage *.deb *.rpm *.tar.gz > SHA256SUMS)
}

# ── positive: full desktop bundle passes ──
setup_staging
make_bundle
touch "$dist/baud-0.0.6-x86_64.AppImage" "$dist/baud_0.0.6_amd64.deb" "$dist/baud-0.0.6-1.x86_64.rpm"
make_manifest
"$repo_root/tools/packaging/verify_release_assets.sh" "$dist"

# ── negative: invalid manifest ──
printf '0%.0s' {1..64} > "$dist/SHA256SUMS"
printf '  baud_Linux_x86_64.tar.gz\n' >> "$dist/SHA256SUMS"
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: invalid manifest unexpectedly passed" >&2
    exit 1
fi

# ── negative: missing desktop entry ──
setup_staging
rm -f "$tmpdir/staging/share/applications/baud.desktop"
make_bundle
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: missing desktop entry unexpectedly passed" >&2
    exit 1
fi

# ── negative: missing 48 px icon ──
setup_staging
rm -f "$tmpdir/staging/share/icons/hicolor/48x48/apps/baud.png"
make_bundle
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: missing 48 px icon unexpectedly passed" >&2
    exit 1
fi

# ── negative: missing 256 px icon ──
setup_staging
rm -f "$tmpdir/staging/share/icons/hicolor/256x256/apps/baud.png"
make_bundle
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: missing 256 px icon unexpectedly passed" >&2
    exit 1
fi

# ── negative: extra file inside share tree ──
setup_staging
printf 'extra' > "$tmpdir/staging/share/extra.txt"
make_bundle
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: extra file in bundle unexpectedly passed" >&2
    exit 1
fi

# ── negative: traversal path via tar transform ──
setup_staging
rm -f "$tmpdir/staging/share/applications/baud.desktop"
printf 'traversal' > "$tmpdir/staging/trav"
tar czf "$dist/baud_Linux_x86_64.tar.gz" \
    -C "$tmpdir/staging" baud share \
    --transform 's|^trav$|../outside|' \
    -C "$tmpdir/staging" trav 2>/dev/null || true
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: traversal path in bundle unexpectedly passed" >&2
    exit 1
fi

# ── negative: symlink ──
setup_staging
ln -sf /etc/passwd "$tmpdir/staging/share/applications/link.desktop"
make_bundle
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: symlink in bundle unexpectedly passed" >&2
    exit 1
fi

# ── negative: duplicate entry ──
setup_staging
# Add the same file as an extra member with a different name
# by extracting and re-adding
tar czf "$dist/baud_Linux_x86_64.tar.gz" -C "$tmpdir/staging" baud share
# Now append a duplicate using --append (gnutar specific)
tar czf "$dist/tmp.tar.gz" -C "$tmpdir/staging" --transform 's|^baud$|baud_dup|' baud
mkdir -p "$tmpdir/concat"
gunzip -c "$dist/baud_Linux_x86_64.tar.gz" > "$tmpdir/concat/orig.tar"
tar czf "$dist/baud_Linux_x86_64.tar.gz" -C "$tmpdir/staging" baud share \
    --transform 's|^baud.desktop$|baud.desktop|' share/applications/baud.desktop || true
# Simpler approach for duplicate: archive with the same name twice via append
rm -f "$dist/baud_Linux_x86_64.tar.gz"
# tar doesn't deduplicate entries with same path, so archive share/applications twice
mkdir -p "$tmpdir/staging2/share/applications"
cp "$tmpdir/staging/share/applications/baud.desktop" "$tmpdir/staging2/share/applications/baud.desktop"
tar czf "$dist/baud_Linux_x86_64.tar.gz" \
    -C "$tmpdir/staging" baud share \
    -C "$tmpdir/staging2" share/applications/baud.desktop 2>/dev/null || true
make_manifest
if "$repo_root/tools/packaging/verify_release_assets.sh" "$dist"; then
    echo "Error: duplicate file in bundle unexpectedly passed" >&2
    exit 1
fi

echo "release asset verifier tests passed"
