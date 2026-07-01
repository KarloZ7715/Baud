//! Medicion de celda de grid (ancho, alto, baseline).

use glyphon::cosmic_text::{FontSystem, Hinting, Metrics, Shaping};

use crate::config::GlyphOffset;

use super::geometry::CellGeometry;
use super::resolve_family;

/// Dimensiones y offsets de una celda de grid en pixeles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    /// Geometria entera de celda (fuente de verdad para builtins).
    pub geometry: CellGeometry,
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
    /// Y de la baseline respecto al borde superior de la celda.
    pub baseline_y: f32,
    /// Posicion del subrayado respecto a la baseline (px).
    pub underline_position: f32,
    /// Grosor del subrayado (px).
    pub underline_thickness: f32,
    pub glyph_offset_x: f32,
    pub glyph_offset_y: f32,
    pub padding_x: f32,
    pub padding_y: f32,
}

impl CellMetrics {
    /// Mide `cell_w`, `cell_h` y `baseline_y` para la familia y tamano dados.
    pub fn measure(
        font_system: &mut FontSystem,
        family: &str,
        font_size: f32,
        line_height: f32,
        glyph_offset: GlyphOffset,
    ) -> Self {
        let cell_h_f = font_size * line_height;
        let metrics = Metrics::new(font_size, cell_h_f);
        let cell_w_f = measure_cell_width(font_system, metrics, family, font_size);
        let geometry = CellGeometry::new(cell_w_f, cell_h_f);
        let cell_w = geometry.cell_w as f32;
        let cell_h = geometry.cell_h as f32;
        let baseline_y = measure_baseline_y(font_system, metrics, family, cell_w, cell_h);
        let (underline_position, underline_thickness) =
            measure_underline_metrics(font_system, family, font_size);

        Self {
            geometry,
            cell_w,
            cell_h,
            font_size,
            baseline_y,
            underline_position,
            underline_thickness,
            glyph_offset_x: glyph_offset.x,
            glyph_offset_y: glyph_offset.y,
            padding_x: 0.0,
            padding_y: 0.0,
        }
    }
}

fn measure_underline_metrics(
    font_system: &mut FontSystem,
    family: &str,
    font_size: f32,
) -> (f32, f32) {
    // Aproximacion tipografica estandar cuando fontdb no expone underline directamente.
    let position = (font_size * 0.1).max(1.0);
    let thickness = (font_size * 0.05).max(1.0);
    let _ = (font_system, family);
    (position, thickness)
}

/// Mide `cell_w` con `monospace_width` activo (avance real entre columnas).
fn measure_cell_width(
    font_system: &mut FontSystem,
    metrics: Metrics,
    family: &str,
    guess: f32,
) -> f32 {
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    buf.set_monospace_width(font_system, Some(guess));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_text(
        font_system,
        "MMMMMMMMMM",
        &glyphon::Attrs::new().family(resolve_family(family)),
        Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    if let Some(run) = buf.layout_runs().next() {
        if run.glyphs.len() >= 2 {
            let advance = run.glyphs[1].x - run.glyphs[0].x;
            if advance > 0.0 {
                return advance;
            }
        }
        if run.line_w > 0.0 {
            return run.line_w / 10.0;
        }
    }
    guess
}

/// Baseline vertical dentro de la celda (ascent centrado en `cell_h`).
fn measure_baseline_y(
    font_system: &mut FontSystem,
    metrics: Metrics,
    family: &str,
    cell_w: f32,
    cell_h: f32,
) -> f32 {
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    buf.set_monospace_width(font_system, Some(cell_w));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_size(font_system, Some(cell_w), Some(cell_h));
    buf.set_text(
        font_system,
        "M",
        &glyphon::Attrs::new().family(resolve_family(family)),
        Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    buf.layout_runs()
        .next()
        .map(|run| run.line_y)
        .unwrap_or(metrics.font_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FontConfig;
    use crate::renderer::terminal_fallback::create_font_system;

    #[test]
    fn cell_geometry_is_integer_floor() {
        let mut fs = create_font_system();
        let fc = FontConfig::default();
        let m = CellMetrics::measure(
            &mut fs,
            &fc.family,
            fc.size as f32,
            fc.line_height,
            fc.glyph_offset,
        );
        assert_eq!(m.geometry.cell_w, m.cell_w.floor() as u32);
        assert_eq!(m.geometry.cell_h, m.cell_h.floor() as u32);
    }
}
