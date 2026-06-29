//! Punto de selección en coordenadas lógicas (row, col).
//! `row` es absoluta dentro del buffer virtual [scrollback + grid].

use crate::grid::Cell;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionPoint {
    pub row: usize,
    pub col: usize,
}

/// Modo de selección.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelectionMode {
    /// Selección normal por carácter.
    Normal,
    /// Selección por palabra (doble click).
    Word,
    /// Selección por línea (triple click).
    Line,
    /// Selección rectangular (Alt+arrastrar).
    /// Rango de columnas fijo aplicado a todas las filas entre start y end.
    Block,
    /// Selección semántica (smart: URL, path, email…).
    Smart,
}

/// Representa una selección activa en el terminal.
#[derive(Debug, Clone)]
pub struct Selection {
    pub start: SelectionPoint,
    pub end: SelectionPoint,
    pub mode: SelectionMode,
}

/// Caracteres considerados parte de una palabra: alfanuméricos + guión bajo.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Selection {
    /// Crea una nueva selección a partir de un punto inicial.
    pub fn new(start: SelectionPoint) -> Self {
        Self {
            start,
            end: start,
            mode: SelectionMode::Normal,
        }
    }

    /// Actualiza el punto final de la selección.
    pub fn update_end(&mut self, end: SelectionPoint) {
        self.end = end;
    }

    /// Expande la selección para cubrir la palabra completa en la posición `col`
    /// de la fila `row_cells`.
    ///
    /// Si el carácter en `col` no es un carácter de palabra, la selección no cambia.
    pub fn expand_to_word(&mut self, row_cells: &[Cell], col: usize) {
        let cols = row_cells.len();
        if col >= cols || !is_word_char(row_cells[col].ch) {
            return;
        }
        // Escanear hacia la izquierda hasta encontrar un separador.
        let mut start_col = col;
        while start_col > 0 && is_word_char(row_cells[start_col - 1].ch) {
            start_col -= 1;
        }
        // Escanear hacia la derecha hasta encontrar un separador.
        let mut end_col = col;
        while end_col + 1 < cols && is_word_char(row_cells[end_col + 1].ch) {
            end_col += 1;
        }
        self.start.col = start_col;
        self.end.col = end_col;
    }

    /// Expande la selección para cubrir toda la fila `row` (columna 0 a cols_count - 1).
    pub fn expand_to_line(&mut self, row: usize, cols_count: usize) {
        self.start.row = row;
        self.start.col = 0;
        self.end.row = row;
        self.end.col = cols_count.saturating_sub(1);
    }

    /// Retorna el rango normalizado (start_row, start_col, end_row, end_col)
    /// garantizando que start ≤ end lexicográficamente.
    pub fn normalize(&self) -> (usize, usize, usize, usize) {
        if self.start.row < self.end.row
            || (self.start.row == self.end.row && self.start.col <= self.end.col)
        {
            (self.start.row, self.start.col, self.end.row, self.end.col)
        } else {
            (self.end.row, self.end.col, self.start.row, self.start.col)
        }
    }

    /// Verifica si la celda lógica (row, col) está dentro del rango seleccionado.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        match self.mode {
            SelectionMode::Block => self.contains_block(row, col),
            _ => self.contains_normal(row, col),
        }
    }

    /// Contención para selección rectangular: entre filas start/end, columnas
    /// entre min(start.col, end.col) y max(start.col, end.col).
    pub fn contains_block(&self, row: usize, col: usize) -> bool {
        let (start_row, end_row) = if self.start.row <= self.end.row {
            (self.start.row, self.end.row)
        } else {
            (self.end.row, self.start.row)
        };
        let (min_col, max_col) = if self.start.col <= self.end.col {
            (self.start.col, self.end.col)
        } else {
            (self.end.col, self.start.col)
        };
        row >= start_row && row <= end_row && col >= min_col && col <= max_col
    }

    /// Rango de columnas de un bloque para una fila dada (None si la fila
    /// está fuera del rango). Usado por `selected_text` en modo Block.
    pub fn block_col_range(&self, row: usize) -> Option<(usize, usize)> {
        if self.mode != SelectionMode::Block {
            return None;
        }
        let (start_row, end_row) = if self.start.row <= self.end.row {
            (self.start.row, self.end.row)
        } else {
            (self.end.row, self.start.row)
        };
        if row < start_row || row > end_row {
            return None;
        }
        let (min_col, max_col) = if self.start.col <= self.end.col {
            (self.start.col, self.end.col)
        } else {
            (self.end.col, self.start.col)
        };
        Some((min_col, max_col))
    }

    fn contains_normal(&self, row: usize, col: usize) -> bool {
        let (start_row, start_col, end_row, end_col) = self.normalize();
        if row < start_row || row > end_row {
            return false;
        }
        if row == start_row && col < start_col {
            return false;
        }
        if row == end_row && col > end_col {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_contains_forward() {
        let sel = Selection {
            start: SelectionPoint { row: 1, col: 3 },
            end: SelectionPoint { row: 2, col: 5 },
            mode: SelectionMode::Normal,
        };
        assert!(!sel.contains(0, 0));
        assert!(!sel.contains(1, 2));
        assert!(sel.contains(1, 3));
        assert!(sel.contains(1, 4));
        assert!(sel.contains(2, 0));
        assert!(sel.contains(2, 5));
        assert!(!sel.contains(2, 6));
        assert!(!sel.contains(3, 0));
    }

    #[test]
    fn test_selection_contains_reversed() {
        let sel = Selection {
            start: SelectionPoint { row: 2, col: 5 },
            end: SelectionPoint { row: 1, col: 3 },
            mode: SelectionMode::Normal,
        };
        // Normalizado: (1,3)-(2,5)
        assert!(!sel.contains(0, 0));
        assert!(!sel.contains(1, 2));
        assert!(sel.contains(1, 3));
        assert!(sel.contains(1, 4));
        assert!(sel.contains(2, 0));
        assert!(sel.contains(2, 5));
        assert!(!sel.contains(2, 6));
    }

    #[test]
    fn test_selection_new_and_update_end() {
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 5 });
        assert_eq!(sel.start.row, 0);
        assert_eq!(sel.start.col, 5);
        assert_eq!(sel.end.row, 0);
        assert_eq!(sel.end.col, 5);
        assert_eq!(sel.mode, SelectionMode::Normal);

        sel.update_end(SelectionPoint { row: 3, col: 2 });
        assert_eq!(sel.end.row, 3);
        assert_eq!(sel.end.col, 2);
    }

    #[test]
    fn test_normalize_already_forward() {
        let sel = Selection {
            start: SelectionPoint { row: 1, col: 2 },
            end: SelectionPoint { row: 3, col: 4 },
            mode: SelectionMode::Normal,
        };
        assert_eq!(sel.normalize(), (1, 2, 3, 4));
    }

    #[test]
    fn test_normalize_reversed() {
        let sel = Selection {
            start: SelectionPoint { row: 3, col: 4 },
            end: SelectionPoint { row: 1, col: 2 },
            mode: SelectionMode::Normal,
        };
        assert_eq!(sel.normalize(), (1, 2, 3, 4));
    }

    #[test]
    fn test_normalize_same_row() {
        // Misma fila, end.col < start.col
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 5 },
            end: SelectionPoint { row: 0, col: 2 },
            mode: SelectionMode::Normal,
        };
        assert_eq!(sel.normalize(), (0, 2, 0, 5));
    }

    #[test]
    fn test_expand_to_word() {
        use crate::grid::Cell;

        let cells: Vec<Cell> = "hello world foo"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });

        // Click en 'e' de "hello"
        sel.expand_to_word(&cells, 1);
        assert_eq!(sel.start.col, 0);
        assert_eq!(sel.end.col, 4);

        // Click en 'w' de "world"
        sel.expand_to_word(&cells, 6);
        assert_eq!(sel.start.col, 6);
        assert_eq!(sel.end.col, 10);

        // Click en espacio -> no expande
        sel.expand_to_word(&cells, 5);
        assert_eq!(sel.start.col, 6); // unchanged from previous
        assert_eq!(sel.end.col, 10);

        // Click en 'f' de "foo"
        sel.expand_to_word(&cells, 13);
        assert_eq!(sel.start.col, 12);
        assert_eq!(sel.end.col, 14);
    }

    #[test]
    fn test_expand_to_word_underscore() {
        use crate::grid::Cell;

        let cells: Vec<Cell> = "my_var_name"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });

        sel.expand_to_word(&cells, 3); // click on '_' after 'my_'
        assert_eq!(sel.start.col, 0);
        assert_eq!(sel.end.col, 10);
    }

    #[test]
    fn test_expand_to_line() {
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 5 });
        sel.mode = SelectionMode::Word;

        sel.expand_to_line(2, 80);
        assert_eq!(sel.start.row, 2);
        assert_eq!(sel.start.col, 0);
        assert_eq!(sel.end.row, 2);
        assert_eq!(sel.end.col, 79);
    }

    #[test]
    fn test_is_word_char() {
        assert!(is_word_char('a'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('9'));
        assert!(is_word_char('_'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('.'));
        assert!(!is_word_char('/'));
        assert!(!is_word_char('-'));
    }

    // =====================================================================
    // TESTS ADVERSARIALES — Sprint 7 Fase 4
    // Asumen que TODO está roto. Buscan bugs, edge cases, situaciones extremas.
    // =====================================================================

    /// ADVERTARIAL: Valores extremos en contains()
    /// start=(0,0), end=(usize::MAX, usize::MAX) debe no panic y
    /// debe considerar cualquier fila como seleccionada.
    #[test]
    fn test_contains_out_of_bounds() {
        // Selección con row=usize::MAX — cubre todo el espacio lógico
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint {
                row: usize::MAX,
                col: usize::MAX,
            },
            mode: SelectionMode::Normal,
        };
        // Borde inferior
        assert!(sel.contains(0, 0), "debe contener (0,0)");
        // Mitad del rango (overflow si normalize suma mal)
        assert!(
            sel.contains(usize::MAX / 2, 0),
            "debe contener fila intermedia"
        );
        // Borde superior — usize::MAX == end_row después de normalize
        assert!(sel.contains(usize::MAX, 0), "debe contener (MAX, 0)");
        // Columna extrema
        assert!(sel.contains(0, usize::MAX), "debe contener (0, MAX)");
        assert!(
            sel.contains(usize::MAX, usize::MAX),
            "debe contener (MAX, MAX)"
        );

        // Sin selección con start == end en fila normal
        let sel2 = Selection {
            start: SelectionPoint { row: 5, col: 3 },
            end: SelectionPoint { row: 5, col: 3 },
            mode: SelectionMode::Normal,
        };
        // Filas fuera del rango
        assert!(!sel2.contains(0, 0), "fila 0 fuera de rango");
        assert!(!sel2.contains(4, 3), "fila 4 fuera de rango (5,3)-(5,3)");
        // Punto exacto y adyacentes
        assert!(sel2.contains(5, 3), "debe contener (5,3) exacto");
        assert!(!sel2.contains(5, 4), "col 4 fuera de rango");
        assert!(!sel2.contains(6, 0), "fila 6 fuera de rango");
    }

    /// ADVERSARIAL: Múltiples combinaciones de start > end
    /// Incluye start.row significativamente mayor que end.row.
    /// El test verifica que normalize() y contains() funcionen
    /// sin importar la dirección de la selección.
    #[test]
    fn test_contains_when_start_larger_than_end_various() {
        // Caso 1: start.row >> end.row (muchas filas de diferencia, invertido)
        let sel = Selection {
            start: SelectionPoint { row: 100, col: 50 },
            end: SelectionPoint { row: 5, col: 10 },
            mode: SelectionMode::Normal,
        };
        // Normalizado: (5,10)-(100,50)
        assert!(sel.contains(5, 10), "start normalizado (5,10)");
        assert!(sel.contains(100, 50), "end normalizado (100,50)");
        assert!(sel.contains(50, 30), "mitad del rango");
        assert!(!sel.contains(4, 0), "una fila antes del start normalizado");
        assert!(
            !sel.contains(101, 0),
            "una fila después del end normalizado"
        );
        assert!(!sel.contains(5, 9), "una col antes del start normalizado");

        // Caso 2: misma fila, end.col << start.col (invertido en misma fila)
        let sel2 = Selection {
            start: SelectionPoint { row: 3, col: 10 },
            end: SelectionPoint { row: 3, col: 2 },
            mode: SelectionMode::Normal,
        };
        assert!(sel2.contains(3, 2), "start normalizado (3,2)");
        assert!(sel2.contains(3, 10), "end normalizado (3,10)");
        assert!(sel2.contains(3, 5), "col intermedia");
        assert!(!sel2.contains(3, 1), "col antes del start");
        assert!(!sel2.contains(3, 11), "col después del end");

        // Caso 3: start == end (punto único, creación con new())
        let sel3 = Selection::new(SelectionPoint { row: 7, col: 4 });
        assert!(sel3.contains(7, 4), "punto único debe contenerse");
        assert!(!sel3.contains(7, 5), "col adyacente fuera");

        // Caso 4: start.row > end.row, start.col muy a la izquierda
        let sel4 = Selection {
            start: SelectionPoint { row: 5, col: 0 },
            end: SelectionPoint { row: 4, col: 79 },
            mode: SelectionMode::Normal,
        };
        // Normalizado: (4,79,5,0) — la selección normalizada empieza en (4,79)
        assert!(sel4.contains(4, 79), "start normalizado (4,79)");
        assert!(sel4.contains(5, 0), "end normalizado (5,0)");
        // Row 4 solo selecciona desde col 79 en adelante (no hay límite superior
        // porque start_row solo tiene cota inferior en contains())
        assert!(
            !sel4.contains(4, 0),
            "row 4, col 0 está ANTES del start_col normalizado (79)"
        );
        // En la start_row, contiene cualquier col >= start_col
        // (geometrico, sin límite de columnas del grid)
        assert!(
            sel4.contains(4, 80),
            "col 80 >= start_col 79, geometricamente contenida"
        );
        // Row 5 (end_row): solo col <= end_col (0)
        assert!(!sel4.contains(5, 1), "col 1 > end_col 0 en end_row");
    }

    /// ADVERSARIAL: expand_to_word con fila vacía (0 celdas)
    /// No debe panic. Debe comportarse como no-op.
    #[test]
    fn test_expand_to_word_empty_cells() {
        // Fila completamente vacía (0 elementos)
        let cells: Vec<Cell> = vec![];
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.expand_to_word(&cells, 0);
        // col >= cols -> return temprano, no panic
        assert_eq!(sel.start.col, 0, "no debe cambiar start.col");
        assert_eq!(sel.end.col, 0, "no debe cambiar end.col");

        // Fila que solo tiene celdas default (ch=' ')
        let cells2: Vec<Cell> = vec![Cell::default(); 80];
        let mut sel2 = Selection::new(SelectionPoint { row: 0, col: 50 });
        sel2.expand_to_word(&cells2, 50);
        // is_word_char(' ') = false -> return sin cambios
        assert_eq!(
            sel2.start.col, 50,
            "celdas vacías no deben activar expand_to_word"
        );
        assert_eq!(sel2.end.col, 50, "end.col debe permanecer igual");
    }

    /// ADVERSARIAL: expand_to_word en una fila de un SOLO carácter
    /// Verifica que no haya off-by-one.
    #[test]
    fn test_expand_to_word_on_single_char() {
        let cells: Vec<Cell> = "X"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.expand_to_word(&cells, 0);
        // Un solo carácter: expande a (0,0)-(0,0)
        assert_eq!(sel.start.col, 0, "start debe ser 0");
        assert_eq!(sel.end.col, 0, "end debe ser 0 (único char)");
    }

    /// ADVERSARIAL: expand_to_word en el ÚLTIMO carácter de la fila
    /// Debe expandir hacia atrás correctamente.
    #[test]
    fn test_expand_to_word_on_boundary_last_char() {
        let cells: Vec<Cell> = "hello world"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 10 });
        sel.expand_to_word(&cells, 10);
        // 'd' en col 10 (última) debe expandir a "world"
        assert_eq!(sel.start.col, 6, "start debe ser 6 (inicio de 'world')");
        assert_eq!(sel.end.col, 10, "end debe ser 10 (última col)");
    }

    /// ADVERSARIAL: expand_to_word en col=0 (PRIMER carácter)
    /// start_col = 0, el while lo detecta y no hace underflow.
    #[test]
    fn test_expand_to_word_on_boundary_first_char() {
        let cells: Vec<Cell> = "hello world"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.expand_to_word(&cells, 0);
        // 'h' en col 0 (primera)
        assert_eq!(sel.start.col, 0, "start debe ser 0");
        assert_eq!(sel.end.col, 4, "end debe ser 4 (última de 'hello')");
    }

    /// ADVERSARIAL: expand_to_line con cols_count=0
    /// No debe panic. Debe producir start.col=0, end.col=0 (saturating_sub).
    #[test]
    fn test_expand_to_line_zero_cols() {
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 5 });
        sel.expand_to_line(2, 0);
        // cols_count=0 -> end.col = 0.saturating_sub(1) = 0
        assert_eq!(sel.start.row, 2, "row debe ser 2");
        assert_eq!(sel.start.col, 0, "start.col debe ser 0");
        assert_eq!(sel.end.row, 2, "end.row debe ser 2");
        assert_eq!(sel.end.col, 0, "end.col = saturating_sub(1, 0) = 0");
    }

    /// ADVERSARIAL: expand_to_word en fila SIN caracteres de palabra
    /// Solo espacios y puntuación. No debe expandir.
    #[test]
    fn test_expand_to_word_non_word_chars_only() {
        let cells: Vec<Cell> = "...   !!!   ..."
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        // Punto inicial en col 0 (.) -> no es word char -> no expande
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.expand_to_word(&cells, 0);
        assert_eq!(sel.start.col, 0, "'.' no es word char, start no cambia");
        assert_eq!(sel.end.col, 0, "end no cambia");

        // Click en '!' (col 5) -> no es word char
        let mut sel2 = Selection::new(SelectionPoint { row: 0, col: 5 });
        sel2.expand_to_word(&cells, 5);
        assert_eq!(sel2.start.col, 5, "'!' no es word char, start no cambia");
        assert_eq!(sel2.end.col, 5, "end no cambia");
    }

    /// ADVERSARIAL: expand_to_word con col muy grande (out of bounds)
    /// No debe panic. Debe no-op.
    #[test]
    fn test_expand_to_word_col_out_of_bounds() {
        let cells: Vec<Cell> = "hello"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Default::default(),
                width: 1,
            })
            .collect();
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        // col=usize::MAX > len
        sel.expand_to_word(&cells, usize::MAX);
        assert_eq!(sel.start.col, 0, "col out of bounds, no debe cambiar nada");
        assert_eq!(sel.end.col, 0, "end no debe cambiar");

        // col = len (exactamente la longitud)
        sel.expand_to_word(&cells, 5);
        assert_eq!(sel.start.col, 0, "col=len (5) >= cols(5), no-op");
        assert_eq!(sel.end.col, 0, "end no cambia");
    }

    /// ADVERSARIAL: normalize con valores extremos
    /// Verifica overflow en compare y saturación.
    #[test]
    fn test_normalize_extreme_values() {
        // start=MAX, end=0 -> normalize debe devolver (0, 0, MAX, 0)
        // Pero el código compara: start.row(MAX) < end.row(0)? -> false
        // start.row(MAX) == end.row(0)? -> false
        // else -> devuelve (0, 0, MAX, 0)
        let sel = Selection {
            start: SelectionPoint {
                row: usize::MAX,
                col: usize::MAX,
            },
            end: SelectionPoint { row: 0, col: 0 },
            mode: SelectionMode::Normal,
        };
        let (sr, sc, er, ec) = sel.normalize();
        assert_eq!(sr, 0, "start_row normalizado debe ser 0");
        assert_eq!(sc, 0, "start_col normalizado debe ser 0");
        assert_eq!(er, usize::MAX, "end_row normalizado debe ser MAX");
        assert_eq!(ec, usize::MAX, "end_col normalizado debe ser MAX");

        // Ambos MAX -> ya está forward
        let sel2 = Selection {
            start: SelectionPoint {
                row: usize::MAX,
                col: usize::MAX,
            },
            end: SelectionPoint {
                row: usize::MAX,
                col: usize::MAX,
            },
            mode: SelectionMode::Normal,
        };
        assert_eq!(
            sel2.normalize(),
            (usize::MAX, usize::MAX, usize::MAX, usize::MAX)
        );
    }

    /// ADVERSARIAL: contains con selección en alt screen (regresión)
    /// Verifica que contains no dependa del estado de alt_screen.
    #[test]
    fn test_contains_independent_of_term_state() {
        // Selection::contains es puramente geométrico,
        // no debe importar el estado del terminal.
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint { row: 0, col: 5 },
            mode: SelectionMode::Normal,
        };
        assert!(sel.contains(0, 3), "debe estar en rango");
        assert!(!sel.contains(1, 0), "fila 1 fuera de rango");
        // Misma fila, col = start_col (0) debe estar contenida
        assert!(
            sel.contains(0, 0),
            "col 0 es start_col, debe estar contenida"
        );
        // Misma fila, col después de end_col (5) no debe estar contenida
        assert!(!sel.contains(0, 6), "col 6 fuera de end_col");

        // Otra selección con rango muy grande
        let sel2 = Selection {
            start: SelectionPoint {
                row: 0,
                col: usize::MAX,
            },
            end: SelectionPoint {
                row: 0,
                col: usize::MAX,
            },
            mode: SelectionMode::Normal,
        };
        assert!(sel2.contains(0, usize::MAX), "debe contener (0,MAX)");
        assert!(!sel2.contains(0, 0), "col 0 fuera de rango");
        assert!(
            !sel2.contains(0, usize::MAX - 1),
            "col MAX-1 fuera de rango"
        );
    }

    /// Block selection: rectángulo (1,2)-(3,5) debe contener solo ese rango
    /// de columnas en cada fila entre la 1 y la 3.
    #[test]
    fn test_block_contains_forward() {
        let sel = Selection {
            start: SelectionPoint { row: 1, col: 2 },
            end: SelectionPoint { row: 3, col: 5 },
            mode: SelectionMode::Block,
        };
        // Esquinas y centro del bloque
        assert!(sel.contains(1, 2));
        assert!(sel.contains(1, 5));
        assert!(sel.contains(2, 3));
        assert!(sel.contains(3, 2));
        assert!(sel.contains(3, 5));
        // Fuera del bloque
        assert!(!sel.contains(0, 3), "fila arriba del bloque");
        assert!(!sel.contains(4, 3), "fila abajo del bloque");
        assert!(!sel.contains(2, 1), "col izquierda del bloque");
        assert!(!sel.contains(2, 6), "col derecha del bloque");
    }

    /// Block selection invertida (drag de abajo-arriba): mismo rectángulo.
    #[test]
    fn test_block_contains_reversed() {
        let sel = Selection {
            start: SelectionPoint { row: 3, col: 5 },
            end: SelectionPoint { row: 1, col: 2 },
            mode: SelectionMode::Block,
        };
        assert!(sel.contains(2, 3));
        assert!(!sel.contains(2, 1));
        assert!(!sel.contains(2, 6));
        assert_eq!(sel.block_col_range(2), Some((2, 5)));
        assert_eq!(sel.block_col_range(0), None, "fila fuera del bloque");
    }

    /// block_col_range solo aplica en modo Block.
    #[test]
    fn test_block_col_range_only_for_block() {
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint { row: 2, col: 4 },
            mode: SelectionMode::Normal,
        };
        assert_eq!(sel.block_col_range(1), None, "Normal no tiene block range");
    }
}
