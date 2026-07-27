# Limitations

A consolidated list of things Baud does not do yet, gathered from the rest of this site so you don't have to hunt for them page by page.

## Platform

- **No macOS build.** Linux x86_64 and Windows are the only targets.
- **Windows is experimental**, and self-update (`baud update`) doesn't work on Windows at all — see [Platform notes](platform-notes.md).

## Rendering

- **No Sixel graphics, no kitty graphics protocol.** See the [Terminal API](../vt/index.md#not-supported) page.
- **No background images.**
- Grapheme clustering (DEC mode 2027) is always on and cannot be turned off.

## Input and URLs

- **No keyboard-driven URL hint mode** (jump-label style navigation like foot's or kitty's) — `ctrl+click` is the only way to open a link; see [Notifications and URLs](../features/notifications-and-urls.md).
- **No "clear scrollback" action.**
- The kitty keyboard protocol doesn't implement the "report alternate keys" or "report associated text" flags; see the [Terminal API](../vt/index.md#kitty-keyboard-protocol) page.

## VT compatibility notes

- `DECSTBM` (set scrolling region) follows the common xterm convention for an invalid range (reset to full screen) rather than the stricter VT510 behavior (ignore the request). Most software expects the xterm behavior.
- `XTGETTCAP` answers only `RGB`, `Tc`, and `colors`/`Co` — not the full terminfo capability set.
