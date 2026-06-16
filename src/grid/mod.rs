use crate::ansi::Attrs;

/// Numero de filas del grid virtual.
pub const ROWS: usize = 24;
/// Numero de columnas del grid virtual.
pub const COLS: usize = 80;

/// Celda individual del terminal: un caracter con sus atributos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Caracter almacenado.
    pub ch: char,
    /// Atributos de estilo de esta celda.
    pub attrs: Attrs,
}

/// Grid virtual 24x80 que representa el buffer del terminal.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Matriz de celdas: rows[row][col].
    pub rows: Vec<Vec<Cell>>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            attrs: Attrs::default(),
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Grid {
    /// Crea un grid vacio: 24 filas, 80 columnas, todo espacios con atributos por defecto.
    pub fn new() -> Self {
        Self {
            rows: vec![vec![Cell::default(); COLS]; ROWS],
        }
    }

    /// Obtiene una referencia a la celda en (row, col).
    /// panic si row/col estan fuera de rango (no deberia pasar con clamp en cursor).
    pub fn get(&self, row: usize, col: usize) -> &Cell {
        &self.rows[row][col]
    }

    /// Escribe un caracter y atributos en la celda (row, col).
    pub fn set(&mut self, row: usize, col: usize, ch: char, attrs: Attrs) {
        self.rows[row][col].ch = ch;
        self.rows[row][col].attrs = attrs;
    }

    /// Limpia todo el grid: rellena con espacios y atributos por defecto.
    pub fn clear(&mut self) {
        for row in &mut self.rows {
            for cell in row.iter_mut() {
                *cell = Cell::default();
            }
        }
    }

    /// Limpia una linea desde `from` hasta `to` (exclusivo) con espacios.
    pub fn clear_line(&mut self, row: usize, from: usize, to: usize) {
        let end = to.min(COLS);
        for col in from..end {
            self.rows[row][col] = Cell::default();
        }
    }
}
