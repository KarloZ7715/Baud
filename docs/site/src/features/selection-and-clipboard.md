# Selection and clipboard

## Selecting text

Click and drag to select. Double-click extends to a semantic unit under the cursor — a URL, filesystem path, or email address take priority; if none matches, it falls back to a word bounded by `selection.word_delimiters`. Triple-click selects the whole line.

Extend an existing selection from the keyboard:

| Action | Default chord |
| --- | --- |
| Extend selection one word left/right | `ctrl+shift+left` / `ctrl+shift+right` |
| Extend selection to line start/end | `shift+home` / `shift+end` |
| Extend selection to viewport start/end | `ctrl+shift+home` / `ctrl+shift+end` |

## Copying

`ctrl+shift+c` copies the current selection. `selection.copy_on_select` (off by default) additionally copies as soon as you release the mouse button after selecting, after a short delay (`selection.copy_on_select_delay_ms`) that gives a double- or triple-click time to finish extending the selection first. `selection.copy_on_select_target` chooses where that goes: `"clipboard"`, `"primary"`, or `"both"`.

## Pasting

`ctrl+shift+v` pastes from the clipboard. `shift+insert` pastes from the primary selection (X11/Wayland's separate "last selected text" buffer).

## Bypassing mouse reporting

Programs that request mouse reporting (like `less` or full-screen editors) normally intercept mouse clicks themselves. Hold one of the modifiers listed in `selection.bypass_mouse_reporting_modifiers` (`shift`, `alt`, or `ctrl`; `shift` is the default) while clicking to select text with the mouse anyway, bypassing the program's own handling for that click.

## Clipboard backends

Baud prefers [arboard](https://github.com/1Password/arboard) for clipboard access, with a CLI-tool fallback on Linux if arboard's backend fails to initialize. Backend selection depends on the session:

- **Wayland** (`WAYLAND_DISPLAY` set) or **X11** (`DISPLAY` set): full support for both the clipboard and the primary selection.
- **Windows**: clipboard only — Windows has no primary-selection concept.
- **Neither** (headless, no display server detected): clipboard access is disabled; copy and paste are silent no-ops.

## OSC 52

Programs can read and write the clipboard directly through the OSC 52 escape sequence, which matters over SSH where the program has no other way to reach your local clipboard. Writes (a program setting the clipboard) always go through. Reads (a program asking what's currently in the clipboard) are additionally gated by `allow_osc52_read`, since letting an arbitrary program read your clipboard is a real trust boundary — disable it if you run software you do not fully trust interactively. See the [configuration reference](../reference/config.md#config-allow_osc52_read) for the key.
