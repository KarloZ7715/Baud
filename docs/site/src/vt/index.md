# Terminal API (VT)

What Baud's ANSI/VT parser accepts, so an application developer can predict its behavior without trial and error. Every row below was probed against a running build. Unlisted sequences fall through to Baud's default handling for that category (ignored) rather than causing an error.

## Cursor movement and editing

| Sequence | Name | Status |
| --- | --- | --- |
| CSI Ps A/B/C/D | Cursor up/down/forward/back | Supported |
| CSI Ps ; Ps H / f | Cursor position | Supported |
| CSI Ps G / `` ` `` | Cursor horizontal absolute | Supported |
| CSI Ps d | Line position absolute | Supported |
| CSI Ps E/F | Cursor next/previous line | Supported |
| CSI Ps r | DECSTBM, set scrolling region | Supported (xterm convention: an invalid range resets to full screen, rather than being ignored per strict VT510) |
| CSI Ps L/M | Insert/delete line | Supported |
| CSI Ps @ / P | Insert/delete character | Supported |
| CSI Ps X | Erase character | Supported |
| CSI Ps S/T | Scroll up/down within the scrolling region | Supported |
| CSI Ps J/K | Erase in display/line | Supported |
| CSI Ps Z | Cursor backward tabulation | Supported |
| CSI Ps g | Tab clear | Supported |
| ESC 7/8 | DECSC/DECRC, save/restore cursor | Supported |
| ESC D/E/M | IND/NEL/RI | Supported |
| ESC H | HTS, set tab stop | Supported |
| ESC = / > | DECKPAM/DECKPNM, keypad mode | Supported |
| ESC # 8 | DECALN, screen alignment test | Supported |

## SGR (text attributes)

| Attribute | Codes | Status |
| --- | --- | --- |
| Reset, bold, dim, italic | 0, 1, 2, 3 | Supported |
| Underline (single, double, curly, dotted, dashed) | 4, `4:1`–`4:5` | Supported |
| Blink, reverse, invisible, strikethrough, overline | 5/6, 7, 8, 9, 53 | Supported |
| 16-color and bright foreground/background | 30–37, 40–47, 90–97, 100–107 | Supported |
| 256-color and truecolor foreground/background/underline | 38, 48, 58 (`;5;n` or `;2;r;g;b`) | Supported |
| Default foreground/background/underline color | 39, 49, 59 | Supported |

## DEC private modes (`CSI ? Pm h`/`l`)

| Mode | Name | Status |
| --- | --- | --- |
| 1 | DECCKM, application cursor keys | Supported |
| 6 | DECOM, origin mode | Supported |
| 7 | DECAWM, autowrap | Supported |
| 25 | DECTCEM, cursor visibility | Supported |
| 1000 | X10/normal mouse tracking | Supported |
| 1002 | Button-event mouse tracking | Supported |
| 1003 | Any-motion mouse tracking | Supported |
| 1004 | Focus in/out reporting | Supported |
| 1006 | SGR extended mouse coordinates | Supported |
| 1049 | Alternate screen buffer with save/restore cursor | Supported |
| 2004 | Bracketed paste | Supported |
| 2026 | Synchronized output | Supported |
| 2027 | Grapheme clustering | Always on — reported as permanently set; `h`/`l` toward it are accepted but cannot turn it off |

DECRQM (`CSI Ps $p` / `CSI ? Ps $p`) answers all modes listed above, plus IRM (mode 4) and LNM (mode 20) from the non-private mode space.

## OSC handlers

| OSC | Purpose | Status |
| --- | --- | --- |
| 0, 1, 2 | Set icon name and/or window title | Supported |
| 4 | Palette color set/query | Supported |
| 7 | Report current working directory (`file://` URI) | Supported |
| 8 | Hyperlinks | Supported — see [Notifications and URLs](../features/notifications-and-urls.md) |
| 9 | Desktop notification | Supported (opt-in) — see [Notifications and URLs](../features/notifications-and-urls.md) |
| 10, 11, 12 | Foreground/background/cursor color set/query | Supported |
| 52 | Clipboard read/write | Supported — see [Selection and clipboard](../features/selection-and-clipboard.md) |
| 133 | Semantic prompts (shell integration) | Supported — see [Shell integration](../features/shell-integration.md) |
| 1337 | iTerm2 terminal feature reporting | Partially supported — only the `Capabilities` subcommand; the other iTerm2 extensions are ignored |
| 777 | `notify` (rxvt-style desktop notification) | Supported (opt-in), other 777 subcommands are not |

Every other OSC number is parsed and ignored rather than erroring.

## Kitty keyboard protocol

`CSI > u` (push), `CSI = u` (set), `CSI < u` (pop), and `CSI ? u` (query) are all supported. Of the protocol's flag bits, Baud implements:

| Flag | Status |
| --- | --- |
| Disambiguate escape codes (1) | Supported |
| Report event types (2) | Supported |
| Report alternate keys (4) | Not supported |
| Report all keys as escape codes (8) | Supported |
| Report associated text (16) | Not supported |

## Device attributes and status reports

| Sequence | Purpose | Status |
| --- | --- | --- |
| CSI c / CSI > c | Primary/secondary Device Attributes | Supported |
| CSI 5 n | Device status report (always reports OK) | Supported |
| CSI 6 n / CSI ? 6 n | Cursor position report | Supported |
| CSI Ps SP q | DECSCUSR, set cursor style | Supported |
| DCS + q (XTGETTCAP) | Answers `RGB`, `Tc`, `colors`/`Co`, `setrgbf`, `setrgbb`, `Ms`, `Sync`, `smulx`, `Su` and `TN` | Supported |

Primary Device Attributes answers `CSI ?62;22c`: a VT220 with ANSI colour. Baud reports nothing
beyond that on purpose. The other DA1 parameters stand for sixel graphics, selective erase,
user-defined keys, national replacement character sets, the locator, windowing and rectangular
editing — none of which Baud implements. Claiming them would make applications reach for
features that are not there, which fails in a far more confusing way than not claiming them.

See [TERM and terminfo](term-and-terminfo.md) for what Baud reports as its terminal type and why.

## Not supported

- **Sixel graphics** — no support yet; tracked internally as follow-up work.
- **Kitty graphics protocol** — no support yet; tracked internally as follow-up work.

Neither is probed above because there is nothing to probe; both are simply absent.
