# CI

Workflows live under `.github/workflows/`.

## `ci.yml` — pull requests and pushes to `master`

| Job | What it protects |
| --- | --- |
| `checks` | `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo run --bin docs-gen -- --check` |
| `test` | `cargo test --all --verbose` |
| `build` | `cargo build --release --locked` |
| `windows-compile` | Windows `cargo check --all-targets`, `cargo build --locked`, and ConPTY OS-fallback unit tests |
| `windows-conpty` | ConPTY session tests plus OS-fallback and bundled-dll resolution (soft gate: `continue-on-error`, retries, long timeout) |
| `xvfb-smoke` | X11 smoke via `tools/linux_session_smoke.sh --xvfb --build` (soft gate) |
| `release-policy` | `tools/ci/verify_release_policy.sh` (manual version bumps need the `release:manual-version` label) |
| `shell-fixtures` | `desktop-file-validate`, `tools/ci/test_release_assets.sh`, `tools/ci/test_install_release.sh` |

Treat green `checks`, `test`, `build`, `windows-compile`, `release-policy`, and `shell-fixtures` as required for a first PR. Soft gates can fail without blocking merge; still fix ConPTY or smoke failures when your change touches those paths.

## `fuzz.yml` — parser and config input

| Job | What it protects |
| --- | --- |
| `fuzz-smoke` | 60 s libFuzzer per target on PRs that touch `src/ansi/`, `src/grid/`, `src/config/`, or `fuzz/` |
| `fuzz-weekly` | 30 min per target every Monday 06:00 UTC; caches `fuzz/corpus/`, uploads crash artifacts, opens `fuzz: crash in <target>` |

See [Testing](testing.md#fuzzing) for local commands and what to do with a trophy.

## `docs.yml` — documentation site

| Job | What it protects |
| --- | --- |
| `build` | mdBook 0.5.4 + mdbook-mermaid 0.17.0 build, HTML post-process, reject absolute internal site URLs, offline lychee link check |
| `deploy` | GitHub Pages deploy on push to `master` only |

Triggers on changes under `docs/site/`, `CHANGELOG.md`, `docs-gen`, and the workflow file itself.

## `release-plz.yml` — version automation on `master`

| Job | What it protects |
| --- | --- |
| `release-pr` | Opens or updates the Release PR; may regenerate reference docs with `docs-gen` |
| `release` | After a merged Release PR, creates the version tag via `release-plz release` |

Gated on repository identity and repo variables (`BAUD_RELEASE_APP_CLIENT_ID`, `BAUD_RELEASE_AUTOMATION_ENABLED`). Details of what enters a release are on [Release](release.md).

## `release.yml` — tags `v*.*.*` (and dry-run dispatch)

| Job | What it protects |
| --- | --- |
| `license-gate` | `LICENSE` present; `Cargo.toml` license is Apache-2.0 |
| `docs-gate` | `docs-gen --check` and full site build |
| `prepare-release` | Tag `vX.Y.Z` matches `Cargo.toml`; draft GitHub release (`workflow_dispatch` stays dry-run) |
| `linux-packages` | deb, rpm, AppImage, tarball, `SHA256SUMS`, signed update manifest, asset verify, upload |
| `windows-packages` | portable zip, MSI, `SHA256SUMS-windows`, static CRT |
| `publish-release` | Undrafts the release when both package jobs succeed and publish is enabled |

See [Packaging](packaging.md) for scripts and artifact names.
