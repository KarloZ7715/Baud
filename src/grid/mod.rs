use crate::ansi::Attrs;
use std::collections::VecDeque;

/// Numero de filas del grid virtual.
pub const ROWS: usize = 24;
/// Numero de columnas del grid virtual.
pub const COLS: usize = 80;

/// Maximo numero de lineas guardadas en el scrollback (MVP).
// ponytail: 100 lineas fijas, sin configuracion. Sprint 6 agrega config.
pub const MAX_SCROLLBACK: usize = 100;

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
    /// Lineas que hicieron scroll por arriba de la region.
    /// La fila mas reciente esta al final.
    // ponytail: scrollback minimo sin reflow; Sprint 6 agrega reflow + page up/down.
    pub scrollback: VecDeque<Vec<Cell>>,
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
            scrollback: VecDeque::with_capacity(MAX_SCROLLBACK),
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

    /// Scroll up: mueve todas las filas de la region [top, bottom] una posicion
    /// hacia arriba. La fila `bottom` queda en blanco.
    // ponytail: alt screen tambien acumula scrollback (bug aceptado); Sprint 6 decide si lo limpia.
    pub fn scroll_up_region(&mut self, n: usize, top: usize, bottom: usize) {
        for _ in 0..n {
            if top < ROWS && bottom < ROWS && top <= bottom {
                let row_to_save = self.rows[top].clone();
                self.push_scrollback(row_to_save);
                self.rows.remove(top);
                self.rows.insert(bottom, vec![Cell::default(); COLS]);
            }
        }
    }

    /// Scroll down: mueve todas las filas de la region [top, bottom] una posicion
    /// hacia abajo. La fila `top` queda en blanco. En Sprint 4 solo se usa
    /// internamente; no expuesto en CSI todavia.
    #[allow(dead_code)]
    pub fn scroll_down_region(&mut self, n: usize, top: usize, bottom: usize) {
        for _ in 0..n {
            if top < ROWS && bottom < ROWS && top <= bottom {
                self.rows.remove(bottom);
                self.rows.insert(top, vec![Cell::default(); COLS]);
            }
        }
    }

    /// Desplaza las filas [row, total_rows) una posicion hacia abajo. La fila
    /// `row` queda en blanco. Usado por IL (insert line).
    /// ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn insert_line(&mut self, row: usize) {
        if row < ROWS {
            let blank = vec![Cell::default(); COLS];
            self.rows.remove(ROWS - 1);
            self.rows.insert(row, blank);
        }
    }

    /// Desplaza las filas [row+1, total_rows) una posicion hacia arriba. La fila
    /// (total_rows - 1) queda en blanco. Usado por DL (delete line).
    /// ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn delete_line(&mut self, row: usize) {
        if row < ROWS {
            self.rows.remove(row);
            self.rows.push(vec![Cell::default(); COLS]);
        }
    }

    /// Inserta `n` caracteres en blanco en la posicion (row, col), desplazando
    /// el resto de la linea a la derecha. Caracteres que salen por la derecha
    /// se pierden. Usado por ICH (insert character).
    pub fn insert_chars(&mut self, row: usize, col: usize, n: usize) {
        if row < ROWS && col < COLS {
            let actual_n = n.min(COLS - col);
            for _ in 0..actual_n {
                self.rows[row].pop();
                self.rows[row].insert(col, Cell::default());
            }
        }
    }

    /// Borra `n` caracteres en la posicion (row, col), desplazando el resto de
    /// la linea a la izquierda. Caracteres que quedan al final se llenan con
    /// blancos. Usado por DCH (delete character).
    pub fn delete_chars(&mut self, row: usize, col: usize, n: usize) {
        if row < ROWS && col < COLS {
            let actual_n = n.min(COLS - col);
            for _ in 0..actual_n {
                self.rows[row].remove(col);
                self.rows[row].push(Cell::default());
            }
        }
    }

    /// Guarda una fila en el scrollback cuando sale por arriba de la pantalla.
    fn push_scrollback(&mut self, row: Vec<Cell>) {
        if self.scrollback.len() >= MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollback_pushes_on_scroll_up() {
        let mut grid = Grid::new();
        // Llenar 24 lineas + 1 mas para forzar scroll up
        grid.scroll_up_region(1, 0, ROWS - 1);
        assert_eq!(grid.scrollback.len(), 1);
    }

    #[test]
    fn test_scrollback_drops_oldest_when_full() {
        let mut grid = Grid::new();
        for _ in 0..=MAX_SCROLLBACK {
            grid.scroll_up_region(1, 0, ROWS - 1);
        }
        assert_eq!(grid.scrollback.len(), MAX_SCROLLBACK);
    }
}
