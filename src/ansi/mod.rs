use crate::cursor::Cursor;
use crate::grid::{Grid, COLS, ROWS};

/// Colores basicos del terminal ANSI.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Color {
    #[default]
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Color {
    /// Convierte un codigo SGR (30-37 o 40-47) a Color.
    /// Devuelve Color::Default para codigos fuera de rango.
    pub fn from_code(code: u16) -> Self {
        match code {
            30 | 40 => Color::Black,
            31 | 41 => Color::Red,
            32 | 42 => Color::Green,
            33 | 43 => Color::Yellow,
            34 | 44 => Color::Blue,
            35 | 45 => Color::Magenta,
            36 | 46 => Color::Cyan,
            37 | 47 => Color::White,
            _ => Color::Default,
        }
    }
}

/// Atributos de estilo de texto (foreground, background, bold, underline).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Attrs {
    /// Color de foreground.
    pub fg: Color,
    /// Color de background.
    pub bg: Color,
    /// Negrita activada.
    pub bold: bool,
    /// Subrayado activado.
    pub underline: bool,
}

/// Estado completo del terminal virtual.
pub struct Term {
    /// Grid de caracteres 24x80.
    pub grid: Grid,
    /// Posicion del cursor.
    pub cursor: Cursor,
    /// Atributos actuales (se aplican a los caracteres que se escriben).
    pub attrs: Attrs,
    /// Si el cursor esta visible o no.
    pub cursor_visible: bool,
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}

impl Term {
    /// Crea un terminal nuevo: grid vacio, cursor en (0,0), atributos por defecto.
    pub fn new() -> Self {
        Self {
            grid: Grid::new(),
            cursor: Cursor::new(),
            attrs: Attrs::default(),
            cursor_visible: true,
        }
    }
}

/// Implementa el trait vte::Perform para procesar secuencias ANSI.
///
/// En Ronda 1 solo `print` es funcional. Los demas metodos son placeholders
/// que se implementaran en Rondas 2A/3.
impl vte::Perform for Term {
    /// Escribe un caracter en la posicion actual del cursor y avanza la columna.
    fn print(&mut self, c: char) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        // ponytail: wrap al final de linea llega en Sprint 3 con scroll.
        self.grid.set(row, col, c, self.attrs);
        self.cursor.col = self.cursor.col.saturating_add(1);
    }

    /// Ejecuta un byte de control C0 (BEL, BS, TAB, LF, CR, etc.).
    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL: placeholder, un emulador real haria beep.
            }
            0x08 => {
                // BS (backspace): retrocede una columna, no sale del grid.
                self.cursor.move_back(1);
            }
            0x09 => {
                // TAB: avanza al proximo tab stop (cada 8 columnas).
                // ponytail: tab stops fijos cada 8 cols.
                let next = ((self.cursor.col / 8) + 1) * 8;
                self.cursor.move_to(self.cursor.row, next.min(COLS - 1));
            }
            0x0A => {
                // LF (line feed): avanza una fila.
                // ponytail: scroll al final del grid llega en Fase 2 (Sprint 3).
                self.cursor.move_down(1);
            }
            0x0D => {
                // CR (carriage return): vuelve al inicio de la linea.
                self.cursor.move_to(self.cursor.row, 0);
            }
            _ => {}
        }
    }

    /// Despacha secuencias CSI (CSI ... action).
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // vte 0.15: Params::iter() yields &[u16] por parametro (subparams agrupados).
        // Para SGR/J/K, el primer subparam es el valor del parametro. Si el slice
        // esta vacio, vte lo trata como 0
        let params: Vec<u16> = params
            .iter()
            .map(|p| p.first().copied().unwrap_or(0))
            .collect();

        // DEC private modes: cursor visible/invisible (param 25).
        // ponytail: handler temprano con return para no ensuciar el match principal.
        if intermediates == b"?" {
            match action {
                'h' if params.first() == Some(&25) => self.cursor_visible = true,
                'l' if params.first() == Some(&25) => self.cursor_visible = false,
                _ => {}
            }
            return;
        }

        match action {
            'm' => {
                // SGR (Select Graphic Rendition)
                // Si no hay params, el estandar dice aplicar 0 (reset).
                if params.is_empty() {
                    self.attrs = Attrs::default();
                }
                for &code in &params {
                    match code {
                        0 => self.attrs = Attrs::default(),
                        1 => self.attrs.bold = true,
                        4 => self.attrs.underline = true,
                        30..=37 => self.attrs.fg = Color::from_code(code),
                        40..=47 => self.attrs.bg = Color::from_code(code),
                        // ponytail: bright variants (90-97, 100-107) con soporte 256-color en Sprint 4.
                        90..=97 | 100..=107 => {}
                        _ => {}
                    }
                }
            }
            'J' => {
                // Clear screen
                let n = params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                match n {
                    0 => {
                        // Cursor al final: limpiar desde cursor hasta fin
                        self.grid.clear_line(cur_row, cur_col, COLS);
                        for row in (cur_row + 1)..ROWS {
                            self.grid.clear_line(row, 0, COLS);
                        }
                    }
                    1 => {
                        // Inicio al cursor: limpiar desde inicio hasta cursor
                        for row in 0..cur_row {
                            self.grid.clear_line(row, 0, COLS);
                        }
                        self.grid.clear_line(cur_row, 0, cur_col + 1);
                    }
                    2 => {
                        // Limpiar todo el grid
                        self.grid.clear();
                    }
                    3 => {
                        // Clear scrollback: no soportado en Sprint 2
                    }
                    _ => {}
                }
            }
            'K' => {
                // Clear line
                let n = params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                match n {
                    0 => {
                        // Cursor al final de linea
                        self.grid.clear_line(cur_row, cur_col, COLS);
                    }
                    1 => {
                        // Inicio de linea al cursor
                        self.grid.clear_line(cur_row, 0, cur_col + 1);
                    }
                    2 => {
                        // Toda la linea
                        self.grid.clear_line(cur_row, 0, COLS);
                    }
                    _ => {}
                }
            }
            'A' => {
                // Cursor up: default 1 si param vacio o 0.
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_up(n as usize);
            }
            'B' => {
                // Cursor down: default 1 si param vacio o 0.
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_down(n as usize);
            }
            'C' => {
                // Cursor forward: default 1 si param vacio o 0.
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_forward(n as usize);
            }
            'D' => {
                // Cursor back: default 1 si param vacio o 0.
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_back(n as usize);
            }
            'H' => {
                // Cursor position: params son 1-indexed, default (1,1).
                // ponytail: 0/1-indexed equivalente, convencion comun.
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor.move_to(row, col);
            }
            _ => {}
        }
    }

    /// Despacha secuencias ESC (ESC ... byte).
    /// Placeholder
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        // noop por ahora
    }
}

