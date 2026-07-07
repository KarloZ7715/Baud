//! Decoraciones de celda: subrayado y estilos de cursor DECSCUSR.

use glyphon::CustomGlyph;

use crate::ansi::{CursorStyle, UnderlineStyle};

use super::display_list::LineKind;
use super::metrics::CellMetrics;

/// Id compartido con fondos solidos (mascara generada en rasterize).
pub const SOLID_MASK_GLYPH_ID: u16 = 0;
/// Ids reservados para patrones de linea (no colisionan con glifos de texto).
pub const LINE_DOUBLE_GLYPH_ID: u16 = 1;
pub const LINE_DOTTED_GLYPH_ID: u16 = 2;
pub const LINE_DASHED_GLYPH_ID: u16 = 3;
pub const LINE_CURLY_GLYPH_ID: u16 = 4;

pub fn underline_style_glyph_id(style: UnderlineStyle) -> u16 {
    match style {
        UnderlineStyle::None | UnderlineStyle::Single => SOLID_MASK_GLYPH_ID,
        UnderlineStyle::Double => LINE_DOUBLE_GLYPH_ID,
        UnderlineStyle::Dotted => LINE_DOTTED_GLYPH_ID,
        UnderlineStyle::Dashed => LINE_DASHED_GLYPH_ID,
        UnderlineStyle::Curly => LINE_CURLY_GLYPH_ID,
    }
}

fn line_height_for_style(style: UnderlineStyle) -> f32 {
    if style == UnderlineStyle::Double {
        3.0
    } else {
        1.0
    }
}

/// Quad de linea decorativa en una celda.
pub fn line_quad(
    row: usize,
    col: usize,
    width_cells: u8,
    kind: LineKind,
    style: UnderlineStyle,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    let row_top = row as f32 * metrics.cell_h + metrics.padding_y;
    let col_left = col as f32 * metrics.cell_w + metrics.padding_x;
    let (top, height) = match kind {
        LineKind::Under => {
            let h = if style == UnderlineStyle::Double {
                line_height_for_style(style)
            } else {
                metrics.underline_thickness.max(1.0)
            };
            (row_top + metrics.baseline_y + metrics.underline_position, h)
        }
        LineKind::Strike => (row_top + metrics.cell_h * 0.5, 1.0),
        LineKind::Over => (row_top + 1.0, 1.0),
    };
    CustomGlyph {
        id: underline_style_glyph_id(style),
        left: col_left,
        top,
        width: metrics.cell_w * width_cells as f32,
        height,
        color: Some(color),
        snap_to_physical_pixel: false,
        metadata: 0,
    }
}

/// Quad de subrayado de 1px justo bajo la baseline de la celda.
pub fn underline_quad(
    row: usize,
    col: usize,
    width_cells: u8,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    line_quad(
        row,
        col,
        width_cells,
        LineKind::Under,
        UnderlineStyle::Single,
        metrics,
        color,
    )
}

#[cfg_attr(not(test), expect(dead_code, reason = "usado en tests de decorations"))]
pub fn strikethrough_quad(
    row: usize,
    col: usize,
    width_cells: u8,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    line_quad(
        row,
        col,
        width_cells,
        LineKind::Strike,
        UnderlineStyle::Single,
        metrics,
        color,
    )
}

