# Contributing to Baud

Baud is a terminal emulator written in Rust. Contributions are welcome, whether they fix a bug, add a feature, or improve the documentation.

## Before you contribute

- Fork the repository and create a branch from `master`. Use a descriptive prefix such as `feat/`, `fix/`, or `docs/`.
- For large changes, open an issue first to discuss the approach.
- Fill in the pull request template and keep the change focused on one thing.

## Development environment

You need a Rust toolchain matching the MSRV declared in `Cargo.toml`:

- Rust **1.87.0** or newer.
- On Linux, install the build dependencies. The exact set is in `.github/workflows/release.yml`, but the common headers are:

  ```sh
  sudo apt-get install -y \
    pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
    libxi-dev libxrandr-dev libvulkan-dev libfontconfig1-dev
  ```

- On Windows, Windows 10 1809+ and a DX12-capable GPU driver are expected. The Windows build links the MSVC C runtime statically, so no separate redistributable is needed.

Clone the repository and install the git hooks once:

```sh
git clone https://github.com/KarloZ7715/Baud.git
cd Baud
lefthook install
```

The `commit-msg` hook enforces [Conventional Commits](https://www.conventionalcommits.org/).

## Building and testing

Run the same checks CI runs before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo run --bin docs-gen -- --check
```

You can also run a subset of tests while working:

```sh
cargo test --lib ansi::
cargo test --test pty_session_conformance
```

`cargo test --all` also builds the `docs-gen` binary, so the documentation generator is covered by the normal test build.

## Debugging

To see what Baud is doing, run it with debug logging:

```sh
RUST_LOG=baud=debug,wgpu_core=warn,winit=warn baud
```

On Linux, you can force software rendering to narrow down GPU issues:

```sh
LIBGL_ALWAYS_SOFTWARE=1 RUST_LOG=baud=debug,wgpu_core=warn,winit=warn baud
```

## Commit messages

Use Conventional Commits with one of these types:

```
feat|fix|chore|docs|style|refactor|perf|test|build|ci
```

Examples:

```
fix(input): ignore repeat keys for chord detection
docs(config): describe scrollback.lines default
```

`release-plz` copies the subjects of `feat`, `fix`, `perf`, `security` and the `(feat|fix)(packaging)` group verbatim into `CHANGELOG.md`. For those commit types, English is the sensible default. Other types are skipped from the changelog.

## Documentation

The site is built with [mdBook](https://rust-lang.github.io/mdBook/) v0.5.4. To edit it locally:

```sh
cp CHANGELOG.md docs/site/src/changelog.md
mdbook serve docs/site
```

The reference pages under `docs/site/src/reference/` are generated. Do not edit them by hand. To change them, edit the descriptions in `docs/site/data/config-descriptions.toml` and run:

```sh
cargo run --bin docs-gen
```

Verify the generated pages are up to date with:

```sh
cargo run --bin docs-gen -- --check
```

See the [documentation style guide](https://karloz7715.github.io/Baud/dev/docs-style.html) for voice, terminology and citation rules.

## Platform support

- Linux x86_64 is the primary, actively tested platform.
- Windows support is experimental; the ConPTY runtime gate in CI still runs in soft mode.
- macOS is not supported.

If your change affects platform-specific code, test it on that platform when possible and mention the result in the PR.

## Pull requests

Use the pull request template. Keep the change focused on one thing, include tests when behavior changes, and verify the checks above pass locally.

## Reporting issues

Use the issue templates in `.github/ISSUE_TEMPLATE/`. The bug report template asks for the data the [troubleshooting page](https://karloz7715.github.io/Baud/help/troubleshooting.html) explains how to collect.

## Release checklist

Before a release tag is pushed:

- The documentation gate in `.github/workflows/release.yml` must pass (`docs-gen --check` and the site build).
- Review the pages affected by the release's user-facing changes, using the `CHANGELOG.md` entries `release-plz` generated for that version.
