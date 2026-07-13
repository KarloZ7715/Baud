#!/bin/sh
# Installs Baud from GitHub releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/KarloZ7715/Baud/master/install.sh | sh

set -e

GITHUB_REPO="KarloZ7715/Baud"
PROGRAM_NAME="baud"

if [ "$(id -u)" -eq 0 ]; then
    PREFIX="${BAUD_INSTALL_PREFIX:-/usr/local}"
    INSTALL_DIR="${PREFIX}/bin"
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
        *)
            print_error "Unsupported architecture: $arch (Baud currently supports Linux x86_64 only)"
            ;;
    esac
}

detect_os() {
    os=$(uname -s)
    case "$os" in
        Linux)
            echo "Linux"
            ;;
        *)
            print_error "Unsupported operating system: $os (Baud currently supports Linux x86_64 only)"
            ;;
    esac
}

get_latest_version() {
    json=$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest") || {
        print_error "Failed to fetch latest release info from GitHub"
    }
    version=$(echo "$json" | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')
    if [ -z "$version" ]; then
        print_error "Could not determine latest version from GitHub API"
    fi
    echo "$version"
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

xdg_data_dir() {
    if [ "$(id -u)" -eq 0 ]; then
        printf '%s/share' "$PREFIX"
        return
    fi
    xdg="${XDG_DATA_HOME:-}"
    if [ -n "$xdg" ]; then
        case "$xdg" in
            /*) printf '%s' "$xdg" ;;
            *)  printf '%s/.local/share' "$HOME" ;;
        esac
    else
        printf '%s/.local/share' "$HOME"
    fi
}

escape_desktop_exec() {
    raw="$1"
    escaped=$(printf '%s' "$raw" | sed 's/%/%%/g')
    case "$escaped" in
        *[[:space:]\"\$\`\\]*)
            printf '"%s"' "$escaped"
            ;;
        *)
            printf '%s' "$escaped"
            ;;
    esac
}

has_desktop_bundle() {
    test -f "${1}/share/applications/baud.desktop"
}

install_desktop_resources() {
    extract_dir="$1"
    data_dir="$2"
    exec_path="$3"

    app_dir="${data_dir}/applications"
    icon48_dir="${data_dir}/icons/hicolor/48x48/apps"
    icon256_dir="${data_dir}/icons/hicolor/256x256/apps"

    mkdir -p "$app_dir" "$icon48_dir" "$icon256_dir"
    chmod 755 "$app_dir" "$icon48_dir" "$icon256_dir" 2>/dev/null || true

    escaped_exec=$(escape_desktop_exec "$exec_path")
    awk -v exec="$escaped_exec" '
        /^Exec=/ { print "Exec=" exec; next }
        { print }
    ' "${extract_dir}/share/applications/baud.desktop" \
        > "${app_dir}/baud.desktop.tmp.$$"
    mv -f "${app_dir}/baud.desktop.tmp.$$" "${app_dir}/baud.desktop"

    cp "${extract_dir}/share/icons/hicolor/48x48/apps/baud.png" "${icon48_dir}/baud.png"
    cp "${extract_dir}/share/icons/hicolor/256x256/apps/baud.png" "${icon256_dir}/baud.png"

    print_message "Desktop entry installed to ${app_dir}/baud.desktop"
}

validate_archive_layout() {
    tar_entries=$(tar tzf "${tmpdir}/${tarball}" 2>/dev/null || true)
    if [ -z "$tar_entries" ]; then
        print_error "Cannot read tarball contents"
    fi

    for entry in $tar_entries; do
        case "$entry" in
            *..*)
                print_error "Rejected: traversal path in tarball: $entry"
                ;;
        esac
    done
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
    checksum_url="https://github.com/${GITHUB_REPO}/releases/download/${version}/SHA256SUMS"

    print_message "Downloading Baud ${version} (${os}/${arch})..."
    tmpdir=$(mktemp -d)
    staged_binary=""
    cleanup() {
        rm -rf -- "$tmpdir"
        if [ -n "$staged_binary" ]; then
            rm -f -- "$staged_binary"
        fi
    }
    trap cleanup EXIT INT TERM HUP

    if ! curl -fsSL --retry 3 -o "${tmpdir}/${tarball}" "$download_url"; then
        print_error "Failed to download ${tarball} from ${download_url}"
    fi

    if ! curl -fsSL --retry 3 -o "${tmpdir}/SHA256SUMS" "$checksum_url"; then
        print_error "Failed to download SHA256SUMS from ${checksum_url}"
    fi

    checksum_count=$(awk -v asset="$tarball" '$2 == asset { count++ } END { print count + 0 }' "${tmpdir}/SHA256SUMS")
    if [ "$checksum_count" -ne 1 ]; then
        print_error "SHA256SUMS must contain exactly one checksum for ${tarball}"
    fi

    expected_checksum=$(awk -v asset="$tarball" '$2 == asset { print $1 }' "${tmpdir}/SHA256SUMS")
    print_message "Verifying checksum..."
    if ! command -v sha256sum >/dev/null 2>&1; then
        print_error "sha256sum is required for checksum verification"
    fi

    if ! (cd "$tmpdir" && printf '%s  %s\n' "$expected_checksum" "$tarball" | sha256sum -c - >/dev/null); then
        print_error "Checksum verification failed for ${tarball}"
    fi

    validate_archive_layout

    mkdir -p "${tmpdir}/extract"
    if ! tar xzf "${tmpdir}/${tarball}" -C "${tmpdir}/extract"; then
        print_error "Failed to extract ${tarball}"
    fi

    if [ ! -f "${tmpdir}/extract/${PROGRAM_NAME}" ]; then
        print_error "Binary not found after extraction. The tarball may have an unexpected structure."
    fi

    print_message "Installing to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    staged_binary="${INSTALL_DIR}/.${PROGRAM_NAME}.tmp.$$"
    cp "${tmpdir}/extract/${PROGRAM_NAME}" "$staged_binary"
    chmod 755 "$staged_binary"
    mv -f "$staged_binary" "${INSTALL_DIR}/${PROGRAM_NAME}"
    staged_binary=""
    exec_path="${INSTALL_DIR}/${PROGRAM_NAME}"
    print_message "Baud ${version} installed to ${exec_path}"

    if has_desktop_bundle "${tmpdir}/extract"; then
        data_dir=$(xdg_data_dir)
        install_desktop_resources "${tmpdir}/extract" "$data_dir" "$exec_path"

        print_message "Baud is now available from your application launcher."
        printf '  If Baud does not appear immediately, your launcher may need\n'
        printf '  to rescan. Try logging out and back in.\n'
    else
        print_message "This release does not include desktop launcher files."
        printf '  Launcher registration requires a newer release with desktop\n'
        printf '  bundle support. The baud command is ready to use.\n'
    fi
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
