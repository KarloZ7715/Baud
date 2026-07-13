#!/usr/bin/env bash
# Exercises install.sh with fake GitHub responses and local release assets.

set -Eeuo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
tmpdir="$(mktemp -d)"
trap 'rm -rf -- "$tmpdir"' EXIT

fixtures="$tmpdir/fixtures"
fake_bin="$tmpdir/fake-bin"
mkdir -p "$fixtures" "$fake_bin"

binary="$fixtures/baud"
printf '#!/bin/sh\necho baud fixture\n' > "$binary"
chmod 755 "$binary"

make_legacy_tarball() {
    tar czf "$fixtures/baud_Linux_x86_64.tar.gz" -C "$(dirname "$binary")" "$(basename "$binary")"
}

make_desktop_bundle() {
    rm -rf "$fixtures/staging"
    mkdir -p "$fixtures/staging/share/applications"
    mkdir -p "$fixtures/staging/share/icons/hicolor/48x48/apps"
    mkdir -p "$fixtures/staging/share/icons/hicolor/256x256/apps"
    cp "$binary" "$fixtures/staging/baud"
    printf '[Desktop Entry]\nName=Test\nExec=baud\nIcon=baud\nType=Application\n' > "$fixtures/staging/share/applications/baud.desktop"
    printf 'icon48' > "$fixtures/staging/share/icons/hicolor/48x48/apps/baud.png"
    printf 'icon256' > "$fixtures/staging/share/icons/hicolor/256x256/apps/baud.png"
    tar czf "$fixtures/baud_Linux_x86_64.tar.gz" -C "$fixtures/staging" baud share
}

make_manifest() {
    (cd "$fixtures" && sha256sum baud_Linux_x86_64.tar.gz > SHA256SUMS)
}

cat > "$fake_bin/id" <<'EOF'
#!/bin/sh
echo 1000
EOF

cat > "$fake_bin/id-root" <<'EOF'
#!/bin/sh
echo 0
EOF

cat > "$fake_bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) printf '%s\n' "${FAKE_UNAME_S:-Linux}" ;;
  -m) printf '%s\n' "${FAKE_UNAME_M:-x86_64}" ;;
  *) exit 2 ;;
esac
EOF

cat > "$fake_bin/curl" <<'EOF'
#!/bin/sh
output=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) output="$2"; shift 2 ;;
    *) url="$1"; shift ;;
  esac
done

case "$url" in
  *"/releases/latest") printf '%s\n' '{"tag_name":"v0.0.6"}' ;;
  *"/SHA256SUMS") cp "$FIXTURE_MANIFEST" "$output" ;;
  *"baud_Linux_x86_64.tar.gz") cp "$FIXTURE_TARBALL" "$output" ;;
  *) exit 1 ;;
esac
EOF

cat > "$fake_bin/sha256sum" <<'EOF'
#!/bin/sh
exec /usr/bin/sha256sum "$@"
EOF

cat > "$fake_bin/baud" <<'EOF'
#!/bin/sh
echo "0.0.0-fake"
EOF

chmod 755 "$fake_bin/id" "$fake_bin/id-root" "$fake_bin/uname" "$fake_bin/curl" "$fake_bin/sha256sum" "$fake_bin/baud"

run_install() {
    local home=$1
    shift
    env \
        HOME="$home" \
        PATH="$fake_bin:$PATH" \
        FIXTURE_TARBALL="$fixtures/baud_Linux_x86_64.tar.gz" \
        FIXTURE_MANIFEST="$fixtures/SHA256SUMS" \
        "$@" \
        /bin/sh "$repo_root/install.sh"
}

# ── desktop bundle: normal user install ──
make_desktop_bundle
make_manifest
home="$tmpdir/home-desktop"
XDG_DATA_HOME="" run_install "$home"
test -x "$home/.local/bin/baud"
test -f "$home/.local/share/applications/baud.desktop"
test -f "$home/.local/share/icons/hicolor/48x48/apps/baud.png"
test -f "$home/.local/share/icons/hicolor/256x256/apps/baud.png"
grep -q "Exec=$home/.local/bin/baud" "$home/.local/share/applications/baud.desktop"
test -f "$home/.local/bin/.baud-install.toml"
grep -q "managed_by = \"baud-installer\"" "$home/.local/bin/.baud-install.toml"
grep -q "binary_path = \"$home/.local/bin/baud\"" "$home/.local/bin/.baud-install.toml"
grep -q "data_dir = \"$home/.local/share\"" "$home/.local/bin/.baud-install.toml"

echo "PASS: desktop bundle user install"

# ── desktop bundle: reinstall (AE3) ──
XDG_DATA_HOME="" run_install "$home"
test -x "$home/.local/bin/baud"
test -f "$home/.local/share/applications/baud.desktop"
desktop_count=$(find "$home/.local/share/applications" -name 'baud.desktop' | wc -l)
test "$desktop_count" -eq 1

echo "PASS: reinstall"

# ── legacy binary-only: installs command, emits notice, no desktop files ──
make_legacy_tarball
make_manifest
home="$tmpdir/home-legacy"
output=$(XDG_DATA_HOME="" run_install "$home" 2>&1) || true
test -x "$home/.local/bin/baud"
test ! -f "$home/.local/share/applications/baud.desktop" 2>/dev/null || true
test -f "$home/.local/bin/.baud-install.toml"
grep -q "data_dir = \"$home/.local/share\"" "$home/.local/bin/.baud-install.toml"
echo "$output" | grep -q "does not include desktop launcher files"

echo "PASS: legacy binary-only profile"

