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
/// Mascara de esquina superior izquierda para la tab activa (variante C).
pub const CORNER_TL_MASK_GLYPH_ID: u16 = 5;
/// Mascara de esquina superior derecha para la tab activa (variante C).
pub const CORNER_TR_MASK_GLYPH_ID: u16 = 6;
/// Mascara del boton minimizar (linea horizontal).
pub const WIN_BTN_MINIMIZE_MASK_GLYPH_ID: u16 = 7;
/// Mascara del boton maximizar (cuadrado con borde).
pub const WIN_BTN_MAXIMIZE_MASK_GLYPH_ID: u16 = 8;
/// Mascara del boton restaurar (dos cuadrados desplazados).
pub const WIN_BTN_RESTORE_MASK_GLYPH_ID: u16 = 9;
/// Mascara del boton cerrar (dos diagonales).
pub const WIN_BTN_CLOSE_MASK_GLYPH_ID: u16 = 10;

pub fn underline_style_glyph_id(style: UnderlineStyle) -> u16 {
    match style {
        UnderlineStyle::None | UnderlineStyle::Single => SOLID_MASK_GLYPH_ID,
        UnderlineStyle::Double => LINE_DOUBLE_GLYPH_ID,
        UnderlineStyle::Dotted => LINE_DOTTED_GLYPH_ID,
        UnderlineStyle::Dashed => LINE_DASHED_GLYPH_ID,
        UnderlineStyle::Curly => LINE_CURLY_GLYPH_ID,
    }
}

/// Escala una constante logica a pixeles enteros (minimo 1).
fn scaled_px(logical: f32, scale: f32) -> f32 {
    (logical * scale).round().max(1.0)
}

