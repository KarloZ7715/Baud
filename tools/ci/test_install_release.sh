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

make_tarball() {
    local source=$1
    tar czf "$fixtures/baud_Linux_x86_64.tar.gz" -C "$(dirname "$source")" "$(basename "$source")"
}

make_manifest() {
    (cd "$fixtures" && sha256sum baud_Linux_x86_64.tar.gz > SHA256SUMS)
}

cat > "$fake_bin/id" <<'EOF'
#!/bin/sh
echo 1000
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

chmod 755 "$fake_bin/id" "$fake_bin/uname" "$fake_bin/curl"

run_install() {
    local home=$1
    env \
        HOME="$home" \
        PATH="$fake_bin:$PATH" \
        FIXTURE_TARBALL="$fixtures/baud_Linux_x86_64.tar.gz" \
        FIXTURE_MANIFEST="$fixtures/SHA256SUMS" \
        /bin/sh "$repo_root/install.sh"
}

make_tarball "$binary"
make_manifest
home="$tmpdir/home-success"
run_install "$home"
test -x "$home/.local/bin/baud"
cmp "$binary" "$home/.local/bin/baud"

printf 'not-a-valid-checksum  baud_Linux_x86_64.tar.gz\n' > "$fixtures/SHA256SUMS"
home="$tmpdir/home-bad-checksum"
if run_install "$home"; then
    echo "Error: invalid checksum unexpectedly installed Baud" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud"

printf '#!/bin/sh\necho wrong\n' > "$fixtures/wrong"
tar czf "$fixtures/baud_Linux_x86_64.tar.gz" -C "$fixtures" wrong
make_manifest
home="$tmpdir/home-bad-layout"
if run_install "$home"; then
    echo "Error: malformed tarball unexpectedly installed Baud" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud"

home="$tmpdir/home-darwin"
if env HOME="$home" PATH="$fake_bin:$PATH" FAKE_UNAME_S=Darwin /bin/sh "$repo_root/install.sh"; then
    echo "Error: Darwin unexpectedly succeeded" >&2
    exit 1
fi
test ! -e "$home/.local/bin/baud"

echo "install.sh release tests passed"
