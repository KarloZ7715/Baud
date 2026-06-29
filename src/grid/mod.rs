use crate::ansi::Attrs;
use std::collections::VecDeque;

mod damage;

pub use damage::{DamageSnapshot, GridDamage};

/// Número de filas por defecto del grid virtual.
pub const DEFAULT_ROWS: usize = 24;
/// Número de columnas por defecto del grid virtual.
pub const DEFAULT_COLS: usize = 80;

/// Máximo número de líneas guardadas en el scrollback (MVP).
// ponytail: 100 lineas fijas, sin configuracion.
pub const MAX_SCROLLBACK: usize = 100;

/// Celda individual del terminal: un carácter con sus atributos y ancho.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Caracter almacenado.
    pub ch: char,
    /// Atributos de estilo de esta celda.
    pub attrs: Attrs,
    /// Ancho del carácter (en columnas). 1 para latino, 2 para CJK, etc.
    pub width: u8,
    /// Indice en `Term::hyperlinks` (OSC 8); `None` si la celda no tiene link.
    pub hyperlink: Option<u32>,
}

/// Grid virtual con tamaño dinámico que representa el buffer del terminal.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Matriz de celdas: rows[row][col].
    pub rows: Vec<Vec<Cell>>,
    /// Líneas que hicieron scroll por arriba de la región.
    /// La fila más reciente está al final.
    // ponytail: scrollback minimo con reflow.
    pub scrollback: VecDeque<Vec<Cell>>,
    /// Número actual de filas del grid.
    pub rows_count: usize,
    /// Número actual de columnas del grid.
    pub cols_count: usize,
    /// Indica si cada fila es continuación de la anterior por soft-wrap (true)
    /// o por hard break / Enter explícito (false).
    /// Usado por reflow para decidir si insertar un newline marker entre filas.
    pub row_continuations: Vec<bool>,
    /// Celdas modificadas desde el último frame (render incremental).
    pub damage: GridDamage,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            attrs: Attrs::default(),
            width: 1,
            hyperlink: None,
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl Grid {
    /// Crea un grid vacío: `DEFAULT_ROWS` filas, `DEFAULT_COLS` columnas,
    /// todo espacios con atributos por defecto.
    pub fn new() -> Self {
        Self::new_sized(DEFAULT_ROWS, DEFAULT_COLS)
    }

    /// Crea un grid vacío con el tamaño especificado.
    pub fn new_sized(rows: usize, cols: usize) -> Self {
        Self {
            rows: vec![vec![Cell::default(); cols]; rows],
            scrollback: VecDeque::with_capacity(MAX_SCROLLBACK),
            rows_count: rows,
            cols_count: cols,
            row_continuations: vec![false; rows],
            damage: GridDamage::new(rows, cols),
        }
    }

    /// Obtiene una referencia a la celda en (row, col).
    /// Panic si row/col están fuera de rango (no debería pasar con clamp en cursor).
    pub fn get(&self, row: usize, col: usize) -> &Cell {
        &self.rows[row][col]
    }

    /// Obtiene una referencia mutable a la celda en (row, col), si existe.
    pub fn cell(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row < self.rows_count && col < self.cols_count {
            self.rows.get_mut(row).and_then(|r| r.get_mut(col))
        } else {
            None
        }
    }

    /// Escribe un carácter y atributos en la celda (row, col).
    pub fn set(&mut self, row: usize, col: usize, ch: char, attrs: Attrs) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            cell.ch = ch;
            cell.attrs = attrs;
            self.damage.mark_cell(row, col);
        }
    }

    /// Marca columnas de continuacion de un glifo ancho (width >= 2).
    pub fn mark_wide_continuation(&mut self, row: usize, col: usize, width: u8, attrs: Attrs) {
        let w = width.max(2) as usize;
        for c in (col + 1)..col.saturating_add(w).min(self.cols_count) {
            if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(c)) {
                cell.ch = ' ';
                cell.width = 0;
                cell.attrs = attrs;
            }
            self.damage.mark_cell(row, c);
        }
    }

    /// Marca una celda y columnas de continuación de glifo ancho.
    pub fn mark_cell_written(&mut self, row: usize, col: usize, width: u8) {
        let w = width.max(1) as usize;
        for c in col..col.saturating_add(w).min(self.cols_count) {
            self.damage.mark_cell(row, c);
        }
    }

    /// Limpia todo el grid: rellena con espacios y atributos por defecto.
    pub fn clear(&mut self) {
        self.resync_continuations();
        for row in &mut self.rows {
            for cell in row.iter_mut() {
                *cell = Cell::default();
            }
        }
        self.row_continuations.fill(false);
        self.damage.mark_all();
    }

    /// Limpia una línea desde `from` hasta `to` (exclusivo) con espacios.
    pub fn clear_line(&mut self, row: usize, from: usize, to: usize) {
        let end = to.min(self.cols_count);
        for col in from..end {
            self.rows[row][col] = Cell::default();
        }
        self.damage.mark_row_range(row, from, end);
    }

    /// Scroll up: mueve todas las filas de la región [top, bottom] una posición
    /// hacia arriba. La fila `bottom` queda en blanco.
    // ponytail: alt screen tambien acumula scrollback (bug aceptado).
    pub fn scroll_up_region(&mut self, n: usize, top: usize, bottom: usize) {
        for _ in 0..n {
            if top < self.rows_count && bottom < self.rows_count && top <= bottom {
                let row_to_save = self.rows[top].clone();
                self.push_scrollback(row_to_save);
                self.resync_continuations();
                self.rows.remove(top);
                self.rows
                    .insert(bottom, vec![Cell::default(); self.cols_count]);
                self.row_continuations.remove(top);
                self.row_continuations.insert(bottom, false);
            }
        }
        self.damage.mark_all();
    }

    /// Scroll down: mueve todas las filas de la región [top, bottom] una posición
    /// hacia abajo. La fila `top` queda en blanco. Por ahora solo se usa
    /// internamente; no expuesto en CSI todavía.
    #[allow(dead_code)]
    pub fn scroll_down_region(&mut self, n: usize, top: usize, bottom: usize) {
        for _ in 0..n {
            if top < self.rows_count && bottom < self.rows_count && top <= bottom {
                self.resync_continuations();
                self.rows.remove(bottom);
                self.rows
                    .insert(top, vec![Cell::default(); self.cols_count]);
                self.row_continuations.remove(bottom);
                self.row_continuations.insert(top, false);
            }
        }
    }

    /// Desplaza las filas [row, total_rows) una posición hacia abajo. La fila
    /// `row` queda en blanco. Usado por IL (insert line).
    // ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn insert_line(&mut self, row: usize) {
        if row < self.rows_count {
            self.resync_continuations();
            let blank = vec![Cell::default(); self.cols_count];
            self.rows.remove(self.rows_count - 1);
            self.rows.insert(row, blank);
            self.row_continuations.remove(self.rows_count - 1);
            self.row_continuations.insert(row, false);
            self.damage.mark_all();
        }
    }

    /// Desplaza las filas [row+1, total_rows) una posición hacia arriba. La fila
    /// (total_rows - 1) queda en blanco. Usado por DL (delete line).
    // ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn delete_line(&mut self, row: usize) {
        if row < self.rows_count {
            self.resync_continuations();
            self.rows.remove(row);
            self.rows.push(vec![Cell::default(); self.cols_count]);
            self.row_continuations.remove(row);
            self.row_continuations.push(false);
            self.damage.mark_all();
        }
    }

    /// Inserta `n` caracteres en blanco en la posición (row, col), desplazando
    /// el resto de la línea a la derecha. Caracteres que salen por la derecha
    /// se pierden. Usado por ICH (insert character).
    pub fn insert_chars(&mut self, row: usize, col: usize, n: usize) {
        if row < self.rows_count && col < self.cols_count {
            let actual_n = n.min(self.cols_count - col);
            for _ in 0..actual_n {
                self.rows[row].pop();
                self.rows[row].insert(col, Cell::default());
            }
            self.damage.mark_row_range(row, col, self.cols_count);
        }
    }

    /// Borra `n` caracteres en la posición (row, col), desplazando el resto de
    /// la línea a la izquierda. Caracteres que quedan al final se llenan con
    /// blancos. Usado por DCH (delete character).
    pub fn delete_chars(&mut self, row: usize, col: usize, n: usize) {
        if row < self.rows_count && col < self.cols_count {
            let actual_n = n.min(self.cols_count - col);
            for _ in 0..actual_n {
                self.rows[row].remove(col);
                self.rows[row].push(Cell::default());
            }
            self.damage.mark_row_range(row, col, self.cols_count);
        }
    }

    /// Cambia el tamaño del grid a `new_rows` x `new_cols`.
    /// Preserva el contenido existente tanto como sea posible.
    /// Si el nuevo grid es más grande, las celdas nuevas son default.
    /// Si es más pequeño, se truncan/descartan filas/columnas sobrantes.
    /// Retorna cuantas filas se eliminaron del principio (0 si el grid crecio).
    // ponytail: con reflow.
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) -> usize {
        const MAX_GRID: usize = 4096;
        let new_rows = new_rows.clamp(1, MAX_GRID);
        let new_cols = new_cols.clamp(1, MAX_GRID);
        self.resync_continuations();
        // Primero truncar o expandir columnas en cada fila existente.
        for row in &mut self.rows {
            if new_cols < row.len() {
                row.truncate(new_cols);
            } else {
                row.extend(std::iter::repeat_n(Cell::default(), new_cols - row.len()));
            }
        }

        let mut rows_removed = 0usize;

        // Luego truncar o expandir filas.
        if new_rows < self.rows.len() {
            // Truncar del PRINCIPIO (lo mas antiguo), no del final.
            // El prompt y el contenido reciente deben quedar al fondo.
            rows_removed = self.rows.len() - new_rows;
            let truncated: Vec<Vec<Cell>> = self.rows.drain(..rows_removed).collect();
            // ponytail: truncar sin scrollback en resize; SIGWINCH redibuja prompt.
            drop(truncated);
            self.row_continuations.drain(..rows_removed);
        } else {
            let added = new_rows - self.rows.len();
            let blank_row = vec![Cell::default(); new_cols];
            self.rows.extend(std::iter::repeat_n(blank_row, added));
            self.row_continuations
                .extend(std::iter::repeat_n(false, added));
        }

        self.rows_count = new_rows;
        self.cols_count = new_cols;
        Self::normalize_row_lengths(&mut self.rows, new_cols);
        self.damage.resize(new_rows, new_cols);
        rows_removed
    }

    /// Garantiza que cada fila tenga exactamente `cols` celdas.
    fn normalize_row_lengths(rows: &mut [Vec<Cell>], cols: usize) {
        for row in rows.iter_mut() {
            if row.len() < cols {
                row.extend(std::iter::repeat_n(Cell::default(), cols - row.len()));
            } else if row.len() > cols {
                row.truncate(cols);
            }
        }
    }

    /// Toma el snapshot de daño y resetea el tracker.
    pub fn take_damage(&mut self) -> DamageSnapshot {
        self.damage.take()
    }

    /// Guarda una fila en el scrollback cuando sale por arriba de la pantalla.
    fn push_scrollback(&mut self, row: Vec<Cell>) {
        if self.scrollback.len() >= MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(row);
    }

    /// Marca una fila como continuación de la anterior por soft-wrap (true)
    /// o como hard break / Enter explícito (false).
    pub fn set_continuation(&mut self, row: usize, val: bool) {
        self.resync_continuations();
        if let Some(c) = self.row_continuations.get_mut(row) {
            *c = val;
        }
    }

    /// Ensure row_continuations length matches self.rows, auto-healing
    /// any desync caused by code paths that modify rows without updating
    /// continuations.
    fn resync_continuations(&mut self) {
        while self.row_continuations.len() < self.rows.len() {
            self.row_continuations.push(false);
        }
        self.row_continuations.truncate(self.rows.len());
    }

    fn push_wide_continuation(row: &mut Vec<Cell>) {
        row.push(Cell {
            width: 0,
            ..Cell::default()
        });
    }

    /// Cuenta caracteres lógicos escritos antes de la posición del cursor.
    fn logical_offset_before_cursor(
        rows: &[Vec<Cell>],
        cursor_row: usize,
        cursor_col: usize,
    ) -> usize {
        let mut offset = 0usize;
        let max_row = cursor_row.min(rows.len().saturating_sub(1));
        for (idx, row) in rows.iter().enumerate().take(max_row + 1) {
            let end_col = if idx == cursor_row {
                cursor_col.min(row.len())
            } else {
                row.len()
            };
            let mut col = 0;
            while col < end_col {
                if col >= row.len() {
                    break;
                }
                let cell = row[col];
                if cell.width == 0 {
                    col += 1;
                    continue;
                }
                if cell != Cell::default() {
                    offset += 1;
                }
                col += (cell.width as usize).max(1);
            }
        }
        offset
    }

    /// Mapea un offset lógico a (fila, col) tras redistribuir el contenido plano.
    fn cursor_from_offset_in_flat(flat: &[Cell], new_cols: usize, target: usize) -> (usize, usize) {
        if target == 0 {
            return (0, 0);
        }
        let mut placed = 0usize;
        let mut row_idx = 0usize;
        let mut col = 0usize;

        for cell in flat {
            if cell.ch == '\n' && cell.width == 0 {
                row_idx += 1;
                col = 0;
                continue;
            }
            let w = cell.width as usize;
            if w == 0 {
                continue;
            }
            if col > 0 && col + w > new_cols {
                row_idx += 1;
                col = 0;
            }
            if placed == target {
                return (row_idx, col.min(new_cols.saturating_sub(1)));
            }
            placed += 1;
            col += w;
        }
        (row_idx, col.min(new_cols.saturating_sub(1)))
    }

    /// Reflow sin seguimiento de cursor (tests y benchmarks).
    pub fn reflow(&mut self, new_cols: usize) {
        let _ = self.reflow_with_cursor(new_cols, None);
    }

    /// Reflow: concatena todo el contenido logico del grid en una secuencia
    /// plana de celdas (preservando filas vacias como marcadores de nueva linea)
    /// y lo re-divide en filas de `new_cols` columnas.
    ///
    /// * Se inserta un marcador de nueva linea (`Cell { ch: '\\n', width: 0 }`)
    ///   entre filas con contenido consecutivo para preservar los limites de
    ///   linea al ensanchar.
    /// * Los caracteres CJK (width >= 2) se manejan correctamente, saltando
    ///   las celdas de relleno durante la extraccion logica y reinsertandolos
    ///   durante la redistribucion.
    /// * Si el numero de filas resultante excede `rows_count`, las filas
    ///   sobrantes mas antiguas se envian al scrollback.
    /// * Este metodo modifica `cols_count` pero NO `rows_count` (el llamante,
    ///   ej. `resize_grid`, ajusta `rows_count` posteriormente via `resize`).
    pub fn reflow_with_cursor(
        &mut self,
        new_cols: usize,
        cursor: Option<(usize, usize)>,
    ) -> Option<(usize, usize)> {
        let old_rows: Vec<Vec<Cell>> = self.rows.drain(..).collect();
        let old_row_continuations = self.row_continuations.clone();
        let cursor_offset =
            cursor.map(|(r, c)| Self::logical_offset_before_cursor(&old_rows, r, c));
        // Asegurar que continuations tenga la longitud correcta por seguridad
        self.resync_continuations();

        // Encontrar la ultima fila con contenido no-default.
        let last_content_row = old_rows
            .iter()
            .rposition(|row| row.iter().any(|cell| *cell != Cell::default()))
            .unwrap_or(0);

        // ---- Pasos 1-3: aplanar todas las filas en una secuencia logica ----

        let mut flat: Vec<Cell> = Vec::new();

        for (idx, old_row) in old_rows.into_iter().enumerate() {
            if idx > last_content_row {
                break;
            }

            let content_len = old_row
                .iter()
                .rposition(|cell| *cell != Cell::default())
                .map(|pos| pos + 1)
                .unwrap_or(0);

            // Extraer celdas logicas de esta fila, saltando relleno CJK.
            let mut i = 0;
            while i < content_len {
                let cell = old_row[i];
                if cell != Cell::default() {
                    flat.push(cell);
                    i += cell.width as usize;
                } else {
                    flat.push(cell);
                    i += 1;
                }
            }

            // Insertar marcador de nueva linea solo si es un hard break
            // (es decir, la fila SIGUIENTE NO es continuacion por wrap de esta).
            // Los flags de continuation los setea do_pending_wrap: continuation[N] = true
            // significa que la fila N se alcanzo por wrap desde la fila N-1.
            if idx < last_content_row {
                let next_is_continuation =
                    old_row_continuations.get(idx + 1).copied().unwrap_or(false);
                if !next_is_continuation {
                    flat.push(Cell {
                        ch: '\n',
                        width: 0,
                        ..Cell::default()
                    });
                }
            }
        }

        // ---- Step 4: if the grid was completely empty, just fill and return ----

        if flat.is_empty() {
            self.rows = vec![vec![Cell::default(); new_cols]; self.rows_count];
            self.cols_count = new_cols;
            return cursor.map(|(r, c)| {
                (
                    r.min(self.rows_count.saturating_sub(1)),
                    c.min(new_cols.saturating_sub(1)),
                )
            });
        }

        // ---- Step 5: re-divide the flat sequence into rows of new_cols ----
        // Also compute row_continuations: rows split by width (soft wrap) get
        // continuation=true, rows separated by newline in the flat get
        // continuation=false.

        let mut new_rows: Vec<Vec<Cell>> = Vec::new();
        let mut new_continuations: Vec<bool> = Vec::new();
        let mut row_after_newline = true;

        let mut current_row: Vec<Cell> = Vec::with_capacity(new_cols);
        let mut col = 0usize;

        for cell in &flat {
            // Newline marker: flush the current row (preserving empty rows).
            if cell.ch == '\n' && cell.width == 0 {
                if current_row.is_empty() {
                    new_rows.push(vec![Cell::default(); new_cols]);
                } else {
                    while current_row.len() < new_cols {
                        current_row.push(Cell::default());
                    }
                    new_rows.push(current_row);
                }
                new_continuations.push(!row_after_newline);
                current_row = Vec::with_capacity(new_cols);
                col = 0;
                row_after_newline = true; // next row starts after a newline
                continue;
            }

            let w = cell.width as usize;

            // Skip zero-width placeholders (shouldn't appear, but be safe).
            if w == 0 {
                continue;
            }

            if col + w <= new_cols {
                // Fits in the current row.
                current_row.push(*cell);
                for _ in 1..w {
                    Self::push_wide_continuation(&mut current_row);
                }
                col += w;
            } else if col == 0 && w > new_cols {
                // Doesn't fit even as the first character: force it in.
                current_row.push(*cell);
                for _ in 1..w.min(new_cols) {
                    Self::push_wide_continuation(&mut current_row);
                }
                col = w.min(new_cols);
            } else {
                // Doesn't fit at the end: flush current row, start a new one.
                while current_row.len() < new_cols {
                    current_row.push(Cell::default());
                }
                new_rows.push(current_row);
                // This new row is a soft wrap continuation (unless it's right
                // after a newline, in which case row_after_newline is still
                // true and we push false).
                new_continuations.push(!row_after_newline);

                current_row = Vec::with_capacity(new_cols);
                current_row.push(*cell);
                for _ in 1..w {
                    Self::push_wide_continuation(&mut current_row);
                }
                col = w;
                row_after_newline = false; // no hard break before this continuation
            }
        }

        // Flush any remaining content in the last row.
        if !current_row.is_empty() {
            while current_row.len() < new_cols {
                current_row.push(Cell::default());
            }
            new_rows.push(current_row);
            // Last row: its continuation flag depends on how it started.
            new_continuations.push(!row_after_newline);
        }

        // ---- Step 6: overflow rows (oldest first) go to scrollback ----

        let pre_overflow_cursor =
            cursor_offset.map(|offset| Self::cursor_from_offset_in_flat(&flat, new_cols, offset));
        let overflow = new_rows.len().saturating_sub(self.rows_count);

        if overflow > 0 {
            for row in new_rows.drain(..overflow) {
                self.push_scrollback(row);
            }
            new_continuations.drain(..overflow);
        }

        // ---- Step 7: pad with empty rows up to rows_count ----

        while new_rows.len() < self.rows_count {
            new_rows.push(vec![Cell::default(); new_cols]);
            new_continuations.push(false);
        }

        // ---- Step 8: assign ----

        self.rows = new_rows;
        self.row_continuations = new_continuations;
        self.cols_count = new_cols;
        self.damage.mark_all();

        pre_overflow_cursor.map(|(row, col)| {
            let row = row.saturating_sub(overflow);
            (
                row.min(self.rows_count.saturating_sub(1)),
                col.min(new_cols.saturating_sub(1)),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollback_pushes_on_scroll_up() {
        let mut grid = Grid::new();
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        assert_eq!(grid.scrollback.len(), 1);
    }

    #[test]
    fn test_scrollback_drops_oldest_when_full() {
        let mut grid = Grid::new();
        for _ in 0..=MAX_SCROLLBACK {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), MAX_SCROLLBACK);
    }

    #[test]
    fn test_grid_resize_larger() {
        let mut grid = Grid::new();
        // Escribir algunos caracteres
        grid.rows[0][0].ch = 'A';
        grid.rows[1][2].ch = 'B';
        // Agrandar a 30x100
        grid.resize(30, 100);
        assert_eq!(grid.rows_count, 30);
        assert_eq!(grid.cols_count, 100);
        assert_eq!(grid.rows.len(), 30);
        assert_eq!(grid.rows[0].len(), 100);
        // Contenido preservado en su fila original; filas nuevas abajo son default
        assert_eq!(grid.rows[0][0].ch, 'A');
        assert_eq!(grid.rows[1][2].ch, 'B');
        assert_eq!(grid.rows[0][80].ch, ' ');
        assert_eq!(grid.rows[29][0].ch, ' ');
    }

    #[test]
    fn test_grid_resize_smaller_adjusts_cursor_offset() {
        let mut grid = Grid::new();
        grid.resize(40, 80);
        let removed = grid.resize(24, 80);
        assert_eq!(removed, 16);
    }

    #[test]
    fn test_reflow_tracks_cursor_on_narrow() {
        let mut grid = Grid::new();
        for col in 0..10 {
            grid.rows[0][col].ch = (b'A' + col as u8) as char;
        }
        let cursor = grid.reflow_with_cursor(5, Some((0, 7)));
        assert_eq!(cursor, Some((1, 2)));
    }

    #[test]
    fn test_grid_resize_smaller() {
        let mut grid = Grid::new();
        // Escribir en las últimas filas (las que sobreviven al truncar del inicio)
        let last = grid.rows_count - 1;
        grid.rows[last][0].ch = 'Z';
        grid.rows[last][5].ch = 'Y';
        // Achicar a 5x10 — se truncan las primeras filas, las últimas se preservan
        grid.resize(5, 10);
        assert_eq!(grid.rows_count, 5);
        assert_eq!(grid.cols_count, 10);
        assert_eq!(grid.rows.len(), 5);
        assert_eq!(grid.rows[0].len(), 10);
        // Las últimas filas del grid original se preservan (Z, Y deben estar)
        // Después de truncar 24→5, las filas 19-23 se convierten en 0-4
        // row[23] tenía Z, row[23] se convierte en row[4] del nuevo grid
        assert_eq!(grid.rows[4][0].ch, 'Z');
        assert_eq!(grid.rows[4][5].ch, 'Y');
        // ponytail: resize trunca sin scrollback.
        assert!(
            grid.scrollback.is_empty(),
            "resize no debe empujar filas al scrollback"
        );
    }

    // -----------------------------------------------------------------------
    // Tests: reflow
    // -----------------------------------------------------------------------

    /// Reflow a grid angosto: una linea larga se divide en varias filas.
    #[test]
    fn test_reflow_narrower() {
        let mut grid = Grid::new();
        // Llenar fila 0 con "ABCDEFGHIJ" desde col 0
        let chars = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J'];
        for (i, &ch) in chars.iter().enumerate() {
            grid.rows[0][i].ch = ch;
        }
        // Reflow desde DEFAULT_COLS (80) a 5 columnas
        grid.reflow(5);
        assert_eq!(grid.cols_count, 5);
        // Cada fila original se re-envuelve independientemente.
        // Fila 0: "ABCDE"
        assert_eq!(grid.rows[0][0].ch, 'A');
        assert_eq!(grid.rows[0][4].ch, 'E');
        // Fila 1: "FGHIJ"
        assert_eq!(grid.rows[1][0].ch, 'F');
        assert_eq!(grid.rows[1][4].ch, 'J');
        // Tercera fila debe estar vacia (relleno)
        assert_eq!(grid.rows[2][0].ch, ' ');
    }

    /// Reflow a grid mas ancho: filas cortas se quedan en su propia linea.
    #[test]
    fn test_reflow_wider() {
        let mut grid = Grid::new();
        // Escribir "ABC" en fila 0 y "DEF" en fila 1
        grid.rows[0][0].ch = 'A';
        grid.rows[0][1].ch = 'B';
        grid.rows[0][2].ch = 'C';
        grid.rows[1][0].ch = 'D';
        grid.rows[1][1].ch = 'E';
        grid.rows[1][2].ch = 'F';
        // Reflow a 80 columnas (mas ancho que el contenido)
        grid.reflow(80);
        assert_eq!(grid.cols_count, 80);
        // Los limites de linea se preservan: "ABC" se queda en fila 0, "DEF" en fila 1.
        assert_eq!(grid.rows[0][0].ch, 'A');
        assert_eq!(grid.rows[0][1].ch, 'B');
        assert_eq!(grid.rows[0][2].ch, 'C');
        assert_eq!(grid.rows[0][3].ch, ' ');
        assert_eq!(grid.rows[1][0].ch, 'D');
        assert_eq!(grid.rows[1][1].ch, 'E');
        assert_eq!(grid.rows[1][2].ch, 'F');
    }

    /// Reflow con caracteres CJK (width=2) respeta el ancho del caracter.
    #[test]
    fn test_reflow_cjk() {
        let mut grid = Grid::new();
        // '中' (U+4E2D) tiene width=2, colocar uno en col 0 y otro en col 4
        // Fila 0: [中(w=2), _, A(w=1), B(w=1), 中(w=2), _, C(w=1), ...]
        grid.rows[0][0].ch = '\u{4e2d}';
        grid.rows[0][0].width = 2;
        grid.rows[0][2].ch = 'A';
        grid.rows[0][3].ch = 'B';
        grid.rows[0][4].ch = '\u{4e2d}';
        grid.rows[0][4].width = 2;
        grid.rows[0][6].ch = 'C';

        // Reflow a 4 columnas (justo, fuerza division de CJK)
        // Flat: [中(2), space(1), A, B, 中(2), space(1), C, ...]
        // Fila 0 (4 cols): 中, _, A, B  (中 en col 0-1, A en 2, B en 3)
        // Fila 1 (4 cols): 中, _, C, _  (中 en col 0-1, C en 2, _ en 3)
        // Fila 2+: vacia
        grid.reflow(4);
        assert_eq!(grid.cols_count, 4);

        // Row 0: 中 at col 0, default at col 1, A at col 2, B at col 3
        assert_eq!(grid.rows[0][0].ch, '\u{4e2d}');
        assert_eq!(grid.rows[0][0].width, 2);
        assert_eq!(grid.rows[0][1].ch, ' ');
        assert_eq!(grid.rows[0][2].ch, 'A');
        assert_eq!(grid.rows[0][3].ch, 'B');

        // Row 1: 中 at col 0, default at col 1, C at col 2
        assert_eq!(grid.rows[1][0].ch, '\u{4e2d}');
        assert_eq!(grid.rows[1][0].width, 2);
        assert_eq!(grid.rows[1][1].ch, ' ');
        assert_eq!(grid.rows[1][2].ch, 'C');
        assert_eq!(grid.rows[1][3].ch, ' ');
    }

    /// Reflow envia filas sobrantes al scrollback.
    #[test]
    fn test_reflow_overflow_to_scrollback() {
        let mut grid = Grid::new();
        // Llenar primera fila con texto que desbordara al angostar
        let text = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let chars: Vec<char> = text.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            if i < grid.cols_count {
                grid.rows[0][i].ch = ch;
            }
        }
        // Reducir rows_count a 3 y cols a 4, luego reflow
        // Solo la fila 0 tiene contenido (26 celdas). 26/4 = 7 filas de contenido.
        // rows_count=3, entonces 7-3 = 4 desbordan al scrollback.
        grid.rows_count = 3;
        grid.reflow(4);
        // Las nuevas filas deben ser exactamente rows_count (3)
        assert_eq!(grid.rows.len(), 3);
        // El scrollback debe tener las filas desbordadas
        assert_eq!(grid.scrollback.len(), 4);
        // Primera fila visible = flat[16..20] = "QRST"
        assert_eq!(grid.rows[0][0].ch, 'Q');
        assert_eq!(grid.rows[0][1].ch, 'R');
        assert_eq!(grid.rows[0][2].ch, 'S');
        assert_eq!(grid.rows[0][3].ch, 'T');
        // Scrollback fila 0 (mas antigua) = flat[0..4] = "ABCD"
        assert_eq!(grid.scrollback[0][0].ch, 'A');
        assert_eq!(grid.scrollback[0][3].ch, 'D');
        // Scrollback fila 3 (mas reciente) = flat[12..16] = "MNOP"
        assert_eq!(grid.scrollback[3][0].ch, 'M');
        assert_eq!(grid.scrollback[3][3].ch, 'P');
    }

    /// Reflow angosto luego ancho: verifica que las lineas divididas
    /// se fusionan correctamente al ensanchar. Test de regresion
    /// para el reporte de bug del usuario.
    #[test]
    fn test_reflow_narrow_then_wide_merges_lines() {
        let mut grid = Grid::new();
        grid.resize(24, 120);
        for col in 0..120 {
            grid.rows[0][col].ch = 'X';
        }

        // Paso 1: angostar a 50 columnas
        grid.reflow(50);
        assert_eq!(grid.cols_count, 50);
        assert!(grid.row_continuations[1], "fila 1 debe ser continuacion");
        assert!(grid.row_continuations[2], "fila 2 debe ser continuacion");

        // Paso 2: ensanchar de vuelta a 120 columnas
        grid.reflow(120);
        assert_eq!(grid.cols_count, 120);
        let total_x: usize = (0..grid.rows_count)
            .map(|r| grid.rows[r].iter().filter(|c| c.ch == 'X').count())
            .sum();
        assert_eq!(total_x, 120, "all 120 X chars should be preserved");
        let row0_x = grid.rows[0].iter().filter(|c| c.ch == 'X').count();
        assert!(row0_x >= 100, "row 0 should have most content after merge");
    }

    /// Pipeline completo de resize: reflow + resize, simulando resize_grid().
    #[test]
    fn test_reflow_narrow_then_wide_full_pipeline() {
        let mut grid = Grid::new();
        grid.resize(56, 120);
        for col in 0..120 {
            grid.rows[0][col].ch = 'X';
        }

        // Simular angostamiento: reflow + resize como hace resize_grid
        grid.reflow(50);
        grid.resize(56, 50);
        let total_x_before: usize = grid
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter(|c| c.ch == 'X')
            .count();
        assert_eq!(total_x_before, 120, "todas las X tras angostar");

        // Simular ensanchamiento: reflow + resize
        grid.reflow(120);
        grid.resize(56, 120);
        let total_x_after: usize = grid
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter(|c| c.ch == 'X')
            .count();
        assert_eq!(total_x_after, 120, "todas las 120 X tras pipeline completo");
        let row0_x = grid.rows[0].iter().filter(|c| c.ch == 'X').count();
        assert!(
            row0_x >= 100,
            "la fila 0 debe tener la mayoria tras pipeline completo"
        );
    }

    #[test]
    fn resize_shrink_does_not_push_scrollback() {
        let mut grid = Grid::new();
        grid.resize(10, 80);
        let scrollback_before = grid.scrollback.len();
        grid.resize(5, 80);
        assert_eq!(grid.scrollback.len(), scrollback_before);
    }

    /// Verifica que resize (encoger y crecer) no corrompe el grid.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn test_resize_shrink_grow_no_corruption() {
        let mut grid = Grid::new();
        grid.resize(10, 80);
        for r in 0..10 {
            for c in 0..5 {
                grid.rows[r][c].ch = (b'A' + r as u8) as char;
            }
        }
        let original: Vec<String> = grid
            .rows
            .iter()
            .map(|r| r.iter().take(5).map(|c| c.ch).collect())
            .collect();

        // Encoger luego crecer de vuelta
        grid.resize(5, 80);
        grid.resize(10, 80);

        // Tras truncar del inicio, se preservan las ultimas 5 filas del grid original
        for r in 0..5 {
            let s: String = grid.rows[r].iter().take(5).map(|c| c.ch).collect();
            assert_eq!(s, original[r + 5], "la fila {r} debe preservarse");
        }
        // Las filas 5-9 son nuevas (vacias) al crecer por extension abajo
        for r in 5..10 {
            assert!(
                grid.rows[r].iter().all(|c| *c == Cell::default()),
                "la fila {r} debe estar vacia tras crecer"
            );
        }
        assert_eq!(grid.rows[0].len(), 80, "todas las filas tienen new_cols");
    }

    #[test]
    fn test_scrollback_1000_lines() {
        let mut grid = Grid::new();
        for i in 0..1000 {
            // Escribir una linea de texto identificable en la primera fila y hacer scroll
            grid.rows[0][0].ch = char::from_digit((i % 10) as u32, 10).unwrap();
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), MAX_SCROLLBACK);
        // Verificar que la linea mas reciente en scrollback corresponde a i=999
        let last = grid.scrollback.back().unwrap();
        assert_eq!(last[0].ch, '9');
    }
}
