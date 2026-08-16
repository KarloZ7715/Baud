# Platform notes

## Windows: experimental

Windows support is experimental — see [About](../about.md) and [Install on Windows](../install/windows.md). Two things follow directly from that status:

- Builds are unsigned, so SmartScreen warns on first run.
- `baud update` doesn't work at all on Windows; reinstall from a new release instead.

Setting `window.opacity` below `1.0` requests the native Mica translucent backdrop material from the Windows compositor (Windows 11's "frosted glass" look), rather than plain alpha blending.

That look costs input latency: with `window.opacity < 1.0` frames go through DirectComposition, which always presents at the DWM's own cadence. Setting `render.vsync = false` does not recover that latency.

### Custom title bar on Windows

Windows uses `window.decorations = "custom"` by default, which renders Baud's own unified title bar with inline tabs and the minimize/maximize/close buttons. The native resize borders, Aero Snap, and rounded corners are preserved through winit's extended frame. The Snap Layouts flyout that normally appears when hovering the maximize button is not available because winit does not expose `WM_NCHITTEST`; double-clicking the drag area still toggles maximized state.

### Console behavior

`baud.exe` opens a single window — launching it from the Explorer or the Start Menu shortcut no longer opens a second console window alongside it. Running a CLI subcommand (`baud --version`, `baud --help`, or an unrecognized flag) from `cmd.exe` or PowerShell still prints in that same console, by reattaching to it on startup.

### Log file location

Baud always writes a rotating log file, independent of `--verbose` or any config setting — see [diagnostics.log_level](../reference/config.md#config-diagnostics-log_level). On Windows it lives at `%LOCALAPPDATA%\baud\logs\baud.log.<date>` (on Linux, `~/.local/state/baud/logs/`), rotated daily with the last 3 days kept. It's the first thing to check — or attach — when reporting an issue without a live repro.

### ConPTY: what's structural

On Windows, Baud does not sit on a kernel PTY. The child talks to an intermediate console host that owns its own buffer, interprets what the child writes, and re-emits VT to Baud. Official zip and MSI builds load `conpty.dll` + `OpenConsole.exe` from next to `baud.exe` when those files are present (the log line is `conpty: usando el par empaquetado`). A source build, or any layout that omits that pair, uses the console host that shipped with the OS (`conpty: usando el ConPTY del sistema operativo`). Both paths still go through that intermediate host.

These behaviors come from that host. Changing Baud will not remove them:

- **DCS from the child does not reach Baud.** The host consumes Device Control String sequences instead of forwarding them. That is tracked on the console host side as [microsoft/terminal#17313](https://github.com/microsoft/terminal/issues/17313); a passthrough design lives in [microsoft/terminal#1173](https://github.com/microsoft/terminal/issues/1173).
- **Repaints can be full-buffer.** The host may rewrite a screen as a complete paint even when the child only moved the cursor or changed a cell.
- **Cursor and buffer state are the host's.** Baud sees the VT the host emits after it has applied the child's output to its own buffer.

If a Windows-only glitch is one of those three, it is not a Baud parser or renderer bug. Attach the log so it is clear whether the session used the bundled pair or the OS host.

## Wayland vs X11

Baud detects your session's display backend at startup and adjusts a few behaviors — see [Troubleshooting: Display quirks](troubleshooting.md#display-quirks) for the full table. The practical points:

- **Primary selection** (middle-click paste, `shift+insert`) works reliably on X11 and on Wayland under Hyprland, wlroots-based compositors, and KDE, but is unlikely to work under GNOME Wayland — the compositor typically doesn't expose a usable primary selection there.
- **Mouse-leave events**: on any Wayland session, moving the pointer out of the window stops delivering motion events, a protocol limitation rather than something Baud can work around.
- **Window identity**: Baud sets both the Wayland `app_id` and the X11 `WM_CLASS` to `window.app_id` (default `baud`) when the window is created. `--app-id` overrides that value. The packaged desktop entry also sets `StartupWMClass=baud` so launchers can match the window. Changing `window.app_id` requires a restart.
- **Display backend**: Baud does not pick Wayland or X11. winit uses `WAYLAND_DISPLAY` / `WAYLAND_SOCKET` when they are set, and `DISPLAY` otherwise. To force X11 in a Wayland session, unset `WAYLAND_DISPLAY`. If the log says `backend=X11` while you are on Hyprland/Sway/GNOME Wayland, that is a winit fallback — Baud logs `winit eligio X11 en una sesion Wayland` and keeps going. Do not put `WINIT_UNIX_BACKEND` in the desktop file.

## Clipboard backend by session

Clipboard access depends on the same session detection — see [Selection and clipboard](../features/selection-and-clipboard.md#clipboard-backends) for what's available on Wayland, X11, and Windows.

## Surface format and color emoji

Baud negotiates a non-sRGB 8-bit surface format at startup, which keeps the background, text, and color emoji rendering identical across drivers and display servers. On the rare GPU/backend that offers no such format, Baud logs a warning and falls back to an sRGB surface; there, colors stay correct but color emoji can look slightly washed out, a known limitation of the text atlas library that Baud does not patch.
