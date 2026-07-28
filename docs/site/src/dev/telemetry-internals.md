# Telemetry internals

Maintainer view of diagnostics reporting. User-facing behavior and consent UX: [Diagnostics](../features/diagnostics.md). Code: `src/diagnostics/`, activation in `src/event_loop.rs`.

## Consent

`src/diagnostics/consent.rs` — `ConsentState`:

| State | Config | Effect |
| --- | --- | --- |
| `Unset` | `diagnostics.reporting.enabled = None` | First-run consent UI |
| `Accepted` | `Some(true)` | Reporter may activate |
| `Declined` | `Some(false)` | No send |

Persistence uses `persist_reporting_enabled` into `[diagnostics.reporting] enabled`. The reporter starts only after accept (`init_reporter_if_accepted` in `event_loop`).

## DSN resolution

`event_loop::resolve_dsn` — first match wins:

1. `config.diagnostics.reporting.dsn`
2. Build-time `option_env!("BAUD_SENTRY_DSN")`
3. Embedded project default `DEFAULT_SENTRY_DSN` in `event_loop.rs`

A DSN string may always resolve when falling through to the default; **network send still requires Accepted consent**.

## Install id

`src/diagnostics/install_id.rs`:

| API | Behavior |
| --- | --- |
| `generate_install_id` | 32 hex characters |
| `load_or_create_install_id` | `dirs::data_dir()/baud/install_id` |

Created on the accept path when the reporter starts. Decline does not need to create the file. Attached on events as `install_id` together with os/arch/release tags in the reporter.

## Redaction and truncation

`src/diagnostics/sanitize.rs`:

| API | Behavior |
| --- | --- |
| `sanitize_message` | home directory string → `<HOME>`; truncate to 4096 bytes |
| `sanitize_backtrace` | same home redaction; truncate to 8192 bytes |
| `sanitize_home_paths` | replace `dirs::home_dir()` text with `<HOME>` |
| `truncate_bytes` | UTF-8-safe cut with a truncated marker |

Home-path redaction is the content filter implemented here. Other incidental data in messages may remain; do not document stronger scrubbing than this module performs.

## Reporter limits

`src/diagnostics/reporter.rs` (and hooks): queue capacity 64; about 10 events per minute; dedup window on the order of 30 seconds over a prefix of the message. Levels include panics and `baud*` `error`/`warn` via the tracing layer and panic hook.

## Where to change this

| Change | Start in |
| --- | --- |
| Consent states or persist | `src/diagnostics/consent.rs` |
| DSN order or default | `src/event_loop.rs` (`resolve_dsn`) |
| Redaction rules | `src/diagnostics/sanitize.rs` |
| Queue, rate limit, envelope | `src/diagnostics/reporter.rs` |
| Install id format or path | `src/diagnostics/install_id.rs` |
