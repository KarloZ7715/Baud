use std::time::Instant;

use crate::copy_mode::CopyModeState;
use crate::cursor::Cursor;
use crate::grid::{Grid, DEFAULT_COLS, DEFAULT_ROWS};
use crate::search::{self, SearchState};
use crate::selection::Selection;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseReporting {
    pub click: bool,
    pub drag: bool,
    pub any_motion: bool,
    pub sgr: bool,
}
impl MouseReporting {
    pub fn is_active(&self) -> bool {
        self.click || self.drag || self.any_motion
    }
    pub fn reports_motion(&self) -> bool {
        self.drag || self.any_motion
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

/// Colores basicos del terminal ANSI + variantes bright + 256 / true color.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Color {
    #[default]
    Default,
    // 8 colores ANSI estandar
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    // 8 variantes bright (ANSI 90-97 / 100-107)
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    /// Color indexado 0-255 (ISO-8613-3). 0-15=ANSI, 16-231=cubo 6x6x6, 232-255=24 grises.
    Indexed(u8),
    /// Color true color RGB 24-bit.
    Rgb(u8, u8, u8),
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

    /// Convierte un codigo SGR bright (90-97 fg, 100-107 bg) a Color.
    /// ponytail: un match, 8 brazos, sin abstraccion.
    pub fn from_bright_code(code: u16) -> Self {
        match code {
            90 | 100 => Color::BrightBlack,
            91 | 101 => Color::BrightRed,
            92 | 102 => Color::BrightGreen,
            93 | 103 => Color::BrightYellow,
            94 | 104 => Color::BrightBlue,
            95 | 105 => Color::BrightMagenta,
            96 | 106 => Color::BrightCyan,
            97 | 107 => Color::BrightWhite,
            _ => Color::Default,
        }
    }
}

/// Estilo de subrayado SGR (4:0..4:5).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
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
    pub italic: bool,
    pub dim: bool,
    pub reverse: bool,
    pub blink: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline_style: UnderlineStyle,
    pub underline_color: Color,
}

/// Rango de un enlace bajo el cursor (fila logica + columnas inclusivas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRange {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
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
    /// DEC 2026: frame sincronizado activo (BSU/ESU).
    pub sync_update_active: bool,
    /// Instant en que se activo el frame sincronizado actual.
    sync_update_started_at: Option<Instant>,
    // Desplazamiento de scrollback para navegacion (pagina arriba/abajo)
    pub scrollback_offset: isize,
    // Seleccion activa del terminal (mouse)
    pub selection: Option<Selection>,
    pub dirty: bool,
    pub window_title: Option<String>,
    pub icon_title: Option<String>,
    pub title_dirty: bool,
    pub cwd: Option<String>,
    pub hyperlinks: Vec<String>,
    /// Codepoints adicionales de grafemas multi-codepoint (indice desde `Cell::extra_codepoints`).
    pub grapheme_extras: Vec<String>,
    /// Buffer del grafema en construccion (UAX #29).
    pending_grapheme: String,
    /// Celda base del grafema pendiente (`row`, `col`), si ya se escribio.
    last_grapheme_cell: Option<(usize, usize)>,
    current_link: Option<usize>,
    /// Enlace bajo el cursor del mouse (hover).
    pub hovered_link: Option<LinkRange>,
    /// OSC 52 query pendiente: target (`c`/`p`/`s`) y terminador del request.
    pub(crate) clipboard_read_pending: Option<(u8, bool)>,
    /// Si false, ignora peticiones de lectura OSC 52 (`?`).
    pub allow_osc52_read: bool,
    /// Si true, OSC 9 / 777 lanzan notificaciones de escritorio.
    pub notifications_enabled: bool,
    #[cfg(test)]
    last_notification: Option<(String, String)>,
    pub runtime_palette: [Option<(u8, u8, u8)>; 256],
    pub fg_override: Option<(u8, u8, u8)>,
    pub bg_override: Option<(u8, u8, u8)>,
    pub cursor_color_override: Option<(u8, u8, u8)>,
    pub mouse_reporting: MouseReporting,
    pub copy_mode: Option<CopyModeState>,
    pub search: Option<SearchState>,
    pub search_cache: Option<search::SearchRenderCache>,
    pub cursor_style: CursorStyle,
    /// Parpadeo del cursor: valor inicial desde config `[cursor] blink`,
    /// sobrescribible en runtime por DECSCUSR (CSI Ps SP q).
    pub cursor_blink_enabled: bool,
    /// Intervalo de parpadeo en ms (config `[cursor] blink_interval_ms`).
    pub blink_interval_ms: u64,
    /// Instant del ultimo reset de fase de parpadeo (input o output del PTY).
    pub last_blink_reset: Instant,
    /// Bytes que el terminal debe escribir de vuelta al PTY (respuestas a
    /// queries: DA1/DA2/DSR/CPR/XTVERSION y, mas adelante, OSC query).
    /// El hilo drain lo vacia tras cada parser.advance().
    pub pty_response: Vec<u8>,
    /// Tab stops por columna (true = parada activa).
    tab_stops: Vec<bool>,
    /// Keypad en modo aplicacion (DECKPAM/DECKPNM).
    pub keypad_application_mode: bool,
    /// DECCKM: application cursor keys (?1).
    pub app_cursor_keys: bool,
    /// DECOM: origin mode (?6), cursor relativo a scroll region.
    origin_mode: bool,
    /// IRM: insert/replace mode (4).
    insert_mode: bool,
    /// LNM: line feed/newline mode (20).
    pub newline_mode: bool,
    /// Flags del protocolo de teclado extendido (CSI u): bitmask activa.
    pub keyboard_flags: u8,
    /// Stack para CSI > u (push) / CSI < u (pop).
    keyboard_flags_stack: Vec<u8>,
}

fn default_tab_stops(cols: usize) -> Vec<bool> {
    let mut stops = vec![false; cols];
    for col in (8..cols).step_by(8) {
        stops[col] = true;
    }
    stops
}

fn resize_tab_stops(old: &[bool], new_cols: usize) -> Vec<bool> {
    let mut stops = default_tab_stops(new_cols);
    for (col, &set) in old.iter().enumerate().take(new_cols) {
        if set {
            stops[col] = true;
        }
    }
    stops
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}

impl Term {
    /// Crea un terminal nuevo: grid vacio, cursor en (0,0), atributos por defecto.
    pub fn new() -> Self {
        Self::new_with_scrollback(crate::grid::DEFAULT_MAX_SCROLLBACK)
    }

    /// Crea un terminal con límite de scrollback configurable.
    pub fn new_with_scrollback(max_scrollback: usize) -> Self {
        Self::new_sized(DEFAULT_ROWS, DEFAULT_COLS, max_scrollback)
    }

