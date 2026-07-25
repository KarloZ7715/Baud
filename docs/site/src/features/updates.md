# Updates

`baud update` self-updates to the latest GitHub release. It only works on Linux x86_64, and only for an installation the updater recognizes as official.

## Which installs qualify

The [installer script](../install/linux.md) writes an ownership receipt (`.baud-install.toml`) next to the binary at install time; `baud update` refuses to run without it. This means:

- Installed via `install.sh` (to `~/.local/bin` or a root prefix): supported.
- Installed via the `.deb` or `.rpm` package: not supported today — those packages don't write the receipt. Update through your package manager instead.
- Windows: not supported at all; download the next release from the [Releases page](https://github.com/KarloZ7715/Baud/releases) and reinstall.

If the installation predates the receipt (an older install), `baud update` tells you to reinstall once via the official installer to enable it going forward.

## What happens

1. Checks the latest GitHub release tag against your installed version.
2. Downloads that release's signed update manifest and verifies its Ed25519 signature.
3. Downloads the release asset and checks it against the digest recorded in the manifest.
4. Replaces the binary (and desktop resources, if present) atomically, only after every check above passes.

Any verification failure — bad signature, digest mismatch, wrong platform in the manifest — aborts before anything on disk changes.
