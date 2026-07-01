//! Geometria de celda entera para el grid (ancho/alto en pixeles).

/// Dimensiones de celda en pixeles enteros (fuente de verdad para builtins).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGeometry {
    pub cell_w: u32,
    pub cell_h: u32,
}

impl CellGeometry {
    pub fn new(cell_w: f32, cell_h: f32) -> Self {
        Self {
            cell_w: sanitize_dim(cell_w),
            cell_h: sanitize_dim(cell_h),
        }
    }

    pub fn from_u32(cell_w: u32, cell_h: u32) -> Self {
        Self {
            cell_w: cell_w.max(1),
            cell_h: cell_h.max(1),
        }
    }
}

fn sanitize_dim(value: f32) -> u32 {
    if value.is_finite() && value >= 1.0 {
        value.floor().min(256.0) as u32
    } else {
        1
    }
}

/// Origen superior-izquierdo de la celda `(row, col)` en pixeles.
#[inline]
pub fn cell_origin(
    row: usize,
    col: usize,
    geom: CellGeometry,
    padding_x: f32,
    padding_y: f32,
) -> (f32, f32) {
    (
        col as f32 * geom.cell_w as f32 + padding_x,
        row as f32 * geom.cell_h as f32 + padding_y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_cell_dimensions() {
        let g = CellGeometry::new(10.7, 19.2);
        assert_eq!(g.cell_w, 10);
        assert_eq!(g.cell_h, 19);
    }

    #[test]
    fn cell_origin_scales_grid() {
        let g = CellGeometry::from_u32(10, 20);
        assert_eq!(cell_origin(2, 3, g, 0.0, 0.0), (30.0, 40.0));
    }
}
