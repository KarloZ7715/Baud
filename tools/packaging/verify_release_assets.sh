#!/usr/bin/env bash
# Validates the complete Linux asset set before it is uploaded to a release.
# Requires the tarball to contain the desktop bundle profile.

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

if [[ ! -f "$dist_dir/update-manifest.json" ]]; then
    echo "Error: missing update-manifest.json" >&2
    exit 1
fi

if [[ ! -f "$dist_dir/update-manifest.sig" ]]; then
    echo "Error: missing update-manifest.sig" >&2
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

expected_files=(
    "baud"
    "share/applications/baud.desktop"
    "share/icons/hicolor/48x48/apps/baud.png"
    "share/icons/hicolor/256x256/apps/baud.png"
)

tar_entries="$(tar tzf "$dist_dir/$required_tarball")"

declare -A seen
while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue

    case "$entry" in
        *..*)
            echo "Error: traversal path detected in $required_tarball: $entry" >&2
            exit 1
            ;;
    esac

    if [[ -n "${seen["$entry"]:-}" ]]; then
        echo "Error: duplicate entry in $required_tarball: $entry" >&2
        exit 1
    fi
    seen["$entry"]=1
done <<< "$tar_entries"

tar_detail="$(tar tvzf "$dist_dir/$required_tarball")"
while IFS= read -r line; do
    [[ -z "$line" ]] && continue

    type="${line:0:1}"
    entry="${line##* }"

    case "$entry" in
        */)
            if [[ "$type" != "d" ]]; then
                echo "Error: trailing-slash entry is not a directory in $required_tarball: $entry" >&2
                exit 1
            fi
            ;;
        *)
            if [[ "$type" == "l" ]]; then
                echo "Error: symbolic link not allowed in $required_tarball: $entry" >&2
                exit 1
            fi
            if [[ "$type" == "h" ]]; then
                echo "Error: hard link not allowed in $required_tarball: $entry" >&2
                exit 1
            fi
            if [[ "$type" != "-" ]]; then
                echo "Error: unexpected file type in $required_tarball: $entry (type=$type)" >&2
                exit 1
            fi
            ;;
    esac
done <<< "$tar_detail"

present_files=()
while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    case "$entry" in
        */) ;;
        *) present_files+=("$entry") ;;
    esac
done <<< "$tar_entries"

if (( ${#present_files[@]} != ${#expected_files[@]} )); then
    echo "Error: expected ${#expected_files[@]} regular files in $required_tarball, found ${#present_files[@]}" >&2
    exit 1
fi

for expected in "${expected_files[@]}"; do
    found=0
    for present in "${present_files[@]}"; do
        if [[ "$present" == "$expected" ]]; then
            found=1
            break
        fi
    done
    if [[ "$found" -eq 0 ]]; then
        echo "Error: missing expected file in $required_tarball: $expected" >&2
        exit 1
    fi
done

echo "Release assets verified in $dist_dir"
