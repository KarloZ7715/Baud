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
    /// Grid de caracteres 24x80 (pantalla primaria).
    pub grid: Grid,
    /// Alt screen (solo se usa si alt_screen = true).
    pub alt_grid: Grid,
    /// Flag que indica si estamos en alt screen.
    pub alt_screen: bool,
    /// Region de scroll (top, bottom), default (0, ROWS - 1).
    pub scroll_region: (usize, usize),
    /// Auto wrap activo (DECAWM), default true.
    pub auto_wrap: bool,
    /// Flag de wrap pendiente (cursor en ultima columna).
    pub pending_wrap: bool,
    /// Posicion del cursor.
    pub cursor: Cursor,
    /// Atributos actuales (se aplican a los caracteres que se escriben).
    pub attrs: Attrs,
    /// Si el cursor esta visible o no.
    pub cursor_visible: bool,
    /// Cursor guardado para DEC 1049 y DECSC/DECRC.
    pub saved_cursor: Option<(usize, usize)>,
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
            // ponytail: alt_grid siempre inicializado, no se recrea al entrar
            alt_grid: Grid::new(),
            alt_screen: false,
            scroll_region: (0, ROWS - 1),
            auto_wrap: true,
            pending_wrap: false,
            cursor: Cursor::new(),
            attrs: Attrs::default(),
            cursor_visible: true,
            saved_cursor: None,
        }
    }

    /// El grid que el Renderer debe pintar.
    pub fn active_grid(&self) -> &Grid {
        if self.alt_screen {
            &self.alt_grid
        } else {
            &self.grid
        }
    }

    /// Grid mutable interno, accesible solo desde el modulo ansi.
    fn active_grid_mut(&mut self) -> &mut Grid {
        if self.alt_screen {
            &mut self.alt_grid
        } else {
            &mut self.grid
        }
    }

    /// Entra a alt screen. Guarda el cursor primario, limpia la pantalla alt.
    pub fn enter_alt_screen(&mut self) {
        self.alt_grid = Grid::new();
        self.saved_cursor = Some((self.cursor.row, self.cursor.col));
        self.alt_screen = true;
        self.cursor.move_to(0, 0);
        self.pending_wrap = false;
    }

    /// Sale de alt screen. Restaura el cursor primario, contenido primario intacto.
    pub fn exit_alt_screen(&mut self) {
        self.alt_screen = false;
        if let Some((row, col)) = self.saved_cursor.take() {
            self.cursor.move_to(row, col);
        }
        self.pending_wrap = false;
    }

    /// Ejecuta el wrap pendiente: avanza una fila (con scroll si estamos
    /// en el bottom de la scroll region) y mueve el cursor a col 0.
    /// SIEMPRE setea `pending_wrap = false` al inicio para evitar loops.
    fn do_pending_wrap(&mut self) {
        self.pending_wrap = false;
        let (top, bottom) = self.scroll_region;
        if self.cursor.row == bottom {
            self.active_grid_mut().scroll_up_region(1, top, bottom);
            // cursor.row NO avanza; queda en bottom
        } else {
            self.cursor.move_down(1);
        }
        self.cursor.move_to(self.cursor.row, 0);
    }

    /// DECSC: guarda la posicion del cursor.
    fn save_cursor(&mut self) {
        self.saved_cursor = Some((self.cursor.row, self.cursor.col));
    }

    /// DECRC: restaura la posicion del cursor. CANCELA pending_wrap.
    fn restore_cursor(&mut self) {
        if let Some((row, col)) = self.saved_cursor {
            self.cursor.move_to(row, col);
        }
        self.pending_wrap = false;
    }
}

