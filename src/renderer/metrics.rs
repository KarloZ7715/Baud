//! Medicion de celda de grid (ancho, alto, baseline).

use glyphon::cosmic_text::{FontSystem, Hinting, Metrics, Shaping};

use crate::config::GlyphOffset;

use super::resolve_family;

/// Dimensiones y offsets de una celda de grid en pixeles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
    /// Y de la baseline respecto al borde superior de la celda.
    pub baseline_y: f32,
    pub glyph_offset_x: f32,
    pub glyph_offset_y: f32,
    /// Margen interior izquierdo del área de celdas (px).
    pub padding_x: f32,
    /// Margen interior superior del área de celdas (px).
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
        let cell_h = font_size * line_height;
        let metrics = Metrics::new(font_size, cell_h);
        let cell_w = measure_cell_width(font_system, metrics, family, font_size);
        let baseline_y = measure_baseline_y(font_system, metrics, family, cell_w, cell_h);

        Self {
            cell_w: sanitize_cell_metric(cell_w, font_size, line_height, true),
            cell_h: sanitize_cell_metric(cell_h, font_size, line_height, false),
            font_size,
            baseline_y,
            glyph_offset_x: glyph_offset.x,
            glyph_offset_y: glyph_offset.y,
            padding_x: 0.0,
            padding_y: 0.0,
        }
    }
}

/// Evita metricas degeneradas (NaN/0) que colapsan el grid a pocos pixeles.
fn sanitize_cell_metric(value: f32, font_size: f32, line_height: f32, is_width: bool) -> f32 {
    let fallback = if is_width {
        font_size * 0.6
    } else {
        font_size * line_height
    };
    if value.is_finite() && (1.0..=256.0).contains(&value) {
        value
    } else {
        fallback
    }
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
