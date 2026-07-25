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

Check the log for a warning containing the text `keybinding invalido`. The action or chord name must match the strings in the [keybindings reference](../reference/keybindings.md) exactly.

### Configuration changes not applying

Baud hot-reloads the config file when it changes. If the file fails to parse, Baud keeps the previous config and shows a brief status message. Check the log for parse errors.
