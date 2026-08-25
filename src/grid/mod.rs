#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::ansi::{Color, PackedAttrs, Term};
use std::collections::VecDeque;

mod damage;

pub use damage::{DamageSnapshot, GridDamage};

/// Número de filas por defecto del grid virtual.
pub const DEFAULT_ROWS: usize = 24;
/// Número de columnas por defecto del grid virtual.
pub const DEFAULT_COLS: usize = 80;

/// Límite por defecto de líneas en scrollback.
pub const DEFAULT_MAX_SCROLLBACK: usize = 10_000;

/// Alias histórico para tests y benches que fijaban el límite anterior.
pub const MAX_SCROLLBACK: usize = DEFAULT_MAX_SCROLLBACK;

/// Centinela de "sin índice" para `Cell::hyperlink`/`Cell::extra_codepoints`.
/// `0` no sirve como centinela: sería un índice válido (el primer elemento).
const NO_INDEX: u32 = u32::MAX;

/// Celda individual del terminal: un carácter con sus atributos y ancho.
///
/// 24 bytes: `ch` (4) + `attrs` (12: fg/bg empaquetados + flags + índice de
/// underline_color) + `hyperlink` (4) + `extra_codepoints` (4). Los campos
/// crudos son privados; los accesores devuelven los mismos tipos que antes
/// del empaquetado (`Option<u32>`, `Color`, `u8`) para minimizar el impacto
/// en el resto del código.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Caracter almacenado.
    pub ch: char,
    /// Atributos de estilo de esta celda, empaquetados.
    pub attrs: PackedAttrs,
    /// Indice en `Term::hyperlinks` (OSC 8); `NO_INDEX` si no tiene link.
    hyperlink: u32,
    /// Indice en `Term::grapheme_extras`: codepoints del grafema mas alla
    /// de `ch` (siempre el primer codepoint del cluster). `NO_INDEX` si la
    /// celda tiene un solo codepoint.
    extra_codepoints: u32,
}

const _: () = assert!(std::mem::size_of::<Cell>() == 24);

impl Cell {
    /// Ancho del carácter en columnas (0, 1 o 2). 0 marca la continuación
    /// de un carácter ancho o un marcador de línea sin glifo propio.
    pub fn width(&self) -> u8 {
        self.attrs.width()
    }
    pub fn set_width(&mut self, width: u8) {
        self.attrs.set_width(width);
    }

    pub fn hyperlink(&self) -> Option<u32> {
        (self.hyperlink != NO_INDEX).then_some(self.hyperlink)
    }
    pub fn set_hyperlink(&mut self, hyperlink: Option<u32>) {
        self.hyperlink = hyperlink.unwrap_or(NO_INDEX);
    }

    pub fn extra_codepoints(&self) -> Option<u32> {
        (self.extra_codepoints != NO_INDEX).then_some(self.extra_codepoints)
    }
    pub fn set_extra_codepoints(&mut self, extra_codepoints: Option<u32>) {
        self.extra_codepoints = extra_codepoints.unwrap_or(NO_INDEX);
    }

    /// Resuelve el color de subrayado (SGR 58) contra la tabla del `term`
    /// del que salió esta celda.
    pub fn underline_color(&self, term: &Term) -> Color {
        self.attrs.underline_color(term)
    }
    pub fn set_underline_color(&mut self, term: &mut Term, color: Color) {
        self.attrs.set_underline_color(term, color);
    }

    /// Convierte los atributos empaquetados a `Attrs`, resolviendo
    /// `underline_color` contra `term`.
    pub fn to_attrs(&self, term: &Term) -> crate::ansi::Attrs {
        self.attrs.to_attrs(term)
    }

    /// Constructor de conveniencia: celda en blanco con un carácter dado.
    pub fn with_ch(ch: char) -> Self {
        Self {
            ch,
            ..Self::default()
        }
    }
    /// Constructor de conveniencia: celda en blanco con un ancho dado
    /// (0 para marcadores de continuación / newline).
    pub fn with_width(width: u8) -> Self {
        let mut cell = Self::default();
        cell.set_width(width);
        cell
    }
    /// Combina `with_ch` seguido de `set_width`, para marcadores como el
    /// newline synthetic de reflow.
    pub fn with_ch_and_width(ch: char, width: u8) -> Self {
        let mut cell = Self::with_ch(ch);
        cell.set_width(width);
        cell
    }

    /// Verdadero si la celda es indistinguible de `Cell::default()`.
    /// Con el layout empaquetado esto ya es una comparación de unos pocos
    /// enteros (antes recorría tres enums y nueve booleanos por celda).
    pub fn is_blank(&self) -> bool {
        *self == Self::default()
    }
}

