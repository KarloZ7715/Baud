//! Bitmask de celdas modificadas para render incremental.

const BITS_PER_WORD: usize = 64;

/// Tope defensivo de columnas para bitmask de damage (evita OOM si cols es absurdo).
pub(crate) const MAX_DAMAGE_COLS: usize = 4096;

/// Rastrea qué celdas del grid cambiaron desde el último frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridDamage {
    /// `rows[row][word]` cubre columnas `word*64 .. word*64+63`.
    rows: Vec<Vec<u64>>,
    cols: usize,
    /// Invalidación total (resize, clear, reflow).
    full: bool,
}

impl GridDamage {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: vec![vec![0; Self::words_for_cols(cols)]; rows],
            cols,
            full: true,
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let cols = cols.clamp(1, MAX_DAMAGE_COLS);
        let rows = rows.clamp(1, MAX_DAMAGE_COLS);
        self.rows = vec![vec![0; Self::words_for_cols(cols)]; rows];
        self.cols = cols;
        self.full = true;
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn mark_all(&mut self) {
        self.full = true;
        for row in &mut self.rows {
            row.fill(0);
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
    pub fn take(&mut self) -> DamageSnapshot {
        if self.rows.len() > MAX_DAMAGE_COLS || self.cols > MAX_DAMAGE_COLS {
            self.full = true;
        }
        let snapshot = if self.full {
            DamageSnapshot::Full
        } else {
            DamageSnapshot::Cells(self.rows.clone())
        };
        self.full = false;
        for row in &mut self.rows {
            row.fill(0);
        }
        snapshot
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
}

impl DamageSnapshot {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn is_cell_dirty(&self, row: usize, col: usize) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => {
                let word = col / BITS_PER_WORD;
                let bit = col % BITS_PER_WORD;
                rows.get(row)
                    .and_then(|words| words.get(word))
                    .is_some_and(|w| (w & (1_u64 << bit)) != 0)
            }
        }
    }

    /// True si alguna celda de la fila cambio.
    pub fn is_row_dirty(&self, row: usize) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => rows
                .get(row)
                .is_some_and(|words| words.iter().any(|w| *w != 0)),
        }
    }

    /// True si alguna celda esta marcada como dirty.
    pub fn has_any_dirty(&self) -> bool {
        match self {
            Self::Full => true,
            Self::Cells(rows) => rows.iter().any(|words| words.iter().any(|w| *w != 0)),
        }
    }

    /// Marca toda la fila como sucia (p. ej. cambio de seleccion).
    pub fn mark_row_dirty(&mut self, row: usize, cols: usize) {
        if self.is_full() {
            return;
        }
        let cols = cols.clamp(1, MAX_DAMAGE_COLS);
        let Self::Cells(rows) = self else {
            return;
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
}
