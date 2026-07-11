//! Limites defensivos para rasterizado y CustomGlyph (evita OOM / atlas gigante).

/// Maximo ancho/alto de un bitmap de glifo en pixeles.
pub const MAX_GLYPH_DIM: u32 = 512;

/// Maximo tamano de un bitmap en bytes antes de descartarlo.
pub const MAX_RASTER_BYTES: usize = 4 * 1024 * 1024;

/// Maximo tamano de un quad de fondo solido (bytes de mascara).
pub const MAX_BG_MASK_BYTES: usize = 512 * 512;

/// Maximo producto width*height para un CustomGlyph (evita atlas glyphon -> 262144^2).
pub const MAX_CUSTOM_GLYPH_PIXELS: u32 = 4096 * 4096;

#[inline]
pub fn safe_mask_len(width: u16, height: u16) -> Option<usize> {
    let w = width as usize;
    let h = height as usize;
    let len = w.checked_mul(h)?;
    if len > MAX_BG_MASK_BYTES {
        return None;
    }
    Some(len)
}

#[inline]
pub fn clamp_custom_dimension(value: f32, cell_metric: f32, max_cells: u32) -> f32 {
    let max_px = (cell_metric * max_cells as f32).min(MAX_GLYPH_DIM as f32);
    value.clamp(1.0, max_px.max(1.0))
}

/// Mantiene el origen del glifo de modo que `[pos, pos + size]` quede dentro de
/// `[origin, origin + span]` (rectangulo de celda o run).
#[inline]
pub fn clamp_glyph_origin(pos: f32, size: f32, origin: f32, span: f32) -> f32 {
    let max_pos = (origin + span - size).max(origin);
    pos.clamp(origin, max_pos)
}

/// Maximo columnas/filas del grid (evita `cols=usize::MAX` si `cell_w==0`).
pub const MAX_GRID_DIM: usize = 4096;

#[inline]
pub fn clamp_grid_dimension(value: usize) -> usize {
    value.clamp(1, MAX_GRID_DIM)
}

/// Calcula filas/columnas del grid de forma segura (cell_w/h nunca 0).
///
/// `reserved_extra_px` suma pixeles ademas de las filas reservadas (p. ej. hueco de tabs).
#[inline]
#[expect(
    clippy::too_many_arguments,
    reason = "dimensiones de ventana y grid en un solo helper"
)]
pub fn compute_grid_dims(
    win_w: u32,
    win_h: u32,
    cell_w: f32,
    cell_h: f32,
    padding_x: u16,
    padding_y: u16,
    reserved_rows: usize,
    reserved_extra_px: f32,
) -> (usize, usize) {
    let inner_w = win_w.saturating_sub(2 * u32::from(padding_x)).max(1);
    let inner_h = win_h.saturating_sub(2 * u32::from(padding_y)).max(1);
    let cw = cell_w.max(1.0);
    let ch = cell_h.max(1.0);
    let reserved_h = (reserved_rows as f32 * ch + reserved_extra_px.max(0.0))
        .min(inner_h as f32 - ch)
        .max(0.0);
    let usable_h = (inner_h as f32 - reserved_h).max(ch);
    let cols = clamp_grid_dimension((inner_w as f32 / cw).max(1.0) as usize);
    let rows = clamp_grid_dimension((usable_h / ch).max(1.0) as usize);
    (rows, cols)
}

/// Mapea coordenadas de ventana (px) a (row, col) restando padding interior.
#[inline]
pub fn pixel_to_cell_coords(
    x: f64,
    y: f64,
    pad_x: f32,
    pad_y: f32,
    cell_w: f32,
    cell_h: f32,
) -> (usize, usize) {
    if x < 0.0 || y < 0.0 || cell_w <= 0.0 || cell_h <= 0.0 {
        return (usize::MAX, usize::MAX);
    }
    let col = ((x as f32 - pad_x).max(0.0) / cell_w) as usize;
    let row = ((y as f32 - pad_y).max(0.0) / cell_h) as usize;
    (row, col)
}

/// Coordenadas 1-based para reportes CSI de mouse (xterm).
/// Devuelve `None` si `pixel_to_cell_coords` uso sentinel (`usize::MAX`).
pub fn mouse_report_coords(col: usize, row: usize) -> Option<(usize, usize)> {
    if row == usize::MAX || col == usize::MAX {
        return None;
    }
    Some((col.saturating_add(1), row.saturating_add(1)))
}

