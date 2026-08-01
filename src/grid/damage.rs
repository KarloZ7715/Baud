//! Bitmask de celdas modificadas para render incremental.

const BITS_PER_WORD: usize = 64;

/// Tope defensivo de columnas para bitmask de damage (evita OOM si cols es absurdo).
pub(crate) const MAX_DAMAGE_COLS: usize = 4096;

/// Rastrea qué celdas del grid cambiaron desde el último frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridDamage {
    /// `rows[row][word]` cubre columnas `word*64 .. word*64+63`.
    /// Rota en conjunto con `Grid::rows` cuando hay scroll (`mark_scrolled`),
    /// asi que un bit marcado antes del scroll sigue senalando la fila
    /// correcta despues de que su contenido se desplazo.
    rows: Vec<Vec<u64>>,
    cols: usize,
    /// Invalidación total (resize, clear, reflow).
    full: bool,
    /// Región que acumula scrolls compatibles desde el último `take()`.
    /// `None` si no hubo scroll, o si scrolls de regiones distintas se
    /// mezclaron y degradaron a `full`.
    scroll_region: Option<(usize, usize)>,
    /// Desplazamiento neto acumulado para `scroll_region`. Positivo: el
    /// contenido se movio hacia arriba (scroll up). Negativo: hacia abajo.
    scroll_lines: i32,
}

impl GridDamage {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: vec![vec![0; Self::words_for_cols(cols)]; rows],
            cols,
            full: true,
            scroll_region: None,
            scroll_lines: 0,
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let cols = cols.clamp(1, MAX_DAMAGE_COLS);
        let rows = rows.clamp(1, MAX_DAMAGE_COLS);
        self.rows = vec![vec![0; Self::words_for_cols(cols)]; rows];
        self.cols = cols;
        self.full = true;
        self.scroll_region = None;
        self.scroll_lines = 0;
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn mark_all(&mut self) {
        self.full = true;
        self.scroll_region = None;
        self.scroll_lines = 0;
        for row in &mut self.rows {
            row.fill(0);
        }
    }

    /// Registra un scroll de `lines` filas (positivo: arriba, negativo:
    /// abajo) sobre `[top, bottom]`, en vez de invalidar todo el frame.
    /// Rota el propio bitmap junto con el contenido para que los bits ya
    /// marcados sigan apuntando a la fila correcta tras el desplazamiento.
    /// Si ya hay un scroll acumulado de otra región, degrada a `mark_all`
    /// (caso raro: mezclar scrolls de regiones distintas en el mismo frame).
    pub fn mark_scrolled(&mut self, lines: i32, top: usize, bottom: usize) {
        if self.full || lines == 0 {
            return;
        }
        match self.scroll_region {
            Some(region) if region != (top, bottom) => {
                self.mark_all();
                return;
            }
            None => self.scroll_region = Some((top, bottom)),
            Some(_) => {}
        }
        self.scroll_lines += lines;
        self.rotate_rows(lines, top, bottom);
    }

    /// Rota el bitmap de daño `[top, bottom]` en el mismo sentido que el
    /// contenido del grid: mueve punteros de `Vec<u64>` (24 bytes), no
    /// palabras de bits.
    fn rotate_rows(&mut self, lines: i32, top: usize, bottom: usize) {
        if top > bottom || bottom >= self.rows.len() {
            return;
        }
        let region_len = bottom - top + 1;
        let n = (lines.unsigned_abs() as usize).min(region_len);
        if n == 0 {
            return;
        }
        if lines > 0 {
            self.rows[top..=bottom].rotate_left(n);
            for row in &mut self.rows[(bottom + 1 - n)..=bottom] {
                row.fill(0);
            }
        } else {
            self.rows[top..=bottom].rotate_right(n);
            for row in &mut self.rows[top..(top + n)] {
                row.fill(0);
            }
        }
    }

