#!/usr/bin/env bash
# Verifies the repository policy for automated release PRs: without the
# release:manual-version label, a release-plz PR may only bump the patch
# (z) component relative to the latest released tag. The baseline is read
# from git tags rather than hardcoded, so it tracks x/y bumps automatically.

set -Eeuo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
manifest="$repo_root/Cargo.toml"
release_config="$repo_root/release-plz.toml"

require_line() {
    local pattern=$1
    local file=$2

    if ! grep -qE "$pattern" "$file"; then
        printf 'Error: expected %s in %s\n' "$pattern" "$file" >&2
        exit 1
    fi
}

current_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$manifest" | head -n1)"
if [[ ! "$current_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf 'Error: version in %s is not a valid x.y.z semver, got %s\n' "$manifest" "$current_version" >&2
    exit 1
fi

latest_tag="$(git -C "$repo_root" tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1)"
if [[ -z "$latest_tag" ]]; then
    printf 'Error: no previous release tag (vX.Y.Z) found to compare against\n' >&2
    exit 1
fi
baseline_version="${latest_tag#v}"

if [[ "${current_version%.*}" != "${baseline_version%.*}" ]] && [[ "${RELEASE_MANUAL_VERSION:-false}" != "true" ]]; then
    printf 'Error: automated releases may only bump the patch version (baseline %s), got %s\n' "$baseline_version" "$current_version" >&2
    exit 1
fi

require_line '^git_only = true$' "$release_config"
require_line '^publish = false$' "$release_config"
require_line '^release_always = false$' "$release_config"
require_line '^git_release_enable = false$' "$release_config"
require_line '^features_always_increment_minor = false$' "$release_config"
require_line '^release_commits = "\^\(feat\|fix\|perf\|security\)' "$release_config"

printf 'Release policy verified for Baud %s\n' "$current_version"
