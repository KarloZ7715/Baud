# Platform notes

## Windows: experimental

Windows support is experimental — see [About](../about.md) and [Install on Windows](../install/windows.md). Two things follow directly from that status:

- Builds are unsigned, so SmartScreen warns on first run.
- `baud update` doesn't work at all on Windows; reinstall from a new release instead.

Setting `window.opacity` below `1.0` requests the native Mica translucent backdrop material from the Windows compositor (Windows 11's "frosted glass" look), rather than plain alpha blending.

### Console behavior

`baud.exe` opens a single window — launching it from the Explorer or the Start Menu shortcut no longer opens a second console window alongside it. Running a CLI subcommand (`baud --version`, `baud --help`, or an unrecognized flag) from `cmd.exe` or PowerShell still prints in that same console, by reattaching to it on startup.

### Log file location

Baud always writes a rotating log file, independent of `--verbose` or any config setting — see [diagnostics.log_level](../reference/config.md#config-diagnostics-log_level). On Windows it lives at `%LOCALAPPDATA%\baud\logs\baud.log.<date>` (on Linux, `~/.local/state/baud/logs/`), rotated daily with the last 3 days kept. It's the first thing to check — or attach — when reporting an issue without a live repro.

## Wayland vs X11

Baud detects your session's display backend at startup and adjusts a few behaviors — see [Troubleshooting: Display quirks](troubleshooting.md#display-quirks) for the full table. The practical points:

- **Primary selection** (middle-click paste, `shift+insert`) works reliably on X11 and on Wayland under Hyprland, wlroots-based compositors, and KDE, but is unlikely to work under GNOME Wayland — the compositor typically doesn't expose a usable primary selection there.
- **Mouse-leave events**: on any Wayland session, moving the pointer out of the window stops delivering motion events, a protocol limitation rather than something Baud can work around.
- **Window identity**: the `--app-id` flag (see [Getting started](../getting-started.md)) sets both the Wayland `app_id` and the X11 `WM_CLASS` from the same value, for window manager rules that key off either.

## Clipboard backend by session

Clipboard access depends on the same session detection — see [Selection and clipboard](../features/selection-and-clipboard.md#clipboard-backends) for what's available on Wayland, X11, and Windows.