/// Altura del quad de subrayado segun estilo.
pub fn underline_quad_height(style: UnderlineStyle, metrics: &CellMetrics) -> f32 {
    match style {
        UnderlineStyle::Double => scaled_px(3.0, metrics.scale_factor),
        UnderlineStyle::Curly => {
            let thickness = metrics.underline_thickness.max(1.0);
            (thickness * 3.0).max(scaled_px(3.0, metrics.scale_factor))
        }
        UnderlineStyle::None
        | UnderlineStyle::Single
        | UnderlineStyle::Dotted
        | UnderlineStyle::Dashed => metrics.underline_thickness.max(1.0),
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
            let h = underline_quad_height(style, metrics);
            let mut top = row_top + metrics.baseline_y + metrics.underline_position;
            // La onda crece hacia abajo desde la posicion tipografica; si
            // invadiria la fila siguiente, se desplaza hacia arriba.
            if style == UnderlineStyle::Curly {
                let row_bottom = row_top + metrics.cell_h;
                if top + h > row_bottom {
                    top = (row_bottom - h).max(row_top);
                }
            }
            (top, h)
        }
        LineKind::Strike => (
            row_top + metrics.strike_y,
            metrics.strike_thickness.max(1.0),
        ),
        LineKind::Over => (
            row_top + metrics.glyph_offset_y.max(0.0),
            scaled_px(1.0, metrics.scale_factor),
        ),
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
    let bar_w = (metrics.cell_w * 0.2).max(scaled_px(2.0, metrics.scale_factor));
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

/// Contorno de 1 px logico para el cursor bloque sin foco (arriba, abajo, izq, der).
pub fn cursor_outline_quads(
    row: usize,
    col: usize,
    metrics: &CellMetrics,
    color: glyphon::Color,
) -> [CustomGlyph; 4] {
    let t = scaled_px(1.0, metrics.scale_factor);
    let left = col as f32 * metrics.cell_w + metrics.padding_x;
    let top = row as f32 * metrics.cell_h + metrics.padding_y;
    let w = metrics.cell_w;
    let h = metrics.cell_h;
    let solid = |l: f32, tp: f32, width: f32, height: f32| CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: l,
        top: tp,
        width,
        height,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: 0,
    };
    [
        solid(left, top, w, t),
        solid(left, top + h - t, w, t),
        solid(left, top, t, h),
        solid(left + w - t, top, t, h),
    ]
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
///
/// `cell_w` fija el periodo del ondulado; `scale` el paso de punteado/discontinuo.
pub fn rasterize_line_mask(
    width: u16,
    height: u16,
    id: u16,
    cell_w: f32,
    scale: f32,
) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let scale = scale.max(0.01);
    let mut data = vec![0u8; w * h];
    match id {
        LINE_DOUBLE_GLYPH_ID => {
            if h >= 1 {
                data[..w].fill(255);
            }
            if h >= 3 {
                let bottom = h - 1;
                data[bottom * w..bottom * w + w].fill(255);
            }
        }
        LINE_DOTTED_GLYPH_ID => {
            let y = h.saturating_sub(1);
            let step = scaled_px(2.0, scale) as usize;
            for x in (0..w).step_by(step.max(1)) {
                data[y * w + x] = 255;
            }
        }
        LINE_DASHED_GLYPH_ID => {
            let y = h.saturating_sub(1);
            let dash = scaled_px(4.0, scale) as usize;
            let dash = dash.max(1);
            for x in 0..w {
                if (x / dash).is_multiple_of(2) {
                    data[y * w + x] = 255;
                }
            }
        }
        LINE_CURLY_GLYPH_ID => {
            let stroke = ((h as f32) / 3.0).round().max(1.0) as usize;
            let amplitude = ((h as f32 - stroke as f32) / 2.0).max(0.0);
            let period = cell_w.max(1.0);
            let freq = std::f32::consts::TAU / period;
            let mid = h as f32 / 2.0;
            for x in 0..w {
                let wave = (x as f32 * freq).sin() * amplitude;
                let y_center = mid + wave;
                let y0 = (y_center - stroke as f32 / 2.0).round() as isize;
                for dy in 0..stroke as isize {
                    let y = y0 + dy;
                    if y >= 0 && (y as usize) < h {
                        data[y as usize * w + x] = 255;
                    }
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

fn set_px(data: &mut [u8], w: usize, h: usize, x: usize, y: usize) {
    if x < w && y < h {
        data[y * w + x] = 255;
    }
}

/// Borde de un rectangulo [x0..=x1] x [y0..=y1] con grosor `s`.
#[expect(
    clippy::too_many_arguments,
    reason = "mascara de boton: coordenadas del borde"
)]
fn stroke_rect(
    data: &mut [u8],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    s: usize,
) {
    let x1 = x1.min(w.saturating_sub(1));
    let y1 = y1.min(h.saturating_sub(1));
    if x1 < x0 || y1 < y0 {
        return;
    }
    for d in 0..s {
        if y0 + d <= y1 {
            for x in x0..=x1 {
                data[(y0 + d) * w + x] = 255;
            }
        }
        if y1 >= d && y1 - d >= y0 {
            for x in x0..=x1 {
                data[(y1 - d) * w + x] = 255;
            }
        }
        if x0 + d <= x1 {
            for y in y0..=y1 {
                data[y * w + (x0 + d)] = 255;
            }
        }
        if x1 >= d && x1 - d >= x0 {
            for y in y0..=y1 {
                data[y * w + (x1 - d)] = 255;
            }
        }
    }
}

/// Como `stroke_rect` pero oculta los pixels dentro del frente [fx0..=fx1] x [fy0..=fy1].
#[expect(
    clippy::too_many_arguments,
    reason = "mascara de boton: borde recortado por el frente"
)]
fn stroke_rect_behind(
    data: &mut [u8],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    s: usize,
    fx0: usize,
    fy0: usize,
    fx1: usize,
    fy1: usize,
) {
    let x1 = x1.min(w.saturating_sub(1));
    let y1 = y1.min(h.saturating_sub(1));
    if x1 < x0 || y1 < y0 {
        return;
    }
    let hidden = |x: usize, y: usize| x >= fx0 && x <= fx1 && y >= fy0 && y <= fy1;
    for d in 0..s {
        if y0 + d <= y1 {
            for x in x0..=x1 {
                if !hidden(x, y0 + d) {
                    data[(y0 + d) * w + x] = 255;
                }
            }
        }
        if y1 >= d && y1 - d >= y0 {
            for x in x0..=x1 {
                if !hidden(x, y1 - d) {
                    data[(y1 - d) * w + x] = 255;
                }
            }
        }
        if x0 + d <= x1 {
            for y in y0..=y1 {
                if !hidden(x0 + d, y) {
                    data[y * w + (x0 + d)] = 255;
                }
            }
        }
        if x1 >= d && x1 - d >= x0 {
            for y in y0..=y1 {
                if !hidden(x1 - d, y) {
                    data[y * w + (x1 - d)] = 255;
                }
            }
        }
    }
}

/// Genera la mascara de una esquina redondeada superior (tab activa, variante C).
///
/// El tile es cuadrado de lado igual al radio; el recorte es un cuarto de
/// circulo centrado en la esquina opuesta. `id` 5 = sup. izquierda, 6 = sup. derecha.
pub fn rasterize_corner_mask(width: u16, height: u16, id: u16) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let r = w.min(h) as f32;
    let mut data = vec![255u8; w * h];
    let (cx, cy) = if id == CORNER_TR_MASK_GLYPH_ID {
        (0.0, r)
    } else {
        (r, r)
    };
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if dx * dx + dy * dy > r * r {
                data[y * w + x] = 0;
            }
        }
    }
    Some(data)
}

