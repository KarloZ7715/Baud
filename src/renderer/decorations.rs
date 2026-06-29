//! Decoraciones de celda: subrayado y estilos de cursor DECSCUSR.

use glyphon::CustomGlyph;

use crate::ansi::CursorStyle;

use super::metrics::CellMetrics;

/// Id compartido con fondos solidos (mascara llena).
const SOLID_MASK_GLYPH_ID: u16 = 0;

/// Quad de subrayado de 1px justo bajo la baseline de la celda.
pub fn underline_quad(
    row: usize,
    col: usize,
    width_cells: u8,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: col as f32 * metrics.cell_w,
        top: row as f32 * metrics.cell_h + metrics.baseline_y + 1.0,
        width: metrics.cell_w * width_cells as f32,
        height: 1.0,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: 0,
    }
}

/// Barra vertical DECSCUSR (estilo bar) en el borde izquierdo de la celda.
pub fn bar_quad(
    row: usize,
    col: usize,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    let bar_w = (metrics.cell_w * 0.2).max(2.0);
    CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: col as f32 * metrics.cell_w,
        top: row as f32 * metrics.cell_h,
        width: bar_w,
        height: metrics.cell_h,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: 0,
    }
}

/// Caracter de bloque para el estilo de cursor DECSCUSR (copy mode / fallback).
pub fn cursor_glyph(style: CursorStyle, _metrics: &CellMetrics) -> char {
    match style {
        CursorStyle::Block => '\u{2588}',
        CursorStyle::Underline => '\u{2581}',
        CursorStyle::Bar => '\u{258E}',
    }
}

/// Ajuste de ancla (left, top) respecto al origen de celda para el cursor.
pub fn cursor_anchor_offset(
    style: CursorStyle,
    metrics: &CellMetrics,
    _glyph_w: f32,
    glyph_h: f32,
) -> (f32, f32) {
    match style {
        CursorStyle::Block => (0.0, 0.0),
        CursorStyle::Underline => (0.0, metrics.cell_h - glyph_h.max(1.0)),
        CursorStyle::Bar => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_metrics() -> CellMetrics {
        CellMetrics {
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 16.0,
            glyph_offset_x: 0.0,
            glyph_offset_y: 2.0,
        }
    }

    #[test]
    fn underline_quad_sits_one_px_below_baseline() {
        let metrics = test_metrics();
        let quad = underline_quad(2, 3, 1, &metrics, glyphon::Color::rgb(255, 0, 0));
        assert_eq!(quad.left, 30.0);
        assert_eq!(quad.top, 2.0 * 20.0 + 16.0 + 1.0);
        assert_eq!(quad.width, 10.0);
        assert_eq!(quad.height, 1.0);
    }

    #[test]
    fn cursor_glyph_maps_decscusr_styles() {
        let metrics = test_metrics();
        assert_eq!(cursor_glyph(CursorStyle::Block, &metrics), '\u{2588}');
        assert_eq!(cursor_glyph(CursorStyle::Underline, &metrics), '\u{2581}');
        assert_eq!(cursor_glyph(CursorStyle::Bar, &metrics), '\u{258E}');
    }
}
