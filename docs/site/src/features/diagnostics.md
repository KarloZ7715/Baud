# Diagnostics

Diagnostics reporting is **off by default and consent-first**. Baud only sends anything over the network after you explicitly accept a one-time prompt on first launch; declining, or never deciding, means nothing is ever sent.

## What triggers a report

If you've accepted, Baud sends an event to the project's error-tracking service (Sentry) for:

- An application panic (crash) — the panic message and a backtrace.
- Any `warn`-or-higher log line Baud itself emits internally.

Reports are rate-limited to 10 per minute, and an identical message is not sent again within a 30-second window.

## What is sanitized

Before sending, Baud replaces your home directory path with `<HOME>` wherever it appears in the message or backtrace, and truncates very long messages. This is the only redaction performed — a log line or backtrace could still contain other incidental detail (a file path outside your home directory, a URL, part of a command), since Baud does not attempt broader content filtering beyond the home-path replacement.

## What is attached

Each report carries your OS, CPU architecture, and Baud's version, plus a per-install identifier — a random value generated once and stored in your data directory, unrelated to any hardware or account identifier. This identifier is only created after you accept; declining leaves no such file behind.

## Enabling or disabling

Answer the first-launch prompt, or set `diagnostics.reporting.enabled` directly in your config (`true` to opt in, `false` to opt out) — see the [configuration reference](../reference/config.md#config-section-diagnostics).