/// Genera la mascara vectorial de un boton de ventana.
///
/// `id` 7 = minimizar, 8 = maximizar, 9 = restaurar, 10 = cerrar. El grosor
/// escala con el lado del tile (minimo 1 px) para mantenerse nitido a cualquier escala.
pub fn rasterize_button_mask(width: u16, height: u16, id: u16) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let mut data = vec![0u8; w * h];
    // Grosor del trazo = 1 px logico escalado al fisico.
    let s = ((w.min(h) as f32 / 10.0).round() as usize).max(1);
    match id {
        WIN_BTN_MINIMIZE_MASK_GLYPH_ID => {
            let y0 = h.saturating_sub(s) / 2;
            for dy in 0..s {
                let y = y0 + dy;
                if y < h {
                    for x in 0..w {
                        data[y * w + x] = 255;
                    }
                }
            }
        }
        WIN_BTN_MAXIMIZE_MASK_GLYPH_ID => {
            stroke_rect(
                &mut data,
                w,
                h,
                0,
                0,
                w.saturating_sub(1),
                h.saturating_sub(1),
                s,
            );
        }
        WIN_BTN_RESTORE_MASK_GLYPH_ID => {
            // Cuadrado de atras (sup-izq) recortado por el de adelante (inf-der).
            stroke_rect_behind(
                &mut data,
                w,
                h,
                0,
                0,
                w.saturating_sub(1 + s),
                h.saturating_sub(1 + s),
                s,
                s,
                s,
                w.saturating_sub(1),
                h.saturating_sub(1),
            );
            stroke_rect(
                &mut data,
                w,
                h,
                s,
                s,
                w.saturating_sub(1),
                h.saturating_sub(1),
                s,
            );
        }
        WIN_BTN_CLOSE_MASK_GLYPH_ID => {
            for x in 0..w {
                let y = (x * h / w.max(1)).min(h.saturating_sub(1));
                let y2 = h
                    .saturating_sub(1)
                    .saturating_sub((x * h / w.max(1)).min(h.saturating_sub(1)));
                for t in 0..s {
                    set_px(&mut data, w, h, x, y + t);
                    if y2 >= t {
                        set_px(&mut data, w, h, x, y2 - t);
                    }
                }
            }
        }
        _ => return None,
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
            scale_factor: 1.0,
            strike_y: 10.0,
            strike_thickness: 1.0,
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
    fn strikethrough_quad_sits_near_x_height() {
        let m = test_metrics();
        let q = strikethrough_quad(1, 2, 1, &m, glyphon::Color::rgb(0, 0, 0));
        assert!((q.top - (1.0 * 20.0 + m.strike_y)).abs() < 0.5);
        assert!((q.height - m.strike_thickness).abs() < 0.5);
    }

    #[test]
    fn overline_quad_sits_at_glyph_offset() {
        let m = test_metrics();
        let q = overline_quad(1, 2, 1, &m, glyphon::Color::rgb(0, 0, 0));
        assert!((q.top - (1.0 * 20.0 + m.glyph_offset_y)).abs() < 0.5);
    }

    #[test]
    fn rasterize_double_line_mask_has_two_rows() {
        let data = rasterize_line_mask(8, 3, LINE_DOUBLE_GLYPH_ID, 8.0, 1.0).expect("mask");
        assert_eq!(data.len(), 24);
        assert!(data[0..8].iter().all(|&b| b == 255), "fila superior");
        assert!(data[8..16].iter().all(|&b| b == 0), "fila central vacia");
        assert!(data[16..24].iter().all(|&b| b == 255), "fila inferior");
    }

    #[test]
    fn rasterize_dotted_mask_alternates_pixels() {
        let data = rasterize_line_mask(8, 1, LINE_DOTTED_GLYPH_ID, 8.0, 1.0).expect("mask");
        assert_eq!(data, [255, 0, 255, 0, 255, 0, 255, 0]);
    }

    #[test]
    fn rasterize_dashed_mask_has_gaps() {
        let data = rasterize_line_mask(8, 1, LINE_DASHED_GLYPH_ID, 8.0, 1.0).expect("mask");
        assert_eq!(data, [255, 255, 255, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn rasterize_curly_mask_is_non_flat() {
        // Altura que produce line_quad para Curly con thickness=1 y scale=1.
        let metrics = test_metrics();
        let h = underline_quad_height(UnderlineStyle::Curly, &metrics).round() as u16;
        assert!(
            h >= 3,
            "curly en ejecucion debe medir al menos 3 px (h={h})"
        );
        let w = 16u16;
        let data = rasterize_line_mask(
            w,
            h,
            LINE_CURLY_GLYPH_ID,
            metrics.cell_w,
            metrics.scale_factor,
        )
        .expect("mask");
        let rows_with_ink: Vec<usize> = (0..h as usize)
            .filter(|&row| data[row * w as usize..(row + 1) * w as usize].contains(&255))
            .collect();
        assert!(
            rows_with_ink.len() >= 3,
            "curly debe ocupar al menos tres filas distintas, got {rows_with_ink:?}"
        );
        for x in 0..w as usize {
            let col_has_ink = (0..h as usize).any(|y| data[y * w as usize + x] == 255);
            assert!(col_has_ink, "columna {x} no debe quedar vacia");
        }
    }

    #[test]
    fn curly_line_quad_height_is_at_least_three() {
        let metrics = test_metrics();
        let q = line_quad(
            0,
            0,
            1,
            LineKind::Under,
            UnderlineStyle::Curly,
            &metrics,
            glyphon::Color::rgb(255, 0, 0),
        );
        assert!(q.height >= 3.0, "altura curly = {}", q.height);
    }

    #[test]
    fn cursor_glyph_maps_decscusr_styles() {
        let metrics = test_metrics();
        assert_eq!(cursor_glyph(CursorStyle::Block, &metrics), '\u{2588}');
        assert_eq!(cursor_glyph(CursorStyle::Underline, &metrics), '\u{2581}');
        assert_eq!(cursor_glyph(CursorStyle::Bar, &metrics), '\u{258E}');
    }

    #[test]
    fn corner_tl_mask_corta_esquina_sup_izq() {
        let data = rasterize_corner_mask(8, 8, CORNER_TL_MASK_GLYPH_ID).expect("mask");
        assert_eq!(data[0], 0, "la esquina (0,0) debe quedar recortada");
        assert_eq!(
            data[7 * 8 + 7],
            255,
            "el centro del arco (7,7) debe ser opaco"
        );
    }

    #[test]
    fn corner_tr_mask_corta_esquina_sup_der() {
        let data = rasterize_corner_mask(8, 8, CORNER_TR_MASK_GLYPH_ID).expect("mask");
        assert_eq!(data[7], 0, "la esquina sup-derecha debe quedar recortada");
        assert_eq!(data[7 * 8], 255, "el centro del arco (0,7) debe ser opaco");
    }

    #[test]
    fn button_minimize_es_linea_horizontal() {
        let data = rasterize_button_mask(10, 10, WIN_BTN_MINIMIZE_MASK_GLYPH_ID).expect("mask");
        let ink_rows: usize = (0..10)
            .filter(|y| (0..10).any(|x| data[y * 10 + x] == 255))
            .count();
        assert_eq!(ink_rows, 1, "minimizar es una sola linea horizontal");
    }

    #[test]
    fn button_maximize_es_cuadrado_hueco() {
        let data = rasterize_button_mask(10, 10, WIN_BTN_MAXIMIZE_MASK_GLYPH_ID).expect("mask");
        assert_eq!(data[0], 255);
        assert_eq!(data[9 * 10 + 9], 255);
        assert_eq!(data[4 * 10 + 4], 0, "el centro debe estar vacio");
    }

    #[test]
    fn button_close_tiene_dos_diagonales() {
        let data = rasterize_button_mask(10, 10, WIN_BTN_CLOSE_MASK_GLYPH_ID).expect("mask");
        assert_eq!(data[0], 255);
        assert_eq!(data[9 * 10 + 9], 255);
        assert_eq!(data[9], 255);
        assert_eq!(data[9 * 10], 255);
        assert_eq!(data[4 * 10 + 4], 255, "el centro cruza ambas diagonales");
    }

    #[test]
    fn button_restore_muestra_dos_cuadrados() {
        let data = rasterize_button_mask(10, 10, WIN_BTN_RESTORE_MASK_GLYPH_ID).expect("mask");
        assert_eq!(data[0], 255, "esquina sup-izq del cuadrado de atras");
        assert_eq!(
            data[9 * 10 + 9],
            255,
            "esquina inf-der del cuadrado de adelante"
        );
    }
}
