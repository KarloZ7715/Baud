# Security model

This page lists **checkable controls** for self-update and install ownership. Each row names the function or const that enforces the control. If a control is not listed, do not assume it exists.

User-facing summary: [Updates](../features/updates.md). Implementation: `src/updater.rs`, `src/installation.rs`.

## Updater controls

| Control | Where |
| --- | --- |
| Embedded Ed25519 verifying key (`UPDATE_PUBLIC_KEY_BYTES`, key id `baud-update-v1`) | `load_embedded_key` |
| Refuse to run if the key is all zeros | `fetch_and_verify_manifest` → `UpdateError::KeyNotProvisioned` |
| Download manifest + detached signature with size caps | `Updater::fetch_and_verify_manifest` |
| `verify_strict` over raw manifest bytes | same |
| Manifest contract: `version == 1`, matching `key_id`, `platform == "Linux_x86_64"`, `profile == "desktop-bundle"`, asset name `baud_Linux_x86_64.tar.gz`, tag equals release tag, sha256 hex length 64 | same |
| Archive digest equals manifest `sha256` | `Updater::download_asset` |
| `SHA256SUMS` contains exactly one entry for the asset and matches the manifest digest | `parse_checksums` + `download_asset` |
| HTTPS-only downloads | updater HTTP helpers (`https_only`) |
| Platform gate: Linux x86_64 only | `ensure_supported_platform` |
| Latest tag must parse as `vX.Y.Z` | `fetch_latest_release` |
| Stage under a private temp dir (mode `0o700` on Unix) | `stage_archive` |
| Safe tar paths only (no `..`, absolute, or odd components) | `is_safe_archive_path`, `validate_and_extract` |
| Allowlisted members, size limits, no symlinks/dupes | `validate_and_extract`, `validate_file_size`, `allowed_dir` |
| Staging must be exactly the desktop bundle set (`baud`, desktop file, 48 and 256 icons) | `verify_staging_contents` |
| Atomic replace with backup and rollback | `commit_update`, `install_file`, `install_binary`, `restore_backup` |
| `fsync` after commit on Unix | `sync_path` |

## Installation ownership controls

| Control | Where |
| --- | --- |
| Official receipt file `.baud-install.toml` with `managed_by = "baud-installer"` and schema version 1 | `installation` constants + `resolve` / `resolve_with_exe` |
| Receipt records `binary_path` and `data_dir` | same |
| Scope classification user vs root from binary ownership | `classify_scope` |
| Root install without elevated rights → error | `OwnershipError::RootNeedsSudo` |
| Canonical path match; reject symlink on the binary path in the receipt | `canonical_no_symlink` |
| Validate file/dir ownership and root permission bits | `validate_file`, `validate_dir` |
| Legacy official dirs without receipt → `LegacyLocation` hint | `legacy_or_not_owned`, `is_historical_official_dir` |
| Non-Unix → not owned | `resolve` path |

Without a valid official (or legacy) resolution, the updater refuses to modify the install.

## What is not protected

- Self-update on Windows, macOS, or non-x86_64 Linux
- deb/rpm installs (no ownership receipt from those packages)
- Authenticode or package GPG signatures on Windows artifacts
- Separate code-signing of the ELF beyond manifest Ed25519 + archive SHA-256
- Certificate pinning beyond ordinary HTTPS/TLS
- Authenticity of GitHub API “latest” discovery beyond verifying the signed manifest for the chosen tag
- World-writable checks on user-scope paths comparable to the root `0o022` policy

## Where to change this

| Change | Start in |
| --- | --- |
| Manifest fields or verify steps | `src/updater.rs` |
| Receipt format or ownership rules | `src/installation.rs` |
| Installer-written receipt | `install.sh` |
| CI signing | `tools/packaging/sign_update_manifest.sh` |
