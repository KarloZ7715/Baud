# Copy mode

Copy mode lets you navigate and select scrollback text entirely from the keyboard, without touching the mouse — the same idea as tmux's or foot's copy mode.

Press `ctrl+shift+x` to enter. A cursor appears at your current position in the grid; while active:

| Key | Effect |
| --- | --- |
| `h`/`j`/`k`/`l` or the arrow keys | Move the cursor left/down/up/right. |
| Shift + any of the above | Extend the selection while moving. |
| `y` | Copy the current selection to the clipboard and exit. |
| `q` or `Escape` | Exit without copying. |

The cursor moves freely across both the visible grid and scrollback, so you can navigate up into history the same way you'd move around the current screen.

Copy mode is enabled by default; disable it entirely with `copy_mode.enabled = false` if you never use it — see the [configuration reference](../reference/config.md#config-section-copy_mode).
