# Contributing to Baud

Baud is a terminal emulator written in Rust. Contributions are welcome, whether they fix a bug, add a feature, or improve the documentation.

## Quick start

You need a Rust toolchain matching the MSRV declared in `Cargo.toml`:

- Rust **1.87.0** or newer.
- On Linux: the development headers listed in `.github/workflows/release.yml` under the Linux packages job (Wayland, X11, Vulkan, fontconfig, xkbcommon, etc.).
- On Windows: Windows 10 1809+ and a DX12-capable GPU driver for the wgpu backend.

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
cargo build --release --locked
cargo run --bin docs-gen -- --check
```

`cargo test --all` also builds the `docs-gen` binary, so the documentation generator is covered by the normal test build.

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

## Pull requests

Use the pull request template. Keep the change focused on one thing, include tests when behavior changes, and verify the checks above pass locally.

## Reporting issues

Use the issue templates in `.github/ISSUE_TEMPLATE/`. The bug report template asks for the data the [troubleshooting page](https://karloz7715.github.io/Baud/help/troubleshooting.html) explains how to collect.
