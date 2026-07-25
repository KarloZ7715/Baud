# Migration

Where the equivalent settings live if you're coming from alacritty, kitty, or foot. Baud's side of each row links to the [configuration reference](reference/config.md); the other columns are that terminal's own config key for the same concept.

| Concept | alacritty (`alacritty.toml`) | kitty (`kitty.conf`) | foot (`foot.ini`) | Baud |
| --- | --- | --- | --- | --- |
| Font family | `font.normal.family` | `font_family` | `[main] font=` | [`font.family`](reference/config.md#config-font-family) |
| Font size | `font.size` | `font_size` | `[main] font=family:size` | [`font.size`](reference/config.md#config-font-size) |
| Ligatures | not supported | on by default | not supported | [`font.ligatures`](reference/config.md#config-font-ligatures) (off by default) |
| Color palette | `colors.primary`, `colors.normal`, `colors.bright` | `foreground`, `background`, `color0`–`color15` | `[colors-dark]`/`[colors-light]` section | [`[theme]`](config/themes.md) table, or `theme = "preset"` |
| Window opacity | `window.opacity` | `background_opacity` | not supported | [`window.opacity`](reference/config.md#config-window-opacity) |
| Window padding | `window.padding.x`/`.y` | `window_padding_width` (single value) | `[main] pad=XxY` | [`window.padding_x`/`.padding_y`](reference/config.md#config-window-padding_x) |
| Scrollback size | `scrolling.history` | `scrollback_lines` | `[scrollback] lines=` | [`scrollback.lines`](reference/config.md#config-scrollback-lines) |
| Cursor shape | `cursor.style` | `cursor_shape` | `[cursor] style=` | [`cursor.style`](reference/config.md#config-cursor-style) |
| Cursor blink | `cursor.blink_interval` | `cursor_blink_interval` | `[cursor] blink=` | [`cursor.blink`/`.blink_interval_ms`](reference/config.md#config-cursor-blink) |
| Copy on select | `selection.save_to_clipboard` | `copy_on_select` | not supported | [`selection.copy_on_select`](reference/config.md#config-selection-copy_on_select) |
| Bypass app mouse reporting | not configurable (fixed modifier) | `mouse_map` rules | not configurable (fixed modifier) | [`selection.bypass_mouse_reporting_modifiers`](reference/config.md#config-selection-bypass_mouse_reporting_modifiers) |
| Desktop notifications | not supported | supported | supported (customizable command) | [`notifications.enabled`](reference/config.md#config-notifications-enabled) (off by default) |
| Keybinding overrides | `keyboard.bindings` list, each an entry with `key`/`mods`/`action` | `map <chord> <action>` lines | `[key-bindings]` section, `action=chord` | [`[keys]`](config/keybindings.md) table, `"chord" = "action"` |

## Not available yet

A few things these terminals offer have no Baud equivalent today:

- **Keyboard-driven URL hints** (foot's `show-urls-launch`, kitty's `open_url_with_hints`) — Baud only opens a URL via `ctrl+click`; see [Notifications and URLs](features/notifications-and-urls.md).
- **Clear scrollback / full terminal reset as a bound action** (foot and kitty both bind this by default) — no equivalent action exists yet.
- **Background images** (kitty's `background_image`) — not supported.
- **Sixel graphics** (foot) or **kitty's own graphics protocol** — neither is implemented; see the [Terminal API](vt/index.md#not-supported) page.

## What transfers directly

Baud's escape-sequence handling (OSC 133, bracketed paste, synchronized output, truecolor, the kitty keyboard protocol subset) works the same regardless of which terminal you came from — see the [Terminal API](vt/index.md) page for exactly what's covered.
