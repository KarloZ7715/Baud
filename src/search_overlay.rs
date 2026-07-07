//! Barra inferior de busqueda (overlay GPU).

use glyphon::{Attrs, Buffer, Color, FontSystem, Shaping, TextArea, TextBounds};

use crate::config::{parse_hex, ThemeConfig};
use crate::renderer::{resolve_family, ContrastCache, SOLID_MASK_GLYPH_ID};
use crate::search::{format_bar, SearchState};

const LAYER_OVERLAY: usize = 3;
const MIN_DIM_CONTRAST: f64 = 4.5;

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

/// Rellena el buffer de la barra con el estado actual de busqueda.
#[expect(
    clippy::too_many_arguments,
    reason = "bar fill needs font system, metrics and theme contrast"
)]
pub fn fill_bar_buffer(
    state: &SearchState,
    font_system: &mut FontSystem,
    font_family: &str,
    buffer: &mut Buffer,
    cell_w: f32,
    width: f32,
    height: f32,
    theme: &ThemeConfig,
    contrast_cache: &mut ContrastCache,
) {
    let text = format_bar(state);
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

/// Añade fondo y texto de la barra de busqueda.
#[expect(
    clippy::too_many_arguments,
    reason = "overlay push shares layout metrics with theme picker pattern"
)]
pub fn push_bar_overlay<'a>(
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
    let bar_top = panel_h - cell_h;
    let (r, g, b) = parse_hex(&theme.black);
    custom_glyphs.push(solid_quad(
        0.0,
        bar_top,
        panel_w,
        cell_h,
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
        left: cell_h * 0.25,
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
