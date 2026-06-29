//! Copy mode: navegar scrollback con teclado y seleccionar.
//!
//! El cursor de copy mode vive en coordenadas lógicas absolutas
//! (scrollback + grid), igual que `Selection`.

use crate::ansi::Term;
use crate::selection::{Selection, SelectionMode, SelectionPoint};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CopyModeState {
    pub row: usize,
    pub col: usize,
}

impl CopyModeState {
    /// Entra al copy mode posicionando el cursor en la fila lógica del cursor
    /// del terminal (o fila visible superior si hay scrollback activo).
    pub fn enter(term: &Term) -> Self {
        let row = if term.scrollback_offset > 0 && !term.alt_screen {
            term.visible_to_logical_row(0)
        } else {
            term.cursor_logical_row()
        };
        let col = term.cursor.col;
        CopyModeState { row, col }
    }

    /// Mueve el cursor (drow, dcol) con wrap horizontal entre filas lógicas.
    /// `extend` true: en lugar de mover, extiende la selección activa.
    pub fn move_cursor(&mut self, term: &mut Term, drow: isize, dcol: isize, extend: bool) {
        let cols = term.grid.cols_count;
        let sb_len = if term.alt_screen {
            0
        } else {
            term.grid.scrollback.len()
        };
        let total_rows = sb_len + term.grid.rows_count;
        let max_row = total_rows.saturating_sub(1);

        if extend {
            // Asegurar que exista una selección anclada en la posición actual.
            if term.selection.is_none() {
                term.selection = Some(Selection::new(SelectionPoint {
                    row: self.row,
                    col: self.col,
                }));
            }
            let (cur_row, cur_col) = term
                .selection
                .as_ref()
                .map(|s| (s.end.row, s.end.col))
                .unwrap_or((self.row, self.col));
            let mut new_row = cur_row as isize + drow;
            let mut new_col = cur_col as isize + dcol;
            if new_col < 0 {
                new_col = (cols.saturating_sub(1)) as isize;
                new_row -= 1;
            } else if new_col >= cols as isize {
                new_col = 0;
                new_row += 1;
            }
            new_row = new_row.clamp(0, max_row as isize);
            new_col = new_col.clamp(0, (cols.saturating_sub(1)) as isize);
            if let Some(ref mut sel) = term.selection {
                sel.end.row = new_row as usize;
                sel.end.col = new_col as usize;
            }
            term.scroll_to_show_logical_row(new_row as usize);
        } else {
            let mut new_row = self.row as isize + drow;
            let mut new_col = self.col as isize + dcol;
            if new_col < 0 {
                new_col = (cols.saturating_sub(1)) as isize;
                new_row -= 1;
            } else if new_col >= cols as isize {
                new_col = 0;
                new_row += 1;
            }
            new_row = new_row.clamp(0, max_row as isize);
            new_col = new_col.clamp(0, (cols.saturating_sub(1)) as isize);
            self.row = new_row as usize;
            self.col = new_col as usize;
            term.scroll_to_show_logical_row(self.row);
        }
        term.mark_dirty();
    }

    /// Sale del copy mode descartando la selección de teclado.
    pub fn exit(term: &mut Term) {
        term.copy_mode = None;
        term.clear_selection();
    }
}

/// Convierte la posición lógica del copy mode a fila visible para el renderer.
/// Devuelve None si la fila no está actualmente visible.
pub fn logical_to_visible_row(term: &Term, logical_row: usize) -> Option<usize> {
    if term.alt_screen {
        return Some(logical_row);
    }
    let sb_len = term.grid.scrollback.len();
    let offset = term.scrollback_offset as usize;
    let viewport_start = sb_len.saturating_sub(offset);
    if logical_row >= viewport_start && logical_row < viewport_start + term.grid.rows_count {
        Some(logical_row - viewport_start)
    } else {
        None
    }
}

/// Reexporta SelectionMode para que window.rs pueda construir selecciones.
pub fn selection_mode_normal() -> SelectionMode {
    SelectionMode::Normal
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};

    /// Mover el cursor de copy mode baja la fila lógica y hace scroll.
    #[test]
    fn test_copy_mode_move_down() {
        let mut term = Term::new();
        let mut cm = CopyModeState::enter(&term);
        let start_row = cm.row;
        cm.move_cursor(&mut term, 1, 0, false);
        assert_eq!(cm.row, start_row + 1);
        assert!(term.copy_mode.is_none(), "move no activa copy_mode solo");
    }

    /// Extend con selección: crea selección anclada y mueve el final.
    #[test]
    fn test_copy_mode_extend_creates_selection() {
        let mut term = Term::new();
        let mut cm = CopyModeState { row: 5, col: 3 };
        cm.move_cursor(&mut term, 0, 2, true);
        let sel = term.selection.as_ref().expect("selección creada");
        assert_eq!(sel.start.row, 5);
        assert_eq!(sel.start.col, 3);
        assert_eq!(sel.end.col, 5);
    }

    /// Wrap horizontal: col 0 + dcol -1 salta a la fila anterior, última col.
    #[test]
    fn test_copy_mode_wrap_left() {
        let mut term = Term::new();
        let mut cm = CopyModeState { row: 2, col: 0 };
        cm.move_cursor(&mut term, 0, -1, false);
        assert_eq!(cm.row, 1);
        assert_eq!(cm.col, DEFAULT_COLS - 1);
    }

    /// Exit limpia selección y copy_mode.
    #[test]
    fn test_copy_mode_exit_clears() {
        let mut term = Term::new();
        term.selection = Some(Selection::new(SelectionPoint { row: 0, col: 0 }));
        term.copy_mode = Some(CopyModeState { row: 0, col: 0 });
        CopyModeState::exit(&mut term);
        assert!(term.copy_mode.is_none());
        assert!(term.selection.is_none());
    }

    /// logical_to_visible_row fuera del viewport devuelve None.
    #[test]
    fn test_logical_to_visible_out_of_view() {
        let term = Term::new();
        // Sin scrollback, logical 0 == visible 0; logical muy alto None.
        assert_eq!(logical_to_visible_row(&term, 0), Some(0));
        assert_eq!(logical_to_visible_row(&term, DEFAULT_ROWS + 5), None);
    }
}