    pub fn mark_cell(&mut self, row: usize, col: usize) {
        if self.full {
            return;
        }
        if row >= self.rows.len() || col >= self.cols {
            return;
        }
        let word = col / BITS_PER_WORD;
        let bit = col % BITS_PER_WORD;
        if let Some(words) = self.rows.get_mut(row) {
            if let Some(w) = words.get_mut(word) {
                *w |= 1_u64 << bit;
            }
        }
    }

    pub fn mark_row_range(&mut self, row: usize, from_col: usize, to_col: usize) {
        if self.full {
            return;
        }
        let to = to_col.min(self.cols);
        for col in from_col..to {
            self.mark_cell(row, col);
        }
    }

    pub fn is_cell_dirty(&self, row: usize, col: usize) -> bool {
        if self.full {
            return true;
        }
        if row >= self.rows.len() || col >= self.cols {
            return false;
        }
        let word = col / BITS_PER_WORD;
        let bit = col % BITS_PER_WORD;
        self.rows
            .get(row)
            .and_then(|words| words.get(word))
            .is_some_and(|w| (w & (1_u64 << bit)) != 0)
    }

    /// Toma el estado de daño y lo resetea para el próximo frame.
    ///
    /// Sin clonar: `self.rows` se mueve tal cual al snapshot devuelto (que
    /// ya iba a machacarse en el próximo frame) y se reconstruye un buffer
    /// nuevo, ya en cero, para las siguientes escrituras.
    pub fn take(&mut self) -> DamageSnapshot {
        if self.rows.len() > MAX_DAMAGE_COLS || self.cols > MAX_DAMAGE_COLS {
            self.full = true;
        }
        if self.full {
            self.full = false;
            self.scroll_region = None;
            self.scroll_lines = 0;
            for row in &mut self.rows {
                row.fill(0);
            }
            return DamageSnapshot::Full;
        }
        let words = Self::words_for_cols(self.cols);
        let row_count = self.rows.len();
        let taken = std::mem::replace(&mut self.rows, vec![vec![0; words]; row_count]);
        if let Some(region) = self.scroll_region.take() {
            let lines = self.scroll_lines;
            self.scroll_lines = 0;
            if lines != 0 {
                return DamageSnapshot::Scrolled {
                    lines,
                    region,
                    rest: taken,
                };
            }
        }
        DamageSnapshot::Cells(taken)
    }

    pub(crate) fn words_for_cols(cols: usize) -> usize {
        cols.div_ceil(BITS_PER_WORD)
    }
}

/// Daño capturado entre frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageSnapshot {
    Full,
    Cells(Vec<Vec<u64>>),
    /// La región `region` (`(top, bottom)`, inclusive) se desplazó `lines`
    /// filas (positivo: arriba, negativo: abajo) respecto al frame anterior,
    /// además de las celdas marcadas en `rest` (ya en la posición final,
    /// post-desplazamiento). El consumidor puede rotar su propia caché por
    /// fila en vez de reconstruirla, y solo repintar las filas que el
    /// desplazamiento dejó en blanco más las de `rest`.
    Scrolled {
        lines: i32,
        region: (usize, usize),
        rest: Vec<Vec<u64>>,
    },
}