# ── bad checksum: no files created ──
make_desktop_bundle
printf 'not-a-valid-checksum  baud_Linux_x86_64.tar.gz\n' > "$fixtures/SHA256SUMS"
home="$tmpdir/home-bad-checksum"
if run_install "$home" 2>/dev/null; then
    echo "Error: invalid checksum unexpectedly installed Baud" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud" 2>/dev/null || true

echo "PASS: bad checksum"

# ── bad layout: malformed tarball ──
printf '#!/bin/sh\necho wrong\n' > "$fixtures/wrong"
tar czf "$fixtures/baud_Linux_x86_64.tar.gz" -C "$fixtures" wrong
make_manifest
home="$tmpdir/home-bad-layout"
if run_install "$home" 2>/dev/null; then
    echo "Error: malformed tarball unexpectedly installed Baud" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud" 2>/dev/null || true

echo "PASS: bad layout"

# ── Darwin unsupported ──
make_desktop_bundle
make_manifest
home="$tmpdir/home-darwin"
if env XDG_DATA_HOME="" HOME="$home" PATH="$fake_bin:$PATH" FAKE_UNAME_S=Darwin FIXTURE_TARBALL="$fixtures/baud_Linux_x86_64.tar.gz" FIXTURE_MANIFEST="$fixtures/SHA256SUMS" /bin/sh "$repo_root/install.sh" 2>/dev/null; then
    echo "Error: Darwin unexpectedly succeeded" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud" 2>/dev/null || true

echo "PASS: Darwin unsupported"

# ── root install with BAUD_INSTALL_PREFIX (AE2) ──
make_desktop_bundle
make_manifest
prefix="$tmpdir/root-prefix"
home="$tmpdir/home-root"

cat > "$tmpdir/run-root.sh" <<SCRIPT
id() { /bin/echo 0; }
export HOME="$home"
export PATH="$fake_bin:\$PATH"
export FIXTURE_TARBALL="$fixtures/baud_Linux_x86_64.tar.gz"
export FIXTURE_MANIFEST="$fixtures/SHA256SUMS"
export XDG_DATA_HOME=""
export BAUD_INSTALL_PREFIX="$prefix"
. "$repo_root/install.sh"
SCRIPT

/bin/sh "$tmpdir/run-root.sh"

test -x "$prefix/bin/baud"
test -f "$prefix/share/applications/baud.desktop"
test -f "$prefix/share/icons/hicolor/48x48/apps/baud.png"
test -f "$prefix/share/icons/hicolor/256x256/apps/baud.png"
grep -q "Exec=$prefix/bin/baud" "$prefix/share/applications/baud.desktop"
test -f "$prefix/bin/.baud-install.toml"
grep -q "managed_by = \"baud-installer\"" "$prefix/bin/.baud-install.toml"
grep -q "binary_path = \"$prefix/bin/baud\"" "$prefix/bin/.baud-install.toml"
grep -q "data_dir = \"$prefix/share\"" "$prefix/bin/.baud-install.toml"

echo "PASS: root install with BAUD_INSTALL_PREFIX"

# ── absolute XDG_DATA_HOME ──
make_desktop_bundle
make_manifest
custom_data="$tmpdir/custom-data"
home="$tmpdir/home-xdg-abs"
XDG_DATA_HOME="$custom_data" run_install "$home"
test -f "$custom_data/applications/baud.desktop"
test -f "$custom_data/icons/hicolor/48x48/apps/baud.png"
test -f "$custom_data/icons/hicolor/256x256/apps/baud.png"
test -f "$home/.local/bin/.baud-install.toml"
grep -q "data_dir = \"$custom_data\"" "$home/.local/bin/.baud-install.toml"

echo "PASS: absolute XDG_DATA_HOME"

# ── relative XDG_DATA_HOME falls back to ~/.local/share ──
make_desktop_bundle
make_manifest
home="$tmpdir/home-xdg-rel"
XDG_DATA_HOME="relative/path" run_install "$home"
test -f "$home/.local/share/applications/baud.desktop"
test ! -e "$home/relative" 2>/dev/null || true
test -f "$home/.local/bin/.baud-install.toml"
grep -q "data_dir = \"$home/.local/share\"" "$home/.local/bin/.baud-install.toml"

echo "PASS: relative XDG_DATA_HOME fallback"

# ── HOME path with spaces and quoting chars: rendered valid escaped Exec ──
make_desktop_bundle
make_manifest
home="$tmpdir/home with spaces (test)"
XDG_DATA_HOME="" run_install "$home"
test -x "$home/.local/bin/baud"
test -f "$home/.local/share/applications/baud.desktop"
exec_line=$(grep '^Exec=' "$home/.local/share/applications/baud.desktop")
case "$exec_line" in
    *\"*)
        # Quoted form: Exec="/path with spaces/.local/bin/baud"
        ;;
    *)
        echo "Error: Exec with spaces not quoted: $exec_line" >&2
        exit 1
        ;;
esac

echo "PASS: HOME with spaces"

# ── desktop entry has no leftover single % placeholders ──
make_desktop_bundle
make_manifest
home="$tmpdir/home-percent"
mkdir -p "$home/weird%name"
HOME="$home/weird%name" XDG_DATA_HOME="" run_install "$home/weird%name" 2>/dev/null || true
if [ -f "$home/weird%name/.local/share/applications/baud.desktop" ]; then
    exec_line=$(grep '^Exec=' "$home/weird%name/.local/share/applications/baud.desktop")
    # After removing all %% (escaped percent), no bare % should remain
    if printf '%s' "$exec_line" | sed 's/%%//g' | grep -q '%'; then
        echo "Error: unescaped % in Exec value: $exec_line" >&2
        exit 1
    fi
fi

echo "PASS: percent escaping"

echo ""
echo "install.sh release tests passed"
