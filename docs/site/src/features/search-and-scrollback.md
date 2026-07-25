# Search and scrollback

## Search

Press `ctrl+shift+f` to open the search bar. Type to search the visible grid and scrollback; matches highlight as you type, with the current match distinguished from the rest.

| Key | Effect |
| --- | --- |
| Up/Down | Jump to the previous/next match. |
| `Alt+C` | Toggle case-sensitive matching. |
| `Ctrl+U` | Clear the query. |
| `Escape` | Close the search bar. |

## Scrollback

Mouse wheel and touchpad scrolling move through scrollback locally, scaled by `scrollback.multiplier`. `scrollback.lines` caps how much history is kept (`0` disables scrollback entirely), or set `scrollback.unlimited = true` to keep everything.

Scroll from the keyboard:

| Action | Default chord |
| --- | --- |
| Scroll one line up/down | `ctrl+shift+up` / `ctrl+shift+down` |
| Scroll one page up/down | `alt+up`/`down`, `shift+pageup`/`pagedown`, or plain `pageup`/`pagedown` |
| Jump to bottom | `ctrl+end` |

### Full-screen programs

Full-screen programs (`less`, `vim`, `htop`) run on the alternate screen, which has no scrollback of its own. Baud translates wheel scrolling there into synthetic arrow-key presses instead, scaled by `scrollback.faux_multiplier` — so scrolling still does something reasonable inside a pager or editor that doesn't request mouse reporting itself. If the program *does* request mouse reporting (as `less -M` or most TUI apps do), wheel events are forwarded to it directly instead, on either screen.

See the [configuration reference](../reference/config.md#config-section-scrollback) for every `scrollback.*` key.