#[inline]
pub fn custom_pixels(width: f32, height: f32) -> u32 {
    let w = width.round().max(1.0) as u32;
    let h = height.round().max(1.0) as u32;
    w.saturating_mul(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_grid_dims_respects_padding() {
        let (rows_no_pad, cols_no_pad) = compute_grid_dims(800, 600, 10.0, 20.0, 0, 0, 0, 0.0);
        let (rows_pad, cols_pad) = compute_grid_dims(800, 600, 10.0, 20.0, 8, 6, 0, 0.0);
        assert!(cols_pad < cols_no_pad);
        assert!(rows_pad < rows_no_pad);
        assert_eq!(cols_no_pad, 80);
        assert_eq!(rows_no_pad, 30);
        assert_eq!(cols_pad, 78);
        assert_eq!(rows_pad, 29);
    }

    #[test]
    fn pixel_to_cell_coords_subtracts_padding() {
        let (row, col) = pixel_to_cell_coords(28.0, 46.0, 8.0, 6.0, 10.0, 20.0);
        assert_eq!(col, 2);
        assert_eq!(row, 2);
        let (row0, col0) = pixel_to_cell_coords(5.0, 3.0, 8.0, 6.0, 10.0, 20.0);
        assert_eq!((row0, col0), (0, 0));
    }

    #[test]
    fn mouse_report_coords_rechaza_sentinel() {
        assert!(mouse_report_coords(usize::MAX, 0).is_none());
        assert!(mouse_report_coords(0, usize::MAX).is_none());
        assert_eq!(mouse_report_coords(0, 0), Some((1, 1)));
        assert_eq!(mouse_report_coords(79, 29), Some((80, 30)));
    }

    #[test]
    fn zero_cell_w_does_not_explode_grid_dims() {
        let (rows, cols) = compute_grid_dims(3840, 2160, 0.0, 0.0, 0, 0, 0, 0.0);
        assert!(cols <= MAX_GRID_DIM);
        assert!(rows <= MAX_GRID_DIM);
        assert!(cols > 0);
        assert!(rows > 0);
    }

    #[test]
    fn safe_mask_len_rejects_huge_quads() {
        assert!(safe_mask_len(16384, 16384).is_none());
        assert!(safe_mask_len(512, 512).is_some());
    }

    #[test]
    fn custom_pixels_saturates_instead_of_wrapping() {
        let px = custom_pixels(100_000.0, 100_000.0);
        // No debe hacer wrap a un valor pequeno.
        assert!(px > MAX_CUSTOM_GLYPH_PIXELS);
    }

    #[test]
    fn clamp_grid_dimension_never_returns_zero() {
        assert_eq!(clamp_grid_dimension(0), 1);
        assert_eq!(clamp_grid_dimension(usize::MAX), MAX_GRID_DIM);
    }

    #[test]
    fn clamp_custom_dimension_caps_overflow_raster() {
        let cell_w = 7.0;
        let raster_w = 9.0;
        let width = clamp_custom_dimension(raster_w, cell_w, 1);
        assert!(width <= cell_w);
        assert!(width >= 1.0);
    }

    #[test]
    fn clamp_glyph_origin_keeps_quad_inside_cell() {
        let cell_left = 10.0;
        let cell_w = 7.0;
        let width = 7.0;
        // placement negativo empujaria el quad fuera por la izquierda
        let left = clamp_glyph_origin(cell_left - 1.0, width, cell_left, cell_w);
        assert_eq!(left, cell_left);
        assert!(left + width <= cell_left + cell_w);
        // origen demasiado a la derecha
        let left_hi = clamp_glyph_origin(cell_left + 3.0, width, cell_left, cell_w);
        assert_eq!(left_hi, cell_left);
    }

    #[test]
    fn clamp_custom_dimension_tiny_cell_does_not_panic() {
        let w = clamp_custom_dimension(100.0, 0.5, 1);
        let h = clamp_custom_dimension(100.0, 0.5, 1);
        assert!(w >= 1.0);
        assert!(h >= 1.0);
    }

    #[test]
    fn compute_grid_dims_reserved_rows() {
        let (rows_full, _) = compute_grid_dims(800, 600, 10.0, 20.0, 0, 0, 0, 0.0);
        let (rows_tab, _) = compute_grid_dims(800, 600, 10.0, 20.0, 0, 0, 1, 0.0);
        assert_eq!(rows_tab, rows_full.saturating_sub(1));
        let (rows_tab_gap, _) = compute_grid_dims(800, 600, 10.0, 20.0, 0, 0, 1, 3.0);
        assert!(rows_tab_gap <= rows_tab);
    }
}
