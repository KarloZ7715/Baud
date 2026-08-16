//! Overlay de confirmación para un paste riesgoso (sin bracketed paste).

use glyphon::{Attrs, Buffer, Color, FontSystem, Shaping, TextArea, TextBounds};

use crate::config::{parse_hex, ThemeConfig};
use crate::input::PasteRisk;
use crate::renderer::{resolve_family, ContrastCache, SOLID_MASK_GLYPH_ID};

const LAYER_OVERLAY: usize = 3;
const MIN_DIM_CONTRAST: f64 = 4.5;
const PREVIEW_LINES: usize = 3;
const SIDE_PAD_CELLS: f32 = 0.5;

/// Paste a la espera de enter / e / esc.
#[derive(Debug, Clone)]
pub struct PendingPaste {
    pub text: String,
    pub risk: PasteRisk,
}

impl PendingPaste {
    pub fn new(text: String, risk: PasteRisk) -> Self {
        Self { text, risk }
    }
}

pub fn reason_text(risk: PasteRisk, text: &str) -> String {
    match risk {
        PasteRisk::Multiline => {
            let n = text.lines().count().max(1);
            if n == 1 {
                "Paste has 1 line".into()
            } else {
                format!("Paste has {n} lines")
            }
        }
        PasteRisk::ControlChars => "Paste contains control characters".into(),
        PasteRisk::Safe => String::new(),
    }
}

pub fn hint_text(risk: PasteRisk) -> &'static str {
    match risk {
        PasteRisk::Multiline => "enter = paste · e = paste as one line · esc = cancel",
        _ => "enter = paste · esc = cancel",
    }
}

/// Primeras líneas del paste, con controles C0 como `␛` y recorte a `width`.
pub fn preview_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    text.lines()
        .take(PREVIEW_LINES)
        .map(|line| truncate_visual(visualize_controls(line), width))
        .collect()
}

fn visualize_controls(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\t' {
                '␛'
            } else {
                c
            }
        })
        .collect()
}

fn truncate_visual(text: String, width: usize) -> String {
    if text.chars().count() <= width {
        return text;
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn overlay_text(pending: &PendingPaste, width: usize) -> String {
    let mut out = reason_text(pending.risk, &pending.text);
    out.push('\n');
    for line in preview_lines(&pending.text, width) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(hint_text(pending.risk));
    out
}

fn bar_foreground(theme: &ThemeConfig, contrast_cache: &mut ContrastCache) -> Color {
    let fg = parse_hex(&theme.foreground);
    let bg = parse_hex(&theme.background);
    let (r, g, b) = contrast_cache.adjust(fg, bg, theme.minimum_contrast);
    Color::rgb(r, g, b)
}

fn bar_dim(theme: &ThemeConfig, contrast_cache: &mut ContrastCache) -> Color {
    let fg = parse_hex(&theme.bright_black);
    let bg = parse_hex(&theme.background);
    let (r, g, b) = contrast_cache.adjust(fg, bg, MIN_DIM_CONTRAST);
    Color::rgb(r, g, b)
}

/// Rellena el buffer del overlay con motivo, vista previa y atajos.
#[expect(
    clippy::too_many_arguments,
    reason = "fill needs font system, metrics and theme contrast"
)]
pub fn fill_buffer(
    pending: &PendingPaste,
    font_system: &mut FontSystem,
    font_family: &str,
    buffer: &mut Buffer,
    cell_w: f32,
    width: f32,
    height: f32,
    theme: &ThemeConfig,
    contrast_cache: &mut ContrastCache,
) {
    let cols = ((width / cell_w) as usize).saturating_sub(1).max(8);
    let text = overlay_text(pending, cols);
    let family = resolve_family(font_family);
    let fg = bar_foreground(theme, contrast_cache);
    let default_attrs = Attrs::new().family(family);
    let attrs = Attrs::new().family(family).color(fg);
    buffer.set_rich_text(
        font_system,
        [(text.as_str(), attrs)],
        &default_attrs,
        Shaping::Advanced,
        None,
    );
    buffer.set_size(font_system, Some(width), Some(height));
    buffer.set_monospace_width(font_system, Some(cell_w));
    buffer.shape_until_scroll(font_system, false);
}

/// Añade fondo y texto del overlay de paste.
#[expect(
    clippy::too_many_arguments,
    reason = "overlay push shares layout metrics with search bar pattern"
)]
pub fn push_overlay<'a>(
    pending: &PendingPaste,
    buffer: &'a Buffer,
    extra_areas: &mut Vec<TextArea<'a>>,
    custom_glyphs: &mut Vec<glyphon::CustomGlyph>,
    surface_w: u32,
    surface_h: u32,
    cell_h: f32,
    theme: &ThemeConfig,
    contrast_cache: &mut ContrastCache,
) {
    let panel_w = surface_w as f32;
    let panel_h = surface_h as f32;
    let preview = pending.text.lines().take(PREVIEW_LINES).count();
    let lines = 2 + preview;
    let bar_h = cell_h * lines as f32;
    let bar_top = panel_h - bar_h;
    let (r, g, b) = parse_hex(&theme.black);
    custom_glyphs.push(solid_quad(
        0.0,
        bar_top,
        panel_w,
        bar_h,
        Color::rgba(r, g, b, 230),
    ));
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: surface_w as i32,
        bottom: surface_h as i32,
    };
    let dim = bar_dim(theme, contrast_cache);
    extra_areas.push(TextArea {
        buffer,
        left: cell_h * SIDE_PAD_CELLS,
        top: bar_top + cell_h * 0.15,
        scale: 1.0,
        bounds,
        default_color: dim,
        custom_glyphs: &[],
    });
}

fn solid_quad(left: f32, top: f32, width: f32, height: f32, color: Color) -> glyphon::CustomGlyph {
    glyphon::CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left,
        top,
        width,
        height,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: LAYER_OVERLAY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motivo_multiline_cuenta_lineas() {
        assert_eq!(
            reason_text(PasteRisk::Multiline, "a\nb\n"),
            "Paste has 2 lines"
        );
        assert_eq!(
            reason_text(PasteRisk::Multiline, "rm -rf /\n"),
            "Paste has 1 line"
        );
    }

    #[test]
    fn preview_sustituye_controles_y_recorta() {
        let lines = preview_lines("ls\x1b[201~xxxx", 8);
        assert_eq!(lines, vec!["ls␛[201…".to_string()]);
    }
}
