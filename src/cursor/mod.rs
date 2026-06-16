use crate::grid::{COLS, ROWS};

/// Posicion del cursor en el grid virtual (row, col).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    /// Fila actual (0-indexed, 0..24).
    pub row: usize,
    /// Columna actual (0-indexed, 0..80).
    pub col: usize,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    /// Crea un cursor en la posicion (0, 0).
    pub fn new() -> Self {
        Self { row: 0, col: 0 }
    }

    /// Mueve el cursor a la posicion exacta, con clamp a los limites del grid.
    pub fn move_to(&mut self, row: usize, col: usize) {
        self.row = row.min(ROWS - 1);
        self.col = col.min(COLS - 1);
    }

    /// Mueve el cursor hacia arriba `n` lineas. No se sale del grid.
    pub fn move_up(&mut self, n: usize) {
        self.row = self.row.saturating_sub(n);
    }

    /// Mueve el cursor hacia abajo `n` lineas. No se sale del grid.
    pub fn move_down(&mut self, n: usize) {
        self.row = (self.row + n).min(ROWS - 1);
    }

    /// Mueve el cursor hacia adelante `n` columnas. No se sale del grid.
    pub fn move_forward(&mut self, n: usize) {
        self.col = (self.col + n).min(COLS - 1);
    }

    /// Mueve el cursor hacia atras `n` columnas. No se sale del grid.
    pub fn move_back(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
    }
}