/// Grid virtual con tamaño dinámico que representa el buffer del terminal.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Matriz de celdas: rows[row][col].
    /// `VecDeque` para que el scroll de pantalla completa (el caso dominante:
    /// `\n` al final de la pantalla) sea O(1) con `pop_front`/`push_back` en
    /// vez de un memmove sobre un `Vec`.
    pub rows: VecDeque<Vec<Cell>>,
    /// Líneas que hicieron scroll por arriba de la región.
    /// La fila más reciente está al final.
    // ponytail: scrollback minimo con reflow.
    pub scrollback: VecDeque<Vec<Cell>>,
    /// Número actual de filas del grid.
    pub rows_count: usize,
    /// Número actual de columnas del grid.
    pub cols_count: usize,
    /// Máximo de líneas en scrollback para este grid.
    pub max_scrollback: usize,
    /// Total de líneas recortadas del scrollback desde que se creó el Grid.
    /// Monótono creciente; sirve para reconciliar índices lógicos externos.
    pub scrollback_trimmed: u64,
    /// Indica si cada fila es continuación de la anterior por soft-wrap (true)
    /// o por hard break / Enter explícito (false).
    /// Usado por reflow para decidir si insertar un newline marker entre filas.
    pub row_continuations: Vec<bool>,
    /// Celdas modificadas desde el último frame (render incremental).
    pub damage: GridDamage,
    /// Filas en blanco recicladas de scrolls anteriores, listas para reusar
    /// sin asignar. Solo se llena mientras el scrollback aún no está lleno;
    /// con el scrollback lleno el reciclado viene directo de `push_scrollback_recycling`.
    blank_row_pool: Vec<Vec<Cell>>,
}

/// Tope del pool de filas en blanco recicladas: cubre ráfagas de scroll sin
/// dejar que el pool crezca sin límite.
const BLANK_ROW_POOL_CAP: usize = 4;

