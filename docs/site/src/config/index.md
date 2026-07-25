# Configuration

Baud is configured through a single TOML file. There is no in-app settings dialog beyond the [theme picker](themes.md); everything else is edited in the file directly, and picked up automatically while Baud is running.

## File location and load order

Baud looks for a config file in this order and uses the first one it finds:

1. `$XDG_CONFIG_HOME/baud/config.toml` — `~/.config/baud/config.toml` on Linux, `%APPDATA%\baud\config.toml` on Windows.
2. `./baud.toml` in the current working directory.

If neither exists, Baud runs with built-in defaults (equivalent to an empty file). Every section and key is optional — set only what you want to change from the default; see the [configuration reference](../reference/config.md) for the full key list, types, and defaults.

## Hot reload

Baud polls the resolved config file's modification time about once per second while running. When it changes, the whole file is reparsed and applied immediately: theme, font, keybindings, and every other section.

If the edited file fails to parse, Baud keeps running with the last valid configuration and shows a brief status message instead of crashing or falling back to defaults. Fix the file and save again to retry.

The theme picker (see [Themes](themes.md)) writes to this same file. To avoid the picker's own write triggering an unwanted reload, Baud resynchronizes its watched modification time immediately after persisting a picker selection.

## Sections at a glance

The reference groups every key by its TOML section: `[theme]`, `[font]`, `[window]`, `[selection]`, `[copy_mode]`, `[scrollback]`, `[cursor]`, `[process]`, `[notifications]`, `[panes]`, `[status]`, `[diagnostics]`, `[debug]`, `[render]`, plus the root-level `allow_osc52_read`, `bold_is_bright` and the `[keys]` table. Feature pages under [Features](../features/tabs-and-splits.md) link to the specific keys that control each one; start there if you are looking for a particular behavior rather than browsing the whole reference.

## Related pages

- [Themes](themes.md) — the interactive picker, embedded presets, and custom palettes.
- [Keybindings](keybindings.md) — the `[keys]` override syntax and the default chord table.
- [Configuration reference](../reference/config.md) — every key, its type, and its default.
