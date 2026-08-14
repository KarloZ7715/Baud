# Getting started

## Launching

Run `baud` with no arguments to open a new terminal window running your default shell.

```sh
baud
```

### CLI flags

These flags configure the window and initial process before it launches; they never open a second window or block:

| Flag | Effect |
| --- | --- |
| `-e <command> [args...]` | Run `<command>` (and everything after it) in the PTY instead of your default shell. Must be the last flag — everything following `-e` is passed through verbatim, including further `-`-prefixed tokens. |
| `--working-directory <dir>` | Set the initial working directory for the child process. |
| `--title <text>` | Set the initial window title. |
| `--app-id <id>` | Override the Wayland `app_id` / X11 `WM_CLASS` (config `window.app_id`, default `baud`). |
| `--hold` | Keep the window open after the command exits, instead of closing it. |

Flags accept either `--flag value` or `--flag=value` form, and can appear in any order (except `-e`, which must come last).

### Other commands

Baud also recognizes a small set of subcommands, resolved before the graphical backend starts so they still work in a broken graphical session:

| Command | Effect |
| --- | --- |
| `baud version` (or `-v`, `--version`) | Print the installed version and exit. |
| `baud help` (or `-h`, `--help`) | Print usage and exit. |
| `baud update` | Self-update to the latest release. Linux x86_64 only, and only for installations owned by the official installer — see [Updates](features/updates.md). |

Run `man baud` for the same reference from your shell.

## Configuration file

Baud looks for a config file in this order and stops at the first one found:

1. `$XDG_CONFIG_HOME/baud/config.toml` — on Linux this is usually `~/.config/baud/config.toml`; on Windows it is `%APPDATA%\baud\config.toml`.
2. `./baud.toml` in the current working directory.

If neither exists, Baud runs with built-in defaults. See [Configuration](config/index.md) for the full set of keys.

## Hot reload

While Baud is running, it polls the resolved config file's modification time roughly once per second. When the file changes, Baud reparses it and applies the new configuration immediately — no restart needed. If the new file fails to parse, Baud keeps the previous configuration running and shows a brief status message instead of crashing or reverting silently.

Try it: with Baud open, edit your config file's theme and save it. The colors update in place.

## Next steps

- [Configuration](config/index.md) — the full option reference.
- [Themes](config/themes.md) — change the color scheme from a picker or a custom palette.
- [Keybindings](config/keybindings.md) — rebind an action.