/// Implementa el trait vte::Perform para procesar secuencias ANSI.
///
/// En Ronda 1 solo `print` es funcional. Los demas metodos son placeholders
/// que se implementaran en Rondas 2A/3.
impl vte::Perform for Term {
    /// Escribe un caracter en la posicion actual del cursor y avanza la columna.
    /// Si hay pending_wrap activo, primero ejecuta el wrap (avanza fila,
    /// columna a 0).
    /// Si al escribir el cursor llega a COLS, marca pending_wrap si
    /// auto_wrap esta activo.
    fn print(&mut self, c: char) {
        // Si hay wrap pendiente, primero hacer el wrap.
        // ponytail: DECAWM + pending_wrap en print.
        if self.pending_wrap {
            self.do_pending_wrap();
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        let attrs = self.attrs;
        self.active_grid_mut().set(row, col, c, attrs);
        self.cursor.col += 1;
        if self.cursor.col >= COLS {
            // Estamos en la ultima columna. Si auto_wrap esta activo,
            // marcamos pending_wrap; el proximo print ejecutara el wrap.
            if self.auto_wrap {
                self.cursor.col = COLS - 1; // nos quedamos en la ultima col visible
                self.pending_wrap = true;
            } else {
                // sin wrap: permanecemos en la ultima col (no avanza)
                self.cursor.col = COLS - 1;
            }
        }
    }

    /// Ejecuta un byte de control C0 (BEL, BS, TAB, LF, CR, etc.).
    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL: placeholder, un emulador real haria beep.
            }
            0x08 => {
                // BS (backspace): retrocede una columna, no sale del grid.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                self.cursor.move_back(1);
            }
            0x09 => {
                // TAB: avanza al proximo tab stop (cada 8 columnas).
                // ponytail: tab stops fijos cada 8 cols.
                let next = ((self.cursor.col / 8) + 1) * 8;
                self.cursor.move_to(self.cursor.row, next.min(COLS - 1));
            }
            0x0A => {
                // LF (line feed): avanzar una fila. Si estamos en el bottom
                // de la scroll region, hacer scroll_up de la region.
                // CANCELA pending_wrap.
                // ponytail: scroll al final de la scroll region.
                self.pending_wrap = false;
                let (top, bottom) = self.scroll_region;
                if self.cursor.row == bottom {
                    self.active_grid_mut().scroll_up_region(1, top, bottom);
                } else {
                    self.cursor.move_down(1);
                }
            }
            0x0D => {
                // CR (carriage return): vuelve al inicio de la linea.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
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
        // DEC private modes: handler temprano con return para no ensuciar
        // el match principal.
        if intermediates == b"?" {
            let mode = params
                .iter()
                .next()
                .map(|p| p.first().copied().unwrap_or(0))
                .unwrap_or(0);
            match (action, mode) {
                ('h', 25) => self.cursor_visible = true,
                ('l', 25) => self.cursor_visible = false,
                ('h', 7) => self.auto_wrap = true,
                ('l', 7) => self.auto_wrap = false,
                ('h', 1049) => self.enter_alt_screen(),
                ('l', 1049) => self.exit_alt_screen(),
                // ponytail: 1000-1006 son mouse reporting, se ignoran en este sprint
                ('h' | 'l', 1000..=1006) => {}
                _ => {}
            }
            return;
        }

        // Mouse reporting SGR (intermediates == b"<"): CSI < Ps ; Ps ; Ps M o m.
        // ponytail: parseo minimo, no se decodifican las coordenadas del mouse.
        // Sprint 7 implementa el report real.
        if intermediates == b"<" {
            match action {
                'M' | 'm' => return,
                _ => {}
            }
        }

        // vte 0.15: Params::iter() yields &[u16] por parametro (subparams agrupados).
        // Para SGR/J/K, el primer subparam es el valor del parametro. Si el slice
        // esta vacio, vte lo trata como 0
        let params: Vec<u16> = params
            .iter()
            .map(|p| p.first().copied().unwrap_or(0))
            .collect();

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
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_up(n as usize);
            }
            'B' => {
                // Cursor down: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_down(n as usize);
            }
            'C' => {
                // Cursor forward: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_forward(n as usize);
            }
            'D' => {
                // Cursor back: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_back(n as usize);
            }
            'H' => {
                // Cursor position: params son 1-indexed, default (1,1).
                // ponytail: 0/1-indexed equivalente, convencion comun.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor.move_to(row, col);
            }
            'r' => {
                // DECSTBM: set scrolling region. Parametros 1-indexed.
                // Default top=1, bottom=ROWS. Si top >= bottom, resetea a
                // pantalla completa (convencion xterm; VT510 estricto dice
                // "ignorar").
                // ponytail: convencion xterm, no VT510. Discrepancia documentada.
                let top = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let bottom = params
                    .get(1)
                    .copied()
                    .unwrap_or(ROWS as u16)
                    .saturating_sub(1) as usize;
                if top >= bottom || top >= ROWS || bottom >= ROWS {
                    self.scroll_region = (0, ROWS - 1);
                } else {
                    self.scroll_region = (top, bottom);
                }
                self.cursor.move_to(0, 0);
                self.pending_wrap = false;
            }
            'L' => {
                // IL (insert line): inserta n lineas en blanco en la fila
                // del cursor, desplazando las lineas siguientes hacia abajo.
                // La fila final (ROWS-1) se pierde.
                // ponytail: xterm NO respeta la scroll region en IL/DL.
                // El cursor determina la fila, no la region.
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                if row < ROWS {
                    for _ in 0..n {
                        self.active_grid_mut().insert_line(row);
                    }
                }
                self.pending_wrap = false;
            }
            'M' => {
                // DL (delete line): borra n lineas empezando en la fila del
                // cursor, desplazando las lineas siguientes hacia arriba.
                // La fila final queda en blanco.
                // ponytail: xterm NO respeta la scroll region en IL/DL.
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                if row < ROWS {
                    for _ in 0..n {
                        self.active_grid_mut().delete_line(row);
                    }
                }
                self.pending_wrap = false;
            }
            '@' => {
                // ICH (insert character): inserta n chars en blanco en la
                // posicion del cursor, desplazando el resto a la derecha.
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                self.active_grid_mut().insert_chars(row, col, n);
                self.pending_wrap = false;
            }
            'P' => {
                // DCH (delete character): borra n chars desde la posicion
                // del cursor, desplazando el resto a la izquierda.
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                self.active_grid_mut().delete_chars(row, col, n);
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    /// Despacha secuencias ESC (ESC ... byte).
    /// CRITICO: byte == 0x37 para DECSC (ESC 7) y 0x38 para DECRC (ESC 8),
    /// NO confundir con 0x07 (BEL) ni 0x08 (BS). vte ya ejecuta 0x07 y 0x08
    /// via execute(), asi que en esc_dispatch jamas llegan.
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        // ponytail: DECSC / DECRC con 0x37 / 0x38.
        match byte {
            0x37 => self.save_cursor(),    // DECSC: "ESC 7"
            0x38 => self.restore_cursor(), // DECRC: "ESC 8"
            _ => {}
        }
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

    // -----------------------------------------------------------------------
    // Tests Sprint 4: alt screen, DECSTBM, LF scroll
    // -----------------------------------------------------------------------

    #[test]
    fn test_alt_screen_enter_saves_cursor() {
        let mut term = Term::new();
        term.cursor.move_to(5, 10);
        feed(&mut term, b"\x1b[?1049h");
        assert_eq!(term.saved_cursor, Some((5, 10)));
        assert!(term.alt_screen);
        assert_eq!((term.cursor.row, term.cursor.col), (0, 0));
    }

    #[test]
    fn test_alt_screen_exit_restores() {
        let mut term = Term::new();
        term.cursor.move_to(5, 10);
        feed(&mut term, b"\x1b[?1049h"); // enter alt, saves cursor (5,10), cursor to (0,0)
        term.cursor.move_to(3, 7);
        feed(&mut term, b"\x1b[?1049l"); // exit alt, restores cursor to (5,10)
        assert!(!term.alt_screen);
        assert_eq!((term.cursor.row, term.cursor.col), (5, 10));
        assert_eq!(term.saved_cursor, None);
    }

    #[test]
    fn test_decstbm_default_full_screen() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[r");
        assert_eq!(term.scroll_region, (0, ROWS - 1));
    }

    #[test]
    fn test_decstbm_custom_region() {
        let mut term = Term::new();
        term.cursor.move_to(10, 5);
        feed(&mut term, b"\x1b[5;10r");
        assert_eq!(term.scroll_region, (4, 9));
        assert_eq!((term.cursor.row, term.cursor.col), (0, 0));
    }

    #[test]
    fn test_decstbm_top_eq_bottom_resets_to_full() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;5r");
        assert_eq!(term.scroll_region, (0, ROWS - 1));
    }