impl DamageSnapshot {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn is_cell_dirty(&self, row: usize, col: usize) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => Self::cell_dirty_in(rows, row, col),
            Self::Scrolled { .. } => self.is_row_dirty(row),
        }
    }

    fn cell_dirty_in(rows: &[Vec<u64>], row: usize, col: usize) -> bool {
        let word = col / BITS_PER_WORD;
        let bit = col % BITS_PER_WORD;
        rows.get(row)
            .and_then(|words| words.get(word))
            .is_some_and(|w| (w & (1_u64 << bit)) != 0)
    }

    fn row_dirty_in(rows: &[Vec<u64>], row: usize) -> bool {
        rows.get(row)
            .is_some_and(|words| words.iter().any(|w| *w != 0))
    }

    /// True si alguna celda de la fila cambio. Para `Scrolled`, tambien es
    /// cierto si `row` es una de las filas que el desplazamiento dejo en
    /// blanco (nunca reusables desde la cache, sin importar `rest`).
    pub fn is_row_dirty(&self, row: usize) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => Self::row_dirty_in(rows, row),
            Self::Scrolled {
                lines,
                region: (top, bottom),
                rest,
            } => {
                if row < *top || row > *bottom {
                    return false;
                }
                let region_len = bottom - top + 1;
                let n = (lines.unsigned_abs() as usize).min(region_len);
                let newly_exposed = if *lines > 0 {
                    row > bottom.saturating_sub(n)
                } else {
                    row < top + n
                };
                newly_exposed || Self::row_dirty_in(rest, row)
            }
        }
    }

    /// True si alguna celda esta marcada como dirty.
    pub fn has_any_dirty(&self) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => rows.iter().any(|words| words.iter().any(|w| *w != 0)),
            Self::Scrolled { lines, rest, .. } => {
                *lines != 0 || rest.iter().any(|words| words.iter().any(|w| *w != 0))
            }
        }
    }

    /// Marca toda la fila como sucia (p. ej. cambio de seleccion).
    pub fn mark_row_dirty(&mut self, row: usize, cols: usize) {
        if self.is_full() {
            return;
        }
        let cols = cols.clamp(1, MAX_DAMAGE_COLS);
        let rows = match self {
            Self::Cells(rows) => rows,
            Self::Scrolled { rest, .. } => rest,
            Self::Full => return,
        };
        let words_needed = GridDamage::words_for_cols(cols);
        if rows.len() <= row {
            rows.resize(row + 1, vec![0; words_needed]);
        }
        if rows[row].len() < words_needed {
            rows[row].resize(words_needed, 0);
        }
        rows[row].fill(u64::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_cell_sets_bit() {
        let mut d = GridDamage::new(1, 80);
        d.full = false;
        d.mark_cell(0, 5);
        assert!(d.is_cell_dirty(0, 5));
        assert!(!d.is_cell_dirty(0, 6));
    }

    #[test]
    fn mark_cell_beyond_64_cols() {
        let mut d = GridDamage::new(1, 100);
        d.full = false;
        d.mark_cell(0, 70);
        assert!(d.is_cell_dirty(0, 70));
        assert!(!d.is_cell_dirty(0, 10));
    }

    #[test]
    fn take_clears_damage() {
        let mut d = GridDamage::new(2, 10);
        d.full = false;
        d.mark_cell(1, 3);
        let snap = d.take();
        assert!(snap.is_cell_dirty(1, 3));
        assert!(!d.is_cell_dirty(1, 3));
        assert!(!d.is_full());
    }

    #[test]
    fn is_row_dirty_detects_any_bit() {
        let mut d = GridDamage::new(2, 10);
        d.full = false;
        d.mark_cell(1, 3);
        let snap = d.take();
        assert!(!snap.is_row_dirty(0));
        assert!(snap.is_row_dirty(1));
    }

    #[test]
    fn has_any_dirty_false_when_empty() {
        let snap = DamageSnapshot::Cells(vec![vec![0; 1]; 2]);
        assert!(!snap.has_any_dirty());
    }

    #[test]
    fn has_any_dirty_true_when_marked() {
        let mut d = GridDamage::new(2, 10);
        d.full = false;
        d.mark_cell(1, 3);
        let snap = d.take();
        assert!(snap.has_any_dirty());
    }

    #[test]
    fn mark_row_dirty_on_snapshot() {
        let mut snap = DamageSnapshot::Cells(vec![vec![0; 1]; 2]);
        snap.mark_row_dirty(0, 10);
        assert!(snap.is_row_dirty(0));
        assert!(!snap.is_row_dirty(1));
    }

    #[test]
    fn mark_scrolled_produce_snapshot_scrolled() {
        let mut d = GridDamage::new(5, 10);
        d.full = false;
        d.mark_scrolled(1, 0, 4);
        let snap = d.take();
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
    fn mark_scrolled_acumula_lineas_de_la_misma_region() {
        let mut d = GridDamage::new(5, 10);
        d.full = false;
        d.mark_scrolled(1, 0, 4);
        d.mark_scrolled(1, 0, 4);
        d.mark_scrolled(1, 0, 4);
        let snap = d.take();
        let DamageSnapshot::Scrolled { lines, region, .. } = snap else {
            panic!("se esperaba Scrolled");
        };
        assert_eq!(lines, 3);
        assert_eq!(region, (0, 4));
    }

    #[test]
    fn mark_scrolled_con_region_distinta_degrada_a_full() {
        let mut d = GridDamage::new(5, 10);
        d.full = false;
        d.mark_scrolled(1, 0, 4);
        d.mark_scrolled(1, 1, 3);
        assert!(d.is_full());
        assert_eq!(d.take(), DamageSnapshot::Full);
    }

    #[test]
    fn is_row_dirty_scrolled_marca_filas_recien_expuestas() {
        // 5 filas, scroll de 2: las 2 filas del fondo quedaron en blanco.
        let snap = DamageSnapshot::Scrolled {
            lines: 2,
            region: (0, 4),
            rest: vec![vec![0]; 5],
        };
        assert!(!snap.is_row_dirty(0));
        assert!(!snap.is_row_dirty(1));
        assert!(!snap.is_row_dirty(2));
        assert!(snap.is_row_dirty(3));
        assert!(snap.is_row_dirty(4));
    }

    #[test]
    fn is_row_dirty_scrolled_hacia_abajo_marca_filas_del_tope() {
        let snap = DamageSnapshot::Scrolled {
            lines: -2,
            region: (0, 4),
            rest: vec![vec![0]; 5],
        };
        assert!(snap.is_row_dirty(0));
        assert!(snap.is_row_dirty(1));
        assert!(!snap.is_row_dirty(2));
        assert!(!snap.is_row_dirty(3));
        assert!(!snap.is_row_dirty(4));
    }

    #[test]
    fn is_row_dirty_scrolled_respeta_rest_fuera_de_lo_recien_expuesto() {
        let mut rest = vec![vec![0]; 5];
        rest[1][0] = 1; // celda escrita despues del scroll, fuera del borde.
        let snap = DamageSnapshot::Scrolled {
            lines: 1,
            region: (0, 4),
            rest,
        };
        assert!(snap.is_row_dirty(1));
        assert!(!snap.is_row_dirty(2));
    }

    #[test]
    fn mark_cell_antes_de_scroll_sigue_la_fila_tras_rotar() {
        // Escribir en la fila 4 (fondo), luego scrollear 1: ese bit debe
        // reaparecer en la fila 3 tras el desplazamiento.
        let mut d = GridDamage::new(5, 10);
        d.full = false;
        d.mark_cell(4, 2);
        d.mark_scrolled(1, 0, 4);
        let snap = d.take();
        assert!(snap.is_cell_dirty(3, 2));
    }

    #[test]
    fn mark_row_dirty_en_snapshot_scrolled_marca_rest() {
        let mut snap = DamageSnapshot::Scrolled {
            lines: 1,
            region: (0, 4),
            rest: vec![vec![0]; 5],
        };
        snap.mark_row_dirty(2, 10);
        assert!(snap.is_row_dirty(2));
    }

    #[test]
    fn has_any_dirty_scrolled_true_por_el_desplazamiento_solo() {
        let snap = DamageSnapshot::Scrolled {
            lines: 1,
            region: (0, 4),
            rest: vec![vec![0]; 5],
        };
        assert!(snap.has_any_dirty());
    }

    #[test]
    fn resize_descarta_scroll_acumulado() {
        let mut d = GridDamage::new(5, 10);
        d.full = false;
        d.mark_scrolled(1, 0, 4);
        d.resize(5, 20);
        assert_eq!(d.take(), DamageSnapshot::Full);
    }
}
