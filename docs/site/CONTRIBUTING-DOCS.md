# Contributing to the docs

The site lives in `docs/site/` and is built with [mdBook](https://rust-lang.github.io/mdBook/) `v0.5.4` and [mdbook-mermaid](https://github.com/badboy/mdbook-mermaid) `v0.17.0` (both pinned in `.github/workflows/docs.yml`).

## Previewing

Install the preprocessor once (same version as CI), then serve:

```sh
# example: prebuilt linux x86_64 binary
curl -sL https://github.com/badboy/mdbook-mermaid/releases/download/v0.17.0/mdbook-mermaid-v0.17.0-x86_64-unknown-linux-gnu.tar.gz | tar xz -C ~/.local/bin
mdbook serve docs/site
```

Serves the book locally with live reload as you edit files under `docs/site/src/`. Diagrams use fenced `mermaid` code blocks; `mermaid.min.js` and `mermaid-init.js` are vendored next to `book.toml`.

## Generated reference pages

The five pages under `docs/site/src/reference/` (`config.md`, `keybindings.md`, `themes.md`, `cli.md`, `cheatsheet.md`, `example-config.md`) plus the man page (`packaging/man/baud.1`) are generated from the compiled config defaults and keybindings, not written by hand. Regenerate them after changing a config field, a default value, or a default keybinding:

```sh
cargo run --bin docs-gen
```

If you add a new config key, add its prose description to `docs/site/data/config-descriptions.toml` first — `docs-gen` pulls the description from there and the type/default straight from the compiled `Config` struct.

CI runs `cargo run --bin docs-gen -- --check` on every PR, which regenerates the pages in memory and fails if they don't match what's committed. Run that locally before pushing if you're not sure the generated pages are current.

## Everything else

Every other page under `docs/site/src/` is hand-written prose, audited against `src/` directly — see this repository's own doc pages for the tone and structure to follow (second person, present tense, short paragraphs; link to the generated reference instead of restating a default value).
