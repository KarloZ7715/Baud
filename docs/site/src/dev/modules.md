# Module map

Top-level modules from `src/lib.rs`. Line counts are approximate (`wc -l` over the module tree, including colocated tests) and exist only to show relative weight.

| Module | ~LOC | Responsibility |
| --- | ---: | --- |
| `ansi` | 5700 | Virtual terminal: `Term`, `vte::Perform`, modes, OSC/CSI, mouse, links, DEC 2026 sync. |
| `base64` | 100 | In-tree base64 for OSC 52 and the updater. |
| `cli` | 390 | Non-GUI CLI (`update`, `version`, help, launch flags) before winit starts. |
| `clipboard` | 800 | Clipboard façade (arboard plus Unix CLI fallbacks). |
| `color` | 55 | Shared contrast helpers. |
| `config` | 2600 | TOML config, themes, hot-reload watch, persist. |
| `console` | 60 | **Windows only:** attach parent console for `--help` / `--version`. |
| `copy_mode` | 170 | Keyboard scrollback navigation and selection state on `Term`. |
| `cursor` | 70 | Grid cursor position and bounds. |
| `diagnostics` | 1250 | Logging, panic hooks, Sentry reporter, consent UI, sanitize. |
| `display_quirks` | 290 | Wayland/X11/compositor quirks (initial redraw, primary selection, …). |
| `event_loop` | 900 | Process bootstrap: PTY/drain threads, blink timer, config watch, `run`. |
| `grapheme` | 45 | UAX #29 cluster extension for multi-codepoint cells. |
| `grid` | 1300 | Cell grid, scrollback, reflow, damage bitmasks. |
| `input` | 2200 | Key encoding, keybindings/`Action`, wheel policy, paste helpers. |
| `installation` | 460 | Official-install receipt detection (updater gate). |
| `layout` | 1100 | Dwindle pane tree, tabs helpers, smart split, spatial focus. |
| `pty` | 1500 | Session backends and command channel (Unix PTY, ConPTY, WSL helper). |
| `renderer` | 11200 | wgpu/glyphon path: display lists, glyphs, tab bar, decorations. |
| `search` | 270 | Text search state and matches over scrollback and the grid. |
| `search_overlay` | 110 | Bottom search bar GPU overlay. |
| `selection` | 700 | Selection modes and text extraction. |
| `session` | 40 | `Session` / `SessionId` handle (Term, PTY sender, dirty/hold). |
| `smart_select` | 440 | Semantic expand (URL, path, email, word) for smart selection. |
| `theme_picker` | 1080 | Interactive theme picker state and overlay. |
| `updater` | 1170 | Signed self-update for official Linux installs. |
| `watchdog` | 360 | GUI event-loop stall and slow-handler telemetry. |
| `window` | 5000 | winit `App`: input routing, multiplexing, overlays, selection, redraw. |

Platform-gated pieces under `pty` (`unix`, `windows`, `wsl`) and `console` only compile on the relevant OS. See [Platform backends](platform.md).

## Where to change this

| Change | Start in |
| --- | --- |
| Add a crate-level module | `src/lib.rs` and a new `src/<name>.rs` or `src/<name>/` |
| Find ownership of a user-facing behavior | this table, then the linked architecture pages |
