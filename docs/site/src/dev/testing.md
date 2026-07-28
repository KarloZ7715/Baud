# Testing

## Layers

| Layer | Where | How to run |
| --- | --- | --- |
| Unit | `#[cfg(test)]` inside `src/**` | `cargo test --lib` or `cargo test --lib ansi::` |
| Integration | `tests/*.rs` | `cargo test --test <name>` |
| Shell harnesses | `tests/*.sh`, `tools/linux_session_smoke.sh` | invoke the script (see below) |
| Benches | `benches/` (criterion, `harness = false`) | `cargo bench --bench <name>` |

CI's default gate is `cargo test --all` (see [CI](ci.md)).

## Integration tests

| File | Role |
| --- | --- |
| `tests/pty_session_conformance.rs` | Spawn, I/O, resize, interrupt, shutdown (Unix and Windows ConPTY cases) |
| `tests/copy_test.rs` | `selected_text()` against multi-line grid content |
| `tests/selected_text_test.rs` | Selection extraction including CR+LF cases |
| `tests/visual_regression.rs` | Builtin box-drawing mask connectivity (no GPU window; CI-safe) |

```sh
cargo test --test pty_session_conformance -- --test-threads=1 --nocapture
cargo test --test copy_test
cargo test --test selected_text_test
cargo test --test visual_regression
```

## Shell harnesses

| Script | Run | Notes |
| --- | --- | --- |
| `tests/e2e_mouse_selection.sh` | `./tests/e2e_mouse_selection.sh` | Needs ydotool, grim, jq, hyprctl (compositor-specific E2E) |
| `tests/mouse_report_harness.sh` | inside Baud: `bash tests/mouse_report_harness.sh [click\|drag\|anymotion\|focus]` | Dumps mouse-report bytes via `cat -v` |
| `tests/pty_pipeline_load.sh` | inside Baud: `bash tests/pty_pipeline_load.sh` | Optional `LOAD_PROFILE=stress` |
| `tests/pty_pipeline_verify.sh` | `./tests/pty_pipeline_verify.sh` | Runs targeted `event_loop` tests, full `cargo test`, clippy |
| `tools/linux_session_smoke.sh` | `tools/linux_session_smoke.sh [--xvfb] [--build]` | Linux session smoke; skips if no display unless xvfb |

## Benches

```sh
cargo bench --bench bench
cargo bench --bench pty_hot_path          # Unix
cargo bench --bench watchdog_overhead
cargo bench --bench watchdog_overhead -- --json
```

| Bench | Focus |
| --- | --- |
| `benches/bench.rs` | Scroll, reflow, display-list build, builtin glyph path |
| `benches/pty_hot_path.rs` | Coalesced inbound parse and PTY write echo (Unix) |
| `benches/watchdog_overhead.rs` | Watchdog ping / enter-leave / lock-busy atomics |

## Quality gates used in CI

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo run --bin docs-gen -- --check
cargo build --release --locked
```

Run these before opening a pull request. Lefthook covers fmt, clippy, and tests on the git hook path when installed.
