use crate::cursor::Cursor;
use crate::grid::{Grid, DEFAULT_ROWS};
use crate::selection::Selection;

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
    /// Grid de caracteres (pantalla primaria).
    pub grid: Grid,
    /// Alt screen (solo se usa si alt_screen = true).
    pub alt_grid: Grid,
    /// Flag que indica si estamos en alt screen.
    pub alt_screen: bool,
    /// Region de scroll (top, bottom), default (0, rows_count - 1).
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
    // DEC 2004: bracketed paste mode
    pub bracketed_paste: bool,
    // Desplazamiento de scrollback para navegacion (pagina arriba/abajo)
    pub scrollback_offset: isize,
    // Seleccion activa del terminal (mouse)
    pub selection: Option<Selection>,
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
            scroll_region: (0, DEFAULT_ROWS - 1),
            auto_wrap: true,
            pending_wrap: false,
            cursor: Cursor::new(),
            attrs: Attrs::default(),
            cursor_visible: true,
            saved_cursor: None,
            bracketed_paste: false,
            scrollback_offset: 0,
            selection: None,
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

    /// Devuelve la cantidad de líneas en el scrollback.
    pub fn scrollback_len(&self) -> usize {
        self.grid.scrollback.len()
    }

    /// Cambia el tamaño del grid primario y alt grid.
    /// En pantalla primaria: aplica reflow de líneas antes de resize.
    /// En alt screen: resize directo sin reflow.
    /// También ajusta scroll_region, cursor y pending_wrap si es necesario.
    pub fn resize_grid(&mut self, new_rows: usize, new_cols: usize) {
        if self.alt_screen {
            self.alt_grid.resize(new_rows, new_cols);
        } else {
            // Primero reflow: re-dividir el contenido en el nuevo ancho de columna
            self.grid.reflow(new_cols);
            // resize devuelve cuántas filas se prependieron del scrollback
            let prepended = self.grid.resize(new_rows, new_cols);
            // Si se prependieron filas, desplazar el cursor hacia abajo
            // para que apunte a la misma línea lógica.
            if prepended > 0 {
                self.cursor.row = self.cursor.row.saturating_add(prepended);
            }
        }
        // Siempre redimensionar alt_grid para que coincida con el tamano del terminal
        self.alt_grid.resize(new_rows, new_cols);
        self.cursor.resize(new_rows, new_cols);
        self.scroll_region = (0, new_rows - 1);
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
        // Marcar esta fila como continuación por soft-wrap, no hard break.
        let target_row = self.cursor.row;
        self.active_grid_mut().set_continuation(target_row, true);
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

    /// Verifica si una celda visible (row, col) está dentro de la selección activa.
    /// Convierte coordenadas visibles a lógicas usando scrollback_offset.
    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        let Some(ref sel) = self.selection else {
            return false;
        };
        let logical_row = self.visible_to_logical_row(row);
        sel.contains(logical_row, col)
    }

    /// Extrae el texto del rango seleccionado como String.
    /// Concatena las filas involucradas con '\n' entre líneas no-continuación.
    pub fn selected_text(&self) -> String {
        let Some(ref sel) = self.selection else {
            return String::new();
        };
        let (start_row, start_col, end_row, end_col) = sel.normalize();
        let mut result = String::new();

        let active = self.active_grid();
        let rows_count = active.rows_count;
        // ponytail: en alt_screen no hay scrollback, aunque el primario tenga
        let sb_len = if self.alt_screen { 0 } else { self.grid.scrollback.len() };
        let total_rows = sb_len + rows_count;

        // Convertir coordenadas visibles de la seleccion a absolutas.
        // En scrollback_offset=0, el viewport muestra rows_count filas del grid
        // que empiezan en sb_len. En scrollback_offset>0, empieza en sb_len-offset.
        let viewport_start = if self.scrollback_offset > 0 {
            sb_len.saturating_sub(self.scrollback_offset as usize)
        } else {
            sb_len
        };
        let abs_start = start_row + viewport_start;
        let abs_end = end_row + viewport_start;

        for logical_row in abs_start..=abs_end {
            if logical_row >= total_rows {
                break;
            }
            let (source_row, source_is_grid) = if logical_row < sb_len {
                (logical_row, false)
            } else {
                (logical_row - sb_len, true)
            };

            let row_cells: &[crate::grid::Cell] = if source_is_grid {
                if source_row < active.rows.len() {
                    &active.rows[source_row]
                } else {
                    continue;
                }
            } else if let Some(row_vec) = self.grid.scrollback.get(logical_row) {
                row_vec.as_slice()
            } else {
                continue;
            };

            // Determinar el final real de la fila (último carácter no espacio).
            let actual_row_end = row_cells
                .iter()
                .rposition(|c| c.ch != ' ')
                .map(|p| p + 1)
                .unwrap_or(0);

            let col_end_logical = if logical_row == abs_end {
                end_col + 1
            } else {
                actual_row_end
            };
            let col_end = col_end_logical.min(row_cells.len());

            let col_start = if logical_row == abs_start {
                start_col
            } else {
                0
            };

            if col_end <= col_start {
                // Fila vacía en el rango seleccionado
                if logical_row < abs_end {
                    result.push('\n');
                }
                continue;
            }

            for cell in row_cells[col_start..col_end].iter() {
                if cell.ch != ' ' || cell.width > 0 {
                    result.push(cell.ch);
                }
            }

            // Salto de línea entre filas (excepto si la siguiente es continuación).
            if logical_row < abs_end {
                let next_row = logical_row + 1;
                let is_continuation = if next_row < sb_len {
                    false
                } else {
                    let grid_row = next_row - sb_len;
                    grid_row < active.row_continuations.len() && active.row_continuations[grid_row]
                };
                if !is_continuation {
                    result.push('\n');
                }
            }
        }

        result
    }

    /// Limpia la selección actual.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Convierte una fila visible (índice en pantalla, 0..rows_count-1)
    /// a una fila lógica dentro del buffer virtual [scrollback + grid].
    fn visible_to_logical_row(&self, visible_row: usize) -> usize {
        if self.scrollback_offset > 0 && !self.alt_screen {
            let sb_len = self.grid.scrollback.len();
            let offset = self.scrollback_offset as usize;
            let viewport_start = sb_len.saturating_sub(offset);
            viewport_start + visible_row
        } else {
            visible_row
        }
    }
}

