#!/usr/bin/env bash
# Extracts one tagged section from CHANGELOG.md for a GitHub Release body.

set -Eeuo pipefail

if [[ $# -ne 2 ]]; then
    echo "Usage: $0 VERSION CHANGELOG" >&2
    exit 2
fi

version="$1"
changelog="$2"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: version must be X.Y.Z" >&2
    exit 1
fi

if [[ ! -f "$changelog" ]]; then
    echo "Error: changelog not found: $changelog" >&2
    exit 1
fi

awk -v version="$version" '
  $0 ~ "^## \\[" version "\\]" { found = 1; next }
  found && /^## \[/ { exit }
  found { print }
  END {
    if (!found) {
      exit 1
    }
  }
' "$changelog"