impl Default for Cell {
    fn default() -> Self {
        let mut attrs = PackedAttrs::default();
        attrs.set_width(1);
        Self {
            ch: ' ',
            attrs,
            hyperlink: NO_INDEX,
            extra_codepoints: NO_INDEX,
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
        Self::new_sized_with_scrollback(rows, cols, DEFAULT_MAX_SCROLLBACK)
    }

    /// Crea un grid vacío con tamaño y límite de scrollback.
    pub fn new_sized_with_scrollback(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        let cap = max_scrollback.min(1024);
        Self {
            rows: vec![vec![Cell::default(); cols]; rows].into(),
            scrollback: VecDeque::with_capacity(cap),
            rows_count: rows,
            cols_count: cols,
            max_scrollback,
            scrollback_trimmed: 0,
            row_continuations: vec![false; rows],
            damage: GridDamage::new(rows, cols),
            blank_row_pool: Vec::new(),
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
    pub fn set(&mut self, row: usize, col: usize, ch: char, attrs: PackedAttrs) {
        if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(col)) {
            cell.ch = ch;
            cell.attrs = attrs;
            self.damage.mark_cell(row, col);
        }
    }

    /// Marca columnas de continuacion de un glifo ancho (width >= 2).
    pub fn mark_wide_continuation(
        &mut self,
        row: usize,
        col: usize,
        width: u8,
        attrs: PackedAttrs,
    ) {
        let w = width.max(2) as usize;
        for c in (col + 1)..col.saturating_add(w).min(self.cols_count) {
            if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(c)) {
                cell.ch = ' ';
                cell.set_width(0);
                cell.attrs = attrs;
            }
            self.damage.mark_cell(row, c);
        }
    }

    /// Si `(row, col)` era la base de un glifo ancho, restaura las columnas de
    /// continuacion a celdas normales antes de sobrescribir la base.
    pub fn clear_replaced_wide_span(&mut self, row: usize, col: usize) {
        let old_w = self
            .rows
            .get(row)
            .and_then(|r| r.get(col))
            .map(|c| c.width())
            .unwrap_or(0);
        if old_w < 2 {
            return;
        }
        let w = old_w as usize;
        for c in (col + 1)..col.saturating_add(w).min(self.cols_count) {
            if let Some(cell) = self.rows.get_mut(row).and_then(|r| r.get_mut(c)) {
                *cell = Cell::default();
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

    /// Saca una fila en blanco del pool reciclado, o asigna una nueva si el
    /// pool está vacío.
    fn take_blank_row(&mut self) -> Vec<Cell> {
        let mut row = self
            .blank_row_pool
            .pop()
            .unwrap_or_else(|| vec![Cell::default(); self.cols_count]);
        // El scrollback puede devolver una fila guardada con otro ancho.
        if row.len() != self.cols_count {
            row.resize(self.cols_count, Cell::default());
        }
        row
    }

    /// Devuelve una fila ya vaciada al pool reciclado, si hay hueco.
    fn recycle_blank_row(&mut self, mut row: Vec<Cell>) {
        if row.len() != self.cols_count {
            row.resize(self.cols_count, Cell::default());
        }
        if self.blank_row_pool.len() < BLANK_ROW_POOL_CAP {
            self.blank_row_pool.push(row);
        }
    }

    /// Scroll up: mueve todas las filas de la región [top, bottom] una posición
    /// hacia arriba. La fila `bottom` queda en blanco.
    // ponytail: alt screen tambien acumula scrollback (bug aceptado).
    pub fn scroll_up_region(&mut self, n: usize, top: usize, bottom: usize) {
        if top < self.rows_count && bottom < self.rows_count && top <= bottom {
            self.resync_continuations();
            if top == 0 && bottom == self.rows_count - 1 {
                // Camino rapido: caso dominante (\n al final de pantalla, sin
                // scroll region activa). pop_front/push_back es O(1) real,
                // sin memmove ni rotate.
                for _ in 0..n {
                    let Some(saved) = self.rows.pop_front() else {
                        debug_assert!(false, "rows no puede estar vacio con rows_count > 0");
                        break;
                    };
                    let blank = self.take_blank_row();
                    self.rows.push_back(blank);
                    if let Some(recycled) = self.push_scrollback_recycling(saved) {
                        self.recycle_blank_row(recycled);
                    }
                    self.row_continuations.rotate_left(1);
                    let last = self.row_continuations.len() - 1;
                    self.row_continuations[last] = false;
                }
                // El caso dominante deja rotar la cache de render en vez de
                // invalidar el frame entero (ver GridDamage::mark_scrolled).
                if let Ok(lines) = i32::try_from(n) {
                    self.damage.mark_scrolled(lines, top, bottom);
                } else {
                    self.damage.mark_all();
                }
            } else {
                for _ in 0..n {
                    let blank = self.take_blank_row();
                    let saved = std::mem::replace(&mut self.rows[top], blank);
                    if let Some(recycled) = self.push_scrollback_recycling(saved) {
                        self.recycle_blank_row(recycled);
                    }
                    self.rows.make_contiguous()[top..=bottom].rotate_left(1);
                    self.row_continuations[top..=bottom].rotate_left(1);
                    self.row_continuations[bottom] = false;
                }
                self.damage.mark_all();
            }
        } else {
            self.damage.mark_all();
        }
    }

    /// Scroll down: mueve todas las filas de la región [top, bottom] una posición
    /// hacia abajo. La fila `top` queda en blanco.
    pub fn scroll_down_region(&mut self, n: usize, top: usize, bottom: usize) {
        if top < self.rows_count && bottom < self.rows_count && top <= bottom {
            self.resync_continuations();
            for _ in 0..n {
                let blank = self.take_blank_row();
                let mut discarded = std::mem::replace(&mut self.rows[bottom], blank);
                self.rows.make_contiguous()[top..=bottom].rotate_right(1);
                self.row_continuations[top..=bottom].rotate_right(1);
                self.row_continuations[top] = false;
                discarded.fill(Cell::default());
                self.recycle_blank_row(discarded);
            }
        }
        self.damage.mark_all();
    }

    /// Desplaza las filas [row, total_rows) una posición hacia abajo. La fila
    /// `row` queda en blanco. Usado por IL (insert line).
    // ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn insert_line(&mut self, row: usize) {
        if row < self.rows_count {
            self.resync_continuations();
            let blank = self.take_blank_row();
            let mut discarded = std::mem::replace(&mut self.rows[self.rows_count - 1], blank);
            self.rows.make_contiguous()[row..self.rows_count].rotate_right(1);
            self.row_continuations[row..self.rows_count].rotate_right(1);
            self.row_continuations[row] = false;
            discarded.fill(Cell::default());
            self.recycle_blank_row(discarded);
            self.damage.mark_all();
        }
    }

    /// Desplaza las filas [row+1, total_rows) una posición hacia arriba. La fila
    /// (total_rows - 1) queda en blanco. Usado por DL (delete line).
    // ponytail: xterm NO respeta la scroll region en IL/DL.
    pub fn delete_line(&mut self, row: usize) {
        if row < self.rows_count {
            self.resync_continuations();
            let blank = self.take_blank_row();
            let mut discarded = std::mem::replace(&mut self.rows[row], blank);
            self.rows.make_contiguous()[row..self.rows_count].rotate_left(1);
            self.row_continuations[row..self.rows_count].rotate_left(1);
            self.row_continuations[self.rows_count - 1] = false;
            discarded.fill(Cell::default());
            self.recycle_blank_row(discarded);
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
    ///
    /// Encoger con el cursor fuera del nuevo alto empuja las filas de arriba
    /// al scrollback. Crecer las recupera por arriba para que el contenido
    /// reciente quede anclado al fondo. Si el cursor ya cabe, se recorta
    /// por abajo (huecos) sin ensuciar el historial.
    ///
    /// Devuelve `(filas_salidas_por_arriba, filas_recuperadas_del_scrollback)`.
    pub fn resize(&mut self, new_rows: usize, new_cols: usize) -> (usize, usize) {
        self.resize_at_cursor(new_rows, new_cols, None)
    }

    /// Como [`resize`], anclando el recorte vertical a `cursor_row` si se da.
    pub fn resize_at_cursor(
        &mut self,
        new_rows: usize,
        new_cols: usize,
        cursor_row: Option<usize>,
    ) -> (usize, usize) {
        const MAX_GRID: usize = 4096;
        let new_rows = new_rows.clamp(1, MAX_GRID);
        let new_cols = new_cols.clamp(1, MAX_GRID);
        if new_rows == self.rows_count && new_cols == self.cols_count {
            return (0, 0);
        }
        self.resync_continuations();
        // Las filas recicladas tienen la longitud vieja: no sirven tras un resize.
        self.blank_row_pool.clear();
        // Primero truncar o expandir columnas en cada fila existente.
        for row in &mut self.rows {
            if new_cols < row.len() {
                row.truncate(new_cols);
            } else {
                row.extend(std::iter::repeat_n(Cell::default(), new_cols - row.len()));
            }
        }

        let (from_top, from_scrollback) = if new_rows < self.rows.len() {
            self.shrink_rows(new_rows, cursor_row)
        } else {
            (0, self.grow_rows(new_rows, new_cols))
        };

        self.rows_count = new_rows;
        self.cols_count = new_cols;
        Self::normalize_row_lengths(self.rows.make_contiguous(), new_cols);
        self.damage.resize(new_rows, new_cols);
        (from_top, from_scrollback)
    }

    fn shrink_rows(&mut self, new_rows: usize, cursor_row: Option<usize>) -> (usize, usize) {
        let old_rows = self.rows.len();
        let overflow = old_rows - new_rows;
        let cursor = cursor_row.unwrap_or(old_rows.saturating_sub(1));
        // Si el cursor no cabe, nos quedamos con las filas de abajo (el
        // contenido reciente) y el resto pasa al historial. Si cabe, se
        // recortan huecos por abajo y no se toca el scrollback.
        let from_top = if cursor >= new_rows { overflow } else { 0 };
        if from_top > 0 {
            for _ in 0..from_top {
                let Some(row) = self.rows.pop_front() else {
                    break;
                };
                if !self.row_continuations.is_empty() {
                    self.row_continuations.remove(0);
                }
                // No ensuciar el historial con filas vacias (huecos de una
                // ventana mas alta que el contenido).
                if !row.iter().all(Cell::is_blank) {
                    self.push_scrollback(row);
                }
            }
        } else {
            self.rows.truncate(new_rows);
            self.row_continuations.truncate(new_rows);
        }
        (from_top, 0)
    }

    fn grow_rows(&mut self, new_rows: usize, new_cols: usize) -> usize {
        let needed = new_rows - self.rows.len();
        let pulled = needed.min(self.scrollback.len());
        for _ in 0..pulled {
            let Some(mut row) = self.scrollback.pop_back() else {
                break;
            };
            Self::normalize_row_lengths(std::slice::from_mut(&mut row), new_cols);
            self.rows.push_front(row);
            self.row_continuations.insert(0, false);
        }
        let blanks = needed.saturating_sub(pulled);
        if blanks > 0 {
            let blank_row = vec![Cell::default(); new_cols];
            self.rows.extend(std::iter::repeat_n(blank_row, blanks));
            self.row_continuations
                .extend(std::iter::repeat_n(false, blanks));
        }
        pulled
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
        self.push_scrollback_recycling(row);
    }

    /// Guarda `row` en el scrollback y devuelve un buffer reutilizable (la
    /// fila más antigua recortada, ya vaciada) cuando el scrollback estaba
    /// lleno, para evitar asignar una fila en blanco nueva en el llamante.
    fn push_scrollback_recycling(&mut self, mut row: Vec<Cell>) -> Option<Vec<Cell>> {
        if self.max_scrollback == 0 {
            // Sin buffer: la línea se descarta; igual cuenta para reconciliar índices.
            self.scrollback_trimmed += 1;
            row.fill(Cell::default());
            return Some(row);
        }
        let recycled = if self.scrollback.len() >= self.max_scrollback {
            self.scrollback_trimmed += 1;
            self.scrollback.pop_front()
        } else {
            None
        };
        self.scrollback.push_back(row);
        recycled.map(|mut r| {
            r.fill(Cell::default());
            r
        })
    }

    /// Actualiza el límite de scrollback y descarta líneas sobrantes.
    pub fn set_max_scrollback(&mut self, max: usize) {
        self.max_scrollback = max;
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
            self.scrollback_trimmed += 1;
        }
    }

    /// Descarta la historia y cuenta las filas como recorte para que los
    /// índices lógicos (marcas de prompt) se reconcilien.
    pub fn clear_scrollback(&mut self) {
        let dropped = self.scrollback.len() as u64;
        self.scrollback.clear();
        self.scrollback_trimmed = self.scrollback_trimmed.saturating_add(dropped);
        self.damage.mark_all();
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
        row.push(Cell::with_width(0));
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
                if cell.width() == 0 {
                    col += 1;
                    continue;
                }
                if !cell.is_blank() {
                    offset += 1;
                }
                col += (cell.width() as usize).max(1);
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
            if cell.ch == '\n' && cell.width() == 0 {
                row_idx += 1;
                col = 0;
                continue;
            }
            let w = cell.width() as usize;
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

    fn pad_reflow_row(mut row: Vec<Cell>, new_cols: usize) -> Vec<Cell> {
        while row.len() < new_cols {
            row.push(Cell::default());
        }
        row
    }

    /// Encaja `row` en la ventana visible. Lo que sobra por arriba va al
    /// scrollback de inmediato, para no materializar N*M filas al angostar.
    fn emit_reflow_row(
        &mut self,
        row: Vec<Cell>,
        continuation: bool,
        visible: &mut VecDeque<Vec<Cell>>,
        continuations: &mut VecDeque<bool>,
        overflow: &mut usize,
    ) {
        visible.push_back(row);
        continuations.push_back(continuation);
        if visible.len() > self.rows_count {
            if let Some(old) = visible.pop_front() {
                self.push_scrollback(old);
                *overflow += 1;
            }
            let _ = continuations.pop_front();
        }
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
        // Las filas recicladas tienen la longitud vieja: no sirven tras un reflow.
        self.blank_row_pool.clear();
        let old_rows: Vec<Vec<Cell>> = self.rows.drain(..).collect();
        let old_row_continuations = self.row_continuations.clone();
        let cursor_offset =
            cursor.map(|(r, c)| Self::logical_offset_before_cursor(&old_rows, r, c));
        // Asegurar que continuations tenga la longitud correcta por seguridad
        self.resync_continuations();

        // Encontrar la ultima fila con contenido no-default.
        let last_content_row = old_rows
            .iter()
            .rposition(|row| row.iter().any(|cell| !cell.is_blank()))
            .unwrap_or(0);

        // ---- Pasos 1-3: aplanar todas las filas en una secuencia logica ----

        let mut flat: Vec<Cell> = Vec::new();

        for (idx, old_row) in old_rows.into_iter().enumerate() {
            if idx > last_content_row {
                break;
            }

            let content_len = old_row
                .iter()
                .rposition(|cell| !cell.is_blank())
                .map(|pos| pos + 1)
                .unwrap_or(0);

            // Extraer celdas logicas de esta fila, saltando relleno CJK.
            let mut i = 0;
            while i < content_len {
                let cell = old_row[i];
                if !cell.is_blank() {
                    flat.push(cell);
                    i += cell.width() as usize;
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
                    flat.push(Cell::with_ch_and_width('\n', 0));
                }
            }
        }

        // ---- Step 4: if the grid was completely empty, just fill and return ----

        if flat.is_empty() {
            self.rows = vec![vec![Cell::default(); new_cols]; self.rows_count].into();
            self.cols_count = new_cols;
            return cursor.map(|(r, c)| {
                (
                    r.min(self.rows_count.saturating_sub(1)),
                    c.min(new_cols.saturating_sub(1)),
                )
            });
        }

        // ---- Step 5: re-divide. Solo se retienen `rows_count` filas visibles;
        // el resto va al scrollback al vuelo (tope `max_scrollback`).

        let mut visible: VecDeque<Vec<Cell>> = VecDeque::new();
        let mut visible_cont: VecDeque<bool> = VecDeque::new();
        let mut overflow = 0usize;
        let mut row_after_newline = true;

        let mut current_row: Vec<Cell> = Vec::with_capacity(new_cols);
        let mut col = 0usize;

        for cell in &flat {
            if cell.ch == '\n' && cell.width() == 0 {
                let row = if current_row.is_empty() {
                    vec![Cell::default(); new_cols]
                } else {
                    Self::pad_reflow_row(current_row, new_cols)
                };
                self.emit_reflow_row(
                    row,
                    !row_after_newline,
                    &mut visible,
                    &mut visible_cont,
                    &mut overflow,
                );
                current_row = Vec::with_capacity(new_cols);
                col = 0;
                row_after_newline = true;
                continue;
            }

            let w = cell.width() as usize;
            if w == 0 {
                continue;
            }

            if col + w <= new_cols {
                current_row.push(*cell);
                for _ in 1..w {
                    Self::push_wide_continuation(&mut current_row);
                }
                col += w;
            } else if col == 0 && w > new_cols {
                current_row.push(*cell);
                for _ in 1..w.min(new_cols) {
                    Self::push_wide_continuation(&mut current_row);
                }
                col = w.min(new_cols);
            } else {
                let flushed = Self::pad_reflow_row(current_row, new_cols);
                self.emit_reflow_row(
                    flushed,
                    !row_after_newline,
                    &mut visible,
                    &mut visible_cont,
                    &mut overflow,
                );
                current_row = Vec::with_capacity(new_cols);
                current_row.push(*cell);
                for _ in 1..w {
                    Self::push_wide_continuation(&mut current_row);
                }
                col = w;
                row_after_newline = false;
            }
        }

        if !current_row.is_empty() {
            let flushed = Self::pad_reflow_row(current_row, new_cols);
            self.emit_reflow_row(
                flushed,
                !row_after_newline,
                &mut visible,
                &mut visible_cont,
                &mut overflow,
            );
        }

        let pre_overflow_cursor =
            cursor_offset.map(|offset| Self::cursor_from_offset_in_flat(&flat, new_cols, offset));

        while visible.len() < self.rows_count {
            visible.push_back(vec![Cell::default(); new_cols]);
            visible_cont.push_back(false);
        }

        self.rows = visible;
        self.row_continuations = visible_cont.into_iter().collect();
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
    fn cell_default_no_tiene_extra_codepoints() {
        let cell = Cell::default();
        assert_eq!(cell.extra_codepoints(), None);
    }

    #[test]
    fn cell_default_no_tiene_hyperlink() {
        let cell = Cell::default();
        assert_eq!(cell.hyperlink(), None);
    }
    #[test]
    fn push_scrollback_incrementa_trimmed_al_recortar() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 2);
        assert_eq!(grid.scrollback_trimmed, 0);
        for _ in 0..2 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 2);
        assert_eq!(grid.scrollback_trimmed, 0);
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        assert_eq!(grid.scrollback.len(), 2);
        assert_eq!(grid.scrollback_trimmed, 1);
    }

    #[test]
    fn set_max_scrollback_incrementa_trimmed_al_truncar() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 5);
        for _ in 0..5 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback_trimmed, 0);
        grid.set_max_scrollback(2);
        assert_eq!(grid.scrollback.len(), 2);
        assert_eq!(grid.scrollback_trimmed, 3);
    }