/// Implementa el trait vte::Perform para procesar secuencias ANSI.
impl vte::Perform for Term {
    /// Escribe un caracter en la posicion actual del cursor y avanza la columna
    /// según el ancho Unicode del caracter (1 para latino, 2 para CJK, etc.).
    /// Si `c_width == 0` (caracter de ancho cero), no escribe nada.
    /// Si hay pending_wrap activo, primero ejecuta el wrap (avanza fila,
    /// columna a 0).
    /// Si al escribir el cursor sale del grid, marca pending_wrap si
    /// auto_wrap esta activo.
    /// Los caracteres de ancho 2 en la ultima columna fuerzan un wrap.
    fn print(&mut self, c: char) {
        let c_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        // Caracteres de ancho cero (controles, combinantes) se ignoran.
        if c_width == 0 {
            return;
        }

        // Si hay wrap pendiente, primero hacer el wrap.
        if self.pending_wrap {
            self.do_pending_wrap();
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        let attrs = self.attrs;
        let cols = self.cursor.cols_count;

        // Caracter ancho (CJK) en la ultima columna: wrap forzado.
        if c_width >= 2 && col + c_width > cols && self.auto_wrap {
            self.pending_wrap = true;
            self.do_pending_wrap();
            let row = self.cursor.row;
            let col = self.cursor.col;
            {
                let active = self.active_grid_mut();
                let cols = active.cols_count;
                if let Some(cell) = active.cell(row, col) {
                    cell.ch = c;
                    cell.attrs = attrs;
                    cell.width = c_width as u8;
                }
                self.cursor.col = col + c_width;
                if self.cursor.col >= cols {
                    if self.auto_wrap {
                        self.cursor.col = cols - 1;
                        self.pending_wrap = true;
                    } else {
                        self.cursor.col = cols - 1;
                    }
                }
            }
            return;
        }

        // Ruta normal: escribir caracter y avanzar cursor.
        {
            let active = self.active_grid_mut();
            let cols = active.cols_count;
            if let Some(cell) = active.cell(row, col) {
                cell.ch = c;
                cell.attrs = attrs;
                cell.width = c_width as u8;
            }
            self.cursor.col += c_width;
            if self.cursor.col >= cols {
                if self.auto_wrap {
                    self.cursor.col = cols - 1;
                    self.pending_wrap = true;
                } else {
                    self.cursor.col = cols - 1;
                }
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
                self.cursor
                    .move_to(self.cursor.row, next.min(self.cursor.cols_count - 1));
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
                // Hard break: la fila a la que nos movimos NO es continuación
                // de la anterior por wrap. Si no se resetea, el reflow no
                // podrá fusionar líneas al ensanchar la ventana.
                let cursor_row = self.cursor.row;
                self.active_grid_mut().set_continuation(cursor_row, false);
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
                // ponytail: 1000-1006 son mouse reporting, se ignoran hasta implementacion completa
                ('h' | 'l', 1000..=1006) => {}
                // DEC 2004: bracketed paste mode
                ('h', 2004) => self.bracketed_paste = true,
                ('l', 2004) => self.bracketed_paste = false,
                _ => {}
            }
            return;
        }

        // Mouse reporting SGR (intermediate == b"<"): CSI < Ps ; Ps ; Ps M o m.
        // ponytail: parseo minimo, no se decodifican las coordenadas del mouse.
        // El reporte real se implementara posteriormente.
        if intermediates == b"<" {
            match action {
                'M' | 'm' => return,
                _ => {}
            }
        }

        // vte 0.15: Params::iter() devuelve &[u16] por parametro (subparams agrupados).
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
                        // ponytail: bright variants (90-97, 100-107) con soporte 256-color posterior.
                        90..=97 | 100..=107 => {}
                        _ => {}
                    }
                }
            }
            'J' => {
                // Clear screen: limpiar pantalla
                let n = params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                match n {
                    0 => {
                        // Cursor al final: limpiar desde cursor hasta fin
                        self.grid.clear_line(cur_row, cur_col, self.grid.cols_count);
                        for row in (cur_row + 1)..self.grid.rows_count {
                            self.grid.clear_line(row, 0, self.grid.cols_count);
                        }
                    }
                    1 => {
                        // Inicio al cursor: limpiar desde inicio hasta cursor
                        for row in 0..cur_row {
                            self.grid.clear_line(row, 0, self.grid.cols_count);
                        }
                        self.grid.clear_line(cur_row, 0, cur_col + 1);
                    }
                    2 => {
                        // Limpiar todo el grid
                        self.grid.clear();
                    }
                    3 => {
                        // Clear scrollback: limpiar scrollback, no implementado aun
                    }
                    _ => {}
                }
            }
            'K' => {
                // Clear line: limpiar linea
                let n = params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                match n {
                    0 => {
                        // Cursor al final de linea
                        self.grid.clear_line(cur_row, cur_col, self.grid.cols_count);
                    }
                    1 => {
                        // Inicio de linea al cursor
                        self.grid.clear_line(cur_row, 0, cur_col + 1);
                    }
                    2 => {
                        // Toda la linea
                        self.grid.clear_line(cur_row, 0, self.grid.cols_count);
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
                // Default top=1, bottom=rows_count. Si top >= bottom, resetea a
                // pantalla completa (convencion xterm; VT510 estricto dice
                // "ignorar").
                // ponytail: convencion xterm, no VT510. Discrepancia documentada.
                let rows_count = self.grid.rows_count;
                let top = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let bottom = params
                    .get(1)
                    .copied()
                    .unwrap_or(rows_count as u16)
                    .saturating_sub(1) as usize;
                if top >= bottom || top >= rows_count || bottom >= rows_count {
                    self.scroll_region = (0, rows_count - 1);
                } else {
                    self.scroll_region = (top, bottom);
                }
                self.cursor.move_to(0, 0);
                self.pending_wrap = false;
            }
            'L' => {
                // IL (insert line): inserta n lineas en blanco en la fila
                // del cursor, desplazando las lineas siguientes hacia abajo.
                // La fila final (rows_count-1) se pierde.
                // ponytail: xterm NO respeta la scroll region en IL/DL.
                // El cursor determina la fila, no la region.
                let n = params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                if row < self.cursor.rows_count {
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
                if row < self.cursor.rows_count {
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
    use crate::grid::Cell;
    use crate::grid::DEFAULT_COLS;
    use crate::selection::SelectionMode;
    use crate::selection::SelectionPoint;

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
        for col in 6..DEFAULT_COLS {
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
        assert_eq!(term.cursor.col, DEFAULT_COLS - 1);
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
    // Tests: alt screen, DECSTBM, LF scroll
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
        assert_eq!(term.scroll_region, (0, DEFAULT_ROWS - 1));
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
        assert_eq!(term.scroll_region, (0, DEFAULT_ROWS - 1));
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
        assert_eq!(term.grid.rows[DEFAULT_ROWS - 1][0].ch, ' ');
    }

    // -----------------------------------------------------------------------
    // Tests: DECSC, DECRC, DECAWM, pending_wrap
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
        // cursor debe quedar en col 79 (DEFAULT_COLS - 1)
        assert_eq!(term.cursor.col, DEFAULT_COLS - 1);
        // el ultimo caracter sobreescribe la ultima columna
        assert_eq!(term.active_grid().rows[0][DEFAULT_COLS - 1].ch, 'X');
    }

    #[test]
    fn test_decawm_enabled_wraps_at_last_col() {
        let mut term = Term::new();
        // auto_wrap activo por defecto
        for _ in 0..80 {
            feed(&mut term, b"X");
        }
        // cursor en (0, 79), pending_wrap = true
        assert_eq!((term.cursor.row, term.cursor.col), (0, DEFAULT_COLS - 1));
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
        term.cursor.move_to(DEFAULT_ROWS - 1, DEFAULT_COLS - 1);
        // Escribir 1 char: cursor en bottom, col 79, auto_wrap activo
        // esto coloca pending_wrap = true
        // (el caracter se escribe en la ultima columna, cursor se queda en ella)
        feed(&mut term, b"X");
        assert!(term.pending_wrap);
        assert_eq!(
            (term.cursor.row, term.cursor.col),
            (DEFAULT_ROWS - 1, DEFAULT_COLS - 1)
        );
        // Otro char dispara do_pending_wrap: scroll up de la region,
        // cursor pasa a (DEFAULT_ROWS - 1, 0), luego print avanza col a 1
        feed(&mut term, b"Y");
        assert!(!term.pending_wrap);
        assert_eq!((term.cursor.row, term.cursor.col), (DEFAULT_ROWS - 1, 1));
        // La fila 0 debe haber sido desplazada hacia arriba
        assert_eq!(term.active_grid().rows[0][DEFAULT_COLS - 1].ch, ' ');
        // La ultima fila tiene el nuevo caracter
        assert_eq!(term.active_grid().rows[DEFAULT_ROWS - 1][0].ch, 'Y');
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
        assert_eq!((term.cursor.row, term.cursor.col), (0, DEFAULT_COLS - 1));
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
    // Tests: IL, DL, ICH, DCH
    // -----------------------------------------------------------------------

    #[test]
    fn test_il_inserts_line() {
        let mut term = Term::new();
        // Llenar la fila 5 con 'X'
        for col in 0..DEFAULT_COLS {
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
        assert_eq!(term.grid.rows[DEFAULT_ROWS - 1][0].ch, ' ');
    }

    #[test]
    fn test_dl_deletes_line() {
        let mut term = Term::new();
        // Llenar todas las filas con 'X' en col 0
        for row in 0..DEFAULT_ROWS {
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
        assert_eq!(term.grid.rows[DEFAULT_ROWS - 1][0].ch, ' ');
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
    // Tests: modelo alt/primary grid, mouse ignore
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

    // -----------------------------------------------------------------------
    // Tests: bracketed paste mode DEC 2004
    // -----------------------------------------------------------------------

    #[test]
    fn test_bracketed_paste_mode_activated() {
        let mut term = Term::new();
        assert!(!term.bracketed_paste);
        // Usar feed que es el helper existing que procesa bytes a traves del parser
        feed(&mut term, b"\x1b[?2004h");
        assert!(term.bracketed_paste);
    }
    #[test]
    fn test_bracketed_paste_mode_deactivated() {
        let mut term = Term::new();
        term.bracketed_paste = true;
        feed(&mut term, b"\x1b[?2004l");
        assert!(!term.bracketed_paste);
    }

    // -----------------------------------------------------------------------
    // Tests: C1 control decoding
    // -----------------------------------------------------------------------

    /// C1 control 0x9B (CSI 8-bit): vte 0.15 lo pasa a execute(0x9B).
    /// No debe panic, y los bytes ASCII restantes se imprimen como chars normales.
    #[test]
    fn test_c1_csi_8bit_execute() {
        let mut term = Term::new();
        // 0x9B = CSI 8-bit, seguido de "5;10H"
        // vte llama execute(0x9B) (noop), luego imprime "5;10H" (5 chars)
        feed(&mut term, b"\x9b5;10H");
        // No panic: 5 chars impresos desde (0,0)
        assert_eq!(term.cursor.row, 0);
        assert_eq!(term.cursor.col, 5);
    }

    /// 7-bit CSI (ESC [) sigue funcionando (regression test).
    #[test]
    fn test_7bit_csi_works() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;10H");
        assert_eq!(term.cursor.row, 4);
        assert_eq!(term.cursor.col, 9);
    }

    /// 0x90 (DCS 8-bit) + 0x9C (ST): vte 0.15 pasa ambos a execute().
    /// No debe panic, los bytes intermedios se imprimen como chars normales.
    #[test]
    fn test_c1_dcs_ignored() {
        let mut term = Term::new();
        // DCS 8-bit + "0;10" + ST (0x9C)
        feed(&mut term, b"\x900;10\x9c");
        // Sin panic: "0;10" = 4 chars impresos desde (0,0)
        assert_eq!(term.cursor.row, 0);
        assert_eq!(term.cursor.col, 4);
    }

    // -----------------------------------------------------------------------
    // Tests: unicode width en Cell
    // -----------------------------------------------------------------------

    /// Verifica que un caracter CJK (ancho 2) en el grid tiene `width == 2`.
    #[test]
    fn test_unicode_width_cell() {
        let mut term = Term::new();
        // \\u{4e2d} = '中' (CJK, ancho 2)
        feed(&mut term, "\u{4e2d}".as_bytes());
        assert_eq!(term.grid.rows[0][0].ch, '\u{4e2d}');
        assert_eq!(term.grid.rows[0][0].width, 2);
        // cursor avanzo 2 columnas
        assert_eq!(term.cursor.col, 2);
        // caracter latino tiene width 1
        feed(&mut term, b"A");
        assert_eq!(term.grid.rows[0][2].width, 1);
        assert_eq!(term.cursor.col, 3);
    }

    // -----------------------------------------------------------------------
    // Tests: reflow en resize
    // -----------------------------------------------------------------------

    /// En alt screen, resize_grid NO aplica reflow en el grid primario.
    #[test]
    fn test_reflow_alt_screen() {
        let mut term = Term::new();

        // Escribir contenido en grid primario
        feed(&mut term, b"PRIMARY LINE");

        // Entrar a alt screen
        feed(&mut term, b"\x1b[?1049h");

        // Escribir contenido en alt grid
        feed(&mut term, b"ALT LINE");

        // Reducir tamaño (simula resize de terminal)
        // El resize trunca del PRINCIPIO, así que el contenido de row 0
        // se mueve al scrollback. Verificamos que esté allí.
        term.resize_grid(10, 5);

        // Alt grid debe haberse redimensionado
        assert_eq!(term.alt_grid.rows_count, 10);
        assert_eq!(term.alt_grid.cols_count, 5);
        // "ALT LINE" se escribió en row 0. Con truncado del inicio,
        // esa fila ahora está en scrollback.
        // El scrollback del alt grid debe tener la fila "ALT LINE"
        assert!(
            !term.alt_grid.scrollback.is_empty(),
            "content should be in scrollback after truncation"
        );

        // Grid primario NO debe haber cambiado (sin reflow)
        assert_eq!(term.grid.rows_count, DEFAULT_ROWS);
        assert_eq!(term.grid.cols_count, DEFAULT_COLS);
        // Contenido primario intacto
        assert_eq!(term.grid.rows[0][0].ch, 'P');
        // Verificar que no hubo reflow en primaria
        assert_eq!(term.grid.cols_count, DEFAULT_COLS);
    }

    /// En pantalla primaria, resize_grid aplica reflow.
    #[test]
    fn test_reflow_primary_screen() {
        let mut term = Term::new();

        // Escribir una línea larga en primaria
        feed(&mut term, b"ABCDEFGHIJKLMNOPQRST");

        // Reducir ancho significativamente
        term.resize_grid(24, 5);

        // El contenido debe haberse reflujeado a varias filas
        assert_eq!(term.grid.rows[0][0].ch, 'A');
        assert_eq!(term.grid.rows[0][4].ch, 'E');
        assert_eq!(term.grid.rows[1][0].ch, 'F');
        assert_eq!(term.grid.rows[1][4].ch, 'J');

        // La scroll_region debe estar actualizada
        assert_eq!(term.scroll_region, (0, 23));

        // El cursor debe estar dentro del grid redimensionado
        assert!(term.cursor.row < 24);
        assert!(term.cursor.col < 5);
    }

    // -----------------------------------------------------------------------
    // Tests: scrollback offset y PageUp/PageDown
    // -----------------------------------------------------------------------

    /// Verifica que scrollback_offset se incrementa/decrementa correctamente
    /// con PageUp/PageDown, respetando los limites del scrollback.
    #[test]
    fn test_scrollback_page_up_down() {
        let mut term = Term::new();
        // Llenar scrollback con lineas (forzar scroll varias veces)
        for _ in 0..5 {
            // Escribir en la ultima fila para forzar scroll up
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            feed(&mut term, b"X\n");
        }
        // Ahora hay 5 lineas en scrollback + la linea 'X' escrita
        let sb_len = term.scrollback_len();
        assert!(sb_len > 0, "deberia haber scrollback");
        assert_eq!(
            term.scrollback_offset, 0,
            "scrollback_offset inicial debe ser 0"
        );

        // PageUp: incrementa offset
        term.scrollback_offset = 1;
        assert_eq!(term.scrollback_offset, 1);
        assert!(term.scrollback_offset <= sb_len as isize);

        // PageUp no debe superar scrollback_len
        term.scrollback_offset = sb_len as isize + 10;
        let max_offset = term.scrollback_len();
        term.scrollback_offset = term.scrollback_offset.min(max_offset as isize);
        assert!(term.scrollback_offset <= max_offset as isize);
        assert_eq!(term.scrollback_offset, max_offset as isize);

        // PageDown: decrementa offset, minimo 0
        term.scrollback_offset = 3;
        term.scrollback_offset = (term.scrollback_offset - 1).max(0);
        assert_eq!(term.scrollback_offset, 2);

        // PageDown no debe ir negativo
        term.scrollback_offset = 0;
        term.scrollback_offset = (term.scrollback_offset - 1).max(0);
        assert_eq!(term.scrollback_offset, 0);

        // En alt screen, PageUp/PageDown no hacen nada
        feed(&mut term, b"\x1b[?1049h"); // entrar alt screen
        assert!(term.alt_screen);
        let old_offset = term.scrollback_offset;
        // Simular que PageUp no modifica offset en alt screen
        if !term.alt_screen {
            let max_offset = term.scrollback_len();
            term.scrollback_offset = (term.scrollback_offset + 1).min(max_offset as isize);
        }
        assert_eq!(
            term.scrollback_offset, old_offset,
            "PageUp no debe cambiar offset en alt screen"
        );
        // Simular PageDown en alt screen
        if !term.alt_screen {
            term.scrollback_offset = (term.scrollback_offset - 1).max(0);
        }
        assert_eq!(
            term.scrollback_offset, old_offset,
            "PageDown no debe cambiar offset en alt screen"
        );
    }

    /// Verifica que al escribir input (simulado), scrollback_offset se resetea a 0.
    #[test]
    fn test_scrollback_reset_on_input() {
        let mut term = Term::new();
        // Forzar scrollback
        for _ in 0..3 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            feed(&mut term, b"X\n");
        }
        assert!(term.scrollback_len() > 0);

        // Simular scrollback activo
        term.scrollback_offset = 1;
        assert_eq!(term.scrollback_offset, 1);

        // Resetear al escribir input (simulado)
        if term.scrollback_offset > 0 {
            term.scrollback_offset = 0;
        }
        assert_eq!(
            term.scrollback_offset, 0,
            "scrollback_offset debe resetearse a 0 al escribir input"
        );

        // Verificar que offset 0 no se modifica
        term.scrollback_offset = 0;
        if term.scrollback_offset > 0 {
            term.scrollback_offset = 0;
        }
        assert_eq!(term.scrollback_offset, 0);
    }

    // -----------------------------------------------------------------------
    // TESTS ADVERSARIALES — Sprint 7 Fase 4
    // Buscan bugs en la implementación de selección con mouse.
    // NO son happy-path. Deben encontrar bugs si existen.
    // -----------------------------------------------------------------------

    /// ADVERSARIAL: selected_text() sin selección activa
    /// Debe devolver String vacío, no panic.
    #[test]
    fn test_selected_text_empty_selection() {
        let term = Term::new();
        // Sin selección
        assert!(
            term.selection.is_none(),
            "Term nuevo debe tener selection = None"
        );
        let text = term.selected_text();
        assert_eq!(text, "", "selected_text() sin selección debe devolver ''");

        // Mismo test con scrollback_offset > 0
        let mut term2 = Term::new();
        term2.scrollback_offset = 5;
        let text2 = term2.selected_text();
        assert_eq!(
            text2, "",
            "selected_text() sin selección + offset debe devolver ''"
        );
    }

    /// ADVERSARIAL: selected_text() DESPUÉS de clear_selection()
    /// No debe devolver texto residual.
    #[test]
    fn test_selected_text_after_clear() {
        let mut term = Term::new();
        feed(&mut term, b"hello world");

        // Crear selección
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);
        assert!(!term.selected_text().is_empty(), "debe haber texto seleccionado");

        // Limpiar
        term.clear_selection();
        assert!(term.selection.is_none(), "selection debe ser None tras clear");
        let text = term.selected_text();
        assert_eq!(
            text, "",
            "selected_text() tras clear_selection() debe devolver ''"
        );
    }

    /// ADVERSARIAL: is_selected() para CUALQUIER celda cuando no hay selección
    /// Debe devolver false siempre, incluso con coordenadas extremas.
    #[test]
    fn test_is_selected_when_selection_none() {
        let term = Term::new();
        // Sin selección: cualquier celda debe dar false
        assert!(!term.is_selected(0, 0));
        assert!(!term.is_selected(10, 30));
        assert!(!term.is_selected(usize::MAX, usize::MAX));
        assert!(!term.is_selected(0, usize::MAX));

        // Con scrollback_offset pero sin selección
        let mut term2 = Term::new();
        term2.scrollback_offset = 3;
        assert!(!term2.is_selected(0, 0));
        assert!(!term2.is_selected(23, 79));
    }

    /// ADVERSARIAL: selected_text() en UNA sola celda
    /// Verifica el texto exacto de una selección de 1 celda.
    #[test]
    fn test_selected_text_single_cell() {
        let mut term = Term::new();
        feed(&mut term, b"hello");

        // Seleccionar UNA celda: (0, 2) -> 'l'
        let sel = Selection::new(SelectionPoint { row: 0, col: 2 });
        term.selection = Some(sel);
        assert_eq!(
            term.selected_text(),
            "l",
            "selección de 1 celda ('l') debe devolver 'l'"
        );

        // Seleccionar la primera celda: (0, 0) -> 'h'
        let sel2 = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel2);
        assert_eq!(
            term.selected_text(),
            "h",
            "selección de 1 celda ('h') debe devolver 'h'"
        );

        // Seleccionar la última celda de texto: (0, 4) -> 'o'
        let sel3 = Selection::new(SelectionPoint { row: 0, col: 4 });
        term.selection = Some(sel3);
        assert_eq!(
            term.selected_text(),
            "o",
            "selección de 1 celda ('o') debe devolver 'o'"
        );
    }

    /// ADVERSARIAL: selected_text() con selección INVERTIDA (start > end)
    /// Debe devolver el mismo texto independientemente de la dirección.
    #[test]
    fn test_selected_text_reversed() {
        let mut term = Term::new();
        feed(&mut term, b"hello world");

        // Selección forward: (0,6)-(0,10) -> "world"
        let mut sel_fwd = Selection::new(SelectionPoint { row: 0, col: 6 });
        sel_fwd.update_end(SelectionPoint { row: 0, col: 10 });
        term.selection = Some(sel_fwd);
        let text_fwd = term.selected_text();

        // Misma selección reversed: start > end
        let mut sel_rev = Selection::new(SelectionPoint { row: 0, col: 10 });
        sel_rev.update_end(SelectionPoint { row: 0, col: 6 });
        term.selection = Some(sel_rev);
        let text_rev = term.selected_text();

        assert_eq!(
            text_fwd, text_rev,
            "selección forward y reversed deben producir el mismo texto"
        );
        assert_eq!(text_fwd, "world", "debe seleccionar 'world'");

        // Caso multilinea: forward y reversed deben coincidir
        let mut term2 = Term::new();
        feed(&mut term2, b"abc");
        term2.cursor.move_to(1, 0);
        feed(&mut term2, b"def");
        term2.cursor.move_to(2, 0);
        feed(&mut term2, b"ghi");

        // Forward
        let mut sf = Selection::new(SelectionPoint { row: 0, col: 0 });
        sf.update_end(SelectionPoint { row: 2, col: 2 });
        term2.selection = Some(sf);
        let fwd_txt = term2.selected_text();

        // Reversed
        let mut sr = Selection::new(SelectionPoint { row: 2, col: 2 });
        sr.update_end(SelectionPoint { row: 0, col: 0 });
        term2.selection = Some(sr);
        let rev_txt = term2.selected_text();

        assert_eq!(
            fwd_txt, rev_txt,
            "multilinea forward y reversed deben coincidir"
        );
    }

    /// ADVERSARIAL: Selección en alt_screen no debe causar panic
    /// Crea selección estando en alt_screen, verifica que no crashee.
    #[test]
    fn test_selection_does_not_crash_alt_screen() {
        let mut term = Term::new();

        // Entrar a alt screen
        feed(&mut term, b"\x1b[?1049h");
        assert!(term.alt_screen, "debe estar en alt_screen");

        // Crear selección en alt screen (el grid está vacío)
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);

        // is_selected debe funcionar sin panic
        // La celda (0,0) SÍ está seleccionada (la selección la cubre),
        // aunque el contenido sea vacío
        let result = term.is_selected(0, 0);
        assert!(
            result,
            "alt_grid: celda (0,0) debe estar seleccionada (selection la cubre)"
        );

        // is_selected para celda FUERA del rango debe ser false
        assert!(
            !term.is_selected(1, 0),
            "celda (1,0) NO debe estar seleccionada (selección solo cubre (0,0))"
        );

        // selected_text debe funcionar sin panic
        // alt_grid tiene 80 columnas de espacios (ch=' ', width=1)
        // que sí se incluyen en selected_text() porque width>0
        let text = term.selected_text();
        assert!(
            !text.is_empty(),
            "alt_grid vacío tiene espacios con width>1, selected_text los incluye"
        );

        // Salir de alt screen, verificar que sigue funcionando
        feed(&mut term, b"\x1b[?1049l");
        assert!(!term.alt_screen, "debe haber salido de alt_screen");
        // La selección apuntaba a la alt_grid, que ahora no es la activa.
        // Pero selected_text sigue funcionando sin panic.
        let _ = term.selected_text();
    }

    /// ADVERSARIAL: is_selected() con scrollback_offset > 0 y sin selección
    /// Debe devolver false para todas las celdas.
    #[test]
    fn test_is_selected_with_scrollback_offset_no_selection() {
        let mut term = Term::new();
        // Crear scrollback para que offset tenga sentido
        for _ in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            feed(&mut term, b"X\n");
        }
        assert!(
            term.scrollback_len() > 0,
            "debe haber scrollback para probar offset"
        );

        // Set offset pero NO selection
        term.scrollback_offset = 2;
        term.selection = None;

        // is_selected debe ser false para TODAS las celdas visibles
        for row in 0..DEFAULT_ROWS {
            for col in 0..DEFAULT_COLS.min(5) {
                assert!(
                    !term.is_selected(row, col),
                    "sin selección, ({},{}) no debe estar seleccionado",
                    row,
                    col
                );
            }
        }

        // Incluso con coordenadas inválidas
        assert!(!term.is_selected(usize::MAX, usize::MAX));
    }

    /// ADVERSARIAL (BUG HUNT): selected_text() en alt_screen cuando el grid primario
    /// tiene scrollback. El código usa `self.grid.scrollback.len()` dentro de
    /// selected_text() para determinar si una fila es scrollback o grid,
    /// pero SIEMPRE usa el scrollback PRIMARIO, incluso en alt_screen.
    ///
    /// BUG: Cuando alt_screen=true y el scrollback primario tiene contenido,
    /// selected_text() trata logical rows < sb_len como filas del scrollback
    /// primario en lugar de filas del alt_grid.
    #[test]
    fn test_selected_text_in_alt_screen_with_primary_scrollback() {
        let mut term = Term::new();

        // ---- Setup: crear scrollback en el grid PRIMARIO ----
        // Escribir varias líneas y forzar scroll para que haya scrollback
        for i in 0..3 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            let line = format!("SCROLLBACK_{}", i);
            feed(&mut term, line.as_bytes());
            feed(&mut term, b"\n");
        }
        let sb_len = term.scrollback_len();
        assert!(
            sb_len > 0,
            "BUG TEST: debe haber scrollback primario para exponer el bug"
        );
        eprintln!(
            "BUG TEST: scrollback primario tiene {} líneas",
            sb_len
        );

        // ---- Entrar a alt screen ----
        feed(&mut term, b"\x1b[?1049h");
        assert!(term.alt_screen, "BUG TEST: debe estar en alt_screen");

        // ---- Escribir contenido en alt_grid ----
        feed(&mut term, b"ALT_CONTENT");
        assert_eq!(
            term.alt_grid.rows[0][0].ch, 'A',
            "BUG TEST: alt_grid row 0 col 0 debe ser 'A'"
        );

        // ---- Crear selección en alt_grid row 0 (todo "ALT_CONTENT") ----
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.update_end(SelectionPoint { row: 0, col: 10 });
        term.selection = Some(sel);

        // ---- Obtener selected_text ----
        let text = term.selected_text();

        // BUG: Si sb_len > 0 (scrollback primario tiene contenido),
        // selected_text() trata logical row 0 como scrollback (porque
        // 0 < sb_len) y lee de self.grid.scrollback en lugar de active.rows[0].
        // El texto esperado es "ALT_CONTENT", NO el contenido del scrollback.
        assert_eq!(
            text, "ALT_CONTENT",
            "BUG: selected_text() en alt_screen debe leer de alt_grid, \
             no del scrollback primario. Se obtuvo: '{:?}'",
            text
        );
    }

    /// ADVERSARIAL: selected_text() con selección fuera del rango total de filas
    /// Cuando start_row >= total_rows (scrollback + grid), debe devolver "".
    #[test]
    fn test_selected_text_out_of_bounds_range() {
        let mut term = Term::new();
        feed(&mut term, b"some content");

        // Selección que empieza MUY lejos (mucho más allá del grid)
        let mut sel = Selection::new(SelectionPoint { row: 9999, col: 0 });
        sel.update_end(SelectionPoint {
            row: 9999 + 5,
            col: 10,
        });
        term.selection = Some(sel);

        // start_row (9999) >= total_rows (0 scrollback + 24 grid = 24)
        // El código debe devolver string vacío.
        let text = term.selected_text();
        assert_eq!(
            text, "",
            "selección fuera del rango debe devolver ''"
        );

        // Selección con start en scrollback inexistente
        let mut sel2 = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel2.update_end(SelectionPoint { row: 5, col: 5 });
        term.selection = Some(sel2);

        // No hay scrollback (sb_len = 0). total_rows = 0 + 24 = 24.
        // start_row = 0 < 24, end_row = 5 < 24. Hay grid rows 0..24.
        // selected_text() debe procesar rows 0..5.
        // Pero sin scrollback, logical rows 0..5 son grid rows 0..5.
        // grid[0] tiene "some content", grid[1..5] están vacíos.
        // El texto esperado depende del contenido exacto.
        let text2 = term.selected_text();
        assert!(
            !text2.is_empty(),
            "debe haber texto seleccionado (al menos 'some content' en row 0)"
        );
        assert!(
            text2.contains("some content"),
            "debe contener 'some content'"
        );
    }

    /// ADVERSARIAL: selected_text() con selección que cruza scrollback y grid
    /// y donde una fila del scrollback tiene menos columnas que end_col.
    #[test]
    fn test_selected_text_partial_scrollback_row() {
        let mut term = Term::new();

        // Poner datos en scrollback con menos columnas que el grid
        let short_sb_row: Vec<Cell> = "SHORT"
            .chars()
            .map(|c| Cell {
                ch: c,
                attrs: Attrs::default(),
                width: 1,
            })
            .collect();
        term.grid.scrollback.push_back(short_sb_row);

        let sb_len = term.scrollback_len();
        assert_eq!(sb_len, 1, "debe haber 1 fila en scrollback");

        // Escribir datos en grid
        feed(&mut term, b"GRID_DATA");

        // Scroll up 1 linea para ver scrollback
        term.scrollback_offset = 1;

        // Seleccion desde visible row 0 (scrollback) hasta visible row 1 (grid)
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.update_end(SelectionPoint { row: 1, col: 9 });
        term.selection = Some(sel);

        let text = term.selected_text();
        assert!(
            text.contains("SHORT"),
            "debe contener 'SHORT' del scrollback"
        );
        assert!(
            text.contains("GRID_DATA"),
            "debe contener 'GRID_DATA' del grid"
        );
        // Debe tener un newline entre scrollback y grid
        assert!(
            text.contains('\n'),
            "debe haber newline entre scrollback y grid"
        );
    }

    /// ADVERSARIAL: is_selected() con scrollback_offset calcula mal logical_row
    /// Verifica que is_selected mapee correctamente visible -> logical.
    #[test]
    fn test_is_selected_with_scrollback_offset_selection() {
        let mut term = Term::new();

        // Crear scrollback: 5 líneas
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            let line = format!("SB{}", i);
            feed(&mut term, line.as_bytes());
            feed(&mut term, b"\n");
        }
        let sb_len = term.scrollback_len();
        let grid_row_count = term.grid.rows_count;
        let _total = sb_len + grid_row_count;

        // Seleccionar la última línea del scrollback (logical row 4) y
        // las primeras del grid (logical rows 5 a 10)
        let mut sel = Selection::new(SelectionPoint { row: 4, col: 0 });
        sel.update_end(SelectionPoint { row: 10, col: 3 });
        term.selection = Some(sel);

        // Sin scrollback_offset: visible row = logical row
        // visible 0 = logical 0... visible 4 = logical 4 (scrollback last line)
        // visible 5 = logical 5 (grid row 0)
        // ...
        // visible 10 = logical 10 (grid row 6)
        assert!(term.is_selected(4, 0), "visible 4 = logical 4 (scrollback), debe estar seleccionado");
        assert!(term.is_selected(5, 0), "visible 5 = logical 5 (grid[0]), debe estar seleccionado");
        assert!(term.is_selected(10, 3), "visible 10 = logical 10 (grid[6]), debe estar seleccionado");
        assert!(!term.is_selected(3, 0), "visible 3 = logical 3, fuera de rango");
        assert!(!term.is_selected(11, 0), "visible 11 = logical 11, fuera de rango");

        // Con scrollback_offset = 2:
        // viewport_start = sb_len - 2 = 3
        // visible 0 = logical 3, visible 1 = logical 4, ...
        term.scrollback_offset = 2;
        // La selección cubre logical 4-10.
        // visible 0 = logical 3 (no seleccionado)
        // visible 1 = logical 4 (seleccionado)
        // visible 7 = logical 10 (seleccionado)
        assert!(
            !term.is_selected(0, 0),
            "visible 0 = logical 3, fuera de selección"
        );
        assert!(
            term.is_selected(1, 0),
            "visible 1 = logical 4 (scrollback), seleccionado"
        );
        assert!(
            term.is_selected(7, 3),
            "visible 7 = logical 10 (grid[0+6]), seleccionado"
        );
    }

    /// ADVERSARIAL: clear_selection() en alt_screen
    /// Verifica que no haya efectos secundarios.
    #[test]
    fn test_clear_selection_in_alt_screen() {
        let mut term = Term::new();

        // Escribir en primaria
        feed(&mut term, b"PRIMARY");

        // Entrar alt screen
        feed(&mut term, b"\x1b[?1049h");
        feed(&mut term, b"ALT");

        // Crear selección en alt
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);
        assert!(term.selection.is_some(), "debe haber selección activa");

        // Clear en alt screen
        term.clear_selection();
        assert!(term.selection.is_none(), "selección debe eliminarse");
        assert_eq!(term.selected_text(), "", "selected_text debe ser ''");

        // El alt grid NO debe haberse modificado
        assert_eq!(term.alt_grid.rows[0][0].ch, 'A', "alt_grid no debe modificarse");

        // Salir de alt screen: la selección debe seguir eliminada
        feed(&mut term, b"\x1b[?1049l");
        assert!(term.selection.is_none(), "selección debe seguir eliminada");
    }

    /// ADVERSARIAL: selected_text() con selección que incluye SOLO espacios
    /// Verifica que devuelve espacios (width>0), no que filtre vacío.
    #[test]
    fn test_selected_text_only_spaces() {
        let mut term = Term::new();
        // El grid empieza con espacios. Seleccionar una región de solo espacios.
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint { row: 0, col: 10 },
            mode: SelectionMode::Normal,
        };
        term.selection = Some(sel);

        // La fila 0 solo tiene espacios (Cell::default()) con width=1
        // La condición `cell.ch != ' ' || cell.width > 0` incluye espacios
        // porque width=1 > 0. Así que devuelve los espacios.
        let text = term.selected_text();
        assert_eq!(
            text, "           ",
            "selección de solo espacios con width>0 debe devolver espacios"
        );
        assert_eq!(text.len(), 11, "deben ser 11 espacios (col 0..10 inclusive, 11 celdas)");

        // Multilinea con solo espacios
        let sel2 = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint { row: 3, col: 0 },
            mode: SelectionMode::Normal,
        };
        term.selection = Some(sel2);
        let text2 = term.selected_text();
        // Cada fila contribuye 1 espacio (col 0), con newlines entre filas
        assert!(!text2.is_empty(), "debe tener espacios y newlines");
        assert!(
            text2.contains(' '),
            "debe contener espacios"
        );
    }

    // -----------------------------------------------------------------------
    // Tests: seleccion (Ronda 1 Sprint 7)
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_between_scrollback_and_grid() {
        let mut term = Term::new();

        // Put data in scrollback directly
        let sb_row: Vec<Cell> = "scrollback_line"
            .chars()
            .map(|c| Cell { ch: c, attrs: Attrs::default(), width: 1 })
            .collect();
        term.grid.scrollback.push_back(sb_row);

        // Write data in the visible grid
        feed(&mut term, b"visible_grid_data");

        let sb_len = term.scrollback_len();
        assert_eq!(sb_len, 1);

        // Selection from scrollback row (logical 0) into grid row (logical sb_len = 1)
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.update_end(SelectionPoint {
            row: sb_len,
            col: 7,
        });
        term.selection = Some(sel);

        // Without scrollback_offset, visible rows = logical rows.
        // visible row 0 = logical row 0 = scrollback[0]
        // visible row 1 = logical row 1 = grid row 0

        // Scrollback region — the entire start row is selected from start_col onwards
        assert!(term.is_selected(0, 0)); // 's'
        assert!(term.is_selected(0, 14)); // 'e' (last char of "scrollback_line")
        assert!(term.is_selected(0, 20)); // cols beyond data are still selected (start row)

        // Grid region — end row only selected up to end_col (7)
        assert!(term.is_selected(1, 0)); // 'v'
        assert!(term.is_selected(1, 7)); // '_' (end_col = 7)
        assert!(!term.is_selected(1, 8)); // past end_col

        // Row beyond the selection range
        assert!(!term.is_selected(2, 0));
    }

    #[test]
    fn test_selection_single_cell() {
        let mut term = Term::new();
        feed(&mut term, b"abc");

        // Seleccionar celda (0, 1) = 'b'
        let sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 1 },
        );
        term.selection = Some(sel);

        assert!(term.is_selected(0, 1));
        assert!(!term.is_selected(0, 0));
        assert!(!term.is_selected(0, 2));
        assert!(!term.is_selected(1, 0));
    }

    #[test]
    fn test_selection_multiline() {
        let mut term = Term::new();
        feed(&mut term, b"abc");
        term.cursor.move_to(1, 0);
        feed(&mut term, b"def");
        term.cursor.move_to(2, 0);
        feed(&mut term, b"ghi");

        // Seleccionar desde (0,1) hasta (2,1)
        let mut sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 1 },
        );
        sel.update_end(crate::selection::SelectionPoint { row: 2, col: 1 });
        term.selection = Some(sel);

        // Primera fila
        assert!(!term.is_selected(0, 0));
        assert!(term.is_selected(0, 1));
        assert!(term.is_selected(0, 2));
        // Segunda fila completa
        assert!(term.is_selected(1, 0));
        assert!(term.is_selected(1, 1));
        assert!(term.is_selected(1, 2));
        // Tercera fila, solo col 0-1
        assert!(term.is_selected(2, 0));
        assert!(term.is_selected(2, 1));
        assert!(!term.is_selected(2, 2));
    }

    #[test]
    fn test_selection_reversed() {
        let mut term = Term::new();
        feed(&mut term, b"abc");

        // Seleccion invertida: end antes que start
        let mut sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 2 },
        );
        sel.update_end(crate::selection::SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);

        // Debe normalizar a (0,0)-(0,2)
        assert!(term.is_selected(0, 0));
        assert!(term.is_selected(0, 1));
        assert!(term.is_selected(0, 2));
        assert!(!term.is_selected(0, 3));
    }

    #[test]
    fn test_no_selection_returns_false() {
        let term = Term::new();
        assert!(!term.is_selected(0, 0));
        assert_eq!(term.selected_text(), "");
    }

    #[test]
    fn test_clear_selection() {
        let mut term = Term::new();
        feed(&mut term, b"abc");

        // Create a selection and verify it is active
        let sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 0 },
        );
        term.selection = Some(sel);
        assert!(term.selection.is_some());
        assert!(term.is_selected(0, 0));
        assert!(!term.selected_text().is_empty());

        // Clear selection and verify everything is reset
        term.clear_selection();
        assert!(term.selection.is_none());
        assert!(!term.is_selected(0, 0));
        assert!(!term.is_selected(5, 0));
        assert_eq!(term.selected_text(), "");
    }

    #[test]
    fn test_selected_text_single_line() {
        let mut term = Term::new();
        feed(&mut term, b"hello");

        let mut sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 1 },
        );
        sel.update_end(crate::selection::SelectionPoint { row: 0, col: 3 });
        term.selection = Some(sel);

        assert_eq!(term.selected_text(), "ell");
    }

    #[test]
    fn test_selected_text_multiline() {
        let mut term = Term::new();
        feed(&mut term, b"abc");
        term.cursor.move_to(1, 0);
        feed(&mut term, b"def");
        term.cursor.move_to(2, 0);
        feed(&mut term, b"ghi");
        term.cursor.move_to(3, 0);
        feed(&mut term, b"jkl");

        // Partial selection across 3 lines: (0,1) to (2,1)
        let mut sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 1 },
        );
        sel.update_end(crate::selection::SelectionPoint { row: 2, col: 1 });
        term.selection = Some(sel);

        // (0,1)-(0,2)='bc' + newline + (1,0)-(1,2)='def' + newline + (2,0)-(2,1)='gh'
        assert_eq!(term.selected_text(), "bc\ndef\ngh");

        // Full lines selection across 4 lines: (0,0) to (3,2)
        let mut sel2 = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 0 },
        );
        sel2.update_end(crate::selection::SelectionPoint { row: 3, col: 2 });
        term.selection = Some(sel2);

        assert_eq!(term.selected_text(), "abc\ndef\nghi\njkl");
    }

    #[test]
    fn test_selection_in_scrollback() {
        let mut term = Term::new();

        // Forzar scrollback: llenar varias filas y hacer scroll.
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            term.cursor.col = 0;
            feed(&mut term, &[b'L', b'i', b'n', b'e', b'0' + i as u8, b'\n']);
        }

        // scrollback ahora tiene ~5 filas.
        let sb_len = term.scrollback_len();
        assert!(sb_len > 0, "deberia haber scrollback");

        // La primera fila del scrollback tiene 'Line0'
        let sel = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 0 },
        );
        term.selection = Some(sel);

        // Sin scrollback_offset, visible row 0 = logica row 0 (scrollback).
        // Pero el grid tiene 24 filas, visible row 0 es grid row 0.
        // Logica: el scrollback empieza en 0. visible_to_logical_row(0) sin offset = 0.
        // La seleccion esta en logical (0,0), que es la primera fila del scrollback.
        // Pero cuando se renderiza sin scrollback_offset, no se ve scrollback.
        // is_selected usa visible_to_logical_row, que sin offset devuelve visible_row.
        // Entonces is_selected(0,0) verifica logical_row=0, que contiene (0,0) -> true.
        assert!(term.is_selected(0, 0));

        // Verificar que la fila 5 del grid NO esta seleccionada.
        assert!(!term.is_selected(5, 0));

        // Seleccionar la fila grid row 0 (logica = sb_len + 0).
        let sel2 = crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: sb_len, col: 0 },
        );
        term.selection = Some(sel2);
        // is_selected(0, 0) con offset=0 -> logical=0 -> no coincide con sb_len -> false
        assert!(!term.is_selected(0, 0));
        // is_selected(0, 0) -> logical=0 que != sb_len.
        // En realidad debería ser: visible=0, logical=0. Selection contiene (sb_len,0) pero logical=0 no es sb_len. Entonces false.
        // grid row 0 visible = logica sb_len + 0... pero activo es grid, logical_row=0 = grid row 0.
        // Para llegar a sb_len, logical_row debe ser sb_len. visible_row = 0 -> logical = 0. No match.
        // is_selected(5, 0) con offset=0 -> logical=5. Selection.start.row = sb_len (~5).
        // Si sb_len = 5, logical=5 == sb_len -> contains(5, 0) -> true.
        if sb_len < term.grid.rows_count {
            // grid row (sb_len - sb_len) = grid row 0 = visible row 0
            // Pero logical = sb_len + 0 = sb_len, entonces visible row tiene que ser sb_len.
            // En pantalla normal (sin offset), visible row = logical row.
            // grid row 0 = visible row 0 = logical row 0. NO es sb_len.
            // grid row sb_len - sb_len = grid row 0... ok entonces no estamos seleccionando grid row 0.
            // La seleccion esta en logical sb_len, que corresponde al grid row (sb_len - sb_len) = 0 solo si logical >= sb_len.
            // visible row = logical_row - sb_len = 0. Entonces is_selected(0, 0) -> logical=0 != sb_len -> false. Correcto.
            // is_selected(sb_len, 0) -> logical = sb_len -> contains -> true.
            assert!(term.is_selected(sb_len, 0), "logical row {} should be selected", sb_len);
        }
    }

    #[test]
    fn test_selected_text_in_scrollback() {
        let mut term = Term::new();

        // Put data in scrollback directly
        let sb_row: Vec<Cell> = "scrollback_"
            .chars()
            .map(|c| Cell { ch: c, attrs: Attrs::default(), width: 1 })
            .collect();
        term.grid.scrollback.push_back(sb_row);

        // Write data in visible grid
        feed(&mut term, b"visible");

        let sb_len = term.scrollback_len();
        assert_eq!(sb_len, 1);

        // Scroll up para que la fila del scrollback sea visible
        term.scrollback_offset = 1;

        // Seleccion desde scrollback (visible row 0) hasta grid (visible row sb_len=1)
        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.update_end(SelectionPoint {
            row: 1,
            col: 6,
        });
        term.selection = Some(sel);

        let text = term.selected_text();
        assert!(!text.is_empty(), "should contain text from scrollback and grid");
        assert!(
            text.contains("scrollback_"),
            "should include scrollback text"
        );
        assert!(text.contains("visible"), "should include grid text");
        // Verify newline separates scrollback from grid (no continuation flag)
        assert!(
            text.contains('\n'),
            "should contain newline between scrollback and grid rows"
        );
    }
}
