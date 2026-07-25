# Fonts and rendering

## Font selection

`font.family` picks the primary font — see the [configuration reference](../reference/config.md#config-font-family) for the platform defaults. If the configured family isn't installed, Baud falls back through a built-in chain tuned for terminal use — platform symbol/emoji fonts, then Nerd Font variants on Linux or Consolas/Segoe UI Symbol on Windows — rather than silently landing on a random proportional font. Add more families of your own with `font.fallback` (for example, a CJK or icon font to layer on top).

`font.size` sets the base size in points; zoom at runtime with `ctrl+=`/`ctrl+-`/`ctrl+0` (reset), clamped between 6 and 72pt regardless of the configured value. `font.line_height` scales line spacing. `font.glyph_offset.x`/`.y` nudge glyphs within their cell, for fonts that render slightly off-center.

## Ligatures

`font.ligatures` enables multi-character shaping, so sequences like `->` or `!=` in a ligature-aware font render as a single joined glyph.

## Box drawing and Powerline

`font.builtin_box_drawing` draws box-drawing characters (`U+2500`–`U+259F`) and Powerline separators (`U+E0B0`–`U+E0B3`) programmatically instead of pulling them from the font. This keeps borders and status-line separators pixel-aligned and consistent across every font, including ones that ship incomplete or inconsistent glyphs for these ranges. Set it to `false` to use the font's own glyphs instead.

## Performance

`render.max_fps` caps the redraw rate (`0` removes the cap). Toggle a live FPS counter with `ctrl+shift+f12` — it only responds once `debug.fps_counter_enabled = true` is set in your config.

See the [configuration reference](../reference/config.md#config-section-font) for every `font.*` key.
