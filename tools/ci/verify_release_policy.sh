#!/usr/bin/env bash
# Verifies the repository policy for automated pre-1.0 release PRs.

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
if [[ ! "$current_version" =~ ^0\.0\.[0-9]+$ ]] && [[ "${RELEASE_MANUAL_VERSION:-false}" != "true" ]]; then
    printf 'Error: automated releases require a 0.0.z version, got %s\n' "$current_version" >&2
    exit 1
fi

require_line '^git_only = true$' "$release_config"
require_line '^publish = false$' "$release_config"
require_line '^release_always = false$' "$release_config"
require_line '^git_release_enable = false$' "$release_config"
require_line '^features_always_increment_minor = false$' "$release_config"
require_line '^release_commits = "\^\(feat\|fix\|perf\|security\)' "$release_config"

printf 'Release policy verified for Baud %s\n' "$current_version"
