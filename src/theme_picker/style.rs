//! Colores del theme picker via motor de contraste unificado.

use glyphon::Color;

use crate::config::{parse_hex, ThemeConfig, MIN_LEGIBLE_CONTRAST};
use crate::renderer::ContrastCache;

const MIN_DIM_CONTRAST: f64 = 4.5;

fn fg_on_bg(fg_hex: &str, bg_hex: &str, min: f64, cache: &mut ContrastCache) -> Color {
    let fg = parse_hex(fg_hex);
    let bg = parse_hex(bg_hex);
    let (r, g, b) = cache.adjust(fg, bg, min);
    Color::rgb(r, g, b)
}

/// Texto principal legible sobre el fondo del tema (header del panel).
pub fn picker_foreground(theme: &ThemeConfig, cache: &mut ContrastCache) -> Color {
    fg_on_bg(
        &theme.foreground,
        &theme.background,
        theme.minimum_contrast,
        cache,
    )
}

/// Texto de la lista de presets sobre el panel `black`.
pub fn picker_list_fg(theme: &ThemeConfig, cache: &mut ContrastCache) -> Color {
    fg_on_bg(
        &theme.foreground,
        &theme.black,
        theme.minimum_contrast,
        cache,
    )
}

/// Texto secundario legible (etiquetas de sección).
pub fn picker_dim(theme: &ThemeConfig, cache: &mut ContrastCache) -> Color {
    fg_on_bg(
        &theme.bright_black,
        &theme.background,
        MIN_DIM_CONTRAST,
        cache,
    )
}

/// Color de pie de página.
pub fn picker_footer(theme: &ThemeConfig, cache: &mut ContrastCache) -> Color {
    let bg = &theme.background;
    let blend = blend_hex(&theme.foreground, bg, 0.35);
    fg_on_bg(&blend, bg, MIN_LEGIBLE_CONTRAST, cache)
}

fn blend_hex(a: &str, b: &str, t: f64) -> String {
    let (ar, ag, ab) = parse_hex(a);
    let (br, bg, bb) = parse_hex(b);
    let t = t.clamp(0.0, 1.0);
    let lerp = |from: u8, to: u8| -> u8 {
        (f64::from(from) + (f64::from(to) - f64::from(from)) * t).round() as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        lerp(ar, br),
        lerp(ag, bg),
        lerp(ab, bb)
    )
}
