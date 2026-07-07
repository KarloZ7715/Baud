//! Ventana principal de Baud.
//!
//! App implementa ApplicationHandler<UserEvent> de winit 0.30.
//! El Renderer se inicializa en resumed() y se invoca en redraw_requested().
//! El Term se comparte con el hilo drain via Arc<Mutex<Term>>.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};

use crate::ansi::Term;
use crate::clipboard::{self, CopyTarget};
use crate::config::{Config, StartupState};
use crate::copy_mode::CopyModeState;
use crate::event_loop::{PtyCommand, PtyCommandSender};
use crate::grid::Cell;
use crate::input::actions::{normalize_binding_key, Action, Keybindings};
use crate::input::keymap::{self, Key as KKey, KeyEventKind, KeyModes, Mods};
use crate::renderer::Renderer;
use crate::selection::{Selection, SelectionMode, SelectionPoint};
use crate::smart_select;
use winit::application::ApplicationHandler;
use winit::event::ElementState;
use winit::event::MouseButton;
use winit::event::MouseScrollDelta;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

/// Eventos enviados desde el hilo drain al hilo GUI.
#[derive(Debug)]
pub enum UserEvent {
    /// El drain termino de procesar bytes del PTY; la GUI debe redibujar.
    RedrawNeeded,
    /// El child termino (EOF en master fd).
    PtyExited(i32),
    /// Error de I/O del PTY.
    PtyError(String),
    /// OSC 0/1/2: actualizar titulo de ventana.
    SetTitle(String),
    /// OSC 52 query: leer clipboard y responder al PTY (target, bell_terminated).
    ReadClipboard(u8, bool),
    /// Config recargada desde disco.
    ConfigReloaded(Box<Config>),
}

fn winit_to_key(k: &Key) -> Option<KKey> {
    Some(match k {
        Key::Named(NamedKey::Enter) => KKey::Enter,
        Key::Named(NamedKey::Tab) => KKey::Tab,
        Key::Named(NamedKey::Backspace) => KKey::Backspace,
        Key::Named(NamedKey::Escape) => KKey::Escape,
        Key::Named(NamedKey::Space) => KKey::Char(' '),
        Key::Named(NamedKey::ArrowUp) => KKey::Up,
        Key::Named(NamedKey::ArrowDown) => KKey::Down,
        Key::Named(NamedKey::ArrowLeft) => KKey::Left,
        Key::Named(NamedKey::ArrowRight) => KKey::Right,
        Key::Named(NamedKey::Home) => KKey::Home,
        Key::Named(NamedKey::End) => KKey::End,
        Key::Named(NamedKey::PageUp) => KKey::PageUp,
        Key::Named(NamedKey::PageDown) => KKey::PageDown,
        Key::Named(NamedKey::Insert) => KKey::Insert,
        Key::Named(NamedKey::Delete) => KKey::Delete,
        Key::Named(NamedKey::F1) => KKey::F(1),
        Key::Named(NamedKey::F2) => KKey::F(2),
        Key::Named(NamedKey::F3) => KKey::F(3),
        Key::Named(NamedKey::F4) => KKey::F(4),
        Key::Named(NamedKey::F5) => KKey::F(5),
        Key::Named(NamedKey::F6) => KKey::F(6),
        Key::Named(NamedKey::F7) => KKey::F(7),
        Key::Named(NamedKey::F8) => KKey::F(8),
        Key::Named(NamedKey::F9) => KKey::F(9),
        Key::Named(NamedKey::F10) => KKey::F(10),
        Key::Named(NamedKey::F11) => KKey::F(11),
        Key::Named(NamedKey::F12) => KKey::F(12),
        Key::Character(s) => KKey::Char(s.chars().next()?),
        _ => return None,
    })
}

fn current_key_modes(term: &Arc<Mutex<Term>>) -> KeyModes {
    if let Ok(g) = term.lock() {
        KeyModes {
            app_cursor_keys: g.app_cursor_keys,
            app_keypad: g.keypad_application_mode,
            newline_mode: g.newline_mode,
            keyboard_flags: g.keyboard_flags,
        }
    } else {
        KeyModes::default()
    }
}

fn clamp_font_size(current: u16, dir: i8) -> u16 {
    let next = current as i32 + dir as i32;
    next.clamp(6, 72) as u16
}

const GUI_METRICS_LOG_INTERVAL: Duration = Duration::from_secs(5);

struct GuiRedrawMetrics {
    redraws: u64,
    interval_sum_ms: f64,
    interval_samples: u64,
    period_start: Instant,
}

impl GuiRedrawMetrics {
    fn new() -> Self {
        Self {
            redraws: 0,
            interval_sum_ms: 0.0,
            interval_samples: 0,
            period_start: Instant::now(),
        }
    }

    fn record_redraw(&mut self, since_last: Option<Duration>) {
        self.redraws += 1;
        if let Some(dt) = since_last {
            self.interval_sum_ms += dt.as_secs_f64() * 1000.0;
            self.interval_samples += 1;
        }
    }

    fn maybe_log(&mut self) {
        let elapsed = self.period_start.elapsed();
        if elapsed < GUI_METRICS_LOG_INTERVAL {
            return;
        }
        let secs = elapsed.as_secs_f64();
        let avg_ms = if self.interval_samples > 0 {
            self.interval_sum_ms / self.interval_samples as f64
        } else {
            0.0
        };
        tracing::debug!(
            target: "baud::pipeline",
            "gui: {:.0} redraws/s, intervalo medio {:.1}ms",
            self.redraws as f64 / secs,
            avg_ms,
        );
        *self = Self::new();
    }
}

/// Estado de la aplicación GUI.
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    term: Arc<Mutex<Term>>,
    pty_tx: Arc<Mutex<Option<PtyCommandSender>>>,
    config: Config,
    /// Tamano de fuente efectivo en runtime (puede diferir del config tras zoom).
    font_size: u16,
    /// Estado de teclas modificadoras (Ctrl, Shift, Alt, etc.).
    modifiers: winit::event::Modifiers,
    /// Indica si el botón izquierdo del mouse está presionado.
    /// Arc<AtomicBool> para compartir con el thread de auto-scroll.
    mouse_down: Arc<AtomicBool>,
    /// Punto inicial de la selección actual (si se está arrastrando).
    mouse_start: Option<SelectionPoint>,
    /// Última posición conocida del mouse (para usar en MouseInput).
    mouse_x: f64,
    mouse_y: f64,
    /// Dimensiones de la ventana en píxeles (para detectar cuando el mouse sale del viewport).
    window_width: f32,
    window_height: f32,
    /// Instant del último click izquierdo (para detectar doble/triple click).
    last_click_time: Option<Instant>,
    /// Ultima celda reportada al PTY en mouse motion (evita flood por pixel).
    last_reported_cell: Option<(usize, usize)>,
    /// Mapa de atajos de teclado (defaults + overrides de config).
    keybindings: Keybindings,
    last_gui_redraw: Option<Instant>,
    gui_redraw_metrics: GuiRedrawMetrics,
}