#[cfg_attr(not(test), expect(dead_code, reason = "usado en tests de decorations"))]
pub fn overline_quad(
    row: usize,
    col: usize,
    width_cells: u8,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> CustomGlyph {
    line_quad(
        row,
        col,
        width_cells,
        LineKind::Over,
        UnderlineStyle::Single,
        metrics,
        color,
    )
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
        left: col as f32 * metrics.cell_w + metrics.padding_x,
        top: row as f32 * metrics.cell_h + metrics.padding_y,
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

/// Genera mascara de linea segun id de glifo reservado.
pub fn rasterize_line_mask(width: u16, height: u16, id: u16) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let mut data = vec![0u8; w * h];
    match id {
        LINE_DOUBLE_GLYPH_ID => {
            if h >= 1 {
                data[..w].fill(255);
            }
            if h >= 3 {
                data[2 * w..2 * w + w].fill(255);
            }
        }
        LINE_DOTTED_GLYPH_ID => {
            let y = h.saturating_sub(1);
            for x in (0..w).step_by(2) {
                data[y * w + x] = 255;
            }
        }
        LINE_DASHED_GLYPH_ID => {
            let y = h.saturating_sub(1);
            for x in 0..w {
                if (x / 4) % 2 == 0 {
                    data[y * w + x] = 255;
                }
            }
        }
        LINE_CURLY_GLYPH_ID => {
            for x in 0..w {
                let wave = (x as f32 * 0.5).sin() * 1.0;
                let y = ((h as f32 / 2.0) + wave).round() as usize;
                if y < h {
                    data[y * w + x] = 255;
                }
            }
        }
        _ => {
            let y = h.saturating_sub(1);
            for x in 0..w {
                data[y * w + x] = 255;
            }
        }
    }
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::geometry::CellGeometry;

    fn test_metrics() -> CellMetrics {
        CellMetrics {
            geometry: CellGeometry::from_u32(10, 20),
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 16.0,
            underline_position: 1.0,
            underline_thickness: 1.0,
            glyph_offset_x: 0.0,
            glyph_offset_y: 2.0,
            padding_x: 0.0,
            padding_y: 0.0,
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
    fn strikethrough_quad_sits_mid_cell() {
        let m = test_metrics();
        let q = strikethrough_quad(1, 2, 1, &m, glyphon::Color::rgb(0, 0, 0));
        assert!((q.top - (1.0 * 20.0 + 20.0 * 0.5)).abs() < 2.0);
    }

    #[test]
    fn overline_quad_sits_near_top() {
        let m = test_metrics();
        let q = overline_quad(1, 2, 1, &m, glyphon::Color::rgb(0, 0, 0));
        assert!((q.top - (1.0 * 20.0 + 1.0)).abs() < 2.0);
    }

    #[test]
    fn rasterize_double_line_mask_has_two_rows() {
        let data = rasterize_line_mask(8, 3, LINE_DOUBLE_GLYPH_ID).expect("mask");
        assert_eq!(data.len(), 24);
        assert!(data[0..8].iter().all(|&b| b == 255), "fila superior");
        assert!(data[8..16].iter().all(|&b| b == 0), "fila central vacia");
        assert!(data[16..24].iter().all(|&b| b == 255), "fila inferior");
    }

    #[test]
    fn rasterize_dotted_mask_alternates_pixels() {
        let data = rasterize_line_mask(8, 1, LINE_DOTTED_GLYPH_ID).expect("mask");
        assert_eq!(data, [255, 0, 255, 0, 255, 0, 255, 0]);
    }

    #[test]
    fn rasterize_dashed_mask_has_gaps() {
        let data = rasterize_line_mask(8, 1, LINE_DASHED_GLYPH_ID).expect("mask");
        assert_eq!(data, [255, 255, 255, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn rasterize_curly_mask_is_non_flat() {
        let data = rasterize_line_mask(16, 3, LINE_CURLY_GLYPH_ID).expect("mask");
        let rows_with_ink: Vec<usize> = (0..3)
            .filter(|&row| data[row * 16..(row + 1) * 16].contains(&255))
            .collect();
        assert!(
            rows_with_ink.len() > 1,
            "curly debe ocupar mas de una fila de mascara"
        );
    }

    #[test]
    fn cursor_glyph_maps_decscusr_styles() {
        let metrics = test_metrics();
        assert_eq!(cursor_glyph(CursorStyle::Block, &metrics), '\u{2588}');
        assert_eq!(cursor_glyph(CursorStyle::Underline, &metrics), '\u{2581}');
        assert_eq!(cursor_glyph(CursorStyle::Bar, &metrics), '\u{258E}');
    }
}
