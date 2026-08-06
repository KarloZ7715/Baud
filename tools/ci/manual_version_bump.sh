#!/usr/bin/env bash
# Redoes what a `release:manual-version` label authorizes but release-plz
# itself never computes: overrides the automated 0.0.z bump on an
# already-open release-plz PR branch with an intentional X or Y release.
#
# Run from inside the checked-out release-plz PR branch, e.g.:
#   gh pr checkout <PR number>
#   tools/ci/manual_version_bump.sh 0.1.0
#
# Leaves the result staged for review; does not commit, push, or touch
# labels — that stays a deliberate, separate step.
set -Eeuo pipefail

usage() {
    echo "Usage: tools/ci/manual_version_bump.sh <new-version>" >&2
    exit 1
}

[[ $# -eq 1 ]] || usage

new_version=$1
[[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "Error: version must be X.Y.Z, got $new_version" >&2
    exit 1
}

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
manifest="$repo_root/Cargo.toml"
changelog="$repo_root/CHANGELOG.md"

current_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$manifest" | head -n1)"
[[ -n "$current_version" ]] || {
    echo "Error: could not read current version from $manifest" >&2
    exit 1
}
[[ "$current_version" =~ ^0\.0\.[0-9]+$ ]] || {
    echo "Error: expected an automated 0.0.z version in $manifest, got $current_version" >&2
    echo "(this script overrides a pending release-plz PR, not an arbitrary bump)" >&2
    exit 1
}

echo "Bumping baud: $current_version -> $new_version"

# Only the [package] table's own version line, never a dependency pinned
# to the same string.
sed -i "/^\[package\]/,/^\[/ s/^version = \"$current_version\"\$/version = \"$new_version\"/" "$manifest"

# Retitle the pending CHANGELOG heading release-plz generated, keeping the
# base tag and date it picked, e.g.:
#   ## [0.0.9](.../compare/v0.0.8...v0.0.9) - 2026-08-05
# becomes
#   ## [0.1.0](.../compare/v0.0.8...v0.1.0) - 2026-08-05
current_escaped=$(printf '%s' "$current_version" | sed 's/\./\\./g')
sed -i \
    -e "s/^## \[$current_escaped\]/## [$new_version]/" \
    -e "s/\.\.\.v$current_escaped)/...v$new_version)/" \
    "$changelog"

# Resync Cargo.lock's own entry for this workspace member. `cargo metadata`
# rewrites a stale local-package version in place; it never touches
# dependency resolution since nothing else changed.
( cd "$repo_root" && cargo metadata --offline --format-version=1 > /dev/null )

# The generated man page embeds the crate version, so it goes stale the
# moment Cargo.toml changes.
( cd "$repo_root" && cargo run --bin docs-gen > /dev/null )

echo
echo "Done. Review the diff, then:"
echo "  RELEASE_MANUAL_VERSION=true tools/ci/verify_release_policy.sh"
echo "  git add Cargo.toml Cargo.lock CHANGELOG.md packaging/man/baud.1"
echo "  git commit -m \"chore: bump version to v$new_version\""
