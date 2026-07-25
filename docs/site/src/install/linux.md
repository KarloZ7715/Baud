# Install on Linux

Baud currently supports Linux x86_64. The install script rejects any other OS or architecture.

## Installer script

```sh
curl -fsSL https://raw.githubusercontent.com/KarloZ7715/Baud/master/install.sh | sh
```

The script:

1. Downloads the latest release tarball for `Linux/x86_64` and its `SHA256SUMS` file from GitHub, and refuses to install unless the checksum matches exactly.
2. Installs the `baud` binary to `~/.local/bin` (or `${BAUD_INSTALL_PREFIX:-/usr/local}/bin` when run as root).
3. Registers a desktop entry and application icons under the matching XDG data directory (`~/.local/share` by default, or `$XDG_DATA_HOME` if set), so Baud appears in your application launcher.
4. Writes an ownership receipt (`.baud-install.toml`) next to the binary. Only installations with this receipt can use `baud update` later — see [Updates](../features/updates.md).

Running the script again reinstalls in place; it does not duplicate desktop entries.

To install into a system prefix instead of your home directory, run as root:

```sh
sudo sh -c 'curl -fsSL https://raw.githubusercontent.com/KarloZ7715/Baud/master/install.sh | sh'
```

Root installs go to `${BAUD_INSTALL_PREFIX:-/usr/local}/bin` and `${BAUD_INSTALL_PREFIX:-/usr/local}/share`. The script refuses to install into a prefix whose directories are symlinks, not owned by the installing user (or root), or group/world-writable.

## Distribution packages

Release builds also produce a `.deb` and an `.rpm` package (built with `cargo-deb` and `cargo-generate-rpm`) containing the binary, the desktop entry, icons, and the man page (`man baud`). Install whichever matches your distribution from the [Releases page](https://github.com/KarloZ7715/Baud/releases).

## After installing

If `~/.local/bin` is not already on your `PATH`, the installer prints the line to add to your shell profile. Continue with [Getting started](../getting-started.md).
