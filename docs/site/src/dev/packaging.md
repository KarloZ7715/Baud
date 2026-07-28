# Packaging

Scripts live under `tools/packaging/`. CI invokes them from `.github/workflows/release.yml`.

## Linux artifacts

| Artifact | Script | Typical output |
| --- | --- | --- |
| deb | `build_deb.sh` | via `cargo-deb` and `[package.metadata.deb]` in `Cargo.toml` |
| rpm | `build_rpm.sh` | via `cargo-generate-rpm` (binary stripped) |
| AppImage | `build_appimage.sh` | linuxdeploy-based AppImage |
| Desktop bundle tarball | `build_tarball.sh` | `baud_Linux_x86_64.tar.gz` (binary, desktop file, icons) |

Checksums: `dist/SHA256SUMS` lists digests for AppImage, deb, rpm, and tar.gz.

## Windows artifacts

| Artifact | Script | Typical output |
| --- | --- | --- |
| Portable zip | `build_windows_portable.ps1` | `baud-<ver>-windows-x64.zip` |
| MSI | `build_windows_msi.ps1` | WiX 4.x installer |

Checksums: `dist/SHA256SUMS-windows` over zip and MSI. Builds use static CRT (`RUSTFLAGS=-C target-feature=+crt-static`). There is no Ed25519 self-update path on Windows.

## Update manifest (Linux desktop bundle)

| Piece | Role |
| --- | --- |
| `update-manifest.json` | version contract, asset name, sha256, platform, profile, tag |
| `update-manifest.sig` | Ed25519 detached signature |
| `sign_update_manifest.sh` + `update_signer/` | CI signing with `BAUD_UPDATE_SIGNING_KEY` / `UPDATE_SIGNING_KEY` |

Only the Linux x86_64 desktop-bundle tarball is on the self-update path. deb/rpm installs do not get the ownership receipt the updater requires; see [Updates](../features/updates.md).

## Verification

`tools/packaging/verify_release_assets.sh` checks that required package types exist, that the tarball name matches the contract, that `SHA256SUMS` verifies with `sha256sum -c`, that the tarball contains the expected desktop-bundle file set, and that the manifest and signature are present.

CI also runs packaging fixtures:

- `tools/ci/test_release_assets.sh`
- `tools/ci/test_install_release.sh`

The public installer is `install.sh` at the repo root (writes `.baud-install.toml`).

## Release notes

`tools/packaging/extract_release_notes.sh` helps pull notes for the GitHub release body from changelog material.
