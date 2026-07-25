# Themes

## Setting a theme

The simplest form sets a preset by name at the config root:

```toml
theme = "dracula"
```

Every embedded preset is listed in the [themes reference](../reference/themes.md) with its exact colors. `claude-dark` is the default when `theme` is unset.

To override individual colors on top of a preset (or the default), use the `[theme]` table with a `name` key plus any overrides:

```toml
[theme]
name = "dracula"
background = "#000000"
```

Any color not listed keeps the preset's value. See the [configuration reference](../reference/config.md#config-section-theme) for every color key.

## Interactive picker

Press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>T</kbd> to open the theme picker (on Windows, also bound to <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd> — see [Keybindings](keybindings.md) for why some chords have a platform-specific alternate). Inside the picker:

| Key | Effect |
| --- | --- |
| <kbd>↑</kbd>/<kbd>↓</kbd> or <kbd>j</kbd>/<kbd>k</kbd> | Move selection, with live preview on the running terminal. |
| <kbd>PageUp</kbd>/<kbd>PageDown</kbd>, <kbd>Home</kbd>/<kbd>End</kbd> | Jump by page or to the first/last preset. |
| <kbd>/</kbd> | Filter the list by substring as you type. |
| <kbd>Enter</kbd> | Apply the selected preset and persist it to the config file. |
| <kbd>Escape</kbd> or <kbd>q</kbd> | Cancel and restore the theme that was active before the picker opened. |

Confirming a selection writes the config file in place with the parser [toml_edit](https://docs.rs/toml_edit), which edits only the `theme` key or table and leaves every other line — comments, formatting, unrelated sections — untouched. If your config already declares a `[theme]` table with color overrides, those overrides are kept; only the preset name changes. The status bar confirms which case happened.

## Contrast floor

`theme.minimum_contrast` (default `1.0`, range `1.0`–`21.0`) dynamically lightens or darkens the effective foreground of each cell against its background to guarantee a minimum WCAG contrast ratio, using an OKLab-based adjustment rather than a flat color swap. `1.0` disables the adjustment entirely and shows the theme's raw colors. `3.0` is the WCAG floor for large text; `4.5` is the WCAG AA floor for body text. Values outside the range are clamped to the nearest bound.

`bold_is_bright` (settable at the config root or inside `[theme]`; either one being `true` enables it) maps SGR bold text to the bright variant of its ANSI color instead of just changing weight.
