# Conventions

## Commits

[Conventional Commits](https://www.conventionalcommits.org/) are enforced by the `commit-msg` hook in `lefthook.yml`:

```text
^(feat|fix|chore|docs|style|refactor|perf|test|build|ci)(\(.+\))?: .+
```

Examples:

```text
fix(input): ignore repeat keys for chord detection
docs(dev): document wheel path in input page
```

`release-plz` copies subjects of `feat`, `fix`, `perf`, `security`, and packaging feat/fix groups into `CHANGELOG.md`. Write those subjects in English and in the imperative. Other types are skipped from the changelog.

> [!NOTE]
> CONTRIBUTING and `release-plz.toml` also recognize a `security` type, but the local `commit-msg` regex in `lefthook.yml` does not include it today. A `security:` commit may need `--no-verify` until the hook is widened, or use `fix:` with a security-focused body when that is accurate.

## Hooks (`lefthook.yml`)

| Hook | Jobs |
| --- | --- |
| `pre-commit` | `cargo fmt --all -- --check` (can stage fixes), `cargo clippy --all-targets -- -D warnings` — parallel, Rust globs |
| `pre-push` | `cargo test --all` |
| `commit-msg` | Conventional Commits grep above |

```sh
lefthook install
lefthook run pre-commit
```

Skip a single commit only when you must: `git commit --no-verify`.

## Language

| Surface | Language |
| --- | --- |
| Published site and user-facing strings | English |
| Source comments and personal notes under `docs/` (outside `docs/site/`) | Spanish allowed |
| Changelog-bound commit subjects | English imperative |

Site rules for voice, terminology, and citations are in [Documentation style](docs-style.md).

## Plans (maintainer workflow)

Implementation plans for this project are stored under `docs/archive/plans/` (development, implemented, to-do, artifacts). They are maintainer process notes, not published product docs, and must not be cited as proof of current behavior on the site (see style guide rule on current sources). Finished plans move to `implemented/` when the maintainer closes them out.