    #[test]
    fn test_grid_scrollback_zero_no_almacena() {
        let mut grid = Grid::new_sized_with_scrollback(24, 80, 0);
        for _ in 0..5 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 0);
        assert_eq!(grid.scrollback_trimmed, 5);
    }

    #[test]
    fn test_grid_max_scrollback_configurable() {
        let mut grid = Grid::new_sized_with_scrollback(24, 80, 3);
        for _ in 0..10 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 3);
    }

    #[test]
    fn set_max_scrollback_trunca_lineas_sobrantes() {
        let mut grid = Grid::new_sized_with_scrollback(24, 80, 5);
        for i in 0..5usize {
            let mut row = vec![Cell::default(); 80];
            row[0].ch = (b'a' + i as u8) as char;
            grid.scrollback.push_back(row);
        }
        grid.set_max_scrollback(2);
        assert_eq!(grid.scrollback.len(), 2);
        assert_eq!(grid.scrollback[0][0].ch, 'd');
        assert_eq!(grid.scrollback[1][0].ch, 'e');
    }

    #[test]
    fn clear_scrollback_vacia_historia_y_cuenta_recorte() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for _ in 0..3 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 3);
        let trimmed_antes = grid.scrollback_trimmed;
        grid.clear_scrollback();
        assert!(grid.scrollback.is_empty());
        assert_eq!(grid.scrollback_trimmed, trimmed_antes + 3);
    }

    #[test]
    fn test_scrollback_pushes_on_scroll_up() {
        let mut grid = Grid::new();
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        assert_eq!(grid.scrollback.len(), 1);
    }

    #[test]
    fn test_scrollback_drops_oldest_when_full() {
        let mut grid = Grid::new_sized_with_scrollback(24, 80, 100);
        for _ in 0..=100 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 100);
    }

    #[test]
    fn scroll_up_recicla_fila_sin_contenido_previo() {
        // Scrollback lleno: cada scroll recicla la fila mas antigua recortada.
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 2);
        for i in 0..2usize {
            grid.rows[0][0].ch = (b'a' + i as u8) as char;
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        // La fila nueva del fondo debe estar en blanco, no arrastrar contenido reciclado.
        let bottom = grid.rows_count - 1;
        assert_eq!(grid.rows[bottom][0], Cell::default());
    }

    #[test]
    fn scroll_up_region_parcial_no_toca_filas_fuera_de_rango() {
        // top != 0 y bottom != rows_count - 1: fuerza el camino make_contiguous.
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for r in 0..grid.rows_count {
            grid.rows[r][0].ch = (b'a' + r as u8) as char;
        }
        grid.scroll_up_region(1, 1, 3);
        assert_eq!(grid.rows[0][0].ch, 'a', "fuera de la region: intacta");
        assert_eq!(grid.rows[1][0].ch, 'c');
        assert_eq!(grid.rows[2][0].ch, 'd');
        assert_eq!(grid.rows[3][0], Cell::default());
        assert_eq!(grid.rows[4][0].ch, 'e', "fuera de la region: intacta");
        // La region no incluye row 0, no debe ir al scrollback.
        assert_eq!(grid.scrollback.len(), 1);
        assert_eq!(grid.scrollback[0][0].ch, 'b');
    }

    #[test]
    fn scroll_down_region_parcial_no_toca_filas_fuera_de_rango() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for r in 0..grid.rows_count {
            grid.rows[r][0].ch = (b'a' + r as u8) as char;
        }
        grid.scroll_down_region(1, 1, 3);
        assert_eq!(grid.rows[0][0].ch, 'a', "fuera de la region: intacta");
        assert_eq!(grid.rows[1][0], Cell::default());
        assert_eq!(grid.rows[2][0].ch, 'b');
        assert_eq!(grid.rows[3][0].ch, 'c');
        assert_eq!(grid.rows[4][0].ch, 'e', "fuera de la region: intacta");
    }

    #[test]
    fn scroll_down_region_desplaza_contenido_y_limpia_top() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for r in 0..grid.rows_count {
            grid.rows[r][0].ch = (b'a' + r as u8) as char;
        }
        grid.scroll_down_region(1, 0, grid.rows_count - 1);
        assert_eq!(grid.rows[0][0], Cell::default());
        assert_eq!(grid.rows[1][0].ch, 'a');
        assert_eq!(grid.rows[grid.rows_count - 1][0].ch, 'd');
    }

    #[test]
    fn insert_line_desplaza_hacia_abajo_y_descarta_ultima_fila() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for r in 0..grid.rows_count {
            grid.rows[r][0].ch = (b'a' + r as u8) as char;
        }
        grid.insert_line(1);
        assert_eq!(grid.rows[0][0].ch, 'a');
        assert_eq!(grid.rows[1][0], Cell::default());
        assert_eq!(grid.rows[2][0].ch, 'b');
        assert_eq!(grid.rows[grid.rows_count - 1][0].ch, 'd');
    }

    #[test]
    fn delete_line_desplaza_hacia_arriba_y_deja_ultima_en_blanco() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for r in 0..grid.rows_count {
            grid.rows[r][0].ch = (b'a' + r as u8) as char;
        }
        grid.delete_line(1);
        assert_eq!(grid.rows[0][0].ch, 'a');
        assert_eq!(grid.rows[1][0].ch, 'c');
        assert_eq!(grid.rows[grid.rows_count - 1][0], Cell::default());
    }

    #[test]
    fn scroll_up_pantalla_completa_conserva_dimensiones() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        for _ in 0..20 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.rows.len(), grid.rows_count);
        assert_eq!(grid.rows[0].len(), grid.cols_count);
    }

    #[test]
    fn scroll_up_pantalla_completa_produce_damage_scrolled() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        let _ = grid.damage.take(); // descarta el damage inicial (full).
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        let snap = grid.damage.take();
        assert_eq!(
            snap,
            DamageSnapshot::Scrolled {
                lines: 1,
                region: (0, 4),
                rest: vec![vec![0]; 5],
            }
        );
    }

    #[test]
    fn varios_scrolls_en_el_mismo_frame_acumulan_lineas() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 10);
        let _ = grid.damage.take();
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        let snap = grid.damage.take();
        let DamageSnapshot::Scrolled { lines, region, .. } = snap else {
            panic!("se esperaba Scrolled tras varios scrolls del mismo frame");
        };
        assert_eq!(lines, 3);
        assert_eq!(region, (0, 4));
    }

    #[test]
    fn scrollback_reciclado_tras_resize_de_ancho_tiene_el_ancho_nuevo() {
        let mut grid = Grid::new_sized_with_scrollback(5, 20, 10);
        for _ in 0..10 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        grid.resize(5, 80);
        for _ in 0..5 {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert!(
            grid.rows.iter().all(|row| row.len() == 80),
            "una fila reciclada del scrollback conservo el ancho anterior"
        );
        grid.clear_line(grid.rows_count - 1, 70, 80);
    }

    #[test]
    fn resize_vacia_el_pool_reciclado_para_evitar_filas_de_longitud_vieja() {
        let mut grid = Grid::new_sized_with_scrollback(5, 10, 0);
        // max_scrollback == 0: cada scroll recicla directamente la propia fila.
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        grid.resize(5, 20);
        // Tras el resize toda fila nueva por scroll debe tener el ancho nuevo.
        grid.scroll_up_region(1, 0, grid.rows_count - 1);
        for row in &grid.rows {
            assert_eq!(row.len(), 20);
        }
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
        let (from_top, pulled) = grid.resize(24, 80);
        assert_eq!(from_top, 16);
        assert_eq!(pulled, 0);
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
        // Solo habia contenido en la ultima fila, que se conserva; las
        // filas vacias de arriba no ensucian el historial.
        assert!(grid.scrollback.is_empty());
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
        grid.rows[0][0].set_width(2);
        grid.rows[0][2].ch = 'A';
        grid.rows[0][3].ch = 'B';
        grid.rows[0][4].ch = '\u{4e2d}';
        grid.rows[0][4].set_width(2);
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
        assert_eq!(grid.rows[0][0].width(), 2);
        assert_eq!(grid.rows[0][1].ch, ' ');
        assert_eq!(grid.rows[0][2].ch, 'A');
        assert_eq!(grid.rows[0][3].ch, 'B');

        // Row 1: 中 at col 0, default at col 1, C at col 2
        assert_eq!(grid.rows[1][0].ch, '\u{4e2d}');
        assert_eq!(grid.rows[1][0].width(), 2);
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

    /// Reflow a 1 columna con tope de historial chico: el contenido extra
    /// se recorta. No reproducir aquí el trophy de fuzz (OOM en CI).
    #[test]
    fn reflow_a_una_columna_respeta_max_scrollback() {
        let visible = 12;
        let cols = 40;
        let max_sb = 8;
        let mut grid = Grid::new_sized_with_scrollback(visible, cols, max_sb);
        for r in 0..visible {
            for c in 0..cols {
                grid.rows[r][c].ch = 'X';
            }
        }
        grid.reflow(1);
        assert_eq!(grid.cols_count, 1);
        assert_eq!(grid.rows.len(), visible);
        assert!(
            grid.scrollback.len() <= max_sb,
            "scrollback {} > max {max_sb}",
            grid.scrollback.len()
        );
        assert!(grid.rows.iter().all(|row| row.len() == 1));
        let xs = grid.rows.iter().filter(|row| row[0].ch == 'X').count()
            + grid
                .scrollback
                .iter()
                .filter(|row| !row.is_empty() && row[0].ch == 'X')
                .count();
        assert_eq!(xs, visible + max_sb);
    }

    #[test]
    fn resize_shrink_con_cursor_abajo_guarda_en_scrollback() {
        let mut grid = Grid::new_sized(10, 80);
        for r in 0..10 {
            grid.rows[r][0].ch = (b'A' + r as u8) as char;
        }
        let (from_top, pulled) = grid.resize_at_cursor(5, 80, Some(9));
        assert_eq!(from_top, 5);
        assert_eq!(pulled, 0);
        assert_eq!(grid.rows[4][0].ch, 'J');
        assert_eq!(grid.scrollback.len(), 5);
        assert_eq!(grid.scrollback[0][0].ch, 'A');
        assert_eq!(grid.scrollback[4][0].ch, 'E');
    }

    #[test]
    fn resize_shrink_grow_restaura_el_contenido() {
        let mut grid = Grid::new_sized(10, 80);
        for r in 0..10 {
            grid.rows[r][0].ch = (b'A' + r as u8) as char;
        }
        grid.resize_at_cursor(5, 80, Some(9));
        let (from_top, pulled) = grid.resize_at_cursor(10, 80, Some(4));
        assert_eq!(from_top, 0);
        assert_eq!(pulled, 5);
        for r in 0..10 {
            assert_eq!(grid.rows[r][0].ch, (b'A' + r as u8) as char);
        }
        assert!(grid.scrollback.is_empty());
    }

    #[test]
    fn resize_shrink_con_cursor_arriba_no_tira_el_contenido() {
        let mut grid = Grid::new_sized(10, 80);
        grid.rows[0][0].ch = 'X';
        grid.rows[1][0].ch = 'Y';
        grid.resize_at_cursor(5, 80, Some(1));
        assert_eq!(grid.rows[0][0].ch, 'X');
        assert_eq!(grid.rows[1][0].ch, 'Y');
        assert!(
            grid.scrollback.is_empty(),
            "recortar huecos de abajo no ensucia el historial"
        );
    }

    #[test]
    fn resize_grow_recupera_scrollback_y_deja_el_fondo_abajo() {
        let mut grid = Grid::new_sized(5, 80);
        for ch in ['A', 'B', 'C'] {
            let mut row = vec![Cell::default(); 80];
            row[0].ch = ch;
            grid.scrollback.push_back(row);
        }
        grid.rows[4][0].ch = '$';
        grid.resize_at_cursor(8, 80, Some(4));
        assert_eq!(grid.rows[0][0].ch, 'A');
        assert_eq!(grid.rows[2][0].ch, 'C');
        assert_eq!(grid.rows[7][0].ch, '$');
        assert!(grid.scrollback.is_empty());
    }

    /// Verifica que resize (encoger y crecer) no corrompe el grid.
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn test_resize_shrink_grow_no_corruption() {
        let mut grid = Grid::new_sized(10, 80);
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

        grid.resize(5, 80);
        grid.resize(10, 80);

        for r in 0..10 {
            let s: String = grid.rows[r].iter().take(5).map(|c| c.ch).collect();
            assert_eq!(s, original[r], "la fila {r} debe restaurarse");
        }
        assert_eq!(grid.rows[0].len(), 80, "todas las filas tienen new_cols");
    }

    #[test]
    fn resize_vertical_mantiene_visible_la_ultima_linea() {
        let mut grid = Grid::new();
        grid.resize(24, 80);
        for r in 0..24 {
            grid.rows[r][0].ch = char::from_u32(b'A' as u32 + r as u32).unwrap();
        }
        grid.rows[23][0].ch = '$';

        grid.resize(10, 80);
        assert!(
            grid.rows.iter().any(|row| row[0].ch == '$'),
            "la ultima linea sigue visible al encoger"
        );

        grid.resize(24, 80);
        assert_eq!(
            grid.rows[23][0].ch, '$',
            "la ultima linea queda anclada al fondo al volver a 24 filas"
        );
    }

    #[test]
    fn test_scrollback_1000_lines() {
        let mut grid = Grid::new_sized_with_scrollback(24, 80, 100);
        for i in 0..1000 {
            grid.rows[0][0].ch = char::from_digit((i % 10) as u32, 10).unwrap();
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        assert_eq!(grid.scrollback.len(), 100);
        // Verificar que la linea mas reciente en scrollback corresponde a i=999
        let last = grid.scrollback.back().unwrap();
        assert_eq!(last[0].ch, '9');
    }
}