    #[test]
    fn test_lf_scrolls_up_at_bottom() {
        let mut term = Term::new();
        // Escribir contenido en las primeras 23 filas
        for i in 0..23 {
            term.cursor.move_to(i, 0);
            feed(&mut term, b"X");
            feed(&mut term, b"\n");
        }
        // Cursor esta en la fila 23 (bottom de la scroll region full)
        // Un LF mas debe hacer scroll up de la region
        feed(&mut term, b"\n");
        // La fila 0 ahora tiene el contenido que estaba en la fila 1
        assert_eq!(term.grid.rows[0][0].ch, 'X');
        // La ultima fila debe estar en blanco
        assert_eq!(term.grid.rows[ROWS - 1][0].ch, ' ');
    }

    // -----------------------------------------------------------------------
    // Tests Sprint 4 Ronda 2: DECSC, DECRC, DECAWM, pending_wrap
    // -----------------------------------------------------------------------

    #[test]
    fn test_decsc_decrc_round_trip() {
        let mut term = Term::new();
        term.cursor.move_to(5, 10);
        feed(&mut term, b"\x1b7"); // DECSC: guarda cursor (5,10)
        term.cursor.move_to(0, 0);
        feed(&mut term, b"\x1b8"); // DECRC: restaura cursor a (5,10)
        assert_eq!((term.cursor.row, term.cursor.col), (5, 10));
    }