// ---------------------------------------------------------------------------
// Suite de tests unitarios para Term / Grid / Cursor / Attrs
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::COLS;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Alimenta bytes crudos al parser vte con Term como performer.
    fn feed(term: &mut Term, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, data);
    }

    // -----------------------------------------------------------------------
    // Tests SGR
    // -----------------------------------------------------------------------

    #[test]
    fn test_sgr_red() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31mR");
        assert_eq!(term.grid.rows[0][0].ch, 'R');
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Red);
    }

    #[test]
    fn test_sgr_reset() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31m\x1b[0m");
        assert_eq!(term.attrs, Attrs::default());
    }

    #[test]
    fn test_sgr_bold() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[1mX");
        assert!(term.grid.rows[0][0].attrs.bold);
    }

    #[test]
    fn test_sgr_underline() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[4mX");
        assert!(term.grid.rows[0][0].attrs.underline);
    }

    #[test]
    fn test_sgr_multiparam() {
        let mut term = Term::new();
        // ponytail: 1;31 = bold + red
        feed(&mut term, b"\x1b[1;31mY");
        assert!(term.grid.rows[0][0].attrs.bold);
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Red);
    }

    // -----------------------------------------------------------------------
    // Tests cursor
    // -----------------------------------------------------------------------

    #[test]
    fn test_cursor_up() {
        let mut term = Term::new();
        // Mover a (5,5), luego 2 arriba => fila 3
        term.cursor.move_to(5, 5);
        feed(&mut term, b"\x1b[2A");
        assert_eq!(term.cursor.row, 3);
    }

    #[test]
    fn test_cursor_down() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[2B");
        assert_eq!(term.cursor.row, 2);
    }

    #[test]
    fn test_cursor_forward() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[3C");
        assert_eq!(term.cursor.col, 3);
    }

    #[test]
    fn test_cursor_back() {
        let mut term = Term::new();
        term.cursor.move_to(5, 5);
        feed(&mut term, b"\x1b[2D");
        assert_eq!(term.cursor.col, 3);
    }

    #[test]
    fn test_cursor_position() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;10H");
        assert_eq!(term.cursor.row, 4);
        assert_eq!(term.cursor.col, 9);
    }

    // -----------------------------------------------------------------------
    // Tests clear
    // -----------------------------------------------------------------------

    #[test]
    fn test_clear_screen() {
        let mut term = Term::new();
        feed(&mut term, b"abc");
        feed(&mut term, b"\x1b[2J");
        for row in &term.grid.rows {
            for cell in row {
                assert_eq!(cell.ch, ' ');
                assert_eq!(cell.attrs, Attrs::default());
            }
        }
    }

    #[test]
    fn test_clear_line() {
        let mut term = Term::new();
        feed(&mut term, b"abcdef");
        // Cursor esta en col 6, limpiar desde cursor al fin de linea
        feed(&mut term, b"\x1b[K");
        // Columnas 0-5 deben conservar "abcdef"
        let expected = ['a', 'b', 'c', 'd', 'e', 'f'];
        for (i, &ch) in expected.iter().enumerate() {
            assert_eq!(term.grid.rows[0][i].ch, ch);
        }
        // Columnas 6-79 deben ser espacio
        for col in 6..COLS {
            assert_eq!(term.grid.rows[0][col].ch, ' ');
        }
    }

    // -----------------------------------------------------------------------
    // Tests cursor visible
    // -----------------------------------------------------------------------

    #[test]
    fn test_cursor_visible() {
        let mut term = Term::new();
        term.cursor_visible = false;
        feed(&mut term, b"\x1b[?25h");
        assert!(term.cursor_visible);
    }

    #[test]
    fn test_cursor_hidden() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?25l");
        assert!(!term.cursor_visible);
    }

    // -----------------------------------------------------------------------
    // Tests C0 codes
    // -----------------------------------------------------------------------

    #[test]
    fn test_c0_lf_moves_cursor_down() {
        let mut term = Term::new();
        feed(&mut term, b"a\n");
        assert_eq!(term.cursor.row, 1);
        assert_eq!(term.grid.rows[0][0].ch, 'a');
    }

    #[test]
    fn test_c0_cr_returns_to_col_0() {
        let mut term = Term::new();
        feed(&mut term, b"abc\r");
        assert_eq!(term.cursor.col, 0);
        assert_eq!(term.cursor.row, 0);
    }

    #[test]
    fn test_c0_bs_moves_back() {
        let mut term = Term::new();
        feed(&mut term, b"abc\x08");
        assert_eq!(term.cursor.col, 2);
    }

    #[test]
    fn test_c0_tab_advances_to_next_stop() {
        let mut term = Term::new();
        feed(&mut term, b"\t");
        assert_eq!(term.cursor.col, 8);
    }

    #[test]
    fn test_c0_tab_at_col_79_stays() {
        let mut term = Term::new();
        term.cursor.move_to(0, 79);
        feed(&mut term, b"\t");
        assert_eq!(term.cursor.col, COLS - 1);
    }

    #[test]
    fn test_c0_bel_is_noop() {
        let mut term = Term::new();
        let saved_row = term.cursor.row;
        let saved_col = term.cursor.col;
        feed(&mut term, b"\x07");
        assert_eq!(term.cursor.row, saved_row);
        assert_eq!(term.cursor.col, saved_col);
    }

    // -----------------------------------------------------------------------
    // Tests de flujo / integracion
    // -----------------------------------------------------------------------

    #[test]
    fn test_print_writes_char_to_grid() {
        let mut term = Term::new();
        feed(&mut term, b"a");
        assert_eq!(term.grid.rows[0][0].ch, 'a');
        assert_eq!(term.cursor.col, 1);
    }

    #[test]
    fn test_sgr_applies_color_to_print() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[33mAB");
        assert_eq!(term.grid.rows[0][0].ch, 'A');
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Yellow);
        assert_eq!(term.grid.rows[0][1].ch, 'B');
        assert_eq!(term.grid.rows[0][1].attrs.fg, Color::Yellow);
    }

    #[test]
    fn test_bash_prompt_flow() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31mROJO\x1b[0m\n");
        assert_eq!(term.grid.rows[0][0].ch, 'R');
        assert_eq!(term.grid.rows[0][1].ch, 'O');
        assert_eq!(term.grid.rows[0][2].ch, 'J');
        assert_eq!(term.grid.rows[0][3].ch, 'O');
        // LF avanzo a fila 1, col se queda en 4 (LF no resetea columna)
        assert_eq!(term.cursor.row, 1);
        assert_eq!(term.cursor.col, 4);
    }

    // -----------------------------------------------------------------------
    // Test de bytes parciales (secuencia CSI dividida)
    // -----------------------------------------------------------------------

    #[test]
    fn bytes_partial_split_csi_sequence() {
        let mut term = Term::new();
        let mut expected = Term::new();
        feed(&mut expected, b"\x1b[31mR");

        // Usar un solo parser persistente para simular bytes parciales
        let mut parser = vte::Parser::new();
        parser.advance(&mut term, b"\x1b[");
        parser.advance(&mut term, b"31m");
        parser.advance(&mut term, b"R");

        assert_eq!(term.grid.rows[0][0].ch, expected.grid.rows[0][0].ch);
        assert_eq!(
            term.grid.rows[0][0].attrs.fg,
            expected.grid.rows[0][0].attrs.fg
        );
        assert_eq!(term.cursor, expected.cursor);
    }
}