impl App {
    /// Crea una nueva instancia de App con el term compartido.
    pub fn new(
        term: Arc<Mutex<Term>>,
        pty_tx: Arc<Mutex<Option<PtyCommandSender>>>,
        config: Config,
    ) -> Self {
        let font_size = config.font.size;
        let window_width = config.window.width as f32;
        let window_height = config.window.height as f32;
        let keybindings = config.keybindings();
        Self {
            window: None,
            renderer: None,
            term,
            pty_tx,
            config,
            font_size,
            modifiers: winit::event::Modifiers::default(),
            mouse_down: Arc::new(AtomicBool::new(false)),
            mouse_start: None,
            mouse_x: 0.0,
            mouse_y: 0.0,
            window_width,
            window_height,
            last_click_time: None,
            last_reported_cell: None,
            keybindings,
            last_gui_redraw: None,
            gui_redraw_metrics: GuiRedrawMetrics::new(),
        }
    }

    /// Copia texto al clipboard del sistema (delegado a clipboard.rs).
    fn set_clipboard(&self, text: &str) {
        tracing::info!("set_clipboard: {} bytes", text.len());
        clipboard::set(text, false);
    }

    /// Sincroniza grid emulado y PTY con el tamano de ventana en pixeles.
    fn sync_grid_to_window(
        &self,
        width: u32,
        height: u32,
        cell_w: f32,
        cell_h: f32,
        preserve_scrollback: bool,
        reflow: bool,
    ) -> (usize, usize, usize, usize) {
        let (new_rows, new_cols) = crate::renderer::limits::compute_grid_dims(
            width,
            height,
            cell_w,
            cell_h,
            self.config.window.padding_x,
            self.config.window.padding_y,
        );
        let (old_rows, old_cols) = if let Ok(guard) = self.term.lock() {
            let active = guard.active_grid();
            (active.rows_count, active.cols_count)
        } else {
            (new_rows, new_cols)
        };
        if let Ok(mut guard) = self.term.lock() {
            guard.resize_grid(new_rows, new_cols, reflow);
            if preserve_scrollback {
                let max_offset = guard.scrollback_len();
                guard.scrollback_offset = guard.scrollback_offset.min(max_offset as isize);
            } else {
                guard.scrollback_offset = 0;
            }
        }
        if old_rows != new_rows || old_cols != new_cols {
            if let Some(tx) = self.pty_tx.lock().ok().and_then(|g| g.as_ref().cloned()) {
                let _ = tx.send(PtyCommand::Resize {
                    rows: new_rows as u16,
                    cols: new_cols as u16,
                });
            }
        }
        (old_rows, old_cols, new_rows, new_cols)
    }

