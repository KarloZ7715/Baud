#!/bin/sh
# Installs Baud from GitHub releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/KarloZ7715/Baud/master/install.sh | sh

set -e

GITHUB_REPO="KarloZ7715/Baud"
PROGRAM_NAME="baud"

if [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
else
    INSTALL_DIR="${HOME}/.local/bin"
fi

print_message() {
    printf '\033[1;34m>> %s\033[0m\n' "$1"
}

print_error() {
    printf '\033[1;31mError: %s\033[0m\n' "$1"
    exit 1
}

detect_architecture() {
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            echo "arm64"
            ;;
        i386|i686)
            echo "i386"
            ;;
        *)
            print_error "Unsupported architecture: $arch"
            ;;
    esac
}

detect_os() {
    os=$(uname -s)
    case "$os" in
        Linux)
            echo "Linux"
            ;;
        Darwin)
            echo "Darwin"
            ;;
        *)
            print_error "Unsupported operating system: $os"
            ;;
    esac
}

get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name":' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

ensure_in_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*)
            return 0
            ;;
    esac

    print_message "${INSTALL_DIR} is not in your PATH."
    printf '\n  Add it by running:\n\n'
    printf '    echo '\''export PATH="%s:$PATH"'\'' >> ~/.bashrc  # or ~/.zshrc\n' "$INSTALL_DIR"
    printf '    source ~/.bashrc\n\n'
}

download_and_install() {
    os=$1
    arch=$2
    version=$3

    if [ -z "$version" ]; then
        print_error "Could not determine latest version"
    fi

    tarball="baud_${os}_${arch}.tar.gz"
    download_url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${tarball}"

    print_message "Downloading Baud ${version} (${os}/${arch})..."
    tmpdir=$(mktemp -d)
    trap 'rm -rf -- "$tmpdir"' EXIT

    if ! curl -fsSL --retry 3 -o "${tmpdir}/${tarball}" "$download_url"; then
        print_error "Failed to download ${tarball} from ${download_url}"
    fi

    print_message "Extracting to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    tar xzf "${tmpdir}/${tarball}" -C "$INSTALL_DIR"
    chmod +x "${INSTALL_DIR}/${PROGRAM_NAME}"

    print_message "Baud ${version} installed to ${INSTALL_DIR}/${PROGRAM_NAME}"
}

main() {
    print_message "Installing Baud..."

    if command -v baud >/dev/null 2>&1; then
        current=$(baud --version 2>/dev/null || echo "unknown")
        print_message "Baud ${current} is already installed. Reinstalling..."
    fi

    os=$(detect_os)
    arch=$(detect_architecture)
    version=$(get_latest_version)

    download_and_install "$os" "$arch" "$version"
    ensure_in_path
}

main
