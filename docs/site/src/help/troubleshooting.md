# Troubleshooting

If Baud is not behaving as expected, the fastest way to get help is to open a bug report with the data below. The bug report template asks for the same items, so filling this checklist first saves a round trip.

## Before you report a bug

- Run `baud version` and copy the full output.
- Note your operating system and version.
- Note your session type: Wayland, X11, or Windows.
- Note your GPU and driver.
- Inside Baud, run `echo $TERM` and note the value.
- Capture a log excerpt with debug logging enabled:

  ```sh
  RUST_LOG=baud=debug,wgpu_core=warn,winit=warn baud
  ```

When you have those items, use the [bug report template](https://github.com/KarloZ7715/Baud/issues/new?template=bug_report.yml).

## Common problems

### Blank or frozen window

Launch Baud with debug logging and check for GPU or session errors in the log. On Linux, forcing software rendering can narrow the cause:

```sh
LIBGL_ALWAYS_SOFTWARE=1 RUST_LOG=baud=debug,wgpu_core=warn,winit=warn baud
```

On Windows, check that your GPU driver supports DX12 and that Windows is at least version 1809.

### Keybindings not taking effect

Check the log for a warning containing the text `keybinding invalid`. The action or chord name must match the strings in the [keybindings reference](../reference/keybindings.md) exactly.

### Configuration changes not applying

Baud hot-reloads the config file when it changes. If the file fails to parse, Baud keeps the previous config and shows a brief status message. Check the log for parse errors.

## Logging

Without `RUST_LOG` set, Baud logs at `baud=warn,wgpu_core=warn,winit=warn` — warnings and errors only. Set `RUST_LOG` to raise the level for any of those targets, as in the debug-logging example above.

## The watchdog

Set `diagnostics.watchdog = true` (requires a restart to take effect) to run a background thread that checks the GUI event loop's heartbeat every 2 seconds. If the loop goes quiet for a full interval — usually a GPU, mutex, or I/O stall — it logs a `baud::watchdog` warning naming the handler that was active when the stall started, which narrows down what to look at next. It only detects and logs; it does not attempt to recover the frozen loop.

## Display quirks

At startup Baud detects your display backend (Wayland or X11) and, on Wayland, the compositor family (from `XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`, or `HYPRLAND_INSTANCE_SIGNATURE`), and adjusts a few behaviors accordingly:

- **Hyprland**: forces an initial redraw request, since Hyprland windows have a documented hang if the first frame isn't presented promptly.
- **Any Wayland session**: also forces that initial redraw (the surface otherwise doesn't appear until the first present), and moving the mouse out of the window stops delivering motion events (a Wayland protocol limitation, not a Baud bug).
- **Primary selection**: assumed usable on X11 and on Wayland under Hyprland, wlroots-based compositors (Sway and similar), and KDE — but assumed *not* usable under GNOME Wayland, which often doesn't expose a working primary selection.

These are heuristics from environment variables, resolved once at startup and logged at `info` level — see them in your log as `display quirks resueltos`.