    #[test]
    fn test_decawm_disabled_no_wrap() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?7l"); // auto_wrap off
        for _ in 0..80 {
            feed(&mut term, b"X");
        }
        // cursor debe quedar en col 79 (COLS - 1)
        assert_eq!(term.cursor.col, COLS - 1);
        // el ultimo caracter sobreescribe la ultima columna
        assert_eq!(term.active_grid().rows[0][COLS - 1].ch, 'X');
    }

    #[test]
    fn test_decawm_enabled_wraps_at_last_col() {
        let mut term = Term::new();
        // auto_wrap activo por defecto
        for _ in 0..80 {
            feed(&mut term, b"X");
        }
        // cursor en (0, 79), pending_wrap = true
        assert_eq!((term.cursor.row, term.cursor.col), (0, COLS - 1));
        assert!(term.pending_wrap);
        // un char mas dispara el wrap
        feed(&mut term, b"Y");
        // cursor en (1, 1): do_pending_wrap mueve a (1, 0), print avanza col a 1
        assert_eq!((term.cursor.row, term.cursor.col), (1, 1));
        assert!(!term.pending_wrap);
        assert_eq!(term.active_grid().rows[1][0].ch, 'Y');
    }

    #[test]
    fn test_pending_wrap_cancelled_by_cursor_move() {
        let mut term = Term::new();
        for _ in 0..80 {
            feed(&mut term, b"X");
        }
        // cursor en (0, 79), pending_wrap = true
        assert!(term.pending_wrap);
        // mover cursor cancela el wrap pendiente
        feed(&mut term, b"\x1b[3;5H"); // CUP a (2, 4)
        assert!(!term.pending_wrap);
        assert_eq!((term.cursor.row, term.cursor.col), (2, 4));
        // escribir un char sin wrap
        feed(&mut term, b"Z");
        assert_eq!((term.cursor.row, term.cursor.col), (2, 5));
        assert_eq!(term.active_grid().rows[2][4].ch, 'Z');
    }

    #[test]
    fn test_pending_wrap_scrolls_at_bottom() {
        let mut term = Term::new();
        term.cursor.move_to(ROWS - 1, COLS - 1);
        // Escribir 1 char: cursor en bottom, col 79, auto_wrap activo
        // esto coloca pending_wrap = true
        // (el caracter se escribe en la ultima columna, cursor se queda en ella)
        feed(&mut term, b"X");
        assert!(term.pending_wrap);
        assert_eq!((term.cursor.row, term.cursor.col), (ROWS - 1, COLS - 1));
        // Otro char dispara do_pending_wrap: scroll up de la region,
        // cursor pasa a (ROWS - 1, 0), luego print avanza col a 1
        feed(&mut term, b"Y");
        assert!(!term.pending_wrap);
        assert_eq!((term.cursor.row, term.cursor.col), (ROWS - 1, 1));
        // La fila 0 debe haber sido desplazada hacia arriba
        assert_eq!(term.active_grid().rows[0][COLS - 1].ch, ' ');
        // La ultima fila tiene el nuevo caracter
        assert_eq!(term.active_grid().rows[ROWS - 1][0].ch, 'Y');
    }

    #[test]
    fn test_print_after_pending_wrap_uses_active_grid() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?1049h"); // entra alt screen
                                         // Escribir 80 chars en alt grid, cursor en (0, 79), pending_wrap = true
        for _ in 0..80 {
            feed(&mut term, b"A");
        }
        assert!(term.pending_wrap);
        assert!(term.alt_screen);
        assert_eq!((term.cursor.row, term.cursor.col), (0, COLS - 1));
        // Escribir 81vo char: debe hacer wrap en alt grid, NO en primaria
        feed(&mut term, b"B");
        assert!(!term.pending_wrap);
        // cursor en (1, 1): do_pending_wrap mueve a (1, 0), print avanza col a 1
        assert_eq!((term.cursor.row, term.cursor.col), (1, 1));
        assert_eq!(term.alt_grid.rows[1][0].ch, 'B');
        // Primaria debe estar vacia
        assert_eq!(term.grid.rows[0][0].ch, ' ');
    }

    // -----------------------------------------------------------------------
    // Tests Sprint 4 Ronda 3: IL, DL, ICH, DCH
    // -----------------------------------------------------------------------

    #[test]
    fn test_il_inserts_line() {
        let mut term = Term::new();
        // Llenar la fila 5 con 'X'
        for col in 0..COLS {
            term.grid.rows[5][col].ch = 'X';
        }
        // Cursor en fila 5
        term.cursor.move_to(5, 0);
        // Insertar 1 linea
        feed(&mut term, b"\x1b[1L");
        // Fila 5 debe estar en blanco
        assert_eq!(term.grid.rows[5][0].ch, ' ');
        // Fila 6 debe tener los X que antes estaban en fila 5
        assert_eq!(term.grid.rows[6][0].ch, 'X');
        // La ultima fila (23) debe estar en blanco (se perdio)
        assert_eq!(term.grid.rows[ROWS - 1][0].ch, ' ');
    }

    #[test]
    fn test_dl_deletes_line() {
        let mut term = Term::new();
        // Llenar todas las filas con 'X' en col 0
        for row in 0..ROWS {
            term.grid.rows[row][0].ch = 'X';
        }
        // Cursor en fila 5
        term.cursor.move_to(5, 0);
        // Borrar 1 linea en fila 5
        feed(&mut term, b"\x1b[1M");
        // Fila 5 ahora tiene el contenido que estaba en fila 6
        assert_eq!(term.grid.rows[5][0].ch, 'X');
        // Las filas 6..22 tienen el contenido desplazado (ex-fila 7..23)
        // Todas tienen 'X' porque originalmente todas las filas tenian 'X'
        assert_eq!(term.grid.rows[22][0].ch, 'X');
        // La ultima fila (23) debe estar en blanco (nueva fila vacia al final)
        assert_eq!(term.grid.rows[ROWS - 1][0].ch, ' ');
    }

    #[test]
    fn test_ich_inserts_chars() {
        let mut term = Term::new();
        // Escribir "ABCDE" en col 0..4 de la fila 0
        let chars = ['A', 'B', 'C', 'D', 'E'];
        for (i, &ch) in chars.iter().enumerate() {
            term.grid.rows[0][i].ch = ch;
        }
        // Cursor en col 2
        term.cursor.move_to(0, 2);
        // Insertar 2 chars
        feed(&mut term, b"\x1b[2@");
        // Resultado esperado: A B _ _ C D E ...
        // (cols 0-1 = 'A','B', cols 2-3 = ' ',' ', col 4 = 'C', col 5 = 'D', col 6 = 'E')
        assert_eq!(term.grid.rows[0][0].ch, 'A');
        assert_eq!(term.grid.rows[0][1].ch, 'B');
        assert_eq!(term.grid.rows[0][2].ch, ' ');
        assert_eq!(term.grid.rows[0][3].ch, ' ');
        assert_eq!(term.grid.rows[0][4].ch, 'C');
        assert_eq!(term.grid.rows[0][5].ch, 'D');
        assert_eq!(term.grid.rows[0][6].ch, 'E');
        // pending_wrap debe estar cancelado
        assert!(!term.pending_wrap);
    }

    #[test]
    fn test_dch_deletes_chars() {
        let mut term = Term::new();
        // Escribir "ABCDE" en col 0..4 de la fila 0
        let chars = ['A', 'B', 'C', 'D', 'E'];
        for (i, &ch) in chars.iter().enumerate() {
            term.grid.rows[0][i].ch = ch;
        }
        // Cursor en col 1
        term.cursor.move_to(0, 1);
        // Borrar 2 chars desde col 1
        feed(&mut term, b"\x1b[2P");
        // Resultado esperado: A D E _ _ ...
        // (cols 0 = 'A', col 1 = 'D', col 2 = 'E', col 3 = ' ', col 4 = ' ')
        assert_eq!(term.grid.rows[0][0].ch, 'A');
        assert_eq!(term.grid.rows[0][1].ch, 'D');
        assert_eq!(term.grid.rows[0][2].ch, 'E');
        assert_eq!(term.grid.rows[0][3].ch, ' ');
        assert_eq!(term.grid.rows[0][4].ch, ' ');
        // pending_wrap debe estar cancelado
        assert!(!term.pending_wrap);
    }

    #[test]
    fn test_il_in_alt_screen_operates_on_alt() {
        let mut term = Term::new();
        // Escribir 'A' en la fila 3 de la primaria para verificar que no se modifica
        term.grid.rows[3][0].ch = 'A';
        // Entrar a alt screen
        feed(&mut term, b"\x1b[?1049h");
        // Escribir 'B' en fila 3 del alt grid
        term.cursor.move_to(3, 0);
        feed(&mut term, b"B");
        // Insertar 1 linea en fila 3 del alt grid
        feed(&mut term, b"\x1b[1L");
        // Verificar que la fila 3 del alt grid esta en blanco (IL la limpio)
        assert_eq!(term.alt_grid.rows[3][0].ch, ' ');
        // Verificar que la primaria NO se modifico
        assert_eq!(term.grid.rows[3][0].ch, 'A');
        // La fila 4 del alt grid debe tener la 'B' que estaba en fila 3
        assert_eq!(term.alt_grid.rows[4][0].ch, 'B');
    }

    // -----------------------------------------------------------------------
    // Tests Sprint 4 Ronda 4: modelo alt/primary grid, mouse ignore
    // -----------------------------------------------------------------------

    #[test]
    fn test_alt_grid_independent_from_primary() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?1049h"); // entrar alt
        feed(&mut term, b"A"); // escribe en alt
        feed(&mut term, b"\x1b[?1049l"); // salir alt
        feed(&mut term, b"B"); // escribe en primaria
        assert_eq!(term.grid.rows[0][0].ch, 'B');
        assert_eq!(term.alt_grid.rows[0][0].ch, 'A');
    }

    #[test]
    fn test_active_grid_returns_alt_when_active() {
        let mut term = Term::new();
        assert!(!term.alt_screen);
        assert!(std::ptr::eq(term.active_grid(), &term.grid));
        feed(&mut term, b"\x1b[?1049h");
        assert!(term.alt_screen);
        assert!(std::ptr::eq(term.active_grid(), &term.alt_grid));
    }

    #[test]
    fn test_alt_screen_preserves_primary_content() {
        let mut term = Term::new();
        feed(&mut term, b"HOLA");
        feed(&mut term, b"\x1b[?1049h");
        feed(&mut term, b"VIM ");
        feed(&mut term, b"\x1b[?1049l");
        // La primaria debe empezar con 'HOLA' intacto.
        assert_eq!(term.grid.rows[0][0].ch, 'H');
        assert_eq!(term.grid.rows[0][1].ch, 'O');
        assert_eq!(term.grid.rows[0][2].ch, 'L');
        assert_eq!(term.grid.rows[0][3].ch, 'A');
    }

    #[test]
    fn test_mouse_reporting_ignored() {
        let mut term = Term::new();
        // SGR mouse: ESC [ < 0 ; 10 ; 5 M y m
        feed(&mut term, b"\x1b[<0;10;5M");
        feed(&mut term, b"\x1b[<0;10;5m");
        // DEC private mouse: ESC [ ? 1000 h / 1006 l
        feed(&mut term, b"\x1b[?1000h");
        feed(&mut term, b"\x1b[?1006l");
        // El grid NO debe haber cambiado (sigue siendo el char default, espacio).
        assert_eq!(term.grid.rows[0][0].ch, ' ');
    }
}
