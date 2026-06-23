use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};

/// Posicion del cursor en el grid virtual (row, col).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    /// Fila actual (0-indexed, 0..rows_count).
    pub row: usize,
    /// Columna actual (0-indexed, 0..cols_count).
    pub col: usize,
    /// Número actual de filas del grid.
    pub rows_count: usize,
    /// Número actual de columnas del grid.
    pub cols_count: usize,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    /// Crea un cursor en la posición (0, 0) con tamaño por defecto.
    pub fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            rows_count: DEFAULT_ROWS,
            cols_count: DEFAULT_COLS,
        }
    }

    /// Mueve el cursor a la posición exacta, con clamp a los límites del grid.
    pub fn move_to(&mut self, row: usize, col: usize) {
        self.row = row.min(self.rows_count - 1);
        self.col = col.min(self.cols_count - 1);
    }

    /// Mueve el cursor hacia arriba `n` líneas. No se sale del grid.
    pub fn move_up(&mut self, n: usize) {
        self.row = self.row.saturating_sub(n);
    }

    /// Mueve el cursor hacia abajo `n` líneas. No se sale del grid.
    pub fn move_down(&mut self, n: usize) {
        self.row = (self.row + n).min(self.rows_count - 1);
    }

    /// Mueve el cursor hacia adelante `n` columnas. No se sale del grid.
    pub fn move_forward(&mut self, n: usize) {
        self.col = (self.col + n).min(self.cols_count - 1);
    }

    /// Mueve el cursor hacia atrás `n` columnas. No se sale del grid.
    pub fn move_back(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
    }

    /// Actualiza el tamaño del grid. Ajusta la posición si está fuera de rango.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.rows_count = rows;
        self.cols_count = cols;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
    }
}
