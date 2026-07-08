//! Hyprland smart_split: triángulos desde el centro del pane.

use super::{Orientation, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPlacement {
    pub orient: Orientation,
    /// true = pane viejo es hijo `a` (izq/arriba).
    pub old_first: bool,
}

/// Decide orientación y orden de hijos según cursor dentro del pane (celdas).
pub fn smart_split_decision(pane_rect: Rect, mouse_col: f32, mouse_row: f32) -> SplitPlacement {
    let center_col = pane_rect.cols as f32 / 2.0;
    let center_row = pane_rect.rows as f32 / 2.0;
    let delta_col = mouse_col - center_col;
    let delta_row = mouse_row - center_row;
    let proportions = pane_rect.rows as f32 / pane_rect.cols as f32;

    if delta_col.abs() < f32::EPSILON {
        return SplitPlacement {
            orient: Orientation::Horizontal,
            old_first: delta_row < 0.0,
        };
    }

    let delta_slope = delta_row / delta_col;
    if delta_slope.abs() < proportions {
        let cursor_left = delta_col < 0.0;
        SplitPlacement {
            orient: Orientation::Vertical,
            old_first: !cursor_left,
        }
    } else {
        SplitPlacement {
            orient: Orientation::Horizontal,
            old_first: delta_row < 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_split_cursor_derecha() {
        let r = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let p = smart_split_decision(r, 60.0, 12.0);
        assert_eq!(p.orient, Orientation::Vertical);
    }

    #[test]
    fn smart_split_row_col_en_orden_correcto() {
        let r = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let derecha = smart_split_decision(r, 60.0, 12.0);
        assert_eq!(derecha.orient, Orientation::Vertical);
        // Invertir row/col (bug del fallback) elige otra orientacion.
        let invertido = smart_split_decision(r, 12.0, 60.0);
        assert_eq!(invertido.orient, Orientation::Horizontal);
    }
}