    /// Copia al clipboard: si hay selección activa, copia solo la selección;
    /// si no, retorna sin copiar nada.
    fn handle_copy(&mut self) {
        tracing::info!("handle_copy: INICIANDO");
        let text = {
            let term_guard = match self.term.lock() {
                Ok(g) => g,
                Err(poisoned) => {
                    tracing::warn!("handle_copy: term mutex poisoned: {poisoned}");
                    return;
                }
            };
            if let Some(ref sel) = term_guard.selection {
                tracing::info!(
                    "handle_copy: seleccion DETECTADA: start=({},{}), end=({},{})",
                    sel.start.row,
                    sel.start.col,
                    sel.end.row,
                    sel.end.col
                );
                let t = term_guard.selected_text();
                tracing::info!("handle_copy: selected_text() devolvio {} bytes", t.len());
                if t.is_empty() {
                    tracing::warn!("handle_copy: selected_text() devolvio VACIO");
                } else {
                    tracing::info!(
                        "handle_copy: texto a copiar (primeros 80 chars): {:?}",
                        &t[..t.len().min(80)]
                    );
                }
                t
            } else {
                tracing::warn!("handle_copy: NO hay seleccion activa, cancelando copia");
                return;
            }
        };
        tracing::info!(
            "handle_copy: llamando set_clipboard con {} bytes",
            text.len()
        );
        self.set_clipboard(&text);

        // Mostrar feedback visual.
        if let Some(renderer) = &mut self.renderer {
            renderer.set_status("[Copiado al clipboard]");
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Expande la selección tras un doble clic: smart (URL/path/email) si está
    /// activado en config, si no expand_to_word clásico.
    fn expand_double_click(
        &self,
        sel: &mut Selection,
        row_cells: &Option<Vec<Cell>>,
        col: usize,
        abs_row: usize,
        _cols_count: usize,
    ) {
        let Some(cells) = row_cells else { return };
        if self.config.selection.smart_selection {
            if let Some(range) =
                smart_select::expand_smart(cells, col, &self.config.selection.word_delimiters)
            {
                sel.start.row = abs_row;
                sel.end.row = abs_row;
                sel.start.col = range.start;
                sel.end.col = range.end;
                sel.mode = SelectionMode::Smart;
                return;
            }
        }
        sel.expand_to_word(cells, col);
        sel.mode = SelectionMode::Word;
    }

    /// copy_on_select: copia la selección actual al destino configurado.
    fn copy_selection_on_release(&self) {
        let text = match self.term.lock() {
            Ok(g) => g.selected_text(),
            Err(_) => return,
        };
        if text.is_empty() {
            return;
        }
        let target = CopyTarget::parse(&self.config.selection.copy_on_select_target);
        target.write(&text);
    }

    /// Obtiene texto del clipboard del sistema, lo filtra y lo envia al PTY.
    /// Usa wl-paste (Wayland nativo).
    /// Si bracketed paste mode (DEC 2004) esta activo, envuelve el texto en
    /// \x1b[200~...\x1b[201~ para que readline no ejecute comandos al pegar.
    fn handle_paste(&mut self) {
        tracing::debug!("handle_paste: iniciando");
        let text = clipboard::get(false);
        self.paste_text(&text);
    }

    /// Pega desde la primary selection (botón medio del mouse).
    fn handle_paste_primary(&mut self) {
        tracing::debug!("handle_paste_primary: iniciando");
        let text = clipboard::get(true);
        self.paste_text(&text);
    }

    /// Filtra y envía texto pegado al PTY (con bracketing si aplica).
    fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            tracing::debug!("paste_text: vacio, ignorar");
            return;
        }
        tracing::info!(
            "paste_text: {} bytes: {:?}",
            text.len(),
            &text[..text.len().min(60)]
        );
        let text = text.trim_end_matches('\n').to_string();
        let bracketed = self
            .term
            .lock()
            .ok()
            .map(|t| t.bracketed_paste)
            .unwrap_or(false);
        let filtered = if bracketed {
            crate::input::paste_with_bracketing(&text, true)
        } else {
            crate::input::paste_text(&text)
        };
        if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(filtered));
        }
    }

    /// Envia bytes de input al hilo PTY para escribirlos en el master fd.
    fn send_input(&self, bytes: Vec<u8>) {
        // Resetear scrollback offset al enviar cualquier input al PTY
        if let Ok(mut guard) = self.term.lock() {
            if guard.scrollback_offset > 0 {
                guard.scrollback_offset = 0;
            }
            // Limpiar seleccion al escribir teclas (no en copy mode).
            if guard.copy_mode.is_none() {
                guard.clear_selection();
            }
            guard.reset_blink_phase();
        }
        tracing::debug!("send_input: {} bytes: {:02x?}", bytes.len(), bytes);
        if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(bytes));
        }
    }

    /// Extiende la seleccion con teclado (Shift+arrow).
    /// Si no hay seleccion, crea una desde la posicion del cursor.
    fn extend_selection(&self, drow: isize, dcol: isize) {
        if let Ok(mut guard) = self.term.lock() {
            let cols_count = guard.grid.cols_count;
            let sb_len = if guard.alt_screen {
                0
            } else {
                guard.grid.scrollback.len()
            };
            let total_rows = sb_len + guard.grid.rows_count;
            let max_row = total_rows.saturating_sub(1);

            // Crear seleccion desde el cursor si no existe (coordenadas absolutas).
            if guard.selection.is_none() {
                let abs_row = guard.cursor_logical_row();
                let cur_col = guard.cursor.col;
                if abs_row < total_rows {
                    guard.selection = Some(Selection::new(SelectionPoint {
                        row: abs_row,
                        col: cur_col,
                    }));
                } else {
                    return;
                }
            }

            let (old_row, old_col) = guard
                .selection
                .as_ref()
                .map(|s| (s.end.row, s.end.col))
                .unwrap_or((0, 0));

            let mut new_row = old_row as isize + drow;
            let mut new_col = old_col as isize + dcol;

            // Wrap horizontal entre filas absolutas adyacentes.
            if new_col < 0 {
                new_col = (cols_count - 1) as isize;
                new_row -= 1;
            } else if new_col >= cols_count as isize {
                new_col = 0;
                new_row += 1;
            }

            new_row = new_row.clamp(0, max_row as isize);
            new_col = new_col.clamp(0, (cols_count.saturating_sub(1)) as isize);

            if let Some(ref mut sel) = guard.selection {
                sel.end.row = new_row as usize;
                sel.end.col = new_col as usize;
            }
            guard.scroll_to_show_logical_row(new_row as usize);
            guard.mark_dirty();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn scroll_lines(&mut self, n: isize) {
        let mut guard = self.term.lock().expect("term mutex poisoned");
        if n > 0 {
            if !guard.alt_screen {
                let max_offset = guard.scrollback_len();
                guard.scrollback_offset = (guard.scrollback_offset + n).min(max_offset as isize);
            }
        } else {
            guard.scrollback_offset = (guard.scrollback_offset + n).max(0);
        }
        guard.mark_dirty();
    }

    fn scroll_page(&mut self, dir: isize) {
        let mut guard = self.term.lock().expect("term mutex poisoned");
        let page = guard.grid.rows_count as isize - 1;
        if dir > 0 {
            if !guard.alt_screen {
                let max_offset = guard.scrollback_len();
                guard.scrollback_offset = (guard.scrollback_offset + page).min(max_offset as isize);
            }
        } else {
            guard.scrollback_offset = (guard.scrollback_offset - page).max(0);
        }
        guard.mark_dirty();
    }

    fn scroll_to_bottom(&mut self) {
        let mut guard = self.term.lock().expect("term mutex poisoned");
        guard.scrollback_offset = 0;
        guard.mark_dirty();
    }

    /// Entra en copy mode si esta habilitado en config (no sale; usar q/Esc en copy mode).
    fn toggle_copy_mode(&mut self) {
        if !self.config.copy_mode.enabled {
            return;
        }
        if let Ok(mut guard) = self.term.lock() {
            if guard.copy_mode.is_none() {
                guard.copy_mode = Some(CopyModeState::enter(&guard));
                guard.mark_dirty();
                tracing::info!("KEYBOARD: copy mode activado");
            }
        }
    }

    fn font_zoom(&mut self, dir: i8) {
        let base = self.config.font.size;
        self.font_size = if dir == 0 {
            base
        } else {
            clamp_font_size(self.font_size, dir)
        };
        if let Some(renderer) = &mut self.renderer {
            let (cell_w, cell_h) = renderer.set_font_size(self.font_size);
            if let Some(window) = &self.window {
                let size = window.inner_size();
                let (old_rows, _, new_rows, _) =
                    self.sync_grid_to_window(size.width, size.height, cell_w, cell_h, true, false);
                // Al reducir filas, anclar el borde inferior visible (evita que el contenido "suba").
                if old_rows > new_rows {
                    if let Ok(mut guard) = self.term.lock() {
                        let delta = (old_rows - new_rows) as isize;
                        guard.scrollback_offset = (guard.scrollback_offset - delta).max(0);
                        guard.mark_dirty();
                    }
                }
            }
        }
    }

    fn run_action(&mut self, action: Action) {
        use crate::input::actions::Action::*;
        match action {
            Copy => {
                let in_copy_mode = self
                    .term
                    .lock()
                    .ok()
                    .map(|g| g.copy_mode.is_some())
                    .unwrap_or(false);
                self.handle_copy();
                if in_copy_mode {
                    if let Ok(mut guard) = self.term.lock() {
                        CopyModeState::exit(&mut guard);
                    }
                }
            }
            Paste => self.handle_paste(),
            PastePrimary => self.handle_paste_primary(),
            ToggleCopyMode => self.toggle_copy_mode(),
            ScrollLineUp => self.scroll_lines(1),
            ScrollLineDown => self.scroll_lines(-1),
            ScrollPageUp => self.scroll_page(1),
            ScrollPageDown => self.scroll_page(-1),
            ScrollToBottom => self.scroll_to_bottom(),
            FontZoomIn => self.font_zoom(1),
            FontZoomOut => self.font_zoom(-1),
            FontZoomReset => self.font_zoom(0),
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Maneja una tecla en copy mode. Devuelve true si la tecla fue consumida
    /// (navegación, selección, salir). Flechas mueven; Shift+flechas extienden;
    /// q/Esc salen; `y` copia y sale (vim-style).
    fn handle_copy_mode_key(&mut self, event: &winit::event::KeyEvent, shift: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let (drow, dcol) = match &event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => (0, -1),
            Key::Named(NamedKey::ArrowRight) => (0, 1),
            Key::Named(NamedKey::ArrowUp) => (-1, 0),
            Key::Named(NamedKey::ArrowDown) => (1, 0),
            Key::Character(c) if !shift => match c.as_str() {
                "h" => (0, -1),
                "l" => (0, 1),
                "k" => (-1, 0),
                "j" => (1, 0),
                _ => (0, 0),
            },
            _ => (0, 0),
        };

        let mut exit = false;
        let mut copy_and_exit = false;
        if let Ok(mut guard) = self.term.lock() {
            // Salir con q o Esc.
            match &event.logical_key {
                Key::Character(c) if c == "q" => exit = true,
                Key::Named(NamedKey::Escape) => exit = true,
                Key::Character(c) if c == "y" => copy_and_exit = true,
                _ => {}
            }
            if exit {
                CopyModeState::exit(&mut guard);
                return true;
            }
            if copy_and_exit {
                // Copiar selección actual y salir.
                let text = guard.selected_text();
                if !text.is_empty() {
                    drop(guard);
                    clipboard::set(&text, false);
                    if let Ok(mut g2) = self.term.lock() {
                        CopyModeState::exit(&mut g2);
                    }
                } else if let Ok(mut g2) = self.term.lock() {
                    CopyModeState::exit(&mut g2);
                }
                return true;
            }

            if drow != 0 || dcol != 0 {
                if let Some(cm) = guard.copy_mode.take() {
                    let mut cm = cm;
                    cm.move_cursor(&mut guard, drow, dcol, shift);
                    guard.copy_mode = Some(cm);
                }
            }
        }
        drow != 0 || dcol != 0
    }

    /// Envia bytes al PTY sin efectos secundarios (seleccion, scrollback).
    fn send_pty_bytes(&self, bytes: Vec<u8>) {
        tracing::debug!("send_pty_bytes: {} bytes: {:02x?}", bytes.len(), bytes);
        if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(bytes));
        }
    }

    /// Mapea píxeles de ventana a (row, col); resta el padding del renderer.
    fn pixel_to_cell(&self, x: f64, y: f64, renderer: &Renderer) -> (usize, usize) {
        let (pad_x, pad_y) = renderer.content_padding();
        crate::renderer::limits::pixel_to_cell_coords(
            x,
            y,
            pad_x,
            pad_y,
            renderer.cell_w(),
            renderer.cell_h(),
        )
    }

    /// Coordenadas de celda (row, col) desde la ultima posicion del mouse.
    fn mouse_cell_coords(&self, renderer: &Renderer) -> (usize, usize) {
        self.pixel_to_cell(self.mouse_x, self.mouse_y, renderer)
    }

    /// Baud maneja seleccion local; si la app pidio mouse reporting, forward al PTY.
    /// Modificadores de bypass configurables
    /// Default: ["shift"]. Alt queda libre para selección en bloque.
    fn local_selection_active(&self, mouse_reporting: &crate::ansi::MouseReporting) -> bool {
        let mods = self.modifiers.state();
        let cfg = &self.config.selection;
        if (mods.shift_key() && cfg.bypass_contains("shift"))
            || (mods.alt_key() && cfg.bypass_contains("alt"))
            || (mods.control_key() && cfg.bypass_contains("ctrl"))
        {
            return true;
        }
        !mouse_reporting.is_active()
    }

    /// True si Alt está presionado (modificador de selección en bloque).
    fn block_selection_active(&self) -> bool {
        self.modifiers.state().alt_key()
    }

    /// Solo reenviar eventos de mouse a la app (no seleccion local).
    fn should_forward_mouse_to_app(&self) -> bool {
        if let Ok(guard) = self.term.lock() {
            return guard.mouse_reporting.is_active()
                && !self.local_selection_active(&guard.mouse_reporting);
        }
        false
    }

    fn clamp_mouse_to_grid(
        row: usize,
        col: usize,
        rows: usize,
        cols: usize,
    ) -> Option<(usize, usize)> {
        if row == usize::MAX || col == usize::MAX {
            return None;
        }
        let r = row.min(rows.saturating_sub(1));
        let c = col.min(cols.saturating_sub(1));
        Some((r, c))
    }

    fn encode_mouse_report(
        reporting: &crate::ansi::MouseReporting,
        button: u8,
        col: usize,
        row: usize,
        release: bool,
    ) -> Option<Vec<u8>> {
        let (x, y) = crate::renderer::limits::mouse_report_coords(col, row)?;
        if reporting.sgr {
            let suffix = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{};{};{}{}", button, x, y, suffix).into_bytes())
        } else {
            let b = if release { button + 3 } else { button } + 0x20;
            Some(vec![0x1b, b'M', b, (x + 0x20) as u8, (y + 0x20) as u8])
        }
    }

    fn forward_mouse_button(&self, button: u8, release: bool) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (row, col) = self.mouse_cell_coords(renderer);
        if let Ok(guard) = self.term.lock() {
            if !guard.mouse_reporting.is_active() {
                return;
            }
            let active = guard.active_grid();
            let Some((row, col)) =
                Self::clamp_mouse_to_grid(row, col, active.rows_count, active.cols_count)
            else {
                return;
            };
            let Some(bytes) =
                Self::encode_mouse_report(&guard.mouse_reporting, button, col, row, release)
            else {
                return;
            };
            drop(guard);
            self.send_pty_bytes(bytes);
        }
    }

    fn forward_mouse_motion(&self, button: u8) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (row, col) = self.mouse_cell_coords(renderer);
        if let Ok(guard) = self.term.lock() {
            if !guard.mouse_reporting.is_active() {
                return;
            }
            let active = guard.active_grid();
            let Some((row, col)) =
                Self::clamp_mouse_to_grid(row, col, active.rows_count, active.cols_count)
            else {
                return;
            };
            let Some(bytes) =
                Self::encode_mouse_report(&guard.mouse_reporting, button, col, row, false)
            else {
                return;
            };
            drop(guard);
            self.send_pty_bytes(bytes);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ponytail: solo inicializar una vez.
        if self.window.is_some() {
            return;
        }

        // 1. Crear ventana.
        let wcfg = &self.config.window;
        let mut attrs = Window::default_attributes()
            .with_title("baud")
            .with_inner_size(winit::dpi::LogicalSize::new(wcfg.width, wcfg.height))
            .with_decorations(wcfg.decorations);
        match wcfg.startup {
            StartupState::Maximized => {
                tracing::info!("window: width/height del config no aplican con startup=maximized");
                attrs = attrs.with_maximized(true);
            }
            StartupState::Fullscreen => {
                tracing::info!("window: width/height del config no aplican con startup=fullscreen");
                attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            StartupState::Windowed => {}
        }
        // Solo activar transparencia si la opacidad es < 1.0
        let opacity = self.config.window.opacity;
        let attrs = if opacity < 1.0 {
            attrs.with_transparent(true)
        } else {
            attrs
        };
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("no se pudo crear la ventana"),
        );
        self.window = Some(window.clone());

        // 2. Obtener display handle para wgpu (evita el lifetime de ActiveEventLoop).
        let display_handle = event_loop.owned_display_handle();

        // 3. Inicializar wgpu: instance, adapter, device, queue, surface, config.
        //    wgpu 29 tiene API async (request_adapter, request_device retornan Future).
        //    Usamos block_on() local (sin pollster) para bloquear en nativo.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle)),
        );

        let surface = instance
            .create_surface(window.clone())
            .expect("no se pudo crear la surface wgpu");

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no se encontro adaptador GPU compatible");

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .expect("no se pudo crear el device GPU");

        let size = window.inner_size();
        let surface_w = size.width.clamp(1, 16_384);
        let surface_h = size.height.clamp(1, 16_384);
        let mut config = surface
            .get_default_config(&adapter, surface_w, surface_h)
            .expect("no se encontro formato de surface compatible");
        // Si hay transparencia, asegurar que el alpha mode sea compatible
        if opacity < 1.0 {
            config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
            config.view_formats = vec![config.format.add_srgb_suffix()];
        }
        surface.configure(&device, &config);

        // 4. Crear Renderer.
        self.renderer = Some(Renderer::new(
            window.clone(),
            device,
            queue,
            surface,
            config,
            &self.config.font,
        ));
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_content_padding(wcfg.padding_x, wcfg.padding_y);
        }
        tracing::info!("renderer inicializado");

        let size = window.inner_size();
        if let Some(renderer) = &self.renderer {
            self.sync_grid_to_window(
                size.width,
                size.height,
                renderer.cell_w,
                renderer.cell_h,
                false,
                true,
            );
        }

        // 5. Forzar el primer redraw para que winit dispare RedrawRequested.
        // Sin esto, la ventana queda vacia hasta que el drain envie bytes
        // (lo cual activa el user_event RedrawNeeded -> request_redraw).
        // Con esto, pintamos el estado inicial del term inmediatamente,
        // evitando que el compositor (Hyprland) marque la ventana como
        // "no responde" mientras espera output.
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Enviar Shutdown al hilo PTY para que envie SIGHUP al child.
                if let Some(tx) = self.pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
                    let _ = tx.send(PtyCommand::Shutdown);
                }
                // Salir del event loop. El hilo PTY recibira el Shutdown, hara SIGHUP,
                // esperara 100ms, y morira. El Pty se dropea con SIGKILL safety net.
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                self.window_width = new_size.width as f32;
                self.window_height = new_size.height as f32;
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                renderer.resize(new_size.width, new_size.height, 0);
                let cell_w = renderer.cell_w;
                let cell_h = renderer.cell_h;
                let (_old_rows, _old_cols, new_rows, new_cols) = self.sync_grid_to_window(
                    new_size.width,
                    new_size.height,
                    cell_w,
                    cell_h,
                    false,
                    true,
                );
                tracing::debug!(
                    "[RESIZE] cell_h={:.1} cell_w={:.1} win={}x{} -> grid={}x{}",
                    cell_h,
                    cell_w,
                    new_size.width,
                    new_size.height,
                    new_rows,
                    new_cols,
                );
                if let Ok(guard) = self.term.lock() {
                    let g = guard.active_grid();
                    let n = g.rows.len().min(5);
                    let mut summary_top = String::new();
                    for r in 0..n {
                        let s: String = g.rows[r].iter().take(20).map(|c| c.ch).collect();
                        let cont = if r < g.row_continuations.len() && g.row_continuations[r] {
                            "~"
                        } else {
                            "|"
                        };
                        summary_top.push_str(&format!("{}{}", cont, s));
                    }
                    let rows_len = g.rows.len();
                    let mut summary_bot = String::new();
                    let bot_start = rows_len.saturating_sub(5);
                    for r in bot_start..rows_len {
                        let s: String = g.rows[r].iter().take(20).map(|c| c.ch).collect();
                        let cont = if r < g.row_continuations.len() && g.row_continuations[r] {
                            "~"
                        } else {
                            "|"
                        };
                        summary_bot.push_str(&format!("{}{}", cont, s));
                    }
                    let non_empty = g
                        .rows
                        .iter()
                        .filter(|r| r.iter().any(|c| *c != Cell::default()))
                        .count();
                    tracing::debug!(
                        "[RESIZE] grid: {}x{} sb={} filled={}/{} top=[{}] bot=[{}]",
                        g.rows_count,
                        g.cols_count,
                        guard.grid.scrollback.len(),
                        non_empty,
                        rows_len,
                        summary_top,
                        summary_bot,
                    );
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                let mut term_guard = match self.term.lock() {
                    Ok(g) => g,
                    Err(poisoned) => {
                        tracing::warn!("term mutex poisoned: {poisoned}");
                        return;
                    }
                };
                if !term_guard.dirty && !renderer.status_overlay_active() {
                    tracing::debug!("RedrawRequested: skip (nothing dirty)");
                    return;
                }
                term_guard.take_dirty();
                tracing::debug!("RedrawRequested: renderizando frame");
                let bold = self.config.bold_is_bright || self.config.theme.bold_is_bright;
                if let Err(e) = renderer.render(
                    &mut term_guard,
                    &self.config.theme,
                    bold,
                    self.config.window.opacity,
                ) {
                    tracing::warn!("error al renderizar: {e}");
                }
            }
            // Track modifier state (Ctrl, Shift, Alt, etc.) for keyboard shortcuts.
            // winit 0.30 envia ModifiersChanged separado de KeyboardInput.
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            // Diagnostico: el cursor entro/salio de la ventana.
            // En Wayland, winit traduce wl_pointer.enter a CursorEntered + CursorMoved.
            // Si no se recibe CursorEntered, el compositor quizas no mando wl_pointer.enter.
            WindowEvent::CursorEntered { .. } => {
                tracing::info!("CursorEntered: el cursor entro a la ventana");
            }
            // Mouse moved: si estamos arrastrando, actualizar el final de la seleccion.
            // Si el mouse sale del viewport (y<0 o y>=height), hacer scroll automatico.
            WindowEvent::CursorMoved { position, .. } => {
                tracing::debug!(
                    "CursorMoved: position=({:.1}, {:.1}) mouse_down={}",
                    position.x,
                    position.y,
                    self.mouse_down.load(Ordering::Relaxed),
                );
                let Some(renderer) = &self.renderer else {
                    tracing::warn!("CursorMoved: renderer no disponible");
                    return;
                };
                self.mouse_x = position.x;
                self.mouse_y = position.y;

                if self.should_forward_mouse_to_app() {
                    let mouse_down = self.mouse_down.load(Ordering::Relaxed);
                    if let Ok(guard) = self.term.lock() {
                        let reporting = guard.mouse_reporting;
                        if reporting.reports_motion() {
                            let (row, col) = self.mouse_cell_coords(renderer);
                            let cell = (row, col);
                            if mouse_down && reporting.drag {
                                drop(guard);
                                if self.last_reported_cell != Some(cell) {
                                    self.last_reported_cell = Some(cell);
                                    self.forward_mouse_motion(32);
                                }
                            } else if reporting.any_motion {
                                drop(guard);
                                if self.last_reported_cell != Some(cell) {
                                    self.last_reported_cell = Some(cell);
                                    self.forward_mouse_motion(35);
                                }
                            }
                        }
                    }
                    return;
                }

                if self.mouse_down.load(Ordering::Relaxed) {
                    let (_pad_x, pad_y) = renderer.content_padding();
                    let inner_h = (self.window_height - pad_y * 2.0).max(renderer.cell_h());
                    let visible_rows = (inner_h / renderer.cell_h()) as usize;
                    let (visible_row, col, needs_scroll_up, needs_scroll_down) =
                        if position.y < pad_y as f64 {
                            (0usize, 0usize, true, false)
                        } else if position.y as f32 >= self.window_height - pad_y {
                            (visible_rows.saturating_sub(1), 0usize, false, true)
                        } else {
                            let (r, c) = self.pixel_to_cell(position.x, position.y, renderer);
                            (r, c, r == 0, r >= visible_rows.saturating_sub(1))
                        };

                    if let Ok(mut guard) = self.term.lock() {
                        if !guard.alt_screen {
                            if needs_scroll_up {
                                let max_offset = guard.scrollback_len();
                                guard.scrollback_offset =
                                    (guard.scrollback_offset + 1).min(max_offset as isize);
                            } else if needs_scroll_down {
                                guard.scrollback_offset = (guard.scrollback_offset - 1).max(0);
                            }
                        }
                        let abs_row = guard.visible_to_logical_row(visible_row);
                        if let Some(ref mut sel) = guard.selection {
                            sel.update_end(SelectionPoint { row: abs_row, col });
                        }
                        guard.mark_dirty();
                        tracing::debug!(
                            "CursorMoved: mouse_drag visible_row={} col={} scrollback_offset={}",
                            visible_row,
                            col,
                            guard.scrollback_offset
                        );
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            // Mouse left: el cursor salio de la ventana.
            // En Wayland, winit deja de enviar CursorMoved cuando el mouse sale.
            // Si estamos arrastrando, iniciamos un thread de auto-scroll.
            WindowEvent::CursorLeft { .. } => {
                if self.mouse_down.load(Ordering::Relaxed) {
                    tracing::info!("CursorLeft: mouse_down=true, auto-scroll thread iniciado");
                    let term_clone = Arc::clone(&self.term);
                    let md_clone = Arc::clone(&self.mouse_down);
                    if let Some(w) = &self.window {
                        let win_clone = Arc::clone(w);
                        std::thread::spawn(move || {
                            // Auto-scroll mientras mouse_down se mantenga, max 200 pasos (~10s)
                            for _ in 0..200 {
                                if !md_clone.load(Ordering::Relaxed) {
                                    break;
                                }
                                if let Ok(mut guard) = term_clone.lock() {
                                    if guard.alt_screen {
                                        break;
                                    }
                                    let max_offset = guard.scrollback_len();
                                    if guard.scrollback_offset >= max_offset as isize {
                                        break; // ya no hay más scrollback
                                    }
                                    guard.scrollback_offset =
                                        (guard.scrollback_offset + 1).min(max_offset as isize);
                                    guard.mark_dirty();
                                }
                                win_clone.request_redraw();
                                std::thread::sleep(std::time::Duration::from_millis(50));
                            }
                            tracing::debug!("CursorLeft: auto-scroll thread terminado");
                        });
                    }
                } else {
                    tracing::debug!("CursorLeft: mouse_down=false, no action");
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                tracing::debug!(
                    "MouseInput: state={:?} button={:?} mouse_pos=({:.1}, {:.1})",
                    state,
                    button,
                    self.mouse_x,
                    self.mouse_y,
                );

                if self.should_forward_mouse_to_app() {
                    let btn = match button {
                        MouseButton::Left => 0,
                        MouseButton::Middle => 1,
                        MouseButton::Right => 2,
                        _ => return,
                    };
                    let release = state == ElementState::Released;
                    self.forward_mouse_button(btn, release);
                    if button == MouseButton::Left {
                        self.mouse_down.store(!release, Ordering::Relaxed);
                        if release {
                            self.last_reported_cell = None;
                        }
                    }
                    return;
                }

                // Middle-click: pegar primary selection.
                if button == MouseButton::Middle && state == ElementState::Pressed {
                    self.handle_paste_primary();
                    return;
                }

                if button == MouseButton::Left {
                    let Some(renderer) = &self.renderer else {
                        tracing::warn!("MouseInput(Left): renderer no disponible");
                        return;
                    };
                    match state {
                        ElementState::Pressed => {
                            // Bugfix: ignorar si las coordenadas no son validas
                            if self.mouse_x < 0.0 || self.mouse_y < 0.0 {
                                return;
                            }
                            let (visible_row, col) =
                                self.pixel_to_cell(self.mouse_x, self.mouse_y, renderer);
                            let shift = self.modifiers.state().shift_key();
                            let block = self.block_selection_active();
                            let now = Instant::now();
                            let is_rapid = self
                                .last_click_time
                                .map(|t| now.duration_since(t) < Duration::from_millis(500))
                                .unwrap_or(false);

                            if let Ok(mut guard) = self.term.lock() {
                                let abs_row = guard.visible_to_logical_row(visible_row);
                                let point = SelectionPoint { row: abs_row, col };
                                if block {
                                    // Alt+click: seleccion rectangular.
                                    let mut sel = Selection::new(point);
                                    sel.mode = SelectionMode::Block;
                                    guard.selection = Some(sel);
                                } else if shift && guard.selection.is_some() {
                                    // Shift+click: extender seleccion existente
                                    if let Some(ref mut sel) = guard.selection {
                                        sel.update_end(point);
                                    }
                                } else if is_rapid {
                                    let cols_count = guard.grid.cols_count;
                                    let row_cells = guard.row_cells_at_logical(abs_row);
                                    let mode = guard
                                        .selection
                                        .as_ref()
                                        .map(|s| s.mode)
                                        .unwrap_or(SelectionMode::Normal);
                                    match mode {
                                        SelectionMode::Normal => {
                                            if let Some(ref mut sel) = guard.selection {
                                                self.expand_double_click(
                                                    sel, &row_cells, col, abs_row, cols_count,
                                                );
                                            }
                                        }
                                        SelectionMode::Word | SelectionMode::Smart => {
                                            if let Some(ref mut sel) = guard.selection {
                                                sel.expand_to_line(abs_row, cols_count);
                                                sel.mode = SelectionMode::Line;
                                            }
                                        }
                                        SelectionMode::Line | SelectionMode::Block => {
                                            guard.selection = Some(Selection::new(point));
                                        }
                                    }
                                } else {
                                    // Click normal (no rapido): iniciar nueva seleccion
                                    let sel = Selection::new(point);
                                    guard.selection = Some(sel);
                                }
                                guard.mark_dirty();
                                self.mouse_start = Some(point);
                            }
                            self.mouse_down.store(true, Ordering::Relaxed);
                            self.last_click_time = Some(now);
                            // Bugfix: solicitar redibujo inmediato al crear/modificar seleccion
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        ElementState::Released => {
                            self.mouse_down.store(false, Ordering::Relaxed);
                            self.mouse_start = None;
                            // copy_on_select: copiar al soltar si la config lo pide.
                            if self.config.selection.copy_on_select {
                                self.copy_selection_on_release();
                            }
                            // Bugfix: redibujar al soltar para fijar estado visual final
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.should_forward_mouse_to_app() {
                    let button = match delta {
                        MouseScrollDelta::LineDelta(_, y) if y > 0.0 => 64,
                        MouseScrollDelta::LineDelta(_, y) if y < 0.0 => 65,
                        MouseScrollDelta::PixelDelta(pos) if pos.y > 0.0 => 64,
                        MouseScrollDelta::PixelDelta(pos) if pos.y < 0.0 => 65,
                        _ => return,
                    };
                    self.forward_mouse_button(button, false);
                }
            }
            // Input de teclado completo: letras, Enter, Backspace, Tab, Ctrl+letter, etc.
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Released => {
                let report_events = self
                    .term
                    .lock()
                    .ok()
                    .map(|g| g.keyboard_flags & 2 != 0)
                    .unwrap_or(false);
                if report_events {
                    let mods = Mods {
                        shift: self.modifiers.state().shift_key(),
                        alt: self.modifiers.state().alt_key(),
                        ctrl: self.modifiers.state().control_key(),
                        sup: self.modifiers.state().super_key(),
                    };
                    if let Some(k) = winit_to_key(&event.logical_key) {
                        let modes = current_key_modes(&self.term);
                        if let Some(bytes) =
                            keymap::encode_key_extended(k, mods, modes, KeyEventKind::Release)
                        {
                            self.send_input(bytes);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let ctrl = self.modifiers.state().control_key();
                let shift = self.modifiers.state().shift_key();
                let alt = self.modifiers.state().alt_key();
                tracing::info!(
                    "KEYBOARD: key={:?} text={:?} ctrl={} shift={} alt={}",
                    event.logical_key,
                    event.text,
                    ctrl,
                    shift,
                    alt
                );

                let mods = Mods {
                    shift: self.modifiers.state().shift_key(),
                    alt: self.modifiers.state().alt_key(),
                    ctrl: self.modifiers.state().control_key(),
                    sup: self.modifiers.state().super_key(),
                };

                if let Some(k) = winit_to_key(&event.logical_key) {
                    let k_norm = normalize_binding_key(k, mods);
                    if let Some(action) = self.keybindings.lookup(k_norm, mods) {
                        self.run_action(action);
                        return;
                    }
                }

                // Copy mode: si está activo, las teclas navegan/seleccionan
                // y NO se envían al PTY (excepto Ctrl+Shift+C ya manejado arriba).
                if self
                    .term
                    .lock()
                    .ok()
                    .map(|g| g.copy_mode.is_some())
                    .unwrap_or(false)
                    && self.handle_copy_mode_key(&event, shift)
                {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }

                // ponytail: seleccion por teclado vive fuera del binding map por su estado.
                match &event.logical_key {
                    Key::Named(NamedKey::ArrowLeft) if shift && !ctrl && !alt => {
                        self.extend_selection(0, -1);
                        return;
                    }
                    Key::Named(NamedKey::ArrowRight) if shift && !ctrl && !alt => {
                        self.extend_selection(0, 1);
                        return;
                    }
                    Key::Named(NamedKey::ArrowUp) if shift && !ctrl && !alt => {
                        self.extend_selection(-1, 0);
                        return;
                    }
                    Key::Named(NamedKey::ArrowDown) if shift && !ctrl && !alt => {
                        self.extend_selection(1, 0);
                        return;
                    }
                    _ => {}
                }

                // Fallback: encode_key_extended (CSI u) o encode_key clasico.
                if let Some(k) = winit_to_key(&event.logical_key) {
                    let modes = current_key_modes(&self.term);
                    let kind = if event.repeat {
                        KeyEventKind::Repeat
                    } else {
                        KeyEventKind::Press
                    };
                    if let Some(bytes) = keymap::encode_key_extended(k, mods, modes, kind) {
                        self.send_input(bytes);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    if let Some(bytes) = keymap::encode_key(k, mods, modes) {
                        self.send_input(bytes);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                } else if let Some(text) = event.text.filter(|t| !t.is_empty()) {
                    // ponytail: fallback para teclas que winit expone solo en text (IME, etc.)
                    self.send_input(text.as_bytes().to_vec());
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RedrawNeeded => {
                let since_last = self.last_gui_redraw.map(|t| t.elapsed());
                self.gui_redraw_metrics.record_redraw(since_last);
                self.last_gui_redraw = Some(Instant::now());
                self.gui_redraw_metrics.maybe_log();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::PtyExited(code) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Proceso terminado: codigo {}]", code));
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::PtyError(msg) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Error PTY: {}]", msg));
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::SetTitle(title) => {
                if let Some(window) = &self.window {
                    window.set_title(&title);
                }
            }
            UserEvent::ReadClipboard(target, bell_terminated) => {
                let primary = target == b'p' || target == b's';
                let text = clipboard::get(primary);
                let encoded = crate::base64::encode(text.as_bytes());
                let response = Term::format_osc52_read_response(target, &encoded, bell_terminated);
                self.send_input(response);
            }
            UserEvent::ConfigReloaded(cfg) => {
                tracing::info!("config recargada desde disco");
                let _ = cfg;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }
}

/// Ejecuta un Future de forma sincrona bloqueando el hilo actual.
///
/// Implementacion minimalista usando solo std. En nativo, los futures de
/// wgpu (request_adapter, request_device) se resuelven en la primera poll,
/// asi que el overhead del spin-loop es despreciable.
// ponytail: si en algun momento wgpu requiere waker real, migrar a pollster.
fn block_on<F: Future>(mut future: F) -> F::Output {
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => {}
        }
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_: *const ()| RawWaker::new(std::ptr::null(), &VTABLE),
    |_: *const ()| {},
    |_: *const ()| {},
    |_: *const ()| {},
);

// ---------------------------------------------------------------------------
// Tests adversariales
// ---------------------------------------------------------------------------
// NO se puede testear el event loop de winit (requiere GPU), pero se puede
// testear la lógica de coordenadas de celda, edge cases de división, y
// estado inicial de App.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::limits::pixel_to_cell_coords;

    #[test]
    fn test_font_zoom_clamp() {
        assert_eq!(clamp_font_size(14, 1), 15);
        assert_eq!(clamp_font_size(72, 1), 72);
        assert_eq!(clamp_font_size(6, -1), 6);
    }

    fn coords_to_cell(x: f64, y: f64, cell_w: f32, cell_h: f32) -> (usize, usize) {
        pixel_to_cell_coords(x, y, 0.0, 0.0, cell_w, cell_h)
    }

    #[test]
    fn test_pixel_to_cell_con_padding() {
        let (row, col) = pixel_to_cell_coords(28.0, 46.0, 8.0, 6.0, 10.0, 20.0);
        assert_eq!((row, col), (2, 2));
        let (r0, c0) = pixel_to_cell_coords(8.0, 6.0, 8.0, 6.0, 10.0, 20.0);
        assert_eq!((r0, c0), (0, 0));
    }

    // =====================================================================
    // TESTS ADVERSARIALES
    // =====================================================================

    /// ADVERSARIAL: Las coordenadas iniciales del mouse (mouse_x, mouse_y)
    /// son 0.0 al crear App. Si un evento MouseInput ocurre antes de
    /// cualquier CursorMoved (lo cual es posible en winit), las coordenadas
    /// usadas serán (0,0) en vez de la posición real del cursor.
    ///
    /// Efecto: el primer click sin movimiento previo del mouse siempre
    /// selecciona la celda (0,0) aunque el cursor esté en otra posición.
    #[test]
    fn test_mouse_coordinates_start_at_zero() {
        let app = App::new(
            Arc::new(Mutex::new(Term::new())),
            Arc::new(Mutex::new(None)),
            Config::default(),
        );
        assert_eq!(
            app.mouse_x, 0.0,
            "BUG: mouse_x = {} al crear App. Sin CursorMoved previo, el click usa (0,0)",
            app.mouse_x
        );
        assert_eq!(
            app.mouse_y, 0.0,
            "BUG: mouse_y = {} al crear App. Igual que mouse_x",
            app.mouse_y
        );
    }

    /// ADVERSARIAL: Coordenadas (0,0) deben mapear a celda (0,0)
    /// con cell_w y cell_h positivos (caso normal).
    #[test]
    fn test_coords_zero_zero() {
        let (row, col) = coords_to_cell(0.0, 0.0, 10.0, 20.0);
        assert_eq!((row, col), (0, 0), "(0,0) debe mapear a celda (0,0)");
    }

    /// ADVERSARIAL: Coordenadas justo antes del borde inferior derecho
    /// de la ventana no deben producir overflow.
    #[test]
    fn test_coords_at_bounds() {
        let cell_w = 10.0;
        let cell_h = 20.0;
        let width = 800.0;
        let height = 600.0;

        let (row, col) = coords_to_cell(width - 1.0, height - 1.0, cell_w, cell_h);
        // Cálculo esperado: (800-1)/10 = 79.9 -> trunc -> 79
        // (600-1)/20 = 599/20 = 29.95 -> trunc -> 29
        assert_eq!(
            col,
            ((width - 1.0) / cell_w as f64) as usize,
            "columna en el borde derecho"
        );
        assert_eq!(
            row,
            ((height - 1.0) / cell_h as f64) as usize,
            "fila en el borde inferior"
        );
    }

    /// ADVERSARIAL: Coordenadas NEGATIVAS.
    /// En Rust, casting de f32 negativo a usize satura a 0. Esto es un bug:
    /// un click ARRIBA o a la IZQUIERDA de la ventana (coordenadas negativas)
    /// seleccionaría la celda (0,0) como si el click hubiera sido en la
    /// primera celda del terminal.
    #[test]
    fn test_coords_negative_values() {
        let cell_w = 10.0;
        let cell_h = 20.0;

        // Click en (-50, -30) — fuera de la ventana, arriba-izquierda
        let (row, col) = coords_to_cell(-50.0, -30.0, cell_w, cell_h);
        assert_eq!(
            (row, col),
            (usize::MAX, usize::MAX),
            "BUG: click en (-50,-30) fuera de la ventana debe retornar sentinel, no (0,0)"
        );

        // Click en (-1, -1) — justo fuera del borde
        let (row, col) = coords_to_cell(-1.0, -1.0, cell_w, cell_h);
        assert_eq!(
            (row, col),
            (usize::MAX, usize::MAX),
            "BUG: click en (-1,-1) debe retornar sentinel"
        );
    }

    /// ADVERSARIAL: Valores enormes (f64::MAX) no deben panic.
    /// f64::MAX / cell_w -> inf en f32 -> inf as usize = usize::MAX.
    /// Esto puede causar index out of bounds si se usa como índice.
    #[test]
    fn test_coords_huge_values() {
        let cell_w = 10.0;
        let cell_h = 20.0;

        // f64::MAX -> f32::MAX? No: f64::MAX as f32 = f32::INFINITY
        let (row, col) = coords_to_cell(f64::MAX, f64::MAX, cell_w, cell_h);
        assert_eq!(
            col,
            usize::MAX,
            "BUG: f64::MAX / cell_w -> inf -> usize::MAX, posible index out of bounds"
        );
        assert_eq!(
            row,
            usize::MAX,
            "BUG: f64::MAX / cell_h -> inf -> usize::MAX, igual"
        );
    }

    /// ADVERSARIAL: cell_w=0 produce división por cero en f32.
    /// 100.0 / 0.0 = inf, inf as usize = usize::MAX.
    /// El código no protege contra cell_w=0 y produce un índice INVALIDO.
    #[test]
    fn test_division_by_zero_cell_w() {
        // cell_w=0 -> guard retorna sentinel en ambos ejes
        let (row, col) = coords_to_cell(100.0, 100.0, 0.0, 20.0);
        assert_eq!(
            (row, col),
            (usize::MAX, usize::MAX),
            "cell_w=0 debe retornar sentinel"
        );
    }

    /// Regresion: en shell normal (sin mouse reporting) el mouse es local,
    /// no se reenvia al PTY — de lo contrario la seleccion con raton no funciona.
    #[test]
    fn test_mouse_shell_uses_local_selection() {
        use crate::ansi::MouseReporting;

        let term = Arc::new(Mutex::new(Term::new()));
        let app = App::new(term.clone(), Arc::new(Mutex::new(None)), Config::default());
        assert!(
            !app.should_forward_mouse_to_app(),
            "shell: no reenviar mouse al PTY (seleccion local)"
        );

        term.lock().expect("term lock").mouse_reporting = MouseReporting {
            click: true,
            drag: true,
            any_motion: false,
            sgr: true,
        };
        let app_vim = App::new(term, Arc::new(Mutex::new(None)), Config::default());
        assert!(
            app_vim.should_forward_mouse_to_app(),
            "vim: app captura mouse sin modificadores"
        );
    }
}