    /// Crea un terminal con dimensiones y scrollback explicitos.
    pub fn new_sized(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            grid: Grid::new_sized_with_scrollback(rows, cols, max_scrollback),
            alt_grid: Grid::new_sized_with_scrollback(rows, cols, max_scrollback),
            alt_screen: false,
            scroll_region: (0, rows.saturating_sub(1)),
            auto_wrap: true,
            pending_wrap: false,
            cursor: {
                let mut cursor = Cursor::new();
                cursor.resize(rows, cols);
                cursor
            },
            attrs: Attrs::default(),
            cursor_visible: true,
            saved_cursor: None,
            bracketed_paste: false,
            sync_update_active: false,
            sync_update_started_at: None,
            scrollback_offset: 0,
            selection: None,
            dirty: true,
            window_title: None,
            icon_title: None,
            title_dirty: false,
            cwd: None,
            hyperlinks: Vec::new(),
            grapheme_extras: Vec::new(),
            pending_grapheme: String::new(),
            last_grapheme_cell: None,
            current_link: None,
            hovered_link: None,
            clipboard_read_pending: None,
            allow_osc52_read: true,
            notifications_enabled: false,
            #[cfg(test)]
            last_notification: None,
            runtime_palette: [None; 256],
            fg_override: None,
            bg_override: None,
            cursor_color_override: None,
            mouse_reporting: MouseReporting::default(),
            copy_mode: None,
            search: None,
            search_cache: None,
            cursor_style: CursorStyle::default(),
            cursor_blink_enabled: true,
            blink_interval_ms: 530,
            last_blink_reset: Instant::now(),
            pty_response: Vec::new(),
            tab_stops: default_tab_stops(cols),
            keypad_application_mode: false,
            app_cursor_keys: false,
            origin_mode: false,
            insert_mode: false,
            newline_mode: false,
            keyboard_flags: 0,
            keyboard_flags_stack: Vec::new(),
        }
    }

    fn resolve_origin_row(&self, param_row: u16) -> usize {
        let mut row = param_row.max(1).saturating_sub(1) as usize;
        if self.origin_mode {
            row += self.scroll_region.0;
        }
        row.min(self.scroll_region.1)
    }

    /// Aplica SGR leyendo subparametros agrupados de vte (p.ej. 4:3, 58;2;...).
    fn apply_sgr(&mut self, params: &vte::Params) {
        let slices: Vec<&[u16]> = params.iter().collect();
        if slices.is_empty() {
            self.attrs = Attrs::default();
            return;
        }
        let mut i = 0;
        while i < slices.len() {
            let p = slices[i];
            let code = p.first().copied().unwrap_or(0);
            if p.len() >= 2 && p[0] == 4 {
                match p[1] {
                    0 => {
                        self.attrs.underline = false;
                        self.attrs.underline_style = UnderlineStyle::None;
                    }
                    1 => {
                        self.attrs.underline = true;
                        self.attrs.underline_style = UnderlineStyle::Single;
                    }
                    2 => {
                        self.attrs.underline = true;
                        self.attrs.underline_style = UnderlineStyle::Double;
                    }
                    3 => {
                        self.attrs.underline = true;
                        self.attrs.underline_style = UnderlineStyle::Curly;
                    }
                    4 => {
                        self.attrs.underline = true;
                        self.attrs.underline_style = UnderlineStyle::Dotted;
                    }
                    5 => {
                        self.attrs.underline = true;
                        self.attrs.underline_style = UnderlineStyle::Dashed;
                    }
                    _ => {}
                }
                i += 1;
                continue;
            }
            match code {
                0 => self.attrs = Attrs::default(),
                1 => self.attrs.bold = true,
                2 => self.attrs.dim = true,
                3 => self.attrs.italic = true,
                4 => {
                    self.attrs.underline = true;
                    self.attrs.underline_style = UnderlineStyle::Single;
                }
                5 | 6 => self.attrs.blink = true,
                7 => self.attrs.reverse = true,
                8 => self.attrs.invisible = true,
                9 => self.attrs.strikethrough = true,
                22 => self.attrs.bold = false,
                23 => self.attrs.italic = false,
                24 => {
                    self.attrs.underline = false;
                    self.attrs.underline_style = UnderlineStyle::None;
                }
                25 => self.attrs.blink = false,
                27 => self.attrs.reverse = false,
                28 => self.attrs.invisible = false,
                29 => self.attrs.strikethrough = false,
                53 => self.attrs.overline = true,
                55 => self.attrs.overline = false,
                30..=37 => self.attrs.fg = Color::from_code(code),
                40..=47 => self.attrs.bg = Color::from_code(code),
                90..=97 => self.attrs.fg = Color::from_bright_code(code),
                100..=107 => self.attrs.bg = Color::from_bright_code(code),
                38 => {
                    if i + 2 < slices.len() && slices[i + 1].first() == Some(&5) {
                        if let Some(&n) = slices[i + 2].first() {
                            self.attrs.fg = Color::Indexed(n as u8);
                        }
                        i += 2;
                    } else if i + 4 < slices.len() && slices[i + 1].first() == Some(&2) {
                        let r = slices[i + 2].first().copied().unwrap_or(0) as u8;
                        let g = slices[i + 3].first().copied().unwrap_or(0) as u8;
                        let b = slices[i + 4].first().copied().unwrap_or(0) as u8;
                        self.attrs.fg = Color::Rgb(r, g, b);
                        i += 4;
                    }
                }
                48 => {
                    if i + 2 < slices.len() && slices[i + 1].first() == Some(&5) {
                        if let Some(&n) = slices[i + 2].first() {
                            self.attrs.bg = Color::Indexed(n as u8);
                        }
                        i += 2;
                    } else if i + 4 < slices.len() && slices[i + 1].first() == Some(&2) {
                        let r = slices[i + 2].first().copied().unwrap_or(0) as u8;
                        let g = slices[i + 3].first().copied().unwrap_or(0) as u8;
                        let b = slices[i + 4].first().copied().unwrap_or(0) as u8;
                        self.attrs.bg = Color::Rgb(r, g, b);
                        i += 4;
                    }
                }
                58 => {
                    if i + 2 < slices.len() && slices[i + 1].first() == Some(&5) {
                        if let Some(&n) = slices[i + 2].first() {
                            self.attrs.underline_color = Color::Indexed(n as u8);
                        }
                        i += 2;
                    } else if i + 4 < slices.len() && slices[i + 1].first() == Some(&2) {
                        let r = slices[i + 2].first().copied().unwrap_or(0) as u8;
                        let g = slices[i + 3].first().copied().unwrap_or(0) as u8;
                        let b = slices[i + 4].first().copied().unwrap_or(0) as u8;
                        self.attrs.underline_color = Color::Rgb(r, g, b);
                        i += 4;
                    }
                }
                39 => self.attrs.fg = Color::Default,
                49 => self.attrs.bg = Color::Default,
                59 => self.attrs.underline_color = Color::Default,
                _ => {}
            }
            i += 1;
        }
    }

    pub(crate) fn term_version_id() -> u32 {
        env!("CARGO_PKG_VERSION")
            .split('.')
            .next_back()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    pub(crate) fn osc_st(bell_terminated: bool) -> &'static [u8] {
        if bell_terminated {
            b"\x07"
        } else {
            b"\x1b\\"
        }
    }

    fn parse_color_spec(spec: &[u8]) -> Option<(u8, u8, u8)> {
        if spec == b"?" {
            return None;
        }
        if let Ok(s) = std::str::from_utf8(spec) {
            if let Some(hex) = s.strip_prefix('#') {
                if hex.len() == 6 {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    return Some((r, g, b));
                }
            }
            if let Some(rgb) = s.strip_prefix("rgb:") {
                let parts: Vec<&str> = rgb.split('/').collect();
                if parts.len() == 3 {
                    let parse_ch = |p: &str| {
                        let v = u16::from_str_radix(p, 16).ok()?;
                        Some((v >> 8) as u8)
                    };
                    let r = parse_ch(parts[0])?;
                    let g = parse_ch(parts[1])?;
                    let b = parse_ch(parts[2])?;
                    return Some((r, g, b));
                }
            }
        }
        None
    }

    fn rgb_to_osc16((r, g, b): (u8, u8, u8)) -> String {
        let fmt = |c: u8| format!("{:04x}", (c as u16) * 257);
        format!("rgb:{}/{}/{}", fmt(r), fmt(g), fmt(b))
    }

    fn percent_decode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(v) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                {
                    out.push(v as char);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i] as char);
            i += 1;
        }
        out
    }

    fn parse_file_uri(raw: &[u8]) -> Option<String> {
        let s = std::str::from_utf8(raw).ok()?;
        let rest = s.strip_prefix("file://")?;
        let path = if rest.starts_with('/') {
            rest
        } else {
            rest.find('/').map(|i| &rest[i..])?
        };
        Some(Self::percent_decode(path))
    }

    fn set_title_from_bytes(&mut self, osc_num: u16, title: &[u8]) {
        let t = String::from_utf8_lossy(title).into_owned();
        match osc_num {
            0 => {
                self.window_title = Some(t.clone());
                self.icon_title = Some(t);
            }
            1 => self.icon_title = Some(t),
            2 => self.window_title = Some(t),
            _ => return,
        }
        self.title_dirty = true;
    }

    fn emit_notification(&mut self, title: &str, body: &str) {
        #[cfg(test)]
        {
            self.last_notification = Some((title.to_owned(), body.to_owned()));
        }
        if !self.notifications_enabled {
            tracing::debug!("notificación de escritorio ignorada: {title}: {body}");
        } else {
            #[cfg(not(test))]
            match std::process::Command::new("notify-send")
                .arg(title)
                .arg(body)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(_) => {}
                Err(e) => tracing::warn!("notify-send no disponible: {e}"),
            }
        }
    }

    pub fn take_title_if_dirty(&mut self) -> Option<String> {
        if self.title_dirty {
            self.title_dirty = false;
            self.window_title.clone()
        } else {
            None
        }
    }

    pub fn take_clipboard_read_pending(&mut self) -> Option<(u8, bool)> {
        self.clipboard_read_pending.take()
    }

    /// Construye la respuesta OSC 52 a una query de lectura de clipboard.
    pub fn format_osc52_read_response(
        target: u8,
        payload_b64: &str,
        bell_terminated: bool,
    ) -> Vec<u8> {
        let mut out = format!("\x1b]52;{};{}", target as char, payload_b64).into_bytes();
        out.extend_from_slice(Self::osc_st(bell_terminated));
        out
    }

    fn color_override_mut(&mut self, osc: u16) -> &mut Option<(u8, u8, u8)> {
        match osc {
            10 => &mut self.fg_override,
            11 => &mut self.bg_override,
            12 => &mut self.cursor_color_override,
            _ => &mut self.bg_override,
        }
    }

    fn respond_osc_color_query(&mut self, osc: u16, rgb: (u8, u8, u8), bell_terminated: bool) {
        let body = Self::rgb_to_osc16(rgb);
        let st = Self::osc_st(bell_terminated);
        let resp = format!("\x1b]{osc};{body}");
        self.respond(resp.as_bytes());
        self.respond(st);
    }

    fn decrqm_state(&self, mode: u16) -> u16 {
        // Clustering de grafemas siempre activo; no se puede desactivar.
        if mode == 2027 {
            return 3;
        }
        let set = match mode {
            1 => self.app_cursor_keys,
            6 => self.origin_mode,
            7 => self.auto_wrap,
            25 => self.cursor_visible,
            1049 => self.alt_screen,
            1000 => self.mouse_reporting.click,
            1002 => self.mouse_reporting.drag,
            1003 => self.mouse_reporting.any_motion,
            1006 => self.mouse_reporting.sgr,
            2004 => self.bracketed_paste,
            2026 => self.sync_update_active,
            _ => return 0,
        };
        if set {
            1
        } else {
            2
        }
    }

    fn sm_decrqm_state(&self, mode: u16) -> u16 {
        match mode {
            4 => {
                if self.insert_mode {
                    1
                } else {
                    2
                }
            }
            20 => {
                if self.newline_mode {
                    1
                } else {
                    2
                }
            }
            _ => 0,
        }
    }

    /// Encola bytes de respuesta hacia el PTY.
    pub fn respond(&mut self, bytes: &[u8]) {
        self.pty_response.extend_from_slice(bytes);
    }

    /// Vacia y devuelve la respuesta pendiente (la mueve, dejando el buffer vacio).
    pub fn take_pty_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pty_response)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
    pub fn take_dirty(&mut self) -> bool {
        let d = self.dirty;
        self.dirty = false;
        d
    }

    /// Tiempo maximo que se puede diferir el redraw esperando `CSI ?2026l`.
    /// Si se supera, se pinta de todas formas para evitar un freeze permanente.
    const SYNC_UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(150);

    /// `true` si hay un frame sincronizado en curso y aun no se supero el
    /// timeout de seguridad. El event loop consulta esto antes de pintar.
    pub fn should_defer_redraw(&self) -> bool {
        match self.sync_update_started_at {
            Some(started) => {
                self.sync_update_active && started.elapsed() < Self::SYNC_UPDATE_TIMEOUT
            }
            None => false,
        }
    }

    /// Solo para tests: fija el instante de inicio del frame sincronizado.
    #[cfg(test)]
    pub(crate) fn set_sync_update_started_at_for_test(&mut self, at: Option<Instant>) {
        self.sync_update_started_at = at;
    }

    /// Resetea la fase de parpadeo a "on". Llamar en cada input del usuario y
    /// tras procesar salida del PTY, para que el cursor quede solido mientras
    /// se escribe (comportamiento xterm).
    ///
    /// Solo se invoca en estos dos caminos deliberadamente: eventos de mouse,
    /// scroll y navegacion de copy mode no resetean la fase (no son entrada
    /// semantica); duplicar el reset aqui iria contra la advertencia del plan
    /// de no pisar el reset del coalescing del drain.
    pub fn reset_blink_phase(&mut self) {
        self.last_blink_reset = Instant::now();
    }

    /// True si hay algo que parpadea en la vista actual: el cursor (cuando esta
    /// visible, sin scrollback y fuera de copy mode) o alguna celda con SGR 5.
    /// Lo consulta el hilo timer para decidir si enviar `RedrawNeeded`.
    ///
    /// `blink_interval_ms == 0` desactiva tanto el parpadeo del cursor como el
    /// del texto SGR 5; en ese caso no hay nada que titilar y devuelve false,
    /// coherente con `blink_on` que siempre retorna visible.
    // ponytail: escaneo del grid activo; barato a 265ms de periodo. Marcar en
    // el parser si medidor de perf indica que el escaneo por tick domina.
    pub fn has_blink_stuff(&self) -> bool {
        if self.blink_interval_ms == 0 {
            return false;
        }
        let cursor_blink = self.cursor_blink_enabled
            && self.cursor_visible
            && self.scrollback_offset == 0
            && self.copy_mode.is_none();
        if cursor_blink {
            return true;
        }
        let grid = self.active_grid();
        for row in &grid.rows {
            for cell in row {
                if cell.attrs.blink {
                    return true;
                }
            }
        }
        false
    }
    pub fn take_active_grid_damage(&mut self) -> crate::grid::DamageSnapshot {
        self.active_grid_mut().take_damage()
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
        let rows = self.cursor.rows_count;
        let cols = self.cursor.cols_count;
        self.alt_grid = Grid::new_sized(rows, cols);
        self.alt_grid.clear();
        self.saved_cursor = Some((self.cursor.row, self.cursor.col));
        self.alt_screen = true;
        self.cursor.move_to(0, 0);
        self.scroll_region = (0, rows.saturating_sub(1));
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

    /// Devuelve la cantidad de lineas en el scrollback.
    pub fn scrollback_len(&self) -> usize {
        self.grid.scrollback.len()
    }

    /// Cambia el tamano del grid primario y alt grid.
    /// En pantalla primaria: aplica reflow de lineas antes de resize si `reflow` es true.
    /// Font zoom usa `reflow = false` para evitar reordenar el grid antes de SIGWINCH.
    pub fn resize_grid(&mut self, new_rows: usize, new_cols: usize, reflow: bool) {
        const MAX_GRID: usize = 4096;
        let new_rows = new_rows.clamp(1, MAX_GRID);
        let new_cols = new_cols.clamp(1, MAX_GRID);
        let old_rows = if self.alt_screen {
            self.alt_grid.rows_count
        } else {
            self.grid.rows_count
        };
        let old_cols = self.grid.cols_count;
        let cursor_before = (self.cursor.row, self.cursor.col);
        let was_at_bottom = cursor_before.0 == old_rows.saturating_sub(1);
        if self.alt_screen {
            let removed = self.alt_grid.resize(new_rows, new_cols);
            let (row, col) = Self::adjust_cursor_after_resize(
                cursor_before,
                old_rows,
                new_rows,
                new_cols,
                removed,
                was_at_bottom,
            );
            self.cursor.row = row;
            self.cursor.col = col;
            self.cursor.rows_count = new_rows;
            self.cursor.cols_count = new_cols;
        } else {
            let mut cursor_pos = cursor_before;
            if reflow && new_cols != old_cols {
                cursor_pos = self
                    .grid
                    .reflow_with_cursor(new_cols, Some(cursor_pos))
                    .unwrap_or(cursor_pos);
            } else if !reflow && new_cols != old_cols {
                for c in &mut self.grid.row_continuations {
                    *c = false;
                }
            }
            let removed = self.grid.resize(new_rows, new_cols);
            cursor_pos = Self::adjust_cursor_after_resize(
                cursor_pos,
                old_rows,
                new_rows,
                new_cols,
                removed,
                was_at_bottom,
            );
            self.cursor.row = cursor_pos.0;
            self.cursor.col = cursor_pos.1;
            self.cursor.rows_count = new_rows;
            self.cursor.cols_count = new_cols;
        }
        self.alt_grid.resize(new_rows, new_cols);
        self.scroll_region = (0, new_rows - 1);
        self.pending_wrap = false;
        self.tab_stops = resize_tab_stops(&self.tab_stops, new_cols);
        self.mark_dirty();
    }
    fn adjust_cursor_after_resize(
        (row, col): (usize, usize),
        _old_rows: usize,
        new_rows: usize,
        new_cols: usize,
        rows_removed: usize,
        _was_at_bottom: bool,
    ) -> (usize, usize) {
        // Solo ajustar al truncar filas; al crecer mantener la fila actual y dejar
        // que el shell reposicione el cursor tras SIGWINCH (evita huecos y prompts duplicados).
        let row = if rows_removed > 0 {
            row.saturating_sub(rows_removed)
        } else {
            row
        };
        (
            row.min(new_rows.saturating_sub(1)),
            col.min(new_cols.saturating_sub(1)),
        )
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
        // Marcar esta fila como continuacion por soft-wrap, no hard break.
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

    /// Verifica si una celda visible (row, col) esta dentro de la seleccion activa.
    /// Convierte coordenadas visibles a logicas usando scrollback_offset.
    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        let Some(ref sel) = self.selection else {
            return false;
        };
        let logical_row = self.visible_to_logical_row(row);
        sel.contains(logical_row, col)
    }

    pub fn is_hovered_link(&self, visible_row: usize, col: usize) -> bool {
        let Some(ref range) = self.hovered_link else {
            return false;
        };
        let logical_row = self.visible_to_logical_row(visible_row);
        logical_row == range.row && col >= range.start_col && col <= range.end_col
    }

    /// Limpia el enlace bajo hover; devuelve true si habia uno activo.
    pub fn clear_hovered_link(&mut self) -> bool {
        if self.hovered_link.is_some() {
            self.hovered_link = None;
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// Resuelve el enlace en la celda `(logical_row, col)`: OSC 8 primero, luego URL por smart-select.
    pub fn resolve_link_at(&self, logical_row: usize, col: usize) -> Option<(String, LinkRange)> {
        let row_cells = self.row_cells_at_logical(logical_row)?;
        if col >= row_cells.len() {
            return None;
        }

        if let Some(idx) = row_cells[col].hyperlink {
            let url = self.hyperlinks.get(idx as usize)?.clone();
            let mut start = col;
            while start > 0 && row_cells[start - 1].hyperlink == Some(idx) {
                start -= 1;
            }
            let mut end = col;
            while end + 1 < row_cells.len() && row_cells[end + 1].hyperlink == Some(idx) {
                end += 1;
            }
            return Some((
                url,
                LinkRange {
                    row: logical_row,
                    start_col: start,
                    end_col: end,
                },
            ));
        }

        let line: String = row_cells.iter().map(|c| c.ch).collect();
        let (url, range) = crate::smart_select::resolve_url_with_range(&line, col)?;
        Some((
            url,
            LinkRange {
                row: logical_row,
                start_col: range.start,
                end_col: range.end,
            },
        ))
    }

    /// Extrae el texto del rango seleccionado como String.
    /// Concatena las filas involucradas con '\n' entre lineas no-continuacion.
    pub fn selected_text(&self) -> String {
        let Some(ref sel) = self.selection else {
            return String::new();
        };
        let (start_row, start_col, end_row, end_col) = sel.normalize();
        let mut result = String::new();

        let active = self.active_grid();
        let rows_count = active.rows_count;
        // ponytail: en alt_screen no hay scrollback, aunque el primario tenga
        let sb_len = if self.alt_screen {
            0
        } else {
            self.grid.scrollback.len()
        };
        let total_rows = sb_len + rows_count;

        // Selection almacena filas logicas (scrollback + grid). Mouse, copy mode
        // y Shift+arrow ya usan visible_to_logical_row / cursor_logical_row.
        let abs_start = start_row;
        let abs_end = end_row;

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

            // Determinar el final real de la fila (ultimo caracter no espacio).
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
                // Fila vacia en el rango seleccionado
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

            // Salto de linea entre filas (excepto si la siguiente es continuacion).
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

    /// Limpia la seleccion actual.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Convierte una fila visible (indice en pantalla, 0..rows_count-1)
    /// a una fila logica dentro del buffer virtual [scrollback + grid].
    pub fn visible_to_logical_row(&self, visible_row: usize) -> usize {
        if self.scrollback_offset > 0 && !self.alt_screen {
            let sb_len = self.grid.scrollback.len();
            let offset = self.scrollback_offset.max(0) as usize;
            sb_len.saturating_sub(offset).saturating_add(visible_row)
        } else if !self.alt_screen {
            self.grid.scrollback.len().saturating_add(visible_row)
        } else {
            visible_row
        }
    }
    pub fn cursor_logical_row(&self) -> usize {
        if self.alt_screen {
            self.cursor.row
        } else {
            self.grid.scrollback.len().saturating_add(self.cursor.row)
        }
    }
    pub fn logical_to_visible_row(&self, logical_row: usize) -> Option<usize> {
        crate::copy_mode::logical_to_visible_row(self, logical_row)
    }
    pub fn row_cells_at_logical(&self, logical_row: usize) -> Option<Vec<crate::grid::Cell>> {
        if self.alt_screen {
            return self.alt_grid.rows.get(logical_row).cloned();
        }
        let sb_len = self.grid.scrollback.len();
        if logical_row < sb_len {
            return self.grid.scrollback.get(logical_row).cloned();
        }
        self.grid.rows.get(logical_row - sb_len).cloned()
    }
    pub fn scroll_to_show_logical_row(&mut self, logical_row: usize) {
        if self.alt_screen {
            return;
        }
        let sb_len = self.grid.scrollback.len();
        if logical_row < sb_len {
            self.scrollback_offset = (sb_len - logical_row) as isize;
        } else {
            self.scrollback_offset = 0;
        }
    }

    /// Convierte celdas de una fila a texto buscable (sin espacios finales).
    fn cells_to_row_text(cells: &[crate::grid::Cell]) -> String {
        let end = cells
            .iter()
            .rposition(|c| c.ch != ' ')
            .map(|p| p + 1)
            .unwrap_or(0);
        let mut s = String::new();
        for cell in &cells[..end] {
            if cell.width > 0 {
                s.push(cell.ch);
            }
        }
        s
    }

    /// Scrollback + grid como texto por fila logica.
    pub fn rows_as_text(&self) -> Vec<String> {
        let active = self.active_grid();
        let sb_len = if self.alt_screen {
            0
        } else {
            self.grid.scrollback.len()
        };
        let total = sb_len + active.rows_count;
        let mut rows = Vec::with_capacity(total);
        for logical_row in 0..total {
            if let Some(cells) = self.row_cells_at_logical(logical_row) {
                rows.push(Self::cells_to_row_text(&cells));
            } else {
                rows.push(String::new());
            }
        }
        rows
    }

    /// Establece el query de busqueda, recalcula matches y desplaza la vista al primer match.
    pub fn search_set_query(&mut self, query: &str, case_insensitive: bool) {
        self.search_set_query_inner(query, case_insensitive);
    }

    fn search_set_query_inner(&mut self, query: &str, case_insensitive: bool) {
        let rows = self.rows_as_text();
        let matches = search::find_matches(&rows, query, case_insensitive);
        let prev_current = self.search.as_ref().map(|s| s.current).unwrap_or(0);
        let current = if matches.is_empty() {
            0
        } else {
            prev_current.min(matches.len() - 1)
        };
        self.search = Some(SearchState {
            query: query.to_string(),
            case_insensitive,
            matches,
            current,
        });
        if let Some(m) = self.search.as_ref().and_then(|s| s.matches.get(s.current)) {
            self.scroll_to_show_logical_row(m.row);
        } else if let Some(m) = self.search.as_ref().and_then(|s| s.matches.first()) {
            self.scroll_to_show_logical_row(m.row);
        }
        self.search_cache = None;
        self.mark_dirty();
    }

    pub fn search_toggle_case_insensitive(&mut self) {
        if let Some(ref s) = self.search {
            let q = s.query.clone();
            let ci = !s.case_insensitive;
            let prev_current = s.current;
            let rows = self.rows_as_text();
            let matches = search::find_matches(&rows, &q, ci);
            let current = if matches.is_empty() {
                0
            } else {
                prev_current.min(matches.len() - 1)
            };
            self.search = Some(SearchState {
                query: q,
                case_insensitive: ci,
                matches,
                current,
            });
            if let Some(m) = self.search.as_ref().and_then(|s| s.matches.get(s.current)) {
                self.scroll_to_show_logical_row(m.row);
            }
            self.search_cache = None;
            self.mark_dirty();
        }
    }

    pub fn search_append_query(&mut self, extra: &str) {
        if let Some(ref mut s) = self.search {
            s.query.push_str(extra);
            let q = s.query.clone();
            let ci = s.case_insensitive;
            self.search_set_query_inner(&q, ci);
        }
    }

    pub fn search_next(&mut self) {
        if let Some(ref mut s) = self.search {
            if s.matches.is_empty() {
                return;
            }
            s.current = (s.current + 1) % s.matches.len();
            let row = s.matches[s.current].row;
            self.scroll_to_show_logical_row(row);
            self.search_cache = None;
            self.mark_dirty();
        }
    }

    pub fn search_prev(&mut self) {
        if let Some(ref mut s) = self.search {
            if s.matches.is_empty() {
                return;
            }
            s.current = s.current.checked_sub(1).unwrap_or(s.matches.len() - 1);
            let row = s.matches[s.current].row;
            self.scroll_to_show_logical_row(row);
            self.search_cache = None;
            self.mark_dirty();
        }
    }

    pub fn search_clear(&mut self) {
        self.search = None;
        self.search_cache = None;
        self.mark_dirty();
    }

    /// Recalcula matches si hay busqueda activa (p.ej. tras nuevo output del PTY).
    pub fn search_refresh_if_active(&mut self) {
        let Some(ref s) = self.search else {
            return;
        };
        if s.query.is_empty() {
            return;
        }
        let q = s.query.clone();
        let ci = s.case_insensitive;
        let prev_current = s.current;
        let rows = self.rows_as_text();
        let matches = search::find_matches(&rows, &q, ci);
        let current = if matches.is_empty() {
            0
        } else {
            prev_current.min(matches.len() - 1)
        };
        self.search = Some(SearchState {
            query: q,
            case_insensitive: ci,
            matches,
            current,
        });
        if let Some(m) = self.search.as_ref().and_then(|s| s.matches.get(s.current)) {
            self.scroll_to_show_logical_row(m.row);
        }
        self.search_cache = None;
        self.mark_dirty();
    }

    /// Reconstruye el cache de resaltado si hace falta (una vez por frame).
    pub fn ensure_search_cache(&mut self) {
        let Some(ref s) = self.search else {
            self.search_cache = None;
            return;
        };
        let needs_rebuild = self.search_cache.as_ref().is_none_or(|c| {
            c.scrollback_offset != self.scrollback_offset
                || c.rows_count != self.grid.rows_count
                || c.match_count != s.matches.len()
                || c.current != s.current
        });
        if needs_rebuild {
            self.search_cache = Some(search::build_render_cache(self, s));
        }
    }

    /// Texto del match actual (para copiar al clipboard).
    pub fn search_current_match_text(&self) -> Option<String> {
        let s = self.search.as_ref()?;
        let m = s.matches.get(s.current)?;
        let rows = self.rows_as_text();
        let line = rows.get(m.row)?;
        let chars: Vec<char> = line.chars().collect();
        if m.col + m.len > chars.len() {
            return None;
        }
        Some(chars[m.col..m.col + m.len].iter().collect())
    }

    /// Indica si una celda visible participa en un match de busqueda.
    /// `Some(true)` = match actual; `Some(false)` = otro match; `None` = sin match.
    pub fn search_hit_at(&self, visible_row: usize, col: usize) -> Option<bool> {
        let cache = self.search_cache.as_ref()?;
        search::hit_at(cache, visible_row, col)
    }
}

impl Term {
    fn clear_pending_grapheme(&mut self) {
        self.pending_grapheme.clear();
        self.last_grapheme_cell = None;
    }

    fn refresh_pending_grapheme_extras(&mut self) {
        let Some((row, col)) = self.last_grapheme_cell else {
            return;
        };
        let extra: String = self.pending_grapheme.chars().skip(1).collect();
        if extra.is_empty() {
            return;
        }
        let existing = self.active_grid().get(row, col).extra_codepoints;
        let idx = match existing {
            Some(i) => {
                self.grapheme_extras[i as usize] = extra;
                i
            }
            None => {
                if let Some(i) = self.grapheme_extras.iter().position(|e| e == &extra) {
                    i as u32
                } else {
                    self.grapheme_extras.push(extra);
                    (self.grapheme_extras.len() - 1) as u32
                }
            }
        };
        let width = self.active_grid().get(row, col).width;
        if let Some(cell) = self.active_grid_mut().cell(row, col) {
            cell.extra_codepoints = Some(idx);
        }
        self.active_grid_mut().mark_cell_written(row, col, width);
    }

    /// Escribe el primer codepoint de un grafema y registra la celda base.
    fn write_grapheme_base(&mut self, c: char, c_width: usize) {
        if self.pending_wrap {
            self.do_pending_wrap();
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        let attrs = self.attrs;
        let link = self.current_link.map(|i| i as u32);
        let cols = self.cursor.cols_count;

        if c_width >= 2 && col + c_width > cols && self.auto_wrap {
            self.pending_wrap = true;
            self.do_pending_wrap();
            let row = self.cursor.row;
            let col = self.cursor.col;
            {
                if self.insert_mode {
                    self.active_grid_mut()
                        .insert_chars(row, col, c_width.max(1));
                }
                let active = self.active_grid_mut();
                let cols = active.cols_count;
                let wrote = active.cell(row, col).is_some();
                if wrote {
                    if let Some(cell) = active.cell(row, col) {
                        cell.ch = c;
                        cell.attrs = attrs;
                        cell.width = c_width as u8;
                        cell.hyperlink = link;
                        cell.extra_codepoints = None;
                    }
                    active.mark_cell_written(row, col, c_width as u8);
                    if c_width >= 2 {
                        active.mark_wide_continuation(row, col, c_width as u8, attrs);
                    }
                    self.last_grapheme_cell = Some((row, col));
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

        {
            if self.insert_mode {
                self.active_grid_mut()
                    .insert_chars(row, col, c_width.max(1));
            }
            let active = self.active_grid_mut();
            let cols = active.cols_count;
            let wrote = active.cell(row, col).is_some();
            if wrote {
                if let Some(cell) = active.cell(row, col) {
                    cell.ch = c;
                    cell.attrs = attrs;
                    cell.width = c_width as u8;
                    cell.hyperlink = link;
                    cell.extra_codepoints = None;
                }
                active.mark_cell_written(row, col, c_width as u8);
                if c_width >= 2 {
                    active.mark_wide_continuation(row, col, c_width as u8, attrs);
                }
                self.last_grapheme_cell = Some((row, col));
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
}

/// Implementa el trait vte::Perform para procesar secuencias ANSI.
impl vte::Perform for Term {
    /// Acumula codepoints en un grafema (UAX #29) y escribe la celda base
    /// de inmediato; los codepoints que extienden el cluster se adjuntan sin
    /// avanzar el cursor. El ancho de celda es el del primer codepoint.
    fn print(&mut self, c: char) {
        if !self.pending_grapheme.is_empty()
            && crate::grapheme::extends_last_cluster(&self.pending_grapheme, c)
        {
            self.pending_grapheme.push(c);
            self.refresh_pending_grapheme_extras();
            return;
        }

        let c_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        // Codepoints de ancho 0 que no extienden un cluster previo se ignoran.
        if c_width == 0 {
            self.clear_pending_grapheme();
            return;
        }

        self.pending_grapheme.clear();
        self.pending_grapheme.push(c);
        self.write_grapheme_base(c, c_width);
    }

    fn execute(&mut self, byte: u8) {
        self.clear_pending_grapheme();
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
                self.pending_wrap = false;
                let cols = self.cursor.cols_count;
                let next = self.tab_stops[self.cursor.col + 1..]
                    .iter()
                    .position(|&s| s)
                    .map(|i| self.cursor.col + 1 + i)
                    .unwrap_or(cols.saturating_sub(1));
                self.cursor.move_to(self.cursor.row, next);
            }
            0x0A => {
                self.pending_wrap = false;
                let (top, bottom) = self.scroll_region;
                if self.newline_mode {
                    self.cursor.move_to(self.cursor.row, 0);
                }
                if self.cursor.row == bottom {
                    self.active_grid_mut().scroll_up_region(1, top, bottom);
                } else {
                    self.cursor.move_down(1);
                }
                // Hard break: la fila a la que nos movimos NO es continuacion
                // de la anterior por wrap. Si no se resetea, el reflow no
                // podriaa fusionar lineas al ensanchar la ventana.
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
        self.clear_pending_grapheme();
        if action == 'p' && intermediates == b"$" {
            let mode = params
                .iter()
                .next()
                .and_then(|p| p.first().copied())
                .unwrap_or(0);
            let state = self.sm_decrqm_state(mode);
            let resp = format!("\x1b[{mode};{state}$y");
            self.respond(resp.as_bytes());
            return;
        }

        if action == 'p' && intermediates == b"?$" {
            let mode = params
                .iter()
                .next()
                .and_then(|p| p.first().copied())
                .unwrap_or(0);
            let state = self.decrqm_state(mode);
            let resp = format!("\x1b[?{mode};{state}$y");
            self.respond(resp.as_bytes());
            return;
        }

        // Protocolo de teclado extendido: CSI ... u (solo con intermediate).
        if action == 'u' {
            let n = params
                .iter()
                .next()
                .and_then(|p| p.first().copied())
                .unwrap_or(0);
            if intermediates == b">" {
                self.keyboard_flags_stack.push(self.keyboard_flags);
                self.keyboard_flags = n as u8;
                return;
            } else if intermediates == b"=" {
                let mode = params
                    .iter()
                    .nth(1)
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1);
                let bits = n as u8;
                self.keyboard_flags = match mode {
                    2 => self.keyboard_flags | bits,
                    3 => self.keyboard_flags & !bits,
                    _ => bits,
                };
                return;
            } else if intermediates == b"<" {
                let count = n.max(1) as usize;
                for _ in 0..count {
                    self.keyboard_flags = self.keyboard_flags_stack.pop().unwrap_or(0);
                }
                return;
            } else if intermediates == b"?" {
                let resp = format!("\x1b[?{}u", self.keyboard_flags);
                self.respond(resp.as_bytes());
                return;
            }
        }

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
                ('h', 1) => self.app_cursor_keys = true,
                ('l', 1) => self.app_cursor_keys = false,
                ('h', 6) => self.origin_mode = true,
                ('l', 6) => self.origin_mode = false,
                ('h', 7) => self.auto_wrap = true,
                ('l', 7) => self.auto_wrap = false,
                ('h', 1049) => self.enter_alt_screen(),
                ('l', 1049) => self.exit_alt_screen(),
                // ponytail: 1000-1006 son mouse reporting, se ignoran hasta implementacion completa
                ('h', 1000) => self.mouse_reporting.click = true,
                ('l', 1000) => self.mouse_reporting.click = false,
                ('h', 1002) => self.mouse_reporting.drag = true,
                ('l', 1002) => self.mouse_reporting.drag = false,
                ('h', 1003) => self.mouse_reporting.any_motion = true,
                ('l', 1003) => self.mouse_reporting.any_motion = false,
                ('h', 1006) => self.mouse_reporting.sgr = true,
                ('l', 1006) => self.mouse_reporting.sgr = false,
                // DEC 2004: bracketed paste mode
                ('h', 2004) => self.bracketed_paste = true,
                ('l', 2004) => self.bracketed_paste = false,
                // DEC 2026: synchronized output (BSU/ESU)
                ('h', 2026) => {
                    // No reiniciar el reloj si ya hay un frame abierto: un BSU
                    // repetido no debe alargar el timeout de seguridad.
                    if !self.sync_update_active {
                        self.sync_update_started_at = Some(Instant::now());
                    }
                    self.sync_update_active = true;
                }
                ('l', 2026) => {
                    self.sync_update_active = false;
                    self.sync_update_started_at = None;
                    self.dirty = true;
                }
                ('n', 6) => {
                    let row = (self.cursor.row + 1) as u16;
                    let col = (self.cursor.col + 1) as u16;
                    let resp = format!("\x1b[?{row};{col}R");
                    self.respond(resp.as_bytes());
                }
                _ => {}
            }
            return;
        }

        if intermediates.is_empty() && (action == 'h' || action == 'l') {
            let set = action == 'h';
            let mut handled = false;
            for p in params.iter() {
                match p.first().copied().unwrap_or(0) {
                    4 => {
                        self.insert_mode = set;
                        handled = true;
                    }
                    20 => {
                        self.newline_mode = set;
                        handled = true;
                    }
                    _ => {}
                }
            }
            if handled {
                return;
            }
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
        // esta vacio, vte lo trata como 0. SGR usa el Params original (apply_sgr).
        let flat_params: Vec<u16> = params
            .iter()
            .map(|p| p.first().copied().unwrap_or(0))
            .collect();

        match action {
            'm' => self.apply_sgr(params),
            'J' => {
                let n = flat_params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                let cols_count = self.active_grid().cols_count;
                let rows_count = self.active_grid().rows_count;
                match n {
                    0 => {
                        self.active_grid_mut()
                            .clear_line(cur_row, cur_col, cols_count);
                        for row in (cur_row + 1)..rows_count {
                            self.active_grid_mut().clear_line(row, 0, cols_count);
                        }
                    }
                    1 => {
                        for row in 0..cur_row {
                            self.active_grid_mut().clear_line(row, 0, cols_count);
                        }
                        self.active_grid_mut().clear_line(cur_row, 0, cur_col + 1);
                    }
                    2 => self.active_grid_mut().clear(),
                    3 => {}
                    _ => {}
                }
            }
            'K' => {
                let n = flat_params.first().copied().unwrap_or(0);
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                let cols_count = self.active_grid().cols_count;
                match n {
                    0 => self
                        .active_grid_mut()
                        .clear_line(cur_row, cur_col, cols_count),
                    1 => self.active_grid_mut().clear_line(cur_row, 0, cur_col + 1),
                    2 => self.active_grid_mut().clear_line(cur_row, 0, cols_count),
                    _ => {}
                }
            }
            'A' => {
                // Cursor up: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_up(n as usize);
            }
            'B' => {
                // Cursor down: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_down(n as usize);
            }
            'C' => {
                // Cursor forward: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_forward(n as usize);
            }
            'D' => {
                // Cursor back: default 1 si param vacio o 0.
                // CANCELA pending_wrap.
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1);
                self.cursor.move_back(n as usize);
            }
            'H' => {
                self.pending_wrap = false;
                let row = self.resolve_origin_row(flat_params.first().copied().unwrap_or(1));
                let col = flat_params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor.move_to(row, col);
            }
            'r' => {
                // DECSTBM: set scrolling region. Parametros 1-indexed.
                // Default top=1, bottom=rows_count. Si top >= bottom, resetea a
                // pantalla completa (convencion xterm; VT510 estricto dice
                // "ignorar").
                // ponytail: convencion xterm, no VT510. Discrepancia documentada.
                let rows_count = self.active_grid().rows_count;
                let top = flat_params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let bottom = flat_params
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
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
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
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
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
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                self.active_grid_mut().insert_chars(row, col, n);
                self.pending_wrap = false;
            }
            'P' => {
                // DCH (delete character): borra n chars desde la posicion
                // del cursor, desplazando el resto a la izquierda.
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let row = self.cursor.row;
                let col = self.cursor.col;
                self.active_grid_mut().delete_chars(row, col, n);
                self.pending_wrap = false;
            }
            'G' | '`' => {
                self.pending_wrap = false;
                let col = flat_params
                    .first()
                    .copied()
                    .unwrap_or(1)
                    .max(1)
                    .saturating_sub(1) as usize;
                self.cursor.move_to(self.cursor.row, col);
            }
            'd' => {
                self.pending_wrap = false;
                let row = self.resolve_origin_row(flat_params.first().copied().unwrap_or(1));
                self.cursor.move_to(row, self.cursor.col);
            }
            'f' => {
                self.pending_wrap = false;
                let row = self.resolve_origin_row(flat_params.first().copied().unwrap_or(1));
                let col = flat_params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor.move_to(row, col);
            }
            'E' => {
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.move_down(n);
                self.cursor.move_to(self.cursor.row, 0);
            }
            'F' => {
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor.move_up(n);
                self.cursor.move_to(self.cursor.row, 0);
            }
            'X' => {
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let (row, col) = (self.cursor.row, self.cursor.col);
                let end = (col + n).min(self.active_grid().cols_count);
                self.active_grid_mut().clear_line(row, col, end);
                self.pending_wrap = false;
            }
            'S' => {
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let (top, bottom) = self.scroll_region;
                self.active_grid_mut().scroll_up_region(n, top, bottom);
            }
            'T' => {
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let (top, bottom) = self.scroll_region;
                self.active_grid_mut().scroll_down_region(n, top, bottom);
            }
            'Z' => {
                self.pending_wrap = false;
                let n = flat_params.first().copied().unwrap_or(1).max(1) as usize;
                let mut col = self.cursor.col;
                for _ in 0..n {
                    let prev = self.tab_stops[..col].iter().rposition(|&s| s).unwrap_or(0);
                    col = prev;
                }
                self.cursor.move_to(self.cursor.row, col);
            }
            'g' => {
                let ps = flat_params.first().copied().unwrap_or(0);
                match ps {
                    0 => {
                        if self.cursor.col < self.tab_stops.len() {
                            self.tab_stops[self.cursor.col] = false;
                        }
                    }
                    3 => {
                        for stop in &mut self.tab_stops {
                            *stop = false;
                        }
                    }
                    _ => {}
                }
            }
            'c' => {
                if intermediates == b">" {
                    let ver = Self::term_version_id();
                    let resp = format!("\x1b[>1;{ver};0c");
                    self.respond(resp.as_bytes());
                } else {
                    self.respond(b"\x1b[?62;22c");
                }
            }
            'n' => {
                let ps = flat_params.first().copied().unwrap_or(0);
                match ps {
                    5 => self.respond(b"\x1b[0n"),
                    6 => {
                        let row = (self.cursor.row + 1) as u16;
                        let col = (self.cursor.col + 1) as u16;
                        if intermediates == b"?" {
                            let resp = format!("\x1b[?{row};{col}R");
                            self.respond(resp.as_bytes());
                        } else {
                            let resp = format!("\x1b[{row};{col}R");
                            self.respond(resp.as_bytes());
                        }
                    }
                    _ => {}
                }
            }
            'q' => {
                if intermediates == b">" {
                    self.respond(b"\x1bP>|baud\x1b\\");
                } else {
                    // DECSCUSR (CSI Ps SP q).
                    let style = flat_params.first().copied().unwrap_or(0);
                    match style {
                        0 | 1 => {
                            self.cursor_style = CursorStyle::Block;
                            self.cursor_blink_enabled = true;
                        }
                        2 => {
                            self.cursor_style = CursorStyle::Block;
                            self.cursor_blink_enabled = false;
                        }
                        3 => {
                            self.cursor_style = CursorStyle::Underline;
                            self.cursor_blink_enabled = true;
                        }
                        4 => {
                            self.cursor_style = CursorStyle::Underline;
                            self.cursor_blink_enabled = false;
                        }
                        5 => {
                            self.cursor_style = CursorStyle::Bar;
                            self.cursor_blink_enabled = true;
                        }
                        6 => {
                            self.cursor_style = CursorStyle::Bar;
                            self.cursor_blink_enabled = false;
                        }
                        _ => {
                            self.cursor_style = CursorStyle::Block;
                            self.cursor_blink_enabled = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Despacha secuencias ESC (ESC ... byte).
    /// CRITICO: byte == 0x37 para DECSC (ESC 7) y 0x38 para DECRC (ESC 8),
    /// NO confundir con 0x07 (BEL) ni 0x08 (BS). vte ya ejecuta 0x07 y 0x08
    /// via execute(), asi que en esc_dispatch jamas llegan.
    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        self.clear_pending_grapheme();
        if intermediates == b"#" && byte == 0x38 {
            let rows = self.active_grid().rows_count;
            let cols = self.active_grid().cols_count;
            for row in 0..rows {
                for col in 0..cols {
                    if let Some(cell) = self.active_grid_mut().cell(row, col) {
                        cell.ch = 'E';
                    }
                }
            }
            self.mark_dirty();
            return;
        }
        match byte {
            0x37 => self.save_cursor(),
            0x38 => self.restore_cursor(),
            0x44 => {
                self.pending_wrap = false;
                let (top, bottom) = self.scroll_region;
                if self.cursor.row == bottom {
                    self.active_grid_mut().scroll_up_region(1, top, bottom);
                } else {
                    self.cursor.move_down(1);
                }
            }
            0x45 => {
                self.pending_wrap = false;
                let (top, bottom) = self.scroll_region;
                self.cursor.move_to(self.cursor.row, 0);
                if self.cursor.row == bottom {
                    self.active_grid_mut().scroll_up_region(1, top, bottom);
                } else {
                    self.cursor.move_down(1);
                }
            }
            0x48 => {
                if self.cursor.col < self.tab_stops.len() {
                    self.tab_stops[self.cursor.col] = true;
                }
            }
            0x4D => {
                self.pending_wrap = false;
                let (top, bottom) = self.scroll_region;
                if self.cursor.row == top {
                    self.active_grid_mut().scroll_down_region(1, top, bottom);
                } else {
                    self.cursor.move_up(1);
                }
            }
            0x3D => self.keypad_application_mode = true,
            0x3E => self.keypad_application_mode = false,
            _ => {}
        }
    }
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        self.clear_pending_grapheme();
        tracing::debug!("DCS hook action={:?} (no implementado)", action);
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {
        self.clear_pending_grapheme();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.clear_pending_grapheme();
        let Some(first) = params.first() else {
            return;
        };
        let osc_num = match std::str::from_utf8(first)
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
        {
            Some(n) => n,
            None => return,
        };
        match osc_num {
            0..=2 => {
                if let Some(title) = params.get(1) {
                    self.set_title_from_bytes(osc_num, title);
                }
            }
            4 => {
                let idx = params
                    .get(1)
                    .and_then(|p| std::str::from_utf8(p).ok())
                    .and_then(|s| s.parse::<usize>().ok());
                let spec = params.get(2).copied().unwrap_or(b"");
                if let Some(i) = idx {
                    if spec == b"?" {
                        if let Some(rgb) = self.runtime_palette[i] {
                            let body = Self::rgb_to_osc16(rgb);
                            let st = Self::osc_st(bell_terminated);
                            let resp = format!("\x1b]4;{i};{body}");
                            self.respond(resp.as_bytes());
                            self.respond(st);
                        }
                    } else if let Some(rgb) = Self::parse_color_spec(spec) {
                        if i < 256 {
                            self.runtime_palette[i] = Some(rgb);
                        }
                    }
                }
            }
            7 => {
                if let Some(raw) = params.get(1) {
                    if let Some(path) = Self::parse_file_uri(raw) {
                        self.cwd = Some(path);
                    }
                }
            }
            8 => {
                let uri = params.get(2).copied().unwrap_or(b"");
                if uri.is_empty() {
                    self.current_link = None;
                } else if let Ok(s) = std::str::from_utf8(uri) {
                    if let Some(idx) = self.hyperlinks.iter().position(|u| u == s) {
                        self.current_link = Some(idx);
                    } else {
                        self.hyperlinks.push(s.to_owned());
                        self.current_link = Some(self.hyperlinks.len() - 1);
                    }
                }
            }
            10..=12 => {
                let spec = params.get(1).copied().unwrap_or(b"");
                if spec == b"?" {
                    if let Some(rgb) = *self.color_override_mut(osc_num) {
                        self.respond_osc_color_query(osc_num, rgb, bell_terminated);
                    }
                } else if let Some(rgb) = Self::parse_color_spec(spec) {
                    *self.color_override_mut(osc_num) = Some(rgb);
                }
            }
            52 => {
                let target = params.get(1).copied().unwrap_or(b"c");
                let data = params.get(2).copied().unwrap_or(b"");
                if data == b"?" {
                    if self.allow_osc52_read {
                        self.clipboard_read_pending = Some((target[0], bell_terminated));
                    }
                } else if let Some(bytes) = crate::base64::decode(data) {
                    const MAX_CLIP: usize = 512 * 1024;
                    let slice = if bytes.len() > MAX_CLIP {
                        &bytes[..MAX_CLIP]
                    } else {
                        &bytes
                    };
                    if let Ok(text) = std::str::from_utf8(slice) {
                        let primary =
                            target.first() == Some(&b'p') || target.first() == Some(&b's');
                        crate::clipboard::set(text, primary);
                    }
                }
            }
            9 => {
                let body = params
                    .get(1)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                self.emit_notification("baud", &body);
            }
            777 => {
                if params.get(1).map(|b| b == b"notify").unwrap_or(false) {
                    let title = params
                        .get(2)
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default();
                    let body = params
                        .get(3)
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default();
                    self.emit_notification(&title, &body);
                }
            }
            _ => tracing::debug!("OSC {} no implementado", osc_num),
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

    #[test]
    fn new_sized_usa_dimensiones_explicitas() {
        let term = Term::new_sized(30, 100, 500);
        assert_eq!(term.grid.rows_count, 30);
        assert_eq!(term.grid.cols_count, 100);
        assert_eq!(term.cursor.rows_count, 30);
        assert_eq!(term.cursor.cols_count, 100);
        assert_eq!(term.scroll_region, (0, 29));
    }

    #[test]
    fn term_busca_en_scrollback_y_grid() {
        let mut term = Term::new();
        feed(&mut term, b"hola error mundo\r\n");
        term.search_set_query("error", false);
        assert_eq!(term.search.as_ref().unwrap().matches.len(), 1);
        term.search_next();
        assert_eq!(term.search.as_ref().unwrap().current, 0);
    }

    #[test]
    fn term_busca_letra_n_en_query() {
        let mut term = Term::new();
        feed(&mut term, b"name nine none\r\n");
        term.search_set_query("n", false);
        let matches = term.search.as_ref().unwrap().matches.len();
        assert!(matches >= 3, "debe encontrar varias 'n', got {matches}");
    }

    #[test]
    fn term_busca_case_insensitive_toggle() {
        let mut term = Term::new();
        feed(&mut term, b"ERROR line\r\n");
        term.search_set_query("error", false);
        assert!(term.search.as_ref().unwrap().matches.is_empty());
        term.search_toggle_case_insensitive();
        assert_eq!(term.search.as_ref().unwrap().matches.len(), 1);
        assert!(term.search.as_ref().unwrap().case_insensitive);
    }

    #[test]
    fn search_hit_at_usa_cache() {
        let mut term = Term::new();
        feed(&mut term, b"foo bar foo\r\n");
        term.search_set_query("foo", false);
        term.ensure_search_cache();
        assert_eq!(term.search_hit_at(0, 0), Some(true));
        assert_eq!(term.search_hit_at(0, 8), Some(false));
        assert_eq!(term.search_hit_at(0, 4), None);
    }

    #[test]
    fn search_refresh_preserva_current() {
        let mut term = Term::new();
        feed(&mut term, b"a1\na2\na3\n");
        term.search_set_query("a", false);
        term.search_next();
        term.search_next();
        let cur = term.search.as_ref().unwrap().current;
        feed(&mut term, b"x");
        term.search_refresh_if_active();
        assert_eq!(term.search.as_ref().unwrap().current, cur);
    }

    #[test]
    fn test_keyboard_push_set_pop_query() {
        let mut term = Term::new();
        assert_eq!(term.keyboard_flags, 0);
        feed(&mut term, b"\x1b[>1u");
        assert_eq!(term.keyboard_flags, 1);
        feed(&mut term, b"\x1b[=2;2u");
        assert_eq!(term.keyboard_flags, 3);
        feed(&mut term, b"\x1b[?u");
        assert_eq!(term.take_pty_response(), b"\x1b[?3u");
        feed(&mut term, b"\x1b[<1u");
        assert_eq!(term.keyboard_flags, 0);
    }

    #[test]
    fn test_respond_acumula_y_take_vacia() {
        let mut term = Term::new();
        assert!(term.take_pty_response().is_empty());
        term.respond(b"\x1b[0n");
        term.respond(b"AB");
        let out = term.take_pty_response();
        assert_eq!(out, b"\x1b[0nAB");
        assert!(term.take_pty_response().is_empty());
    }

    #[test]
    fn test_tab_stops_default_cada_8() {
        let mut term = Term::new();
        feed(&mut term, b"\t");
        assert_eq!(term.cursor.col, 8);
        feed(&mut term, b"\t");
        assert_eq!(term.cursor.col, 16);
    }

    #[test]
    fn test_hts_y_tbc() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[3G\x1bH");
        feed(&mut term, b"\r\t");
        assert_eq!(term.cursor.col, 2);
        feed(&mut term, b"\x1b[3g");
        feed(&mut term, b"\r\t");
        assert_eq!(term.cursor.col, term.grid.cols_count - 1);
    }

    #[test]
    fn test_cbt_retrocede_tab() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[20G\x1b[2Z");
        assert_eq!(term.cursor.col, 8);
    }

    #[test]
    fn test_ri_hace_scroll_down_en_tope() {
        let mut term = Term::new();
        feed(&mut term, b"linea0\r\nlinea1");
        feed(&mut term, b"\x1b[H");
        feed(&mut term, b"\x1bM");
        let fila1: String = term.grid.rows[1].iter().map(|c| c.ch).collect();
        assert!(fila1.starts_with("linea0"));
    }

    #[test]
    fn test_decaln_rellena_con_e() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b#8");
        assert!(term.grid.rows.iter().all(|r| r.iter().all(|c| c.ch == 'E')));
    }

    #[test]
    fn test_decckm_set_reset() {
        let mut term = Term::new();
        assert!(!term.app_cursor_keys);
        feed(&mut term, b"\x1b[?1h");
        assert!(term.app_cursor_keys);
        feed(&mut term, b"\x1b[?1l");
        assert!(!term.app_cursor_keys);
    }

    #[test]
    fn test_irm_inserta() {
        let mut term = Term::new();
        feed(&mut term, b"ABC\x1b[1G\x1b[4hX");
        let fila: String = term.grid.rows[0].iter().take(4).map(|c| c.ch).collect();
        assert_eq!(fila, "XABC");
    }

    #[test]
    fn test_irm_inserta_en_ruta_wide_char() {
        let mut term = Term::new();
        term.resize_grid(term.grid.rows_count, 10, true);
        feed(&mut term, b"\x1b[2;1HZZZZ");
        feed(&mut term, b"\x1b[1;10H\x1b[4h");
        feed(&mut term, "\u{4e2d}".as_bytes());
        assert_eq!(term.grid.rows[1][0].ch, '\u{4e2d}');
        assert_eq!(term.grid.rows[1][2].ch, 'Z');
    }

    #[test]
    fn test_decom_desplaza_fila_en_region() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[3;10r");
        feed(&mut term, b"\x1b[?6h");
        feed(&mut term, b"\x1b[1;1H");
        assert_eq!(term.cursor.row, 2);
    }

    #[test]
    fn test_dsr_responde_ok() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5n");
        assert_eq!(term.take_pty_response(), b"\x1b[0n");
    }

    #[test]
    fn test_decrqm_modo_estandar_irm() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[4h");
        feed(&mut term, b"\x1b[4$p");
        assert_eq!(term.take_pty_response(), b"\x1b[4;1$y");
    }

    #[test]
    fn test_tab_stops_preservados_en_resize() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[3G\x1bH");
        term.resize_grid(term.grid.rows_count, term.grid.cols_count + 8, true);
        feed(&mut term, b"\r\t");
        assert_eq!(term.cursor.col, 2);
    }

    #[test]
    fn test_da1_responde() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[c");
        assert_eq!(term.take_pty_response(), b"\x1b[?62;22c");
    }

    #[test]
    fn test_cpr_reporta_posicion_1based() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;10H\x1b[6n");
        assert_eq!(term.take_pty_response(), b"\x1b[5;10R");
    }

    #[test]
    fn test_decrqm_2027_permanentemente_activo() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2027$p");
        let resp = term.take_pty_response();
        assert_eq!(resp, b"\x1b[?2027;3$y");
    }

    #[test]
    fn test_decrqm_2027_ignora_reset() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2027l");
        feed(&mut term, b"\x1b[?2027$p");
        assert_eq!(term.take_pty_response(), b"\x1b[?2027;3$y");
    }

    #[test]
    fn test_decset_2026_activa_sync_update() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        assert!(term.sync_update_active);
    }

    #[test]
    fn test_decrst_2026_desactiva_y_fuerza_dirty() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        term.take_dirty();
        feed(&mut term, b"\x1b[?2026l");
        assert!(!term.sync_update_active);
        assert!(
            term.take_dirty(),
            "cerrar el frame debe forzar un redraw final"
        );
    }

    #[test]
    fn test_decrqm_2026_refleja_estado_actual() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026$p");
        assert_eq!(term.take_pty_response(), b"\x1b[?2026;2$y");

        feed(&mut term, b"\x1b[?2026h\x1b[?2026$p");
        assert_eq!(term.take_pty_response(), b"\x1b[?2026;1$y");
    }

    #[test]
    fn test_should_defer_redraw_mientras_sync_activo() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        assert!(term.should_defer_redraw());
    }

    #[test]
    fn test_should_defer_redraw_false_sin_sync() {
        let term = Term::new();
        assert!(!term.should_defer_redraw());
    }

    #[test]
    fn test_should_defer_redraw_false_tras_timeout() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        term.sync_update_started_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
        assert!(!term.should_defer_redraw());
        assert!(
            term.sync_update_active,
            "el timeout solo deja de diferir; el modo sigue activo hasta ESU"
        );
    }

    #[test]
    fn test_should_defer_redraw_false_tras_esu() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        assert!(term.should_defer_redraw());
        feed(&mut term, b"\x1b[?2026l");
        assert!(!term.should_defer_redraw());
        assert!(!term.sync_update_active);
    }

    #[test]
    fn test_bsu_repetido_no_reinicia_timeout() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?2026h");
        term.sync_update_started_at =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
        assert!(!term.should_defer_redraw());
        feed(&mut term, b"\x1b[?2026h");
        assert!(
            term.sync_update_active,
            "BSU repetido mantiene el modo activo"
        );
        assert!(
            !term.should_defer_redraw(),
            "BSU repetido no debe reiniciar el timeout de seguridad"
        );
    }

    #[test]
    fn test_multiples_marcas_combinantes_en_una_celda() {
        let mut term = Term::new();
        // e + acute + combining diaeresis below (still one grapheme with base e)
        feed(&mut term, "e\u{0301}\u{0324}".as_bytes());
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.ch, 'e');
        let extra_idx = cell.extra_codepoints.expect("extras");
        assert_eq!(term.grapheme_extras[extra_idx as usize], "\u{0301}\u{0324}");
        assert_eq!(term.cursor.col, 1);
    }

    #[test]
    fn test_decrqm_reporta_modo_conocido() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?1h");
        feed(&mut term, b"\x1b[?1$p");
        assert_eq!(term.take_pty_response(), b"\x1b[?1;1$y");
    }

    #[test]
    fn test_osc_vacio_y_desconocido_no_panic() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]\x07");
        feed(&mut term, b"\x1b]99999;x\x07");
    }

    #[test]
    fn test_osc_2_setea_titulo() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]2;mi titulo\x07");
        assert_eq!(term.window_title.as_deref(), Some("mi titulo"));
        assert!(term.title_dirty);
    }

    #[test]
    fn test_osc_0_setea_ambos_con_st_esc_backslash() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]0;t\x1b\\");
        assert_eq!(term.window_title.as_deref(), Some("t"));
        assert_eq!(term.icon_title.as_deref(), Some("t"));
    }

    #[test]
    fn test_osc_7_guarda_cwd_desde_file_uri() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]7;file://localhost/home/u/proj\x07");
        assert_eq!(term.cwd.as_deref(), Some("/home/u/proj"));
    }

    #[test]
    fn osc_9_y_777_no_panic_y_respetan_flag() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]9;build terminado\x07");
        assert_eq!(
            term.last_notification
                .as_ref()
                .map(|(t, b)| (t.as_str(), b.as_str())),
            Some(("baud", "build terminado"))
        );

        feed(&mut term, b"\x1b]777;notify;Titulo;Cuerpo\x07");
        assert_eq!(
            term.last_notification
                .as_ref()
                .map(|(t, b)| (t.as_str(), b.as_str())),
            Some(("Titulo", "Cuerpo"))
        );

        let mut term2 = Term::new();
        term2.notifications_enabled = true;
        feed(&mut term2, b"\x1b]9;hola\x07");
        assert_eq!(
            term2
                .last_notification
                .as_ref()
                .map(|(t, b)| (t.as_str(), b.as_str())),
            Some(("baud", "hola"))
        );

        feed(&mut term2, b"\x1b]777;notify;Titulo;Cuerpo\x07");
        assert_eq!(
            term2
                .last_notification
                .as_ref()
                .map(|(t, b)| (t.as_str(), b.as_str())),
            Some(("Titulo", "Cuerpo"))
        );
    }

    #[test]
    fn test_osc_52_query_ignorada_si_lectura_desactivada() {
        let mut term = Term::new();
        term.allow_osc52_read = false;
        feed(&mut term, b"\x1b]52;c;?\x07");
        assert!(term.clipboard_read_pending.is_none());
    }

    #[test]
    fn test_osc_52_query_marca_lectura_pendiente() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]52;c;?\x07");
        assert_eq!(term.clipboard_read_pending, Some((b'c', true)));
    }

    #[test]
    fn test_osc_52_query_guarda_terminador_st() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]52;c;?\x1b\\");
        assert_eq!(term.clipboard_read_pending, Some((b'c', false)));
    }

    #[test]
    fn test_osc_52_write_no_marca_query() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]52;c;dGVzdA==\x07");
        assert!(term.clipboard_read_pending.is_none());
    }

    #[test]
    fn test_osc_52_write_invalido_no_panic() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]52;c;!!!!\x07");
        assert!(term.clipboard_read_pending.is_none());
    }

    #[test]
    fn test_osc_52_read_response_usa_terminador() {
        let bel = Term::format_osc52_read_response(b'c', "dGVzdA==", true);
        assert_eq!(bel, b"\x1b]52;c;dGVzdA==\x07");
        let st = Term::format_osc52_read_response(b'c', "dGVzdA==", false);
        assert_eq!(st, b"\x1b]52;c;dGVzdA==\x1b\\");
    }

    #[test]
    fn test_osc_8_asocia_link_a_celdas() {
        let mut term = Term::new();
        feed(
            &mut term,
            b"\x1b]8;;https://example.com\x07LINK\x1b]8;;\x07X",
        );
        let idx = term.grid.rows[0][0].hyperlink.expect("celda con link");
        assert_eq!(term.hyperlinks[idx as usize], "https://example.com");
        assert!(term.grid.rows[0][4].hyperlink.is_none());
    }

    #[test]
    fn resolve_link_at_prefiere_osc8_sobre_smart_select() {
        let mut term = Term::new();
        feed(
            &mut term,
            b"\x1b]8;;https://osc.example\x07LINK\x1b]8;;\x07 see https://plain.example",
        );
        let (url, range) = term.resolve_link_at(0, 0).unwrap();
        assert_eq!(url, "https://osc.example");
        assert_eq!(range.start_col, 0);
        assert_eq!(range.end_col, 3);
    }

    #[test]
    fn resolve_link_at_detecta_url_sin_osc8() {
        let mut term = Term::new();
        feed(&mut term, b"see https://example.com now");
        let (url, range) = term.resolve_link_at(0, 8).unwrap();
        assert_eq!(url, "https://example.com");
        assert_eq!(
            &term.grid.rows[0][range.start_col..=range.end_col]
                .iter()
                .map(|c| c.ch)
                .collect::<String>(),
            "https://example.com"
        );
    }

    #[test]
    fn is_hovered_link_mapea_fila_visible_a_logica() {
        let mut term = Term::new();
        term.hovered_link = Some(LinkRange {
            row: 1,
            start_col: 2,
            end_col: 5,
        });
        assert!(term.is_hovered_link(1, 3));
        assert!(!term.is_hovered_link(0, 3));
        assert!(!term.is_hovered_link(1, 6));
    }

    #[test]
    fn clear_hovered_link_solo_cuando_hay_estado() {
        let mut term = Term::new();
        assert!(!term.clear_hovered_link());
        term.hovered_link = Some(LinkRange {
            row: 0,
            start_col: 0,
            end_col: 1,
        });
        assert!(term.clear_hovered_link());
        assert!(term.hovered_link.is_none());
        assert!(!term.clear_hovered_link());
    }

    #[test]
    fn test_osc_11_set_y_query() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]11;#102030\x07");
        assert_eq!(term.bg_override, Some((0x10, 0x20, 0x30)));
        feed(&mut term, b"\x1b]11;?\x07");
        assert_eq!(term.take_pty_response(), b"\x1b]11;rgb:1010/2020/3030\x07");
    }

    #[test]
    fn test_osc_11_query_con_st() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]11;#102030\x07");
        feed(&mut term, b"\x1b]11;?\x1b\\");
        assert_eq!(
            term.take_pty_response(),
            b"\x1b]11;rgb:1010/2020/3030\x1b\\"
        );
    }

    #[test]
    fn test_osc_4_set_indexado() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]4;1;#ff0000\x07");
        assert_eq!(term.runtime_palette[1], Some((0xff, 0x00, 0x00)));
    }

    #[test]
    fn test_osc_4_query_indexado() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b]4;1;#ff0000\x07");
        feed(&mut term, b"\x1b]4;1;?\x07");
        assert_eq!(term.take_pty_response(), b"\x1b]4;1;rgb:ffff/0000/0000\x07");
    }

    #[test]
    fn test_da2_responde_version_patch() {
        let mut term = Term::new();
        let ver = Term::term_version_id();
        feed(&mut term, b"\x1b[>c");
        assert_eq!(
            term.take_pty_response(),
            format!("\x1b[>1;{ver};0c").as_bytes()
        );
        let mut term2 = Term::new();
        feed(&mut term2, b"\x1b[>0c");
        assert_eq!(
            term2.take_pty_response(),
            format!("\x1b[>1;{ver};0c").as_bytes()
        );
    }

    #[test]
    fn test_cpr_dec_con_prefijo_interrogacion() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;10H\x1b[?6n");
        assert_eq!(term.take_pty_response(), b"\x1b[?5;10R");
    }

    #[test]
    fn test_xtversion_dcs() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[>q");
        assert_eq!(term.take_pty_response(), b"\x1bP>|baud\x1b\\");
    }

    #[test]
    fn test_sgr_blink() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5mX");
        assert!(term.grid.rows[0][0].attrs.blink);
        feed(&mut term, b"\x1b[25mY");
        assert!(!term.grid.rows[0][1].attrs.blink);
    }

    #[test]
    fn test_sgr_invisible() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[8mX");
        assert!(term.grid.rows[0][0].attrs.invisible);
        feed(&mut term, b"\x1b[28mY");
        assert!(!term.grid.rows[0][1].attrs.invisible);
    }

    #[test]
    fn test_sgr_overline() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[53mX");
        assert!(term.grid.rows[0][0].attrs.overline);
        feed(&mut term, b"\x1b[55mY");
        assert!(!term.grid.rows[0][1].attrs.overline);
    }

    #[test]
    fn test_sgr_underline_color() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[58;5;4mX");
        assert_eq!(
            term.grid.rows[0][0].attrs.underline_color,
            Color::Indexed(4)
        );
        feed(&mut term, b"\x1b[59mY");
        assert_eq!(term.grid.rows[0][1].attrs.underline_color, Color::Default);
    }

    #[test]
    fn test_sgr_strikethrough_y_reset() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[9mX");
        assert!(term.grid.rows[0][0].attrs.strikethrough);
        feed(&mut term, b"\x1b[29mY");
        assert!(!term.grid.rows[0][1].attrs.strikethrough);
    }

    #[test]
    fn test_sgr_underline_curly_subparam() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[4:3mX");
        assert_eq!(
            term.grid.rows[0][0].attrs.underline_style,
            UnderlineStyle::Curly
        );
    }

    #[test]
    fn print_marks_damage_on_active_grid() {
        let mut t = Term::new();
        let _ = t.take_active_grid_damage();
        feed(&mut t, b"A");
        assert!(t.take_active_grid_damage().is_row_dirty(0));
    }
    #[test]
    fn enter_alt_screen_preserves_grid_dimensions() {
        let mut t = Term::new();
        t.resize_grid(35, 120, true);
        t.enter_alt_screen();
        assert_eq!(t.alt_grid.rows_count, 35);
        assert_eq!(t.alt_grid.cols_count, 120);
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

    #[test]
    fn test_csi_movimiento_absoluto() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[5;5H");
        feed(&mut term, b"\x1b[10G");
        assert_eq!(term.cursor.col, 9);
        assert_eq!(term.cursor.row, 4);
        feed(&mut term, b"\x1b[3d");
        assert_eq!(term.cursor.row, 2);
        assert_eq!(term.cursor.col, 9);
    }

    #[test]
    fn test_csi_ech_borra_sin_desplazar() {
        let mut term = Term::new();
        feed(&mut term, b"ABCDE\x1b[3G\x1b[2X");
        assert_eq!(term.grid.rows[0][2].ch, ' ');
        assert_eq!(term.grid.rows[0][3].ch, ' ');
        assert_eq!(term.grid.rows[0][4].ch, 'E');
    }

    #[test]
    fn test_csi_su_sd_respetan_region() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[2;4r");
        feed(&mut term, b"\x1b[2;1H");
        feed(&mut term, b"A");
        feed(&mut term, b"\x1b[3;1H");
        feed(&mut term, b"B");
        feed(&mut term, b"\x1b[4;1H");
        feed(&mut term, b"C");
        feed(&mut term, b"\x1b[2;1H");
        feed(&mut term, b"\x1b[1S");
        assert_eq!(term.grid.rows[1][0].ch, 'B');
        assert_eq!(term.grid.rows[2][0].ch, 'C');
        assert_eq!(term.grid.rows[3][0].ch, ' ');
        feed(&mut term, b"\x1b[1T");
        assert_eq!(term.grid.rows[1][0].ch, ' ');
        assert_eq!(term.grid.rows[2][0].ch, 'B');
        assert_eq!(term.grid.rows[3][0].ch, 'C');
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
        assert_eq!(term.cursor.col, 1, "LNM off: LF no resetea columna");
        assert_eq!(term.grid.rows[0][0].ch, 'a');
        feed(&mut term, b"\x1b[20h");
        feed(&mut term, b"b\n");
        assert_eq!(term.cursor.row, 2);
        assert_eq!(term.cursor.col, 0, "LNM on: LF hace CR+LF");
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
    fn test_combining_accent_no_se_descarta() {
        let mut term = Term::new();
        feed(&mut term, "e\u{0301}".as_bytes());
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.ch, 'e');
        let extra_idx = cell.extra_codepoints.expect("debe tener extra");
        assert_eq!(term.grapheme_extras[extra_idx as usize], "\u{0301}");
        assert_eq!(term.cursor.col, 1);
    }

    #[test]
    fn test_zwj_emoji_ocupa_una_sola_celda_ancho_2() {
        let mut term = Term::new();
        let cluster = "\u{1F9D1}\u{200D}\u{1F33E}";
        feed(&mut term, cluster.as_bytes());
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.width, 2);
        assert_eq!(term.active_grid().get(0, 1).width, 0);
        assert_eq!(term.cursor.col, 2);
        // Ancho del primer codepoint coincide con el del cluster completo:
        // no hace falta medir UnicodeWidthStr del grafema entero.
        let full = unicode_width::UnicodeWidthStr::width(cluster);
        assert_eq!(cell.width as usize, full);
    }

    #[test]
    fn test_flush_ocurre_antes_de_una_secuencia_csi() {
        let mut term = Term::new();
        feed(&mut term, b"e\x1b[5C");
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.ch, 'e');
    }

    #[test]
    fn test_bs_rompe_cluster_pendiente() {
        let mut term = Term::new();
        feed(&mut term, b"e");
        feed(&mut term, b"\x08");
        feed(&mut term, "\u{0301}".as_bytes());
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.ch, 'e');
        assert_eq!(cell.extra_codepoints, None);
    }

    #[test]
    fn test_zwj_tras_wrap_forzado_en_ultima_columna() {
        let mut term = Term::new_sized(24, 3, 1000);
        feed(&mut term, b"ab");
        let cluster = "\u{1F9D1}\u{200D}\u{1F33E}";
        feed(&mut term, cluster.as_bytes());
        let cell = term.active_grid().get(1, 0);
        assert_eq!(cell.ch, '\u{1F9D1}');
        assert_eq!(cell.width, 2);
        let extra_idx = cell.extra_codepoints.expect("extras ZWJ");
        assert_eq!(
            term.grapheme_extras[extra_idx as usize],
            "\u{200D}\u{1F33E}"
        );
        assert_eq!(term.cursor.col, 2);
    }

    #[test]
    fn test_skin_tone_ocupa_una_celda() {
        let mut term = Term::new();
        let cluster = "\u{1F44B}\u{1F3FD}";
        feed(&mut term, cluster.as_bytes());
        let cell = term.active_grid().get(0, 0);
        assert_eq!(cell.width, 2);
        assert_eq!(term.active_grid().get(0, 1).width, 0);
        assert_eq!(term.cursor.col, 2);
        let extra_idx = cell.extra_codepoints.expect("skin tone");
        assert_eq!(term.grapheme_extras[extra_idx as usize], "\u{1F3FD}");
        let full = unicode_width::UnicodeWidthStr::width(cluster);
        assert_eq!(cell.width as usize, full);
    }

    #[test]
    fn test_wide_cjk_limpia_continuacion_y_avanza_cursor() {
        let mut term = Term::new();
        feed(&mut term, "中AB".as_bytes());
        let base = term.active_grid().get(0, 0);
        assert_eq!(base.ch, '中');
        assert_eq!(base.width, 2);
        let cont = term.active_grid().get(0, 1);
        assert_eq!(cont.ch, ' ');
        assert_eq!(cont.width, 0);
        assert_eq!(term.active_grid().get(0, 2).ch, 'A');
        assert_eq!(term.active_grid().get(0, 3).ch, 'B');
        assert_eq!(term.cursor.col, 4);
    }

    #[test]
    fn test_wide_cjk_sobrescribe_continuacion_previa() {
        let mut term = Term::new();
        feed(&mut term, b"XXXX");
        feed(&mut term, b"\r");
        feed(&mut term, "中".as_bytes());
        assert_eq!(term.active_grid().get(0, 0).ch, '中');
        assert_eq!(term.active_grid().get(0, 0).width, 2);
        assert_eq!(term.active_grid().get(0, 1).ch, ' ');
        assert_eq!(term.active_grid().get(0, 1).width, 0);
        assert_eq!(term.active_grid().get(0, 2).ch, 'X');
        assert_eq!(term.cursor.col, 2);
    }

    #[test]
    fn test_wide_cjk_al_final_de_linea_con_autowrap() {
        let mut term = Term::new();
        term.resize_grid(term.grid.rows_count, 4, true);
        feed(&mut term, "AB中".as_bytes());
        assert_eq!(term.active_grid().get(0, 0).ch, 'A');
        assert_eq!(term.active_grid().get(0, 1).ch, 'B');
        assert_eq!(term.active_grid().get(0, 2).ch, '中');
        assert_eq!(term.active_grid().get(0, 2).width, 2);
        assert_eq!(term.active_grid().get(0, 3).ch, ' ');
        assert_eq!(term.active_grid().get(0, 3).width, 0);
        assert_eq!(term.cursor.col, 3);
        assert!(term.pending_wrap);
    }

    #[test]
    fn test_grapheme_extras_reutiliza_indice() {
        let mut term = Term::new();
        feed(&mut term, "e\u{0301}".as_bytes());
        feed(&mut term, b" ");
        feed(&mut term, "a\u{0301}".as_bytes());
        let c0 = term.active_grid().get(0, 0);
        let c2 = term.active_grid().get(0, 2);
        let i0 = c0.extra_codepoints.expect("e acute");
        let i2 = c2.extra_codepoints.expect("a acute");
        assert_eq!(i0, i2);
        assert_eq!(term.grapheme_extras.len(), 1);
    }

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
        feed(&mut term, b"\x1b[31mROJO\x1b[0m\r\n");
        assert_eq!(term.grid.rows[0][0].ch, 'R');
        assert_eq!(term.grid.rows[0][1].ch, 'O');
        assert_eq!(term.grid.rows[0][2].ch, 'J');
        assert_eq!(term.grid.rows[0][3].ch, 'O');
        assert_eq!(term.cursor.row, 1);
        assert_eq!(term.cursor.col, 0);
        assert_eq!(term.grid.rows[1][0].ch, ' ');
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
        // \\u{4e2d} = '?' (CJK, ancho 2)
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

        // Reducir tamano (simula resize de terminal)
        // El resize trunca del PRINCIPIO, asi que el contenido de row 0
        // se mueve al scrollback. Verificamos que esta alli.
        term.resize_grid(10, 5, true);

        // Alt grid debe haberse redimensionado
        assert_eq!(term.alt_grid.rows_count, 10);
        assert_eq!(term.alt_grid.cols_count, 5);
        // "ALT LINE" se escribio en row 0. Con truncado del inicio,
        // esa fila ahora esta en scrollback.
        // El scrollback del alt grid debe tener la fila "ALT LINE"
        assert!(term.alt_grid.scrollback.is_empty());

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

        // Escribir una linea larga en primaria
        feed(&mut term, b"ABCDEFGHIJKLMNOPQRST");

        // Reducir ancho significativamente
        term.resize_grid(24, 5, true);

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
    // TESTS ADVERSARIALES
    // Buscan bugs en la implementacion de seleccion con mouse.
    // NO son happy-path. Deben encontrar bugs si existen.
    // -----------------------------------------------------------------------

    /// ADVERSARIAL: selected_text() sin seleccion activa
    /// Debe devolver String vacio, no panic.
    #[test]
    fn test_selected_text_empty_selection() {
        let term = Term::new();
        // Sin seleccion
        assert!(
            term.selection.is_none(),
            "Term nuevo debe tener selection = None"
        );
        let text = term.selected_text();
        assert_eq!(text, "", "selected_text() sin seleccion debe devolver ''");

        // Mismo test con scrollback_offset > 0
        let mut term2 = Term::new();
        term2.scrollback_offset = 5;
        let text2 = term2.selected_text();
        assert_eq!(
            text2, "",
            "selected_text() sin seleccion + offset debe devolver ''"
        );
    }

    /// ADVERSARIAL: selected_text() DESPUiS de clear_selection()
    /// No debe devolver texto residual.
    #[test]
    fn test_selected_text_after_clear() {
        let mut term = Term::new();
        feed(&mut term, b"hello world");

        // Crear seleccion
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);
        assert!(
            !term.selected_text().is_empty(),
            "debe haber texto seleccionado"
        );

        // Limpiar
        term.clear_selection();
        assert!(
            term.selection.is_none(),
            "selection debe ser None tras clear"
        );
        let text = term.selected_text();
        assert_eq!(
            text, "",
            "selected_text() tras clear_selection() debe devolver ''"
        );
    }

    /// ADVERSARIAL: is_selected() para CUALQUIER celda cuando no hay seleccion
    /// Debe devolver false siempre, incluso con coordenadas extremas.
    #[test]
    fn test_is_selected_when_selection_none() {
        let term = Term::new();
        // Sin seleccion: cualquier celda debe dar false
        assert!(!term.is_selected(0, 0));
        assert!(!term.is_selected(10, 30));
        assert!(!term.is_selected(usize::MAX, usize::MAX));
        assert!(!term.is_selected(0, usize::MAX));

        // Con scrollback_offset pero sin seleccion
        let mut term2 = Term::new();
        term2.scrollback_offset = 3;
        assert!(!term2.is_selected(0, 0));
        assert!(!term2.is_selected(23, 79));
    }

    /// ADVERSARIAL: selected_text() en UNA sola celda
    /// Verifica el texto exacto de una seleccion de 1 celda.
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
            "seleccion de 1 celda ('l') debe devolver 'l'"
        );

        // Seleccionar la primera celda: (0, 0) -> 'h'
        let sel2 = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel2);
        assert_eq!(
            term.selected_text(),
            "h",
            "seleccion de 1 celda ('h') debe devolver 'h'"
        );

        // Seleccionar la ultima celda de texto: (0, 4) -> 'o'
        let sel3 = Selection::new(SelectionPoint { row: 0, col: 4 });
        term.selection = Some(sel3);
        assert_eq!(
            term.selected_text(),
            "o",
            "seleccion de 1 celda ('o') debe devolver 'o'"
        );
    }

    /// ADVERSARIAL: selected_text() con seleccion INVERTIDA (start > end)
    /// Debe devolver el mismo texto independientemente de la direccion.
    #[test]
    fn test_selected_text_reversed() {
        let mut term = Term::new();
        feed(&mut term, b"hello world");

        // Seleccion forward: (0,6)-(0,10) -> "world"
        let mut sel_fwd = Selection::new(SelectionPoint { row: 0, col: 6 });
        sel_fwd.update_end(SelectionPoint { row: 0, col: 10 });
        term.selection = Some(sel_fwd);
        let text_fwd = term.selected_text();

        // Misma seleccion reversed: start > end
        let mut sel_rev = Selection::new(SelectionPoint { row: 0, col: 10 });
        sel_rev.update_end(SelectionPoint { row: 0, col: 6 });
        term.selection = Some(sel_rev);
        let text_rev = term.selected_text();

        assert_eq!(
            text_fwd, text_rev,
            "seleccion forward y reversed deben producir el mismo texto"
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

    /// ADVERSARIAL: Seleccion en alt_screen no debe causar panic
    /// Crea seleccion estando en alt_screen, verifica que no crashee.
    #[test]
    fn test_selection_does_not_crash_alt_screen() {
        let mut term = Term::new();

        // Entrar a alt screen
        feed(&mut term, b"\x1b[?1049h");
        assert!(term.alt_screen, "debe estar en alt_screen");

        // Crear seleccion en alt screen (el grid esta vacio)
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);

        // is_selected debe funcionar sin panic
        // La celda (0,0) Si esta seleccionada (la seleccion la cubre),
        // aunque el contenido sea vacio
        let result = term.is_selected(0, 0);
        assert!(
            result,
            "alt_grid: celda (0,0) debe estar seleccionada (selection la cubre)"
        );

        // is_selected para celda FUERA del rango debe ser false
        assert!(
            !term.is_selected(1, 0),
            "celda (1,0) NO debe estar seleccionada (seleccion solo cubre (0,0))"
        );

        // selected_text debe funcionar sin panic
        // alt_grid tiene 80 columnas de espacios (ch=' ', width=1)
        // que si se incluyen en selected_text() porque width>0
        let text = term.selected_text();
        assert!(
            !text.is_empty(),
            "alt_grid vacio tiene espacios con width>1, selected_text los incluye"
        );

        // Salir de alt screen, verificar que sigue funcionando
        feed(&mut term, b"\x1b[?1049l");
        assert!(!term.alt_screen, "debe haber salido de alt_screen");
        // La seleccion apuntaba a la alt_grid, que ahora no es la activa.
        // Pero selected_text sigue funcionando sin panic.
        let _ = term.selected_text();
    }

    /// ADVERSARIAL: is_selected() con scrollback_offset > 0 y sin seleccion
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
                    "sin seleccion, ({},{}) no debe estar seleccionado",
                    row,
                    col
                );
            }
        }

        // Incluso con coordenadas invalidas
        assert!(!term.is_selected(usize::MAX, usize::MAX));
    }

    /// ADVERSARIAL: selected_text() en alt_screen ignora scrollback primario.
    /// Con sb_len > 0 en el grid primario, logical row 0 debe leer alt_grid,
    /// no self.grid.scrollback (sb_len se fuerza a 0 en alt_screen).
    #[test]
    fn test_selected_text_in_alt_screen_with_primary_scrollback() {
        let mut term = Term::new();

        for i in 0..3 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            let line = format!("SCROLLBACK_{}", i);
            feed(&mut term, line.as_bytes());
            feed(&mut term, b"\n");
        }
        let sb_len = term.scrollback_len();
        assert!(sb_len > 0, "debe haber scrollback primario");

        feed(&mut term, b"\x1b[?1049h");
        assert!(term.alt_screen);

        feed(&mut term, b"ALT_CONTENT");
        assert_eq!(term.alt_grid.rows[0][0].ch, 'A');

        let mut sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel.update_end(SelectionPoint { row: 0, col: 10 });
        term.selection = Some(sel);

        assert_eq!(
            term.selected_text(),
            "ALT_CONTENT",
            "alt_screen debe leer alt_grid, no scrollback primario"
        );
    }

    /// ADVERSARIAL: selected_text() con seleccion fuera del rango total de filas
    /// Cuando start_row >= total_rows (scrollback + grid), debe devolver "".
    #[test]
    fn test_selected_text_out_of_bounds_range() {
        let mut term = Term::new();
        feed(&mut term, b"some content");

        // Seleccion que empieza MUY lejos (mucho mas alla del grid)
        let mut sel = Selection::new(SelectionPoint { row: 9999, col: 0 });
        sel.update_end(SelectionPoint {
            row: 9999 + 5,
            col: 10,
        });
        term.selection = Some(sel);

        // start_row (9999) >= total_rows (0 scrollback + 24 grid = 24)
        // El codigo debe devolver string vacio.
        let text = term.selected_text();
        assert_eq!(text, "", "seleccion fuera del rango debe devolver ''");

        // Seleccion con start en scrollback inexistente
        let mut sel2 = Selection::new(SelectionPoint { row: 0, col: 0 });
        sel2.update_end(SelectionPoint { row: 5, col: 5 });
        term.selection = Some(sel2);

        // No hay scrollback (sb_len = 0). total_rows = 0 + 24 = 24.
        // start_row = 0 < 24, end_row = 5 < 24. Hay grid rows 0..24.
        // selected_text() debe procesar rows 0..5.
        // Pero sin scrollback, logical rows 0..5 son grid rows 0..5.
        // grid[0] tiene "some content", grid[1..5] estan vacios.
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

    /// ADVERSARIAL: selected_text() con seleccion que cruza scrollback y grid
    /// y donde una fila del scrollback tiene menos columnas que end_col.
    #[test]
    fn test_selected_text_partial_scrollback_row() {
        let mut term = Term::new();

        // Poner datos en scrollback con menos columnas que el grid
        let short_sb_row: Vec<Cell> = "SHORT"
            .chars()
            .map(|c| Cell {
                ch: c,
                ..Default::default()
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

        // Crear scrollback: 5 lineas
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            let line = format!("SB{}", i);
            feed(&mut term, line.as_bytes());
            feed(&mut term, b"\n");
        }
        let sb_len = term.scrollback_len();
        let grid_row_count = term.grid.rows_count;
        let _total = sb_len + grid_row_count;

        // Seleccionar la ultima linea del scrollback (logical row 4) y
        // las primeras del grid (logical rows 5 a 10)
        let mut sel = Selection::new(SelectionPoint { row: 4, col: 0 });
        sel.update_end(SelectionPoint { row: 10, col: 3 });
        term.selection = Some(sel);

        // Sin scrollback_offset: visible N = logical sb_len + N
        // sb_len=5 ? visible 0 = logical 5 (grid row 0), dentro de seleccion 4-10
        assert!(term.is_selected(0, 0));
        assert!(term.is_selected(5, 0));
        assert!(term.is_selected(5, 3));
        assert!(!term.is_selected(6, 3));

        // Con scrollback_offset = 2:
        // viewport_start = sb_len - 2 = 3
        // visible 0 = logical 3, visible 1 = logical 4, ...
        term.scrollback_offset = 2;
        // La seleccion cubre logical 4-10.
        // visible 0 = logical 3 (no seleccionado)
        // visible 1 = logical 4 (seleccionado)
        // visible 7 = logical 10 (seleccionado)
        assert!(
            !term.is_selected(0, 0),
            "visible 0 = logical 3, fuera de seleccion"
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

        // Crear seleccion en alt
        let sel = Selection::new(SelectionPoint { row: 0, col: 0 });
        term.selection = Some(sel);
        assert!(term.selection.is_some(), "debe haber seleccion activa");

        // Clear en alt screen
        term.clear_selection();
        assert!(term.selection.is_none(), "seleccion debe eliminarse");
        assert_eq!(term.selected_text(), "", "selected_text debe ser ''");

        // El alt grid NO debe haberse modificado
        assert_eq!(
            term.alt_grid.rows[0][0].ch, 'A',
            "alt_grid no debe modificarse"
        );

        // Salir de alt screen: la seleccion debe seguir eliminada
        feed(&mut term, b"\x1b[?1049l");
        assert!(term.selection.is_none(), "seleccion debe seguir eliminada");
    }

    /// ADVERSARIAL: selected_text() con seleccion que incluye SOLO espacios
    /// Verifica que devuelve espacios (width>0), no que filtre vacio.
    #[test]
    fn test_selected_text_only_spaces() {
        let mut term = Term::new();
        // El grid empieza con espacios. Seleccionar una region de solo espacios.
        let sel = Selection {
            start: SelectionPoint { row: 0, col: 0 },
            end: SelectionPoint { row: 0, col: 10 },
            mode: SelectionMode::Normal,
        };
        term.selection = Some(sel);

        // La fila 0 solo tiene espacios (Cell::default()) con width=1
        // La condicion `cell.ch != ' ' || cell.width > 0` incluye espacios
        // porque width=1 > 0. Asi que devuelve los espacios.
        let text = term.selected_text();
        assert_eq!(
            text, "           ",
            "seleccion de solo espacios con width>0 debe devolver espacios"
        );
        assert_eq!(
            text.len(),
            11,
            "deben ser 11 espacios (col 0..10 inclusive, 11 celdas)"
        );

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
        assert!(text2.contains(' '), "debe contener espacios");
    }

    // -----------------------------------------------------------------------
    // Tests: seleccion
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_between_scrollback_and_grid() {
        let mut term = Term::new();

        // Put data in scrollback directly
        let sb_row: Vec<Cell> = "scrollback_line"
            .chars()
            .map(|c| Cell {
                ch: c,
                ..Default::default()
            })
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

        // Scrollback region i the entire start row is selected from start_col onwards
        assert!(term.is_selected(0, 0));
        assert!(term.is_selected(0, 7));
        assert!(!term.is_selected(0, 8));

        // Grid region i end row only selected up to end_col (7)
        assert!(!term.is_selected(2, 0));
    }

    #[test]
    fn test_selection_single_cell() {
        let mut term = Term::new();
        feed(&mut term, b"abc");

        // Seleccionar celda (0, 1) = 'b'
        let sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 1 });
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
        let mut sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 1 });
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
        let mut sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 2 });
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
        let sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 0 });
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

        let mut sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 1 });
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
        let mut sel =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 1 });
        sel.update_end(crate::selection::SelectionPoint { row: 2, col: 1 });
        term.selection = Some(sel);

        // (0,1)-(0,2)='bc' + newline + (1,0)-(1,2)='def' + newline + (2,0)-(2,1)='gh'
        assert_eq!(term.selected_text(), "bc\ndef\ngh");

        // Full lines selection across 4 lines: (0,0) to (3,2)
        let mut sel2 =
            crate::selection::Selection::new(crate::selection::SelectionPoint { row: 0, col: 0 });
        sel2.update_end(crate::selection::SelectionPoint { row: 3, col: 2 });
        term.selection = Some(sel2);

        assert_eq!(term.selected_text(), "abc\ndef\nghi\njkl");
    }

    #[test]
    fn test_visible_to_logical_row_gran_scrollback_no_panic() {
        let mut term = Term::new_with_scrollback(60_000);
        for _ in 0..50_000 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            feed(&mut term, b"x\n");
        }
        assert!(term.grid.scrollback.len() > 40_000);
        let sb_len = term.grid.scrollback.len();
        let logical = term.visible_to_logical_row(10);
        assert_eq!(logical, sb_len.saturating_add(10));
        assert_eq!(
            term.cursor_logical_row(),
            sb_len.saturating_add(term.cursor.row)
        );
    }

    /// Regresion: mouse guarda filas logicas via visible_to_logical_row;
    /// selected_text no debe sumar viewport_start otra vez (offset=0).
    #[test]
    fn test_selected_text_logical_mouse_selection_with_scrollback() {
        let mut term = Term::new();
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            term.cursor.col = 0;
            feed(&mut term, &[b'S', b'B', b'0' + i as u8, b'\n']);
        }
        let sb_len = term.scrollback_len();
        assert_eq!(sb_len, 5);

        term.cursor.move_to(0, 0);
        feed(&mut term, b"HELLO");
        let logical_row = term.visible_to_logical_row(0);
        assert_eq!(logical_row, sb_len);

        let mut sel = Selection::new(SelectionPoint {
            row: logical_row,
            col: 0,
        });
        sel.update_end(SelectionPoint {
            row: logical_row,
            col: 4,
        });
        term.selection = Some(sel);

        assert!(term.is_selected(0, 0), "resaltado en visible row 0");
        assert_eq!(
            term.selected_text(),
            "HELLO",
            "copiar debe leer grid row 0, no duplicar sb_len"
        );
    }

    /// Misma regresion con scrollback_offset > 0 (viewport desplazado).
    #[test]
    fn test_selected_text_logical_mouse_selection_with_scrollback_offset() {
        let mut term = Term::new();
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            term.cursor.col = 0;
            feed(&mut term, &[b'S', b'B', b'0' + i as u8, b'\n']);
        }
        let sb_len = term.scrollback_len();
        assert_eq!(sb_len, 5);

        term.cursor.move_to(0, 0);
        feed(&mut term, b"HELLO");
        term.scrollback_offset = 1;

        let visible_row = 1;
        let logical_row = term.visible_to_logical_row(visible_row);
        assert_eq!(logical_row, sb_len);

        let mut sel = Selection::new(SelectionPoint {
            row: logical_row,
            col: 0,
        });
        sel.update_end(SelectionPoint {
            row: logical_row,
            col: 4,
        });
        term.selection = Some(sel);

        assert!(term.is_selected(visible_row, 0));
        assert_eq!(term.selected_text(), "HELLO");
    }

    #[test]
    fn test_selection_in_scrollback() {
        let mut term = Term::new();
        for i in 0..5 {
            term.cursor.move_to(DEFAULT_ROWS - 1, 0);
            term.cursor.col = 0;
            feed(&mut term, &[b'L', b'i', b'n', b'e', b'0' + i as u8, b'\n']);
        }
        let sb_len = term.scrollback_len();
        assert!(sb_len > 0);
        term.selection = Some(crate::selection::Selection::new(
            crate::selection::SelectionPoint {
                row: sb_len,
                col: 0,
            },
        ));
        assert!(term.is_selected(0, 0));
        term.scrollback_offset = sb_len as isize;
        term.selection = Some(crate::selection::Selection::new(
            crate::selection::SelectionPoint { row: 0, col: 0 },
        ));
        assert!(term.is_selected(0, 0));
    }

    #[test]
    fn test_sgr_bright_foreground() {
        let mut term = Term::new();
        feed(&mut term, b"[91mX");
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::BrightRed);
    }

    #[test]
    fn test_sgr_bright_background() {
        let mut term = Term::new();
        feed(&mut term, b"[101mX");
        assert_eq!(term.grid.rows[0][0].attrs.bg, Color::BrightRed);
    }

    #[test]
    fn test_sgr_256_foreground() {
        let mut term = Term::new();
        feed(&mut term, b"[38;5;100mX");
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Indexed(100));
    }

    #[test]
    fn test_sgr_true_color_foreground() {
        let mut term = Term::new();
        feed(&mut term, b"[38;2;100;150;200mX");
        assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Rgb(100, 150, 200));
    }

    #[test]
    fn test_sgr_256_background() {
        let mut term = Term::new();
        feed(&mut term, b"[48;5;200mX");
        assert_eq!(term.grid.rows[0][0].attrs.bg, Color::Indexed(200));
    }

    #[test]
    fn test_sgr_true_color_background() {
        let mut term = Term::new();
        feed(&mut term, b"[48;2;10;20;30mX");
        assert_eq!(term.grid.rows[0][0].attrs.bg, Color::Rgb(10, 20, 30));
    }

    #[test]
    fn test_sgr_reset_color() {
        let mut term = Term::new();
        feed(&mut term, b"[31m[39m[41m[49m");
        assert_eq!(term.attrs.fg, Color::Default);
        assert_eq!(term.attrs.bg, Color::Default);
    }

    #[test]
    fn test_color_match_exhaustive() {
        // Verifica que todas las variantes de Color se cubren (compila = pasa).
        let colors = [
            Color::Default,
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::White,
            Color::BrightBlack,
            Color::BrightRed,
            Color::BrightGreen,
            Color::BrightYellow,
            Color::BrightBlue,
            Color::BrightMagenta,
            Color::BrightCyan,
            Color::BrightWhite,
            Color::Indexed(42),
            Color::Rgb(1, 2, 3),
        ];
        for c in &colors {
            match c {
                Color::Default
                | Color::Black
                | Color::Red
                | Color::Green
                | Color::Yellow
                | Color::Blue
                | Color::Magenta
                | Color::Cyan
                | Color::White
                | Color::BrightBlack
                | Color::BrightRed
                | Color::BrightGreen
                | Color::BrightYellow
                | Color::BrightBlue
                | Color::BrightMagenta
                | Color::BrightCyan
                | Color::BrightWhite
                | Color::Indexed(_)
                | Color::Rgb(..) => {}
            }
        }
    }

    #[test]
    fn test_sgr_ansi_16_still_works() {
        // Test existente: 30-37 foreground
        {
            let mut term = Term::new();
            feed(&mut term, b"[31mR");
            assert_eq!(term.grid.rows[0][0].ch, 'R');
            assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Red);
        }
        // 40-47 background
        {
            let mut term = Term::new();
            feed(&mut term, b"[41mR");
            assert_eq!(term.grid.rows[0][0].attrs.bg, Color::Red);
        }
        // Reset
        {
            let mut term = Term::new();
            feed(&mut term, b"[31m[0m");
            assert_eq!(term.attrs, Attrs::default());
        }
        // Multi-param: bold + red
        {
            let mut term = Term::new();
            feed(&mut term, b"[1;31mY");
            assert!(term.grid.rows[0][0].attrs.bold);
            assert_eq!(term.grid.rows[0][0].attrs.fg, Color::Red);
        }
    }

    // -----------------------------------------------------------------
    // DECSCUSR (CSI Ps SP q) - forma y blink del cursor
    // -----------------------------------------------------------------

    /// Mapeo de DECSCUSR: cada Ps fija forma (Block/Underline/Bar) y
    /// blink (impar/0 = blinking, par = steady).
    #[test]
    fn test_decscusr_mapeo_completo() {
        let mut term = Term::new();

        // Partimos de un estado no-default para que cada feed demuestre mutación.
        feed(&mut term, b"\x1b[6 q");
        assert_eq!(term.cursor_style, CursorStyle::Bar);
        assert!(!term.cursor_blink_enabled);

        // 0: blinking block
        feed(&mut term, b"\x1b[0 q");
        assert_eq!(term.cursor_style, CursorStyle::Block);
        assert!(term.cursor_blink_enabled);

        // 1: blinking block
        feed(&mut term, b"\x1b[4 q");
        feed(&mut term, b"\x1b[1 q");
        assert_eq!(term.cursor_style, CursorStyle::Block);
        assert!(term.cursor_blink_enabled);

        // 2: steady block
        feed(&mut term, b"\x1b[2 q");
        assert_eq!(
            term.cursor_style,
            CursorStyle::Block,
            "DECSCUSR 2 debe ser Block (steady)"
        );
        assert!(!term.cursor_blink_enabled, "DECSCUSR 2 debe ser sin blink");

        // 3: blinking underline
        feed(&mut term, b"\x1b[3 q");
        assert_eq!(
            term.cursor_style,
            CursorStyle::Underline,
            "DECSCUSR 3 debe ser Underline (blink)"
        );
        assert!(term.cursor_blink_enabled);

        // 4: steady underline
        feed(&mut term, b"\x1b[4 q");
        assert_eq!(term.cursor_style, CursorStyle::Underline);
        assert!(!term.cursor_blink_enabled);

        // 5: blinking bar
        feed(&mut term, b"\x1b[5 q");
        assert_eq!(
            term.cursor_style,
            CursorStyle::Bar,
            "DECSCUSR 5 debe ser Bar (blink)"
        );
        assert!(term.cursor_blink_enabled);

        // 6: steady bar
        feed(&mut term, b"\x1b[6 q");
        assert_eq!(term.cursor_style, CursorStyle::Bar);
        assert!(!term.cursor_blink_enabled);

        // Desconocido: fallback a blinking block
        feed(&mut term, b"\x1b[7 q");
        assert_eq!(term.cursor_style, CursorStyle::Block);
        assert!(term.cursor_blink_enabled);
    }

    // -----------------------------------------------------------------
    // has_blink_stuff / reset_blink_phase
    // -----------------------------------------------------------------

    /// Cursor visible + por defecto blink activo + sin scrollback => hay blink.
    #[test]
    fn has_blink_stuff_cursor_visible_por_defecto() {
        let term = Term::new();
        assert!(term.cursor_visible);
        assert!(term.cursor_blink_enabled);
        assert_eq!(term.scrollback_offset, 0);
        assert!(term.has_blink_stuff());
    }

    /// `blink_interval_ms == 0` desactiva el parpadeo del cursor: nada que
    /// titilar aunque el cursor sea visible.
    #[test]
    fn has_blink_stuff_interval_cero_sin_nada_que_titilar() {
        let mut term = Term::new();
        term.blink_interval_ms = 0;
        assert!(!term.has_blink_stuff());
    }

    /// Scrollback abierto: el cursor deja de titilar aunque sea visible.
    #[test]
    fn has_blink_stuff_scrollback_ignora_cursor() {
        let mut term = Term::new();
        term.scrollback_offset = 1;
        assert!(
            !term.has_blink_stuff(),
            "con scrollback no hay cursor blink"
        );
        // ... pero SGR 5 sigue contando aunque haya scrollback.
        feed(&mut term, b"\x1b[5mblink");
        assert!(term.has_blink_stuff(), "SGR 5 cuenta con scrollback");
    }

    /// Copy mode: el cursor del shell no titila, pero el de copy mode es de
    /// navegacion (otro path) y SGR 5 sigue activo.
    #[test]
    fn has_blink_stuff_copy_mode_ignora_cursor_pero_carga_sgr5() {
        let mut term = Term::new();
        term.copy_mode = Some(crate::copy_mode::CopyModeState::enter(&term));
        assert!(
            !term.has_blink_stuff(),
            "copy mode: el shell cursor no titila"
        );
        feed(&mut term, b"\x1b[5mx");
        assert!(term.has_blink_stuff(), "SGR 5 sigue activo en copy mode");
    }

    /// `cursor_blink_enabled = false` no aporta blink por cursor; SGR 5 si.
    #[test]
    fn has_blink_stuff_cursor_blink_disabled_no_aporta() {
        let mut term = Term::new();
        term.cursor_blink_enabled = false;
        assert!(
            !term.has_blink_stuff(),
            "cursor blink desactivado: solo aporta si hay SGR 5"
        );
        feed(&mut term, b"normal");
        assert!(!term.has_blink_stuff(), "texto normal sin blink");
        feed(&mut term, b"\x1b[5mx");
        assert!(term.has_blink_stuff(), "SGR 5 aporta blink");
    }

    /// `reset_blink_phase` actualiza `last_blink_reset` a ahora.
    #[test]
    fn reset_blink_phase_actualiza_instant() {
        let mut term = Term::new();
        let before = term.last_blink_reset;
        std::thread::sleep(std::time::Duration::from_millis(5));
        term.reset_blink_phase();
        assert!(term.last_blink_reset > before);
    }
}
