# Notifications and URLs

## Clickable URLs

Baud recognizes links two ways: explicit [OSC 8](https://gitlab.freedesktop.org/dickmao/iterm2/-/blob/master/docs/proprietary_escape_codes.md#osc-8) hyperlinks emitted by a program, and plain URLs, `www.`-prefixed addresses, and bare domains detected directly in the text. Hold `ctrl` and click a recognized link to open it in your default handler. A bare domain or `www.` address is opened as `https://`; only the `http`, `https`, `ftp`, `file`, and `mailto` schemes are allowed through.

Opening a URL uses the platform's own default-handler mechanism: `xdg-open` on Linux, `ShellExecuteW` on Windows.

## Desktop notifications

Programs can request a desktop notification through [OSC 9](https://iterm2.com/documentation-escape-codes.html) or the rxvt-style `OSC 777;notify;<title>;<body>`. Both require `notifications.enabled = true` (see the [configuration reference](../reference/config.md#config-section-notifications)).

Notifications go through D-Bus on Linux and a toast notification on Windows.
