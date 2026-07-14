//! Ventana principal de Baud.
//!
//! App implementa ApplicationHandler<UserEvent> de winit 0.30.
//! El Renderer se inicializa en resumed() y se invoca en redraw_requested().
//! El Term se comparte con el hilo drain via Arc<Mutex<Term>>.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};

use crate::ansi::Term;
use crate::clipboard::{self, CopyTarget};
use crate::config::watch::WatchState;
use crate::config::{persist, Config, ConfigSource, ProcessSection, StartupState};
use crate::copy_mode::CopyModeState;
use crate::display_quirks::{self, DisplayQuirks};
use crate::event_loop::BlinkFocus;
use crate::grid::Cell;
use crate::input::actions::{normalize_binding_key, Action, Keybindings};
use crate::input::keymap::{self, Key as KKey, KeyEventKind, KeyModes, Mods};
use crate::input::wheel::{self, WheelIntent, WheelOwnerHint};
use crate::layout::{Rect as LayoutRect, TabLayout};
use crate::pty::PtyCommand;
use crate::renderer::{
    compute_layout, tab_bar_height_px, PaneRender, PreeditState, Renderer, TabBarLayout,
};
use crate::search::SearchState;
use crate::selection::{Selection, SelectionMode, SelectionPoint};
use crate::session::{Session, SessionId};
use crate::smart_select;
use crate::theme_picker::ThemePickerState;
use crate::watchdog::{self, EventLoopWatchdog};
use winit::application::ApplicationHandler;
use winit::event::ElementState;
use winit::event::Ime;
use winit::event::MouseButton;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::ControlFlow;
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorGrabMode, CursorIcon, Fullscreen, Window, WindowId};

/// Eventos enviados desde el hilo drain al hilo GUI.
#[derive(Debug)]
pub enum UserEvent {
    /// El drain termino de procesar bytes del PTY; la GUI debe redibujar.
    RedrawNeeded(SessionId),
    /// El child termino (EOF en master fd).
    PtyExited(SessionId, i32),
    /// Error de I/O del PTY.
    PtyError(SessionId, String),
    /// OSC 0/1/2: actualizar titulo de ventana.
    SetTitle(SessionId, String),
    /// OSC 52 query: leer clipboard y responder al PTY (target, bell_terminated).
    ReadClipboard(SessionId, u8, bool),
    /// OSC 52: texto ya leído fuera del hilo GUI.
    Osc52ReadReady(SessionId, u8, bool, String),
    /// Pegar en PTY: texto ya leído fuera del hilo GUI.
    PasteReady(String),
    /// Pegar en el buscador: texto ya leído fuera del hilo GUI.
    PasteSearchReady(String),
    /// Config recargada desde disco.
    ConfigReloaded(Box<Config>),
    /// Fallo al recargar config; se conserva la config en memoria.
    ConfigReloadFailed(String),
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
/// Ventana para doble/triple clic y retardo de copy-on-select.
const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(200);

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

    /// FPS promedio en la ventana de métricas actual (0 si sin datos).
    pub fn current_fps(&self) -> f32 {
        let elapsed = self.period_start.elapsed().as_secs_f64();
        if elapsed > 0.0 && self.redraws > 0 {
            self.redraws as f32 / elapsed as f32
        } else {
            0.0
        }
    }
}

/// Sesion con hilos PTY/drain asociados (opcionales en tests).
pub struct SessionHost {
    pub session: Session,
    drain_handle: Option<std::thread::JoinHandle<()>>,
    pty_handle: Option<std::thread::JoinHandle<()>>,
}

impl SessionHost {
    pub fn from_spawned(spawned: crate::event_loop::SpawnedSession) -> Self {
        Self {
            session: spawned.session,
            drain_handle: Some(spawned.drain_handle),
            pty_handle: Some(spawned.pty_handle),
        }
    }

    pub fn test(session: Session) -> Self {
        Self {
            session,
            drain_handle: None,
            pty_handle: None,
        }
    }

    fn join_threads(&mut self) {
        if let Some(h) = self.drain_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.pty_handle.take() {
            let _ = h.join();
        }
    }
}

/// Estado de la aplicación GUI.
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    sessions: Vec<SessionHost>,
    /// Layout de panes por tab (una entrada por tab).
    tabs: Vec<TabLayout>,
    /// Indice de la tab activa.
    focused: usize,
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
    /// Momento en que debe ejecutarse copy-on-select pendiente (tras multi-clic).
    copy_on_select_deadline: Option<Instant>,
    /// Selector interactivo de temas (exclusivo con copy mode).
    theme_picker: Option<ThemePickerState>,
    /// Modal de consentimiento de primer arranque activo.
    consent_prompt_active: bool,
    /// Estado del watcher de config (sync mtime tras persistir tema).
    config_watch: Arc<Mutex<WatchState>>,
    /// Texto provisional del IME (preedit) antes del commit.
    preedit: String,
    /// Rango del cursor dentro del preedit, en bytes (inicio, fin).
    preedit_cursor: Option<(usize, usize)>,
    /// Proxy al event loop para spawn de sesiones adicionales.
    proxy: Option<EventLoopProxy<UserEvent>>,
    /// Cerrar la ultima tab debe salir de la app en el proximo about_to_wait.
    pending_exit: bool,
    /// Sesiones cerradas cuyos hilos se unen al salir de la app.
    detached_hosts: Vec<SessionHost>,
    /// Tab bajo el cursor en la barra (indice de sesion).
    tab_hover: Option<usize>,
    /// Tab que renderiza el boton × (incluye fade-out).
    tab_close_tab: Option<usize>,
    /// Opacidad animada del boton × (0..1).
    tab_close_alpha: f32,
    /// Marca de tiempo para interpolar el fade del ×.
    tab_anim_last: Instant,
    /// Reintenta sync de grids cuando un pane estaba bloqueado por el drain.
    pending_pane_sync: bool,
    /// Feedback de carga de config pendiente hasta que exista renderer.
    pending_config_source: Option<ConfigSource>,
    /// Vigilancia de bloqueos del event loop.
    watchdog: EventLoopWatchdog,
    /// Intervalo mínimo entre redraws (ns). `0` = sin límite.
    redraw_interval_nanos: Arc<AtomicU64>,
    /// Overlay de FPS visible (requiere `debug.fps_counter_enabled`).
    fps_overlay_visible: bool,
    /// Pane activo para animacion de parpadeo (solo uno redibuja por blink).
    blink_focus: Arc<BlinkFocus>,
    /// Quirks de display resueltos una vez en `resumed`.
    display_quirks: DisplayQuirks,
    /// Acumulador residual para eventos de rueda (PixelDelta fraccionarios).
    wheel_residual: f32,
}

fn allowed_open_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "file://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn open_url(url: &str) {
    let Some(normalized) = crate::smart_select::normalize_url_for_open(url) else {
        tracing::warn!("open_url: URL no permitida: {}", url);
        return;
    };
    if !allowed_open_url(&normalized) {
        tracing::warn!("open_url: esquema no permitido: {}", normalized);
        return;
    }
    if let Err(e) = std::process::Command::new("xdg-open")
        .arg(&normalized)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        tracing::warn!("open_url: xdg-open fallo para {}: {e}", normalized);
    }
}

impl App {
    /// Crea una nueva instancia de App con las sesiones dadas.
    pub fn new(
        sessions: Vec<SessionHost>,
        config: Config,
        config_watch: Arc<Mutex<WatchState>>,
        proxy: Option<EventLoopProxy<UserEvent>>,
        blink_focus: Arc<BlinkFocus>,
        config_source: ConfigSource,
        watchdog: EventLoopWatchdog,
    ) -> Self {
        debug_assert!(!sessions.is_empty(), "App requiere al menos una sesion");
        let font_size = config.font.size;
        let window_width = config.window.width as f32;
        let window_height = config.window.height as f32;
        let keybindings = config.keybindings();
        let redraw_interval_nanos = Arc::new(AtomicU64::new(config.render.redraw_interval_nanos()));
        let tabs: Vec<TabLayout> = sessions
            .iter()
            .map(|h| TabLayout::new(h.session.id))
            .collect();
        Self {
            window: None,
            renderer: None,
            sessions,
            tabs,
            focused: 0,
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
            copy_on_select_deadline: None,
            theme_picker: None,
            consent_prompt_active: false,
            config_watch,
            preedit: String::new(),
            preedit_cursor: None,
            proxy,
            pending_exit: false,
            detached_hosts: Vec::new(),
            tab_hover: None,
            tab_close_tab: None,
            tab_close_alpha: 0.0,
            tab_anim_last: Instant::now(),
            pending_pane_sync: false,
            pending_config_source: Some(config_source),
            watchdog,
            redraw_interval_nanos,
            fps_overlay_visible: false,
            blink_focus,
            display_quirks: DisplayQuirks::DEFAULT,
            wheel_residual: 0.0,
        }
    }

    pub fn set_redraw_interval_handle(&mut self, redraw_interval_nanos: Arc<AtomicU64>) {
        self.redraw_interval_nanos = redraw_interval_nanos;
    }

    fn sync_blink_focus(&self) {
        if let Some(tab) = self.tabs.get(self.focused) {
            self.blink_focus.set(tab.focused());
        }
    }

    /// Espera a que terminen los hilos de todas las sesiones.
    pub fn join_session_threads(&mut self) {
        for host in &mut self.sessions {
            host.join_threads();
        }
        for host in &mut self.detached_hosts {
            host.join_threads();
        }
        self.detached_hosts.clear();
    }

    fn focused_session(&self) -> &Session {
        let id = self.tabs[self.focused].focused();
        let idx = self
            .session_by_id(id)
            .expect("sesion enfocada debe existir");
        &self.sessions[idx].session
    }

    fn terminal_area_rect(&self, width: u32, height: u32, cell_w: f32, cell_h: f32) -> LayoutRect {
        let reserved = self.tab_bar_rows();
        let chrome_extra = if reserved > 0 {
            crate::renderer::TAB_CONTENT_GAP_PX
        } else {
            0.0
        };
        let (rows, cols) = crate::renderer::limits::compute_grid_dims(
            width,
            height,
            cell_w,
            cell_h,
            self.config.window.padding_x,
            self.config.window.padding_y,
            reserved,
            chrome_extra,
        );
        LayoutRect {
            x: 0,
            y: 0,
            cols,
            rows,
        }
    }

    /// Cambia la sesion enfocada; redibuja si la nueva sesion tiene output pendiente.
    #[allow(dead_code)]
    pub(crate) fn focus_session(&mut self, index: usize) {
        debug_assert!(index < self.tabs.len());
        if index == self.focused {
            return;
        }
        self.focused = index;
        self.apply_focused_window_title();
        self.sync_blink_focus();
        if let Some(id) = self.tabs.get(index).map(TabLayout::focused) {
            if let Some(idx) = self.session_by_id(id) {
                self.sessions[idx].session.dirty = false;
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    #[allow(dead_code)]
    fn apply_focused_window_title(&self) {
        if let Some(window) = &self.window {
            let title = &self.focused_session().title;
            if !title.is_empty() {
                window.set_title(title);
            }
        }
    }

    fn is_session_in_active_tab(&self, id: SessionId) -> bool {
        self.tabs
            .get(self.focused)
            .is_some_and(|t| t.leaves().contains(&id))
    }

    pub(crate) fn send_startup_input(&self, bytes: Vec<u8>) {
        let _ = self.focused_session().pty_tx.send(PtyCommand::Input(bytes));
    }

    /// Despacha un evento de usuario (usado por el event loop y tests).
    pub(crate) fn dispatch_user_event(&mut self, event: UserEvent) {
        match event {
            UserEvent::RedrawNeeded(id) => {
                if self.is_focused_session(id) {
                    let idx = self.session_by_id(id);
                    let deferred = idx
                        .and_then(|i| self.sessions[i].session.term.try_lock().ok())
                        .map(|term| term.should_defer_redraw())
                        .unwrap_or(false);
                    if deferred {
                        // Mantener dirty; el timer periodico reintenta tras el timeout.
                        if let Some(i) = idx {
                            self.sessions[i].session.dirty = true;
                        }
                    } else {
                        if let Some(i) = idx {
                            self.sessions[i].session.dirty = false;
                        }
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                } else if self.is_session_in_active_tab(id) {
                    if let Some(idx) = self.session_by_id(id) {
                        self.sessions[idx].session.dirty = true;
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if let Some(idx) = self.session_by_id(id) {
                    self.sessions[idx].session.dirty = true;
                }
            }
            UserEvent::PtyExited(id, code) => {
                if self.is_session_in_active_tab(id) {
                    if self.is_focused_session(id) {
                        if let Some(renderer) = &mut self.renderer {
                            renderer.set_status(&format!("[Proceso terminado: codigo {}]", code));
                        }
                    } else if self.tabs[self.focused].leaves().len() > 1 {
                        if let Some(renderer) = &mut self.renderer {
                            renderer.set_status(&format!("[Pane cerrado: codigo {}]", code));
                        }
                        self.close_pane_session(id);
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            UserEvent::PtyError(id, msg) => {
                if self.is_focused_session(id) {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_status(&format!("[Error PTY: {}]", msg));
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if self.is_session_in_active_tab(id) {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_status(&format!("[Error PTY en pane: {}]", msg));
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            UserEvent::SetTitle(id, title) => {
                if let Some(idx) = self.session_by_id(id) {
                    self.sessions[idx].session.title = title.clone();
                    if self.is_focused_session(id) {
                        if let Some(window) = &self.window {
                            window.set_title(&title);
                        }
                    }
                }
            }
            UserEvent::ReadClipboard(id, target, bell_terminated) => {
                if !self.is_focused_session(id) {
                    return;
                }
                let primary = target == b'p' || target == b's';
                self.spawn_clipboard_get(primary, move |text| {
                    UserEvent::Osc52ReadReady(id, target, bell_terminated, text)
                });
            }
            UserEvent::Osc52ReadReady(id, target, bell_terminated, text) => {
                if !self.is_focused_session(id) {
                    return;
                }
                let encoded = crate::base64::encode(text.as_bytes());
                let response = Term::format_osc52_read_response(target, &encoded, bell_terminated);
                self.send_input(response);
            }
            UserEvent::PasteReady(text) => {
                self.paste_text(&text);
            }
            UserEvent::PasteSearchReady(text) => {
                if text.is_empty() {
                    return;
                }
                let text = text.replace(['\n', '\r'], "");
                if let Ok(mut guard) = self.focused_term().lock() {
                    guard.search_append_query(&text);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::ConfigReloaded(cfg) => {
                if self.theme_picker.is_some() {
                    tracing::debug!("config: reload omitido — theme picker activo");
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_status("[Config: reload omitido — theme picker activo]");
                    }
                } else {
                    let restart_msg = self.apply_config(*cfg);
                    if let Some(renderer) = &mut self.renderer {
                        let status = if let Some(msg) = restart_msg {
                            format!("[Config recargada — {msg}]")
                        } else {
                            "[Config recargada]".into()
                        };
                        renderer.set_status(&status);
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            UserEvent::ConfigReloadFailed(msg) => {
                tracing::warn!("config: recarga fallida: {msg}");
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status("[Config: error de parseo — se mantuvo la anterior]");
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }

    fn session_by_id(&self, id: SessionId) -> Option<usize> {
        self.sessions.iter().position(|h| h.session.id == id)
    }

    fn is_focused_session(&self, id: SessionId) -> bool {
        self.tabs
            .get(self.focused)
            .is_some_and(|t| t.focused() == id)
    }

    fn focused_term(&self) -> &Arc<Mutex<Term>> {
        &self.focused_session().term
    }

    fn cursor_visible_cell(&self) -> (usize, usize) {
        self.focused_term()
            .lock()
            .map(|guard| {
                let col = guard.cursor.col;
                let row =
                    crate::copy_mode::logical_to_visible_row(&guard, guard.cursor_logical_row())
                        .unwrap_or(guard.cursor.row);
                (row, col)
            })
            .unwrap_or((0, 0))
    }

    fn update_ime_area(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (row, col) = self.cursor_visible_cell();
        let (pad_x, pad_y) = renderer.grid_padding();
        let cell_w = renderer.cell_w();
        let cell_h = renderer.cell_h();
        let area = self.terminal_area_rect(
            self.window_width as u32,
            self.window_height as u32,
            cell_w,
            cell_h,
        );
        let focused_id = self.tabs[self.focused].focused();
        let pane_rect = self.tabs[self.focused]
            .layout()
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == focused_id)
            .map(|(_, r)| r)
            .unwrap_or(area);
        let x = pad_x + (pane_rect.x as f32 + col as f32) * cell_w;
        let y = pad_y + (pane_rect.y as f32 + row as f32) * cell_h;
        window.set_ime_cursor_area(
            winit::dpi::PhysicalPosition::new(x as i32, y as i32),
            winit::dpi::PhysicalSize::new(cell_w as u32, cell_h as u32),
        );
    }

    fn effective_theme(&self) -> crate::config::ThemeConfig {
        self.theme_picker
            .as_ref()
            .map(ThemePickerState::preview_theme)
            .unwrap_or_else(|| self.config.theme.clone())
    }

    fn process_section_changed(prev: &ProcessSection, next: &ProcessSection) -> bool {
        prev.program != next.program
            || prev.args != next.args
            || prev.working_directory != next.working_directory
            || prev.env != next.env
            || prev.startup_command != next.startup_command
            || prev.login != next.login
    }

    fn restart_required_fields(prev: &Config, next: &Config) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if prev.window.decorations != next.window.decorations {
            fields.push("window.decorations");
        }
        if prev.window.startup != next.window.startup {
            fields.push("window.startup");
        }
        if (prev.window.opacity < 1.0) != (next.window.opacity < 1.0) {
            fields.push("window.opacity");
        }
        if prev.window.width != next.window.width || prev.window.height != next.window.height {
            fields.push("window.width/height");
        }
        if Self::process_section_changed(&prev.process, &next.process) {
            fields.push("process");
        }
        fields
    }

    /// Aplica una config recargada: tema, fuente, atajos, cursor, scrollback y toggles.
    ///
    /// Devuelve mensaje si hay campos que requieren reinicio.
    fn apply_config(&mut self, new_cfg: Config) -> Option<String> {
        let prev = self.config.clone();
        let restart_fields = Self::restart_required_fields(&prev, &new_cfg);

        self.keybindings = new_cfg.keybindings();
        self.font_size = new_cfg.font.size;

        if let Ok(mut term) = self.focused_term().lock() {
            new_cfg.apply_to_term(&mut term);
            let max = new_cfg.scrollback_max_lines();
            term.grid.set_max_scrollback(max);
            term.alt_grid.set_max_scrollback(max);
            term.mark_dirty();
        }

        if let Some(renderer) = &mut self.renderer {
            renderer.apply_font_config(&new_cfg.font, self.font_size);
            renderer.set_content_padding(new_cfg.window.padding_x, new_cfg.window.padding_y);
        }
        if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
            let size = window.inner_size();
            let (_, _, _, _, deferred) = self.sync_grid_to_window(
                size.width,
                size.height,
                renderer.cell_w,
                renderer.cell_h,
                true,
                false,
            );
            self.pending_pane_sync = deferred;
        }

        self.config = new_cfg;
        self.redraw_interval_nanos.store(
            self.config.render.redraw_interval_nanos(),
            Ordering::Relaxed,
        );

        if !self.config.debug.fps_counter_enabled && self.fps_overlay_visible {
            self.fps_overlay_visible = false;
            if let Some(renderer) = &mut self.renderer {
                renderer.set_status("");
            }
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }

        if restart_fields.is_empty() {
            None
        } else {
            let msg = format!(
                "Config: {} requiere reinicio para aplicarse",
                restart_fields.join(", ")
            );
            tracing::info!("{msg}");
            Some(msg)
        }
    }

    /// Copia texto al clipboard del sistema sin bloquear el hilo GUI.
    fn set_clipboard(&self, text: &str) {
        tracing::info!("set_clipboard: {} bytes (detached)", text.len());
        clipboard::set_detached(text.to_owned(), false);
    }

    /// Lee el clipboard en un hilo worker y reinyecta el resultado vía `UserEvent`.
    fn spawn_clipboard_get(
        &self,
        primary: bool,
        map: impl FnOnce(String) -> UserEvent + Send + 'static,
    ) {
        let Some(proxy) = self.proxy.clone() else {
            tracing::warn!("clipboard get: sin EventLoopProxy; omitiendo lectura");
            return;
        };
        let _ = std::thread::Builder::new()
            .name("baud-clipboard-get".into())
            .spawn(move || {
                let text = clipboard::get(primary);
                let _ = proxy.send_event(map(text));
            });
    }

    /// Sincroniza grid emulado y PTY con el tamano de ventana en pixeles.
    /// Sincroniza el grid con el tamano de ventana y el layout de panes activo.
    /// Devuelve `true` si alguna sesion no pudo redimensionarse (mutex ocupado).
    fn sync_grid_to_window(
        &mut self,
        width: u32,
        height: u32,
        cell_w: f32,
        cell_h: f32,
        preserve_scrollback: bool,
        reflow: bool,
    ) -> (usize, usize, usize, usize, bool) {
        let reserved = self.tab_bar_rows();
        if let Some(renderer) = &mut self.renderer {
            let chrome_px = if reserved > 0 {
                crate::renderer::tab_chrome_reserve_px(cell_h)
            } else {
                0.0
            };
            renderer.set_grid_top_offset(chrome_px);
        }
        let focused_id = self.focused_session().id;
        let area = self.terminal_area_rect(width, height, cell_w, cell_h);
        let mult = self.config.panes.split_width_multiplier;
        self.tabs[self.focused].recalc_dwindle_orients(area, mult);
        let pane_rects = self.tabs[self.focused].layout().rects(area);
        let mut deferred = false;
        for host in &mut self.sessions {
            let pane = pane_rects
                .iter()
                .find(|(id, _)| *id == host.session.id)
                .map(|(_, r)| r);
            let (new_rows, new_cols) = if let Some(r) = pane {
                (r.rows, r.cols)
            } else if let Ok(guard) = host.session.term.try_lock() {
                let active = guard.active_grid();
                (active.rows_count, active.cols_count)
            } else {
                deferred = true;
                host.session.dirty = true;
                continue;
            };
            let Ok(mut guard) = host.session.term.try_lock() else {
                deferred = true;
                host.session.dirty = true;
                continue;
            };
            let active = guard.active_grid();
            let old_r = active.rows_count;
            let old_c = active.cols_count;
            guard.resize_grid(new_rows, new_cols, reflow);
            if preserve_scrollback && host.session.id == focused_id {
                let max_offset = guard.scrollback_len();
                guard.scrollback_offset = guard.scrollback_offset.min(max_offset as isize);
            } else if !preserve_scrollback {
                guard.scrollback_offset = 0;
            }
            if old_r != new_rows || old_c != new_cols {
                let _ = host.session.pty_tx.send(PtyCommand::Resize {
                    rows: new_rows as u16,
                    cols: new_cols as u16,
                });
            }
        }
        let (old_rows, old_cols) = if let Ok(guard) = self.focused_term().try_lock() {
            let active = guard.active_grid();
            (active.rows_count, active.cols_count)
        } else {
            (area.rows, area.cols)
        };
        (old_rows, old_cols, area.rows, area.cols, deferred)
    }

    fn tab_bar_rows(&self) -> usize {
        if self.tabs.len() > 1 {
            crate::renderer::TAB_BAR_HEIGHT_ROWS
        } else {
            0
        }
    }

    fn config_with_cwd(&self, cwd: Option<String>) -> Config {
        let mut cfg = self.config.clone();
        if let Some(dir) = cwd {
            cfg.process.working_directory = Some(dir);
        }
        cfg
    }

    fn sync_after_tab_change(&mut self) {
        let (width, height, cell_w, cell_h) = {
            let Some(window) = &self.window else {
                return;
            };
            let Some(renderer) = &self.renderer else {
                return;
            };
            let size = window.inner_size();
            (size.width, size.height, renderer.cell_w, renderer.cell_h)
        };
        let (_, _, _, _, deferred) =
            self.sync_grid_to_window(width, height, cell_w, cell_h, true, false);
        self.pending_pane_sync = deferred;
        if deferred {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn tab_bar_layout(&self, renderer: &Renderer) -> Option<TabBarLayout> {
        if self.tabs.len() <= 1 {
            return None;
        }
        let titles: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                self.session_by_id(tab.focused())
                    .map(|idx| self.sessions[idx].session.title.clone())
            })
            .collect();
        let (pad_x, _) = renderer.content_padding();
        let bar_w = crate::renderer::tab_bar_inner_width(self.window_width, pad_x);
        Some(compute_layout(
            &titles,
            self.focused,
            pad_x,
            bar_w,
            renderer.cell_w(),
        ))
    }

    fn tab_bar_layout_with_mouse(&self, renderer: &Renderer) -> Option<TabBarLayout> {
        let mut layout = self.tab_bar_layout(renderer)?;
        layout.mouse.hover_index = self.tab_hover;
        layout.mouse.close_tab = self.tab_close_tab;
        layout.mouse.close_alpha = self.tab_close_alpha;
        Some(layout)
    }

    fn tick_tab_close_fade(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            self.tab_hover = None;
            self.tab_close_tab = None;
            if self.tab_close_alpha > 0.0 {
                self.tab_close_alpha = 0.0;
                return true;
            }
            return false;
        }
        if self.tab_hover.is_some() {
            return false;
        }
        let Some(_) = self.tab_close_tab else {
            return false;
        };
        let prev = self.tab_close_alpha;
        let dt = self.tab_anim_last.elapsed().as_secs_f32().min(0.05);
        self.tab_anim_last = Instant::now();
        self.tab_close_alpha += (0.0 - self.tab_close_alpha) * (32.0 * dt).min(1.0);
        if self.tab_close_alpha < 0.02 {
            self.tab_close_alpha = 0.0;
            self.tab_close_tab = None;
        }
        (self.tab_close_alpha - prev).abs() > 0.005
    }

    fn update_tab_hover(&mut self, x: f64, y: f64) -> bool {
        let Some(renderer) = &self.renderer else {
            return false;
        };
        if !self.is_in_tab_bar_row(y, renderer) {
            if self.tab_hover.is_some() {
                self.tab_hover = None;
                self.tab_anim_last = Instant::now();
                return true;
            }
            return false;
        }
        let new_hover = self.tab_index_at(x, y, renderer);
        if new_hover != self.tab_hover {
            self.tab_hover = new_hover;
            self.tab_anim_last = Instant::now();
            if let Some(idx) = new_hover {
                self.tab_close_tab = Some(idx);
                self.tab_close_alpha = 1.0;
            }
            return true;
        }
        false
    }

    fn new_tab(&mut self) {
        let Some(proxy) = self.proxy.clone() else {
            tracing::warn!("new_tab: proxy no disponible");
            return;
        };
        let cwd = self.focused_term().lock().ok().and_then(|t| t.cwd.clone());
        let cfg = self.config_with_cwd(cwd);
        let (rows, cols) = self
            .focused_term()
            .lock()
            .ok()
            .map(|g| {
                let grid = g.active_grid();
                (grid.rows_count as u16, grid.cols_count as u16)
            })
            .unwrap_or((
                crate::grid::DEFAULT_ROWS as u16,
                crate::grid::DEFAULT_COLS as u16,
            ));
        let spawned = match crate::event_loop::spawn_session(
            &cfg,
            rows,
            cols,
            proxy.clone(),
            Arc::clone(&self.redraw_interval_nanos),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("new_tab: spawn fallo: {e}");
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[No se pudo abrir tab: {e}]"));
                }
                return;
            }
        };
        crate::event_loop::spawn_blink_timer(
            Arc::clone(&spawned.session.term),
            proxy,
            spawned.session.id,
            Arc::clone(&self.blink_focus),
        );
        self.sessions.push(SessionHost::from_spawned(spawned));
        let new_id = self
            .sessions
            .last()
            .expect("session recien creada")
            .session
            .id;
        self.tabs.push(TabLayout::new(new_id));
        self.focused = self.tabs.len() - 1;
        self.sync_blink_focus();
        self.apply_focused_window_title();
        self.sync_after_tab_change();
    }

    fn close_tab(&mut self) {
        self.close_tab_at(self.focused);
    }

    fn close_tab_at(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs.len() <= 1 {
            for host in &self.sessions {
                let _ = host.session.pty_tx.send(PtyCommand::Shutdown);
            }
            self.pending_exit = true;
            return;
        }
        let leaf_ids = self.tabs[index].leaves();
        self.tabs.remove(index);
        let mut indices: Vec<usize> = leaf_ids
            .iter()
            .filter_map(|id| self.session_by_id(*id))
            .collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for idx in indices {
            let host = self.sessions.remove(idx);
            let _ = host.session.pty_tx.send(PtyCommand::Shutdown);
            self.detached_hosts.push(host);
        }
        if self.focused > index {
            self.focused -= 1;
        } else if self.focused == index {
            self.focused = index.min(self.tabs.len().saturating_sub(1));
        }
        self.tab_hover = None;
        self.tab_close_tab = None;
        self.tab_close_alpha = 0.0;
        self.sync_blink_focus();
        self.apply_focused_window_title();
        self.sync_after_tab_change();
    }

    fn split_pane(&mut self) {
        let Some(proxy) = self.proxy.clone() else {
            tracing::warn!("split_pane: proxy no disponible");
            return;
        };
        let tab_idx = self.focused;
        let focused_id = self.tabs[tab_idx].focused();

        if let Some(max) = self.config.panes_max() {
            if self.tabs[tab_idx].leaves().len() >= max {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Limite de {max} panes alcanzado]"));
                }
                return;
            }
        }

        let (cell_w, cell_h, win_w, win_h) = match (&self.renderer, &self.window) {
            (Some(r), Some(w)) => {
                let s = w.inner_size();
                (r.cell_w, r.cell_h, s.width, s.height)
            }
            (Some(r), None) => (
                r.cell_w,
                r.cell_h,
                self.window_width as u32,
                self.window_height as u32,
            ),
            _ => {
                tracing::warn!("split_pane: renderer no disponible");
                return;
            }
        };
        let mult = self.config.panes.split_width_multiplier;
        let preserve = self.config.effective_preserve_split();
        let area = self.terminal_area_rect(win_w, win_h, cell_w, cell_h);
        self.tabs[tab_idx].recalc_dwindle_orients(area, mult);
        let focused_rect = self.tabs[tab_idx]
            .layout()
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == focused_id)
            .map(|(_, r)| r)
            .unwrap_or(area);

        let (orient, old_first) = if self.config.panes.smart_split {
            let Some(renderer) = &self.renderer else {
                return;
            };
            let (mouse_row, mouse_col) =
                self.mouse_cell_coords_in_focused_pane(renderer, &focused_rect);
            let p = crate::layout::smart_split_decision(focused_rect, mouse_col, mouse_row);
            let orient = if crate::layout::can_split(
                focused_rect,
                p.orient,
                crate::layout::MIN_PANE_COLS,
                crate::layout::MIN_PANE_ROWS,
            ) {
                p.orient
            } else {
                match p.orient {
                    crate::layout::Orientation::Vertical => crate::layout::Orientation::Horizontal,
                    crate::layout::Orientation::Horizontal => crate::layout::Orientation::Vertical,
                }
            };
            if !crate::layout::can_split(
                focused_rect,
                orient,
                crate::layout::MIN_PANE_COLS,
                crate::layout::MIN_PANE_ROWS,
            ) {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status("[Pane demasiado pequeno para dividir]");
                }
                return;
            }
            let old_first = if orient == p.orient {
                p.old_first
            } else {
                true
            };
            (orient, old_first)
        } else {
            let Some(orient) = crate::layout::dwindle_split_orient(focused_rect, mult) else {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status("[Pane demasiado pequeno para dividir]");
                }
                return;
            };
            (orient, true)
        };

        let (rect_a, rect_b) = crate::layout::split_rect(focused_rect, orient, 0.5);
        let (old_rect, new_rect) = if old_first {
            (rect_a, rect_b)
        } else {
            (rect_b, rect_a)
        };
        tracing::info!(
            "split_pane: {}x{} -> {}x{} + {}x{}",
            focused_rect.cols,
            focused_rect.rows,
            old_rect.cols,
            old_rect.rows,
            new_rect.cols,
            new_rect.rows
        );

        let cwd = self
            .focused_term()
            .try_lock()
            .ok()
            .and_then(|t| t.cwd.clone());
        let cfg = self.config_with_cwd(cwd);

        if let Some(idx) = self.session_by_id(focused_id) {
            let host = &self.sessions[idx];
            if let Ok(mut guard) = host.session.term.try_lock() {
                guard.resize_grid(old_rect.rows, old_rect.cols, false);
                drop(guard);
                let _ = host.session.pty_tx.send(PtyCommand::Resize {
                    rows: old_rect.rows as u16,
                    cols: old_rect.cols as u16,
                });
            } else {
                self.pending_pane_sync = true;
            }
        }

        let spawned = match crate::event_loop::spawn_session(
            &cfg,
            new_rect.rows as u16,
            new_rect.cols as u16,
            proxy.clone(),
            Arc::clone(&self.redraw_interval_nanos),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("split_pane: spawn fallo: {e}");
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[No se pudo abrir pane: {e}]"));
                }
                return;
            }
        };
        crate::event_loop::spawn_blink_timer(
            Arc::clone(&spawned.session.term),
            proxy,
            spawned.session.id,
            Arc::clone(&self.blink_focus),
        );
        let new_id = spawned.session.id;
        self.sessions.push(SessionHost::from_spawned(spawned));
        self.tabs[tab_idx].split_dwindle_ordered(new_id, orient, preserve, old_first);
        for id in self.tabs[tab_idx].leaves() {
            if let Some(idx) = self.session_by_id(id) {
                self.sessions[idx].session.dirty = true;
            }
        }
        self.apply_focused_window_title();
        self.sync_blink_focus();
        self.sync_after_tab_change();
    }

    fn close_pane(&mut self) {
        let Some(closed_id) = self.tabs[self.focused].close_focused() else {
            return;
        };
        self.remove_pane_session(closed_id);
    }

    fn close_pane_session(&mut self, closed_id: SessionId) {
        if self.tabs[self.focused].close_pane(closed_id).is_none() {
            return;
        }
        self.remove_pane_session(closed_id);
    }

    fn remove_pane_session(&mut self, closed_id: SessionId) {
        if let Some(idx) = self.session_by_id(closed_id) {
            let host = self.sessions.remove(idx);
            let _ = host.session.pty_tx.send(PtyCommand::Shutdown);
            self.detached_hosts.push(host);
        }
        self.apply_focused_window_title();
        self.sync_blink_focus();
        self.sync_after_tab_change();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn mark_tab_panes_dirty_for_chrome(&mut self) {
        for id in self.tabs[self.focused].leaves() {
            if let Some(idx) = self.session_by_id(id) {
                self.sessions[idx].session.dirty = true;
                if let Ok(mut guard) = self.sessions[idx].session.term.try_lock() {
                    guard.mark_dirty();
                }
            }
        }
    }

    fn focus_pane_by_id(&mut self, id: SessionId) -> bool {
        if !self.tabs[self.focused].focus_pane(id) {
            return false;
        }
        self.sync_blink_focus();
        self.mark_tab_panes_dirty_for_chrome();
        self.apply_focused_window_title();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        true
    }

    fn focus_next_pane(&mut self) {
        self.tabs[self.focused].focus_next();
        self.sync_blink_focus();
        self.mark_tab_panes_dirty_for_chrome();
        self.apply_focused_window_title();
    }

    fn focus_prev_pane(&mut self) {
        self.tabs[self.focused].focus_prev();
        self.sync_blink_focus();
        self.mark_tab_panes_dirty_for_chrome();
        self.apply_focused_window_title();
    }

    fn focus_pane_direction(&mut self, dir: crate::layout::Direction) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (cell_w, cell_h) = (renderer.cell_w(), renderer.cell_h());
        let area = self.terminal_area_rect(
            self.window_width as u32,
            self.window_height as u32,
            cell_w,
            cell_h,
        );
        if self.tabs[self.focused].focus_direction(area, dir) {
            self.sync_blink_focus();
            self.mark_tab_panes_dirty_for_chrome();
            self.apply_focused_window_title();
        }
    }

    fn toggle_split(&mut self) {
        if self.tabs[self.focused].toggle_split_focused() {
            self.sync_after_tab_change();
        }
    }

    fn swap_split(&mut self) {
        if self.tabs[self.focused].swap_split_focused() {
            self.sync_after_tab_change();
        }
    }

    fn next_tab(&mut self) {
        let len = self.tabs.len();
        if len <= 1 {
            return;
        }
        self.focused = (self.focused + 1) % len;
        self.sync_blink_focus();
        self.apply_focused_window_title();
        self.sync_after_tab_change();
    }

    fn prev_tab(&mut self) {
        let len = self.tabs.len();
        if len <= 1 {
            return;
        }
        self.focused = (self.focused + len - 1) % len;
        self.sync_blink_focus();
        self.apply_focused_window_title();
        self.sync_after_tab_change();
    }

    fn goto_tab(&mut self, n: u8) {
        let len = self.tabs.len();
        if len == 0 || n == 0 {
            return;
        }
        self.focused = ((n as usize) - 1).min(len - 1);
        self.sync_blink_focus();
        self.apply_focused_window_title();
        self.sync_after_tab_change();
    }

    fn tab_index_at(&self, x: f64, y: f64, renderer: &Renderer) -> Option<usize> {
        let layout = self.tab_bar_layout_with_mouse(renderer)?;
        let (_, pad_y) = renderer.content_padding();
        crate::renderer::tab_index_at(&layout, x, y, pad_y, tab_bar_height_px(renderer.cell_h()))
    }

    fn tab_close_at(&self, x: f64, y: f64, renderer: &Renderer) -> Option<usize> {
        let layout = self.tab_bar_layout_with_mouse(renderer)?;
        let (_, pad_y) = renderer.content_padding();
        crate::renderer::tab_close_at(
            &layout,
            x,
            y,
            pad_y,
            tab_bar_height_px(renderer.cell_h()),
            renderer.cell_w(),
        )
    }

    fn is_in_tab_bar_row(&self, y: f64, renderer: &Renderer) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        let (_, pad_y) = renderer.content_padding();
        let top = f64::from(pad_y);
        let bottom = top + f64::from(tab_bar_height_px(renderer.cell_h()));
        (top..bottom).contains(&y)
    }

    /// Copia al clipboard: si hay selección activa, copia solo la selección;
    /// si hay búsqueda activa con match, copia el texto del match;
    /// si no, retorna sin copiar nada.
    fn handle_copy(&mut self) {
        tracing::info!("handle_copy: INICIANDO");
        let text = {
            let term_guard = match self.focused_term().lock() {
                Ok(g) => g,
                Err(poisoned) => {
                    tracing::warn!("handle_copy: term mutex poisoned: {poisoned}");
                    return;
                }
            };
            if let Some(search_text) = term_guard.search_current_match_text() {
                if !search_text.is_empty() {
                    tracing::info!("handle_copy: copiando match de busqueda");
                    search_text
                } else {
                    return;
                }
            } else if let Some(ref sel) = term_guard.selection {
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
            renderer.set_status_with_config(
                "Copiado al clipboard",
                "✓",
                &self.config.theme,
                &self.config.status,
            );
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

    /// True si la selección actual merece copy-on-select (no un clic suelto).
    fn selection_qualifies_for_copy_on_select(&self) -> bool {
        let Ok(guard) = self.focused_term().lock() else {
            return false;
        };
        guard
            .selection
            .as_ref()
            .is_some_and(Self::selection_qualifies)
    }

    fn selection_qualifies(sel: &Selection) -> bool {
        match sel.mode {
            SelectionMode::Word | SelectionMode::Smart | SelectionMode::Line => true,
            SelectionMode::Normal | SelectionMode::Block => {
                let (sr, sc, er, ec) = sel.normalize();
                sr != er || sc != ec
            }
        }
    }

    /// Ejecuta copy-on-select: copia, limpia la selección y muestra estado.
    fn finish_copy_on_select(&mut self) {
        if !self.config.selection.copy_on_select {
            return;
        }
        let text = match self.focused_term().lock() {
            Ok(g) => g.selected_text(),
            Err(_) => return,
        };
        if text.is_empty() {
            tracing::debug!("copy_on_select: seleccion vacia, sin copiar");
            return;
        }
        let target = CopyTarget::parse(&self.config.selection.copy_on_select_target);
        tracing::info!("copy_on_select: {} bytes -> {}", text.len(), target.label());
        target.write(&text);
        if let Ok(mut guard) = self.focused_term().lock() {
            guard.clear_selection();
            guard.mark_dirty();
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_status_with_config(
                &format!("Copiado ({})", target.label()),
                "✓",
                &self.config.theme,
                &self.config.status,
            );
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn schedule_copy_on_select(&mut self) {
        if !self.config.selection.copy_on_select {
            return;
        }
        if !self.selection_qualifies_for_copy_on_select() {
            return;
        }
        let delay = self.config.selection.copy_on_select_delay();
        if delay.is_zero() {
            self.finish_copy_on_select();
            return;
        }
        self.copy_on_select_deadline = Some(Instant::now() + delay);
    }

    fn cancel_copy_on_select(&mut self) {
        self.copy_on_select_deadline = None;
    }

    fn paste_to_search(&mut self, primary: bool) {
        tracing::debug!("paste_to_search: lectura detached (primary={primary})");
        self.spawn_clipboard_get(primary, UserEvent::PasteSearchReady);
    }

    /// Encola lectura del clipboard y pega en el PTY cuando el worker responde.
    /// Si bracketed paste mode (DEC 2004) esta activo, envuelve el texto en
    /// \x1b[200~...\x1b[201~ para que readline no ejecute comandos al pegar.
    fn handle_paste(&mut self) {
        tracing::debug!("handle_paste: lectura detached");
        self.spawn_clipboard_get(false, UserEvent::PasteReady);
    }

    /// Pega desde la primary selection (botón medio del mouse).
    fn handle_paste_primary(&mut self) {
        tracing::debug!("handle_paste_primary: lectura detached");
        self.spawn_clipboard_get(true, UserEvent::PasteReady);
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
            .focused_term()
            .lock()
            .ok()
            .map(|t| t.bracketed_paste)
            .unwrap_or(false);
        let filtered = if bracketed {
            crate::input::paste_with_bracketing(&text, true)
        } else {
            crate::input::paste_text(&text)
        };
        let _ = self
            .focused_session()
            .pty_tx
            .send(PtyCommand::Input(filtered));
    }

    /// Envia bytes de input al hilo PTY para escribirlos en el master fd.
    fn send_input(&self, bytes: Vec<u8>) {
        // Resetear scrollback offset al enviar cualquier input al PTY
        if let Ok(mut guard) = self.focused_term().lock() {
            if guard.scrollback_offset > 0 {
                guard.scrollback_offset = 0;
            }
            // Limpiar seleccion al escribir teclas (no en copy mode).
            if guard.copy_mode.is_none() {
                guard.clear_selection();
            }
            guard.reset_blink_phase();
            // El PTY puede no generar ningun eco (Space, Delete en linea vacia, Tab...),
            // en cuyo caso term.dirty seguiria en false y el guard de RedrawRequested
            // saltaria el repintado. Marcar dirty aqui cubre todo byte escrito por el
            // usuario sin enumerar teclas sin eco una por una.
            guard.mark_dirty();
        }
        tracing::debug!("send_input: {} bytes: {:02x?}", bytes.len(), bytes);
        let _ = self.focused_session().pty_tx.send(PtyCommand::Input(bytes));
    }

    /// Extiende la seleccion con teclado (Shift+arrow).
    /// Si no hay seleccion, crea una desde la posicion del cursor.
    fn extend_selection(&self, drow: isize, dcol: isize) {
        if let Ok(mut guard) = self.focused_term().lock() {
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
        let mut guard = self.focused_term().lock().expect("term mutex poisoned");
        if n > 0 {
            if !guard.alt_screen {
                let max_offset = guard.scrollback_len();
                guard.scrollback_offset = (guard.scrollback_offset + n).min(max_offset as isize);
            }
        } else {
            guard.scrollback_offset = (guard.scrollback_offset + n).max(0);
        }
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    fn scroll_page(&mut self, dir: isize) {
        let mut guard = self.focused_term().lock().expect("term mutex poisoned");
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
        drop(guard);
        self.clear_link_hover_state();
    }

    fn scroll_to_bottom(&mut self) {
        let mut guard = self.focused_term().lock().expect("term mutex poisoned");
        guard.scrollback_offset = 0;
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    fn jump_to_prev_prompt(&mut self) {
        let mut guard = self.focused_term().lock().expect("term mutex poisoned");
        guard.jump_to_prev_prompt();
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    fn jump_to_next_prompt(&mut self) {
        let mut guard = self.focused_term().lock().expect("term mutex poisoned");
        guard.jump_to_next_prompt();
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    /// Entra en copy mode si esta habilitado en config (no sale; usar q/Esc en copy mode).
    fn toggle_copy_mode(&mut self) {
        if !self.config.copy_mode.enabled {
            return;
        }
        if self.theme_picker.is_some() {
            return;
        }
        if let Ok(mut guard) = self.focused_term().lock() {
            if guard.copy_mode.is_none() {
                guard.copy_mode = Some(CopyModeState::enter(&guard));
                guard.search_clear();
                guard.mark_dirty();
                tracing::info!("KEYBOARD: copy mode activado");
            }
        }
    }

    fn toggle_search(&mut self) {
        if self.theme_picker.is_some() {
            return;
        }
        if let Ok(mut guard) = self.focused_term().lock() {
            if guard.search.is_some() {
                guard.search_clear();
                tracing::info!("KEYBOARD: busqueda desactivada");
            } else {
                if guard.copy_mode.is_some() {
                    guard.copy_mode = None;
                    guard.clear_selection();
                }
                guard.search = Some(SearchState::new());
                guard.mark_dirty();
                tracing::info!("KEYBOARD: busqueda activada");
            }
        }
    }

    /// Verifica si debe mostrarse el modal de consentimiento de primer arranque.
    fn check_first_run_consent(&mut self) {
        // Saltar si la variable de entorno lo pide.
        if std::env::var_os("BAUD_SKIP_CONSENT_UI").is_some_and(|v| v == "1") {
            return;
        }

        // Si ya decidió, no mostrar modal.
        if self.config.diagnostics.reporting.enabled.is_some() {
            return;
        }

        self.consent_prompt_active = true;
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_consent_active(true);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Maneja teclas del modal de consentimiento.
    /// Retorna `true` si la tecla fue consumida (Y/S/N).
    fn handle_consent_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let key = match &event.logical_key {
            winit::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("y") => Some(true),
            winit::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("s") => Some(true),
            winit::keyboard::Key::Character(c) if c.eq_ignore_ascii_case("n") => Some(false),
            _ => None,
        };

        let accepted = match key {
            Some(v) => v,
            None => return false,
        };

        self.consent_prompt_active = false;
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_consent_active(false);
        }

        // Persistir la decisión en config.toml
        match crate::diagnostics::consent::persist_reporting_enabled(accepted) {
            Ok(_) => {
                tracing::info!("consent persisted: enabled = {accepted}");
            }
            Err(e) => {
                tracing::warn!("could not persist consent: {e}");
            }
        }

        // Actualizar la config en memoria
        self.config.diagnostics.reporting.enabled = Some(accepted);

        // Si aceptó, crear y registrar el reporter
        if accepted {
            let dsn = crate::event_loop::resolve_dsn(&self.config);
            if let Some(dsn) = dsn {
                crate::event_loop::activate_reporter(dsn);
            }
        }

        true
    }

    fn toggle_theme_picker(&mut self) {
        if let Some(picker) = self.theme_picker.take() {
            self.cancel_theme_picker(picker);
            return;
        }
        let saved_copy_mode = self
            .focused_term()
            .lock()
            .ok()
            .and_then(|mut guard| guard.copy_mode.take());
        let preset = self.config.active_preset_name();
        self.theme_picker = Some(ThemePickerState::open(
            &self.config.theme,
            preset,
            saved_copy_mode,
        ));
        if let Ok(mut guard) = self.focused_term().lock() {
            guard.mark_dirty();
        }
        tracing::info!("KEYBOARD: theme picker activado");
    }

    fn cancel_theme_picker(&mut self, picker: ThemePickerState) {
        self.config.theme = picker.saved_theme().clone();
        self.config.theme_preset = picker.saved_preset().map(str::to_string);
        self.theme_picker = None;
        if let Ok(mut guard) = self.focused_term().lock() {
            if let Some(cm) = picker.saved_copy_mode() {
                guard.copy_mode = Some(cm);
            }
            guard.mark_dirty();
        }
        tracing::info!("KEYBOARD: theme picker cancelado");
    }

    fn confirm_theme_picker(&mut self, picker: ThemePickerState) {
        let Some(name) = picker.try_selected_name() else {
            self.theme_picker = Some(picker);
            if let Some(renderer) = &mut self.renderer {
                renderer.set_status("[Theme picker: sin coincidencias para aplicar]");
            }
            return;
        };
        let name = name.to_string();
        match persist::write_theme_preset(&name) {
            Ok(outcome) => {
                if let Ok(mut watch) = self.config_watch.lock() {
                    watch.sync(persist::file_mtime(&outcome.path));
                }
                self.config.theme = picker.preview_theme();
                self.config.theme_preset = Some(name.clone());
                self.theme_picker = None;
                if let Ok(mut guard) = self.focused_term().lock() {
                    if let Some(cm) = picker.saved_copy_mode() {
                        guard.copy_mode = Some(cm);
                    }
                    guard.mark_dirty();
                }
                if let Some(renderer) = &mut self.renderer {
                    let status = if outcome.preserved_theme_overrides {
                        format!("Tema aplicado: {name} (overrides en [theme] conservados)")
                    } else {
                        format!("Tema aplicado: {name}")
                    };
                    renderer.set_status(&status);
                }
                tracing::info!(
                    "theme picker: preset '{name}' persistido en {}",
                    outcome.path.display()
                );
            }
            Err(e) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status(&format!("[Error al guardar tema: {e}]"));
                }
                self.theme_picker = Some(picker);
            }
        }
    }

    /// Maneja teclas en theme picker. Devuelve true si la tecla fue consumida.
    fn handle_theme_picker_key(&mut self, event: &winit::event::KeyEvent, shift: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};

        let Some(picker) = self.theme_picker.as_mut() else {
            return false;
        };

        if picker.is_search_mode() {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) => picker.cancel_search(),
                Key::Named(NamedKey::Backspace) => picker.pop_filter_char(),
                Key::Named(NamedKey::Enter) => picker.commit_search(),
                Key::Named(NamedKey::ArrowDown) => picker.move_next(),
                Key::Named(NamedKey::ArrowUp) => picker.move_prev(),
                Key::Named(NamedKey::PageDown) => picker.page_down(),
                Key::Named(NamedKey::PageUp) => picker.page_up(),
                Key::Named(NamedKey::Home) => picker.move_home(),
                Key::Named(NamedKey::End) => picker.move_end(),
                Key::Character(c) if !shift => match c.as_str() {
                    "j" => picker.move_next(),
                    "k" => picker.move_prev(),
                    ch => {
                        if let Some(ch) = ch.chars().next() {
                            picker.push_filter_char(ch);
                        }
                    }
                },
                _ => return false,
            }
            return true;
        }

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                let picker = self.theme_picker.take().expect("picker activo");
                self.cancel_theme_picker(picker);
            }
            Key::Named(NamedKey::Enter) => {
                if !picker.can_confirm() {
                    return true;
                }
                let picker = self.theme_picker.take().expect("picker activo");
                self.confirm_theme_picker(picker);
            }
            Key::Named(NamedKey::ArrowDown) => picker.move_next(),
            Key::Named(NamedKey::ArrowUp) => picker.move_prev(),
            Key::Named(NamedKey::PageDown) => picker.page_down(),
            Key::Named(NamedKey::PageUp) => picker.page_up(),
            Key::Named(NamedKey::Home) => picker.move_home(),
            Key::Named(NamedKey::End) => picker.move_end(),
            Key::Character(c) if !shift => match c.as_str() {
                "j" => picker.move_next(),
                "k" => picker.move_prev(),
                "q" => {
                    let picker = self.theme_picker.take().expect("picker activo");
                    self.cancel_theme_picker(picker);
                }
                "/" => picker.start_search(),
                _ => return false,
            },
            _ => return false,
        }
        true
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
                let (old_rows, _, new_rows, _, deferred) =
                    self.sync_grid_to_window(size.width, size.height, cell_w, cell_h, true, false);
                self.pending_pane_sync = deferred;
                // Al reducir filas, anclar el borde inferior visible (evita que el contenido "suba").
                if old_rows > new_rows {
                    if let Ok(mut guard) = self.focused_term().lock() {
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
                    .focused_term()
                    .lock()
                    .ok()
                    .map(|g| g.copy_mode.is_some())
                    .unwrap_or(false);
                self.handle_copy();
                if in_copy_mode {
                    if let Ok(mut guard) = self.focused_term().lock() {
                        CopyModeState::exit(&mut guard);
                    }
                }
            }
            Paste => {
                if self
                    .focused_term()
                    .lock()
                    .ok()
                    .map(|g| g.search.is_some())
                    .unwrap_or(false)
                {
                    self.paste_to_search(false);
                } else {
                    self.handle_paste();
                }
            }
            PastePrimary => {
                if self
                    .focused_term()
                    .lock()
                    .ok()
                    .map(|g| g.search.is_some())
                    .unwrap_or(false)
                {
                    self.paste_to_search(true);
                } else {
                    self.handle_paste_primary();
                }
            }
            ToggleCopyMode => self.toggle_copy_mode(),
            ToggleSearch => self.toggle_search(),
            ScrollLineUp => self.scroll_lines(1),
            ScrollLineDown => self.scroll_lines(-1),
            ScrollPageUp => self.scroll_page(1),
            ScrollPageDown => self.scroll_page(-1),
            ScrollToBottom => self.scroll_to_bottom(),
            JumpToPrevPrompt => self.jump_to_prev_prompt(),
            JumpToNextPrompt => self.jump_to_next_prompt(),
            FontZoomIn => self.font_zoom(1),
            FontZoomOut => self.font_zoom(-1),
            FontZoomReset => self.font_zoom(0),
            ToggleThemePicker => self.toggle_theme_picker(),
            NewTab => self.new_tab(),
            CloseTab => self.close_tab(),
            NextTab => self.next_tab(),
            PrevTab => self.prev_tab(),
            GotoTab(n) => self.goto_tab(n),
            SplitPane => self.split_pane(),
            ToggleSplit => self.toggle_split(),
            SwapSplit => self.swap_split(),
            FocusNextPane => self.focus_next_pane(),
            FocusPrevPane => self.focus_prev_pane(),
            FocusPaneUp => self.focus_pane_direction(crate::layout::Direction::Up),
            FocusPaneDown => self.focus_pane_direction(crate::layout::Direction::Down),
            FocusPaneLeft => self.focus_pane_direction(crate::layout::Direction::Left),
            FocusPaneRight => self.focus_pane_direction(crate::layout::Direction::Right),
            ClosePane => self.close_pane(),
            ToggleFpsCounter => {
                if self.config.debug.fps_counter_enabled {
                    self.fps_overlay_visible = !self.fps_overlay_visible;
                    if !self.fps_overlay_visible {
                        if let Some(renderer) = &mut self.renderer {
                            renderer.set_status("");
                        }
                    }
                }
            }
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
        if let Ok(mut guard) = self.focused_term().lock() {
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
                    clipboard::set_detached(text, false);
                    if let Ok(mut g2) = self.focused_term().lock() {
                        CopyModeState::exit(&mut g2);
                    }
                } else if let Ok(mut g2) = self.focused_term().lock() {
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

    /// Maneja teclas en modo busqueda. Devuelve true si la tecla fue consumida.
    fn handle_search_mode_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.state().control_key();
        let alt = self.modifiers.state().alt_key();

        if let Ok(mut guard) = self.focused_term().lock() {
            if guard.search.is_none() {
                return false;
            }

            match &event.logical_key {
                Key::Named(NamedKey::Escape) => {
                    guard.search_clear();
                    return true;
                }
                Key::Named(NamedKey::Enter) => return true,
                Key::Named(NamedKey::Backspace) => {
                    if let Some(ref mut s) = guard.search {
                        s.query.pop();
                        let q = s.query.clone();
                        let ci = s.case_insensitive;
                        guard.search_set_query(&q, ci);
                    }
                    return true;
                }
                Key::Named(NamedKey::ArrowDown)
                | Key::Named(NamedKey::ArrowRight)
                | Key::Named(NamedKey::PageDown) => {
                    if guard.search.as_ref().is_some_and(|s| !s.matches.is_empty()) {
                        guard.search_next();
                    }
                    return true;
                }
                Key::Named(NamedKey::ArrowUp)
                | Key::Named(NamedKey::ArrowLeft)
                | Key::Named(NamedKey::PageUp) => {
                    if guard.search.as_ref().is_some_and(|s| !s.matches.is_empty()) {
                        guard.search_prev();
                    }
                    return true;
                }
                Key::Named(NamedKey::Space) if !ctrl && !alt => {
                    if let Some(ref mut s) = guard.search {
                        s.query.push(' ');
                        let q = s.query.clone();
                        let ci = s.case_insensitive;
                        guard.search_set_query(&q, ci);
                    }
                    return true;
                }
                Key::Character(c) if ctrl && c == "u" => {
                    if let Some(ref mut s) = guard.search {
                        s.query.clear();
                        let ci = s.case_insensitive;
                        guard.search_set_query("", ci);
                    }
                    return true;
                }
                Key::Character(c) if alt && c.eq_ignore_ascii_case("c") => {
                    guard.search_toggle_case_insensitive();
                    return true;
                }
                Key::Character(_) => {
                    if ctrl || alt {
                        return false;
                    }
                    let ch = event
                        .text
                        .as_deref()
                        .and_then(|t| t.chars().next())
                        .or_else(|| {
                            if let Key::Character(c) = &event.logical_key {
                                c.chars().next()
                            } else {
                                None
                            }
                        })
                        .filter(|ch| !ch.is_control());
                    if let Some(ch) = ch {
                        if let Some(ref mut s) = guard.search {
                            s.query.push(ch);
                            let q = s.query.clone();
                            let ci = s.case_insensitive;
                            guard.search_set_query(&q, ci);
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Envia bytes al PTY sin efectos secundarios (seleccion, scrollback).
    fn send_pty_bytes(&self, bytes: Vec<u8>) {
        tracing::debug!("send_pty_bytes: {} bytes: {:02x?}", bytes.len(), bytes);
        let _ = self.focused_session().pty_tx.send(PtyCommand::Input(bytes));
    }

    /// Origen en pixeles del pane (pad incluye tab bar via `grid_padding`).
    fn pane_pixel_origin(renderer: &Renderer, rect: &LayoutRect) -> (f32, f32) {
        let (pad_x, pad_y) = renderer.grid_padding();
        let cell_w = renderer.cell_w();
        let cell_h = renderer.cell_h();
        (
            pad_x + rect.x as f32 * cell_w,
            pad_y + rect.y as f32 * cell_h,
        )
    }

    /// Mapea pixeles de ventana a (session, row, col) dentro del pane bajo el cursor.
    fn pixel_to_pane_cell(
        &self,
        x: f64,
        y: f64,
        renderer: &Renderer,
    ) -> Option<(SessionId, usize, usize)> {
        let cell_w = renderer.cell_w();
        let cell_h = renderer.cell_h();
        let area = self.terminal_area_rect(
            self.window_width as u32,
            self.window_height as u32,
            cell_w,
            cell_h,
        );
        for (id, rect) in self.tabs[self.focused].layout().rects(area) {
            let (origin_x, origin_y) = Self::pane_pixel_origin(renderer, &rect);
            let (row, col) = crate::renderer::limits::pixel_to_cell_coords(
                x, y, origin_x, origin_y, cell_w, cell_h,
            );
            if row == usize::MAX || col == usize::MAX {
                continue;
            }
            if row < rect.rows && col < rect.cols {
                return Some((id, row, col));
            }
        }
        None
    }

    fn pane_is_dirty(&self, id: SessionId) -> bool {
        let Some(idx) = self.session_by_id(id) else {
            return false;
        };
        if self.sessions[idx].session.dirty {
            return true;
        }
        self.sessions[idx]
            .session
            .term
            .try_lock()
            .map(|t| t.dirty)
            .unwrap_or(true)
    }

    /// Coordenadas de celda (row, col) dentro del pane enfocado para smart_split.
    fn mouse_cell_coords_in_focused_pane(
        &self,
        renderer: &Renderer,
        pane_rect: &LayoutRect,
    ) -> (f32, f32) {
        let (row, col) = self.mouse_cell_coords(renderer);
        if row == usize::MAX {
            return (pane_rect.rows as f32 / 2.0, pane_rect.cols as f32 / 2.0);
        }
        (row as f32 + 0.5, col as f32 + 0.5)
    }

    /// Coordenadas de celda (row, col) desde la ultima posicion del mouse.
    fn mouse_cell_coords(&self, renderer: &Renderer) -> (usize, usize) {
        let focused_id = self.tabs[self.focused].focused();
        if let Some((id, row, col)) = self.pixel_to_pane_cell(self.mouse_x, self.mouse_y, renderer)
        {
            if id == focused_id {
                return (row, col);
            }
        }
        (usize::MAX, usize::MAX)
    }

    /// Actualiza `hovered_link` y el cursor segun la celda bajo el puntero.
    /// Devuelve true si el estado de hover cambio.
    fn focused_pane_rect(&self, cell_w: f32, cell_h: f32) -> LayoutRect {
        let area = self.terminal_area_rect(
            self.window_width as u32,
            self.window_height as u32,
            cell_w,
            cell_h,
        );
        let focused_id = self.tabs[self.focused].focused();
        self.tabs[self.focused]
            .layout()
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == focused_id)
            .map(|(_, r)| r)
            .unwrap_or(area)
    }

    fn update_link_hover_at(&mut self, x: f64, y: f64) -> bool {
        let Some(renderer) = self.renderer.as_ref() else {
            return false;
        };
        let focused_id = self.tabs[self.focused].focused();
        let Some((id, visible_row, col)) = self.pixel_to_pane_cell(x, y, renderer) else {
            self.clear_link_hover_state();
            return false;
        };
        if id != focused_id {
            self.clear_link_hover_state();
            return false;
        }
        let Ok(mut guard) = self.focused_term().try_lock() else {
            self.watchdog.note_term_lock_busy();
            return false;
        };
        let logical_row = guard.visible_to_logical_row(visible_row);
        let new_hovered = guard
            .resolve_link_at(logical_row, col)
            .map(|(_, range)| range);
        let link_changed = guard.hovered_link != new_hovered;
        let has_link = new_hovered.is_some();
        if link_changed {
            guard.hovered_link = new_hovered;
            guard.mark_dirty();
        }
        drop(guard);
        if let Some(window) = &self.window {
            window.set_cursor(if has_link {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
            if link_changed {
                window.request_redraw();
            }
        }
        link_changed
    }

    fn clear_link_hover_state(&mut self) {
        let cleared = match self.focused_term().try_lock() {
            Ok(mut guard) => guard.clear_hovered_link(),
            Err(_) => {
                self.watchdog.note_term_lock_busy();
                false
            }
        };
        if cleared {
            if let Some(window) = &self.window {
                window.set_cursor(CursorIcon::Default);
                window.request_redraw();
            }
        }
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
    ///
    /// `None` = no se pudo tomar el Term (contención); el caller debe
    /// descartar el evento en lugar de caer a selección local.
    fn try_should_forward_mouse_to_app(&self) -> Option<bool> {
        let guard = self.focused_term().try_lock().ok()?;
        Some(
            guard.mouse_reporting.is_active()
                && !self.local_selection_active(&guard.mouse_reporting),
        )
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
        let Ok(guard) = self.focused_term().try_lock() else {
            self.watchdog.note_term_lock_busy();
            return;
        };
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

    fn forward_mouse_motion(&self, button: u8) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        let (row, col) = self.mouse_cell_coords(renderer);
        let Ok(guard) = self.focused_term().try_lock() else {
            self.watchdog.note_term_lock_busy();
            return;
        };
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

impl ApplicationHandler<UserEvent> for App {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.watchdog.ping();
        if self.pending_exit {
            event_loop.exit();
            return;
        }
        if self.tick_tab_close_fade() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + std::time::Duration::from_millis(16),
            ));
            return;
        }
        // Despertar al expirar el status para ocultarlo sin esperar input.
        let mut wake_at: Option<Instant> = None;
        if let Some(deadline) = self.renderer.as_ref().and_then(|r| r.status_expiry()) {
            let now = Instant::now();
            if now >= deadline {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_status("");
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            } else {
                wake_at = Some(deadline);
            }
        }
        if self.fps_overlay_visible && self.config.debug.fps_counter_enabled {
            let now = Instant::now();
            if let Some(window) = &self.window {
                let interval_nanos = self.redraw_interval_nanos.load(Ordering::Relaxed);
                if interval_nanos == 0 {
                    window.request_redraw();
                    return;
                }
                let interval = Duration::from_nanos(interval_nanos);
                let deadline = self.last_gui_redraw.map(|t| t + interval).unwrap_or(now);
                if now >= deadline {
                    window.request_redraw();
                    event_loop.set_control_flow(ControlFlow::WaitUntil(now + interval));
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
                return;
            }
        }
        if let Some(deadline) = self.copy_on_select_deadline {
            if Instant::now() >= deadline {
                self.copy_on_select_deadline = None;
                self.finish_copy_on_select();
            } else {
                wake_at = Some(match wake_at {
                    Some(existing) => existing.min(deadline),
                    None => deadline,
                });
            }
        }
        if let Some(deadline) = wake_at {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ponytail: solo inicializar una vez.
        if self.window.is_some() {
            return;
        }

        self.display_quirks = display_quirks::snapshot_for_event_loop(event_loop);

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
        window.set_ime_allowed(true);

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

        let t_gpu_init = Instant::now();
        tracing::info!("wgpu: solicitando adaptador GPU...");
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no se encontro adaptador GPU compatible");
        tracing::info!(
            "wgpu: adaptador listo en {}ms",
            t_gpu_init.elapsed().as_millis()
        );

        let t_device = Instant::now();
        tracing::info!("wgpu: solicitando device...");
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
        tracing::info!(
            "wgpu: device listo en {}ms (init GPU total {}ms)",
            t_device.elapsed().as_millis(),
            t_gpu_init.elapsed().as_millis()
        );

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
            if let Some(source) = self.pending_config_source.take() {
                match source {
                    ConfigSource::NotFound => {
                        renderer.set_status_with_config(
                            "Sin archivo de config, usando defaults",
                            "⚡",
                            &self.config.theme,
                            &self.config.status,
                        );
                    }
                    ConfigSource::ParseError { path, message } => {
                        let msg = format!("Error en {path}: {message}");
                        renderer.set_status_with_config(
                            &msg,
                            "✗",
                            &self.config.theme,
                            &self.config.status,
                        );
                    }
                    ConfigSource::Ok => {}
                }
            }
        }
        tracing::info!("renderer inicializado");

        // Verificar si hay que mostrar el modal de consentimiento de primer arranque.
        self.check_first_run_consent();

        clipboard::warm_up();

        let size = window.inner_size();
        if let Some(renderer) = &self.renderer {
            let (_, _, _, _, deferred) = self.sync_grid_to_window(
                size.width,
                size.height,
                renderer.cell_w,
                renderer.cell_h,
                false,
                true,
            );
            self.pending_pane_sync = deferred;
        }

        // 5. Primer present según quirks: en Wayland la superficie no aparece
        // hasta dibujar; ciertas familias además marcan la ventana como colgada
        // si no hay redraw temprano.
        if self.display_quirks.force_initial_redraw {
            window.request_redraw();
        }
        self.update_ime_area();

        let cfg = self.config.clone();
        self.apply_config(cfg);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let _phase = self.watchdog.enter(watchdog::window_event_phase(&event));
        match event {
            WindowEvent::CloseRequested => {
                for host in &self.sessions {
                    let _ = host.session.pty_tx.send(PtyCommand::Shutdown);
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
                let (_old_rows, _old_cols, new_rows, new_cols, deferred) = self
                    .sync_grid_to_window(
                        new_size.width,
                        new_size.height,
                        cell_w,
                        cell_h,
                        false,
                        true,
                    );
                self.pending_pane_sync = deferred;
                tracing::debug!(
                    "[RESIZE] cell_h={:.1} cell_w={:.1} win={}x{} -> grid={}x{}",
                    cell_h,
                    cell_w,
                    new_size.width,
                    new_size.height,
                    new_rows,
                    new_cols,
                );
                if let Ok(guard) = self.focused_term().lock() {
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
                self.update_ime_area();
            }
            WindowEvent::RedrawRequested => {
                if self.pending_pane_sync {
                    self.sync_after_tab_change();
                }
                let theme = self.effective_theme();
                let picker = self.theme_picker.as_ref();
                let preedit_empty = self.preedit.is_empty();
                let preedit = if preedit_empty {
                    None
                } else {
                    let (row, col) = self.cursor_visible_cell();
                    Some(PreeditState {
                        text: self.preedit.clone(),
                        row,
                        col,
                    })
                };
                self.update_ime_area();
                let tab_layout = self
                    .renderer
                    .as_ref()
                    .and_then(|r| self.tab_bar_layout_with_mouse(r));
                let (cell_w, cell_h) = self
                    .renderer
                    .as_ref()
                    .map(|r| (r.cell_w(), r.cell_h()))
                    .unwrap_or((0.0, 0.0));
                let terminal_area = self.terminal_area_rect(
                    self.window_width as u32,
                    self.window_height as u32,
                    cell_w,
                    cell_h,
                );
                self.tabs[self.focused].recalc_dwindle_orients(
                    terminal_area,
                    self.config.panes.split_width_multiplier,
                );
                let pane_rects = self.tabs[self.focused].layout().rects(terminal_area);
                let focused_id = self.tabs[self.focused].focused();

                let any_pane_dirty = pane_rects.iter().any(|(id, _)| self.pane_is_dirty(*id));
                let search_active = self
                    .focused_term()
                    .try_lock()
                    .ok()
                    .is_some_and(|t| t.search.is_some());
                let status_needs_present = self
                    .renderer
                    .as_ref()
                    .is_some_and(|r| r.status_needs_present());
                let picker_active = self
                    .renderer
                    .as_ref()
                    .is_some_and(|r| r.theme_picker_active(picker));
                let consent_active = self
                    .renderer
                    .as_ref()
                    .is_some_and(|r| r.is_consent_active());

                if !any_pane_dirty
                    && !status_needs_present
                    && !picker_active
                    && !consent_active
                    && !search_active
                    && !self.fps_overlay_visible
                    && preedit_empty
                {
                    tracing::debug!("RedrawRequested: skip (nothing dirty)");
                    return;
                }

                let pane_jobs: Vec<(SessionId, LayoutRect, usize, bool, bool)> = pane_rects
                    .iter()
                    .filter_map(|(id, rect)| {
                        let idx = self.session_by_id(*id)?;
                        let renderer = self.renderer.as_ref()?;
                        let deferred = self.sessions[idx]
                            .session
                            .term
                            .try_lock()
                            .map(|t| t.should_defer_redraw())
                            .unwrap_or(false);
                        // Durante sync, reutilizar el frame cacheado; no reconstruir a medias.
                        let rebuild =
                            !deferred && (self.pane_is_dirty(*id) || !renderer.has_pane_cache(*id));
                        Some((*id, *rect, idx, rebuild, deferred))
                    })
                    .collect();

                // Snapshot del deferral al armar el frame: el post-render no debe
                // re-consultar should_defer_redraw (ESU/timeout a mitad de frame
                // limpiaria dirty tras haber pintado solo la cache).
                let deferred_at_schedule: Vec<SessionId> = pane_jobs
                    .iter()
                    .filter(|(_, _, _, _, deferred)| *deferred)
                    .map(|(id, _, _, _, _)| *id)
                    .collect();

                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                let panes: Vec<PaneRender> = pane_jobs
                    .into_iter()
                    .map(|(id, rect, idx, rebuild, _deferred)| PaneRender {
                        session_id: id,
                        term: Arc::clone(&self.sessions[idx].session.term),
                        rect,
                        focused: id == focused_id,
                        rebuild,
                    })
                    .collect();

                tracing::debug!(
                    "RedrawRequested: renderizando frame ({} panes, {} rebuild)",
                    panes.len(),
                    panes.iter().filter(|p| p.rebuild).count()
                );
                let since_last = self.last_gui_redraw.map(|t| t.elapsed());
                self.gui_redraw_metrics.record_redraw(since_last);
                self.last_gui_redraw = Some(Instant::now());
                self.gui_redraw_metrics.maybe_log();
                let bold = self.config.bold_is_bright || self.config.theme.bold_is_bright;
                let layout = self.tabs[self.focused].layout().clone();
                let t_render = Instant::now();
                match renderer.render(
                    &panes,
                    terminal_area,
                    &layout,
                    &theme,
                    bold,
                    self.config.window.opacity,
                    picker,
                    preedit,
                    tab_layout.as_ref(),
                ) {
                    Ok(updated) => {
                        for id in updated {
                            if let Some(idx) = self.session_by_id(id) {
                                if deferred_at_schedule.contains(&id) {
                                    // Frame diferido: no limpiar dirty; reintentar al cerrar sync.
                                    self.sessions[idx].session.dirty = true;
                                    continue;
                                }
                                self.sessions[idx].session.dirty = false;
                                if let Ok(mut guard) = self.sessions[idx].session.term.try_lock() {
                                    guard.take_dirty();
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!("error al renderizar: {e}"),
                }
                if let Some(renderer) = &mut self.renderer {
                    renderer.clear_status_present();
                }
                if self.fps_overlay_visible && self.config.debug.fps_counter_enabled {
                    if let Some(renderer) = &mut self.renderer {
                        let fps = self.gui_redraw_metrics.current_fps();
                        let text = format!("FPS: {:.0}", fps);
                        renderer.set_status_with_config(
                            &text,
                            "",
                            &self.config.theme,
                            &self.config.status,
                        );
                    }
                }
                let render_ms = t_render.elapsed().as_millis();
                if render_ms > 250 {
                    tracing::warn!(
                        "render lento: {}ms ({} panes, status_present={}, search={})",
                        render_ms,
                        panes.len(),
                        status_needs_present,
                        search_active
                    );
                }
            }
            // Track modifier state (Ctrl, Shift, Alt, etc.) for keyboard shortcuts.
            // winit 0.30 envia ModifiersChanged separado de KeyboardInput.
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            // Diagnostico: el cursor entro/salio de la ventana.
            // En backends donde el enter del puntero es fiable, se registra a info.
            WindowEvent::CursorEntered { .. } => {
                tracing::info!(
                    backend = ?self.display_quirks.backend,
                    family = ?self.display_quirks.family,
                    "CursorEntered: el cursor entro a la ventana"
                );
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
                let (cell_w, cell_h) = {
                    let Some(renderer) = &self.renderer else {
                        tracing::warn!("CursorMoved: renderer no disponible");
                        return;
                    };
                    (renderer.cell_w(), renderer.cell_h())
                };
                self.mouse_x = position.x;
                self.mouse_y = position.y;

                if let Some(renderer) = &self.renderer {
                    if self.is_in_tab_bar_row(position.y, renderer) {
                        if self.update_tab_hover(position.x, position.y) {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        return;
                    }
                    if self.tab_hover.is_some() {
                        self.tab_hover = None;
                        self.tab_anim_last = Instant::now();
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }

                if !self.mouse_down.load(Ordering::Relaxed) {
                    self.update_link_hover_at(position.x, position.y);
                }

                match self.try_should_forward_mouse_to_app() {
                    None => {
                        self.watchdog.note_term_lock_busy();
                        return;
                    }
                    Some(true) => {
                        let mouse_down = self.mouse_down.load(Ordering::Relaxed);
                        let term = Arc::clone(self.focused_term());
                        let Some(renderer) = &self.renderer else {
                            return;
                        };
                        let Ok(guard) = term.try_lock() else {
                            self.watchdog.note_term_lock_busy();
                            return;
                        };
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
                        return;
                    }
                    Some(false) => {}
                }

                if self.mouse_down.load(Ordering::Relaxed) {
                    let Some(renderer) = &self.renderer else {
                        return;
                    };
                    let pane = self.focused_pane_rect(cell_w, cell_h);
                    let (_, pane_origin_y) = Self::pane_pixel_origin(renderer, &pane);
                    let pane_top = pane_origin_y;
                    let pane_bottom = pane_top + pane.rows as f32 * cell_h;
                    let (visible_row, col, needs_scroll_up, needs_scroll_down) =
                        if position.y < f64::from(pane_top) {
                            (0usize, 0usize, true, false)
                        } else if position.y as f32 >= pane_bottom {
                            (pane.rows.saturating_sub(1), 0usize, false, true)
                        } else {
                            let (r, c) = self.mouse_cell_coords(renderer);
                            (r, c, r == 0, r >= pane.rows.saturating_sub(1))
                        };

                    let scroll_changed = needs_scroll_up || needs_scroll_down;
                    let Ok(mut guard) = self.focused_term().try_lock() else {
                        self.watchdog.note_term_lock_busy();
                        return;
                    };
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
                        match sel.mode {
                            SelectionMode::Word | SelectionMode::Smart | SelectionMode::Line => {}
                            SelectionMode::Normal | SelectionMode::Block => {
                                sel.update_end(SelectionPoint { row: abs_row, col });
                            }
                        }
                    }
                    guard.mark_dirty();
                    tracing::debug!(
                        "CursorMoved: mouse_drag visible_row={} col={} scrollback_offset={}",
                        visible_row,
                        col,
                        guard.scrollback_offset
                    );
                    drop(guard);
                    if scroll_changed {
                        self.clear_link_hover_state();
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            // Mouse left: el cursor salio de la ventana.
            // Si cursor_left_stops_moved, el backend deja de emitir CursorMoved;
            // con arrastre activo arrancamos auto-scroll en un hilo aparte.
            WindowEvent::CursorLeft { .. } => {
                if self.display_quirks.cursor_left_stops_moved {
                    tracing::debug!("CursorLeft: backend deja de emitir CursorMoved tras salir");
                }
                if self.mouse_down.load(Ordering::Relaxed) {
                    tracing::info!("CursorLeft: mouse_down=true, auto-scroll thread iniciado");
                    let term_clone = Arc::clone(self.focused_term());
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
                    self.clear_link_hover_state();
                    if self.tab_hover.is_some() {
                        self.tab_hover = None;
                        self.tab_anim_last = Instant::now();
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
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

                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(renderer) = &self.renderer {
                        if let Some(idx) = self.tab_close_at(self.mouse_x, self.mouse_y, renderer) {
                            self.close_tab_at(idx);
                            return;
                        }
                        if let Some(idx) = self.tab_index_at(self.mouse_x, self.mouse_y, renderer) {
                            self.focus_session(idx);
                            return;
                        }
                    }
                }

                // copy_on_select diferido: deja completar doble/triple clic antes de copiar.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if self.modifiers.state().control_key() {
                        let opened = if let Ok(guard) = self.focused_term().try_lock() {
                            guard.hovered_link.as_ref().is_some_and(|range| {
                                guard
                                    .resolve_link_at(range.row, range.start_col)
                                    .is_some_and(|(url, _)| {
                                        open_url(&url);
                                        true
                                    })
                            })
                        } else {
                            self.watchdog.note_term_lock_busy();
                            false
                        };
                        if opened {
                            return;
                        }
                    } else {
                        self.cancel_copy_on_select();
                    }
                }
                if button == MouseButton::Left && state == ElementState::Released {
                    self.schedule_copy_on_select();
                    self.mouse_down.store(false, Ordering::Relaxed);
                    self.mouse_start = None;
                    if let Some(window) = &self.window {
                        let _ = window.set_cursor_grab(CursorGrabMode::None);
                    }
                }

                match self.try_should_forward_mouse_to_app() {
                    None => {
                        self.watchdog.note_term_lock_busy();
                        return;
                    }
                    Some(true) => {
                        if let Some(renderer) = &self.renderer {
                            if self.is_in_tab_bar_row(self.mouse_y, renderer) {
                                return;
                            }
                        }
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
                    Some(false) => {}
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
                    if self.is_in_tab_bar_row(self.mouse_y, renderer) {
                        return;
                    }
                    if state == ElementState::Pressed {
                        if let Some((id, _, _)) =
                            self.pixel_to_pane_cell(self.mouse_x, self.mouse_y, renderer)
                        {
                            let focused_id = self.tabs[self.focused].focused();
                            if id != focused_id {
                                self.focus_pane_by_id(id);
                                return;
                            }
                        }
                    }
                    match state {
                        ElementState::Pressed => {
                            // Bugfix: ignorar si las coordenadas no son validas
                            if self.mouse_x < 0.0 || self.mouse_y < 0.0 {
                                return;
                            }
                            let (visible_row, col) = self.mouse_cell_coords(renderer);
                            let shift = self.modifiers.state().shift_key();
                            let block = self.block_selection_active();
                            let now = Instant::now();
                            let is_rapid = self
                                .last_click_time
                                .map(|t| now.duration_since(t) < MULTI_CLICK_INTERVAL)
                                .unwrap_or(false);

                            let term = Arc::clone(self.focused_term());
                            let Ok(mut guard) = term.try_lock() else {
                                self.watchdog.note_term_lock_busy();
                                return;
                            };
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
                                if guard.selection.is_none() {
                                    guard.selection = Some(Selection::new(point));
                                }
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
                            drop(guard);
                            self.mouse_down.store(true, Ordering::Relaxed);
                            self.last_click_time = Some(now);
                            if let Some(window) = &self.window {
                                let _ = window.set_cursor_grab(CursorGrabMode::Confined);
                            }
                            // Bugfix: solicitar redibujo inmediato al crear/modificar seleccion
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        ElementState::Released => {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let owner_hint = match self.try_should_forward_mouse_to_app() {
                    None => {
                        self.watchdog.note_term_lock_busy();
                        return;
                    }
                    Some(true) => WheelOwnerHint::App,
                    Some(false) => WheelOwnerHint::Host,
                };

                let cell_h = self.renderer.as_ref().map(|r| r.cell_h).unwrap_or(0.0);
                let lines = wheel::lines_from_delta(&delta, cell_h, &mut self.wheel_residual);

                let Ok(guard) = self.focused_term().try_lock() else {
                    return;
                };
                let alt_screen = guard.alt_screen;
                let app_cursor_keys = guard.app_cursor_keys;
                drop(guard);

                let intent = wheel::resolve(
                    owner_hint,
                    alt_screen,
                    lines,
                    self.config.scrollback.multiplier,
                    self.config.scrollback.faux_multiplier,
                );

                match intent {
                    WheelIntent::None => {}
                    WheelIntent::ForwardReport { button } => {
                        self.forward_mouse_button(button, false);
                    }
                    WheelIntent::LocalLines(n) => {
                        self.scroll_lines(n);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                    WheelIntent::FauxLines { up, count } => {
                        let modes = if app_cursor_keys {
                            KeyModes {
                                app_cursor_keys: true,
                                ..Default::default()
                            }
                        } else {
                            KeyModes::default()
                        };
                        let key = if up { KKey::Up } else { KKey::Down };
                        if let Some(bytes) = keymap::encode_key(key, Mods::NONE, modes) {
                            for _ in 0..count {
                                self.send_pty_bytes(bytes.clone());
                            }
                        }
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    self.send_input(text.into_bytes());
                    self.preedit.clear();
                    self.preedit_cursor = None;
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Ime::Preedit(text, cursor) => {
                    self.preedit = text;
                    self.preedit_cursor = cursor;
                    self.update_ime_area();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Ime::Enabled => {
                    self.preedit.clear();
                    self.preedit_cursor = None;
                    self.update_ime_area();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Ime::Disabled => {
                    self.preedit.clear();
                    self.preedit_cursor = None;
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            },
            // Input de teclado completo: letras, Enter, Backspace, Tab, Ctrl+letter, etc.
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Released => {
                let report_events = self
                    .focused_term()
                    .lock()
                    .ok()
                    .map(|g| g.keyboard_flags & 2 != 0)
                    .unwrap_or(false);
                if report_events && !self.preedit.is_empty() {
                    return;
                }
                if report_events {
                    let mods = Mods {
                        shift: self.modifiers.state().shift_key(),
                        alt: self.modifiers.state().alt_key(),
                        ctrl: self.modifiers.state().control_key(),
                        sup: self.modifiers.state().super_key(),
                    };
                    if let Some(k) = winit_to_key(&event.logical_key) {
                        let modes = current_key_modes(self.focused_term());
                        if let Some(bytes) =
                            keymap::encode_key_extended(k, mods, modes, KeyEventKind::Release)
                        {
                            self.send_input(bytes);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if matches!(event.logical_key, Key::Named(NamedKey::Process)) {
                    return;
                }
                if !self.preedit.is_empty() {
                    return;
                }
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

                if self.consent_prompt_active {
                    if self.handle_consent_key(&event) {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    return;
                }

                if self.theme_picker.is_some() {
                    if let Some(k) = winit_to_key(&event.logical_key) {
                        let k_norm = normalize_binding_key(k, mods);
                        if matches!(
                            self.keybindings.lookup(k_norm, mods),
                            Some(Action::ToggleThemePicker)
                        ) {
                            self.run_action(Action::ToggleThemePicker);
                            return;
                        }
                    }
                    if self.handle_theme_picker_key(&event, shift) {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                    return;
                }

                let in_search = self
                    .focused_term()
                    .lock()
                    .ok()
                    .map(|g| g.search.is_some())
                    .unwrap_or(false);

                if let Some(k) = winit_to_key(&event.logical_key) {
                    let k_norm = normalize_binding_key(k, mods);
                    if let Some(action) = self.keybindings.lookup(k_norm, mods) {
                        if in_search {
                            use crate::input::actions::Action::*;
                            match action {
                                ScrollLineUp | ScrollLineDown | ScrollPageUp | ScrollPageDown
                                | ScrollToBottom | JumpToPrevPrompt | JumpToNextPrompt => {}
                                _ => {
                                    self.run_action(action);
                                    return;
                                }
                            }
                        } else {
                            self.run_action(action);
                            return;
                        }
                    }
                }

                // Copy mode: si está activo, las teclas navegan/seleccionan
                // y NO se envían al PTY (excepto Ctrl+Shift+C ya manejado arriba).
                if self
                    .focused_term()
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

                // Modo busqueda: captura teclas; no enviar al PTY (como theme picker).
                if in_search {
                    self.handle_search_mode_key(&event);
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
                    let modes = current_key_modes(self.focused_term());
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
        let phase = match &event {
            UserEvent::RedrawNeeded(_) => "UserEvent::RedrawNeeded",
            UserEvent::PtyExited(_, _) => "UserEvent::PtyExited",
            UserEvent::PtyError(_, _) => "UserEvent::PtyError",
            UserEvent::SetTitle(_, _) => "UserEvent::SetTitle",
            UserEvent::ReadClipboard(_, _, _) => "UserEvent::ReadClipboard",
            UserEvent::Osc52ReadReady(_, _, _, _) => "UserEvent::Osc52ReadReady",
            UserEvent::PasteReady(_) => "UserEvent::PasteReady",
            UserEvent::PasteSearchReady(_) => "UserEvent::PasteSearchReady",
            UserEvent::ConfigReloaded(_) => "UserEvent::ConfigReloaded",
            UserEvent::ConfigReloadFailed(_) => "UserEvent::ConfigReloadFailed",
        };
        let _guard = self.watchdog.enter(phase);
        self.dispatch_user_event(event);
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
    use crate::config::watch::WatchState;
    use crate::pty::PtyCommandSender;
    use crate::renderer::limits::pixel_to_cell_coords;
    use std::sync::mpsc;

    fn test_config_watch() -> Arc<Mutex<WatchState>> {
        Arc::new(Mutex::new(WatchState::new(None)))
    }

    fn dummy_pty_sender() -> PtyCommandSender {
        let (tx, _rx) = mpsc::channel();
        let wakeup = crate::pty::create_wake().expect("wake para test");
        PtyCommandSender::new_for_test(tx, wakeup)
    }

    fn test_session(term: Arc<Mutex<Term>>) -> Session {
        Session {
            id: SessionId::next(),
            term,
            pty_tx: dummy_pty_sender(),
            title: String::new(),
            dirty: false,
        }
    }

    fn test_app(term: Arc<Mutex<Term>>) -> App {
        let session = test_session(term);
        let id = session.id;
        App::new(
            vec![SessionHost::test(session)],
            Config::default(),
            test_config_watch(),
            None,
            BlinkFocus::new(id),
            ConfigSource::Ok,
            EventLoopWatchdog::noop(),
        )
    }

    #[test]
    fn redraw_needed_background_marca_dirty_sin_enfocada() {
        let session_a = test_session(Arc::new(Mutex::new(Term::new())));
        let id_a = session_a.id;
        let session_b = test_session(Arc::new(Mutex::new(Term::new())));
        let id_b = session_b.id;
        let mut app = App::new(
            vec![SessionHost::test(session_a), SessionHost::test(session_b)],
            Config::default(),
            test_config_watch(),
            None,
            BlinkFocus::new(id_b),
            ConfigSource::Ok,
            EventLoopWatchdog::noop(),
        );
        app.focused = 1;

        app.dispatch_user_event(UserEvent::RedrawNeeded(id_a));
        assert!(app.sessions[0].session.dirty);
        assert!(!app.sessions[1].session.dirty);
    }

    fn feed_term(term: &mut Term, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, data);
    }

    #[test]
    fn redraw_needed_diferido_mientras_sync_update_activo() {
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"\x1b[?2026h");
            assert!(guard.should_defer_redraw());
        }
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;
        app.dispatch_user_event(UserEvent::RedrawNeeded(id));
        assert!(
            app.sessions[0].session.dirty,
            "sync activo debe diferir el redraw y dejar dirty"
        );
    }

    #[test]
    fn redraw_needed_tras_esu_limpia_dirty() {
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"\x1b[?2026h");
            feed_term(&mut guard, b"\x1b[?2026l");
            assert!(!guard.should_defer_redraw());
        }
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;
        app.sessions[0].session.dirty = true;
        app.dispatch_user_event(UserEvent::RedrawNeeded(id));
        assert!(
            !app.sessions[0].session.dirty,
            "tras ESU el redraw final debe limpiar dirty"
        );
    }

    #[test]
    fn redraw_needed_tras_timeout_no_difiere() {
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"\x1b[?2026h");
            guard.set_sync_update_started_at_for_test(Some(
                std::time::Instant::now() - std::time::Duration::from_millis(200),
            ));
            assert!(!guard.should_defer_redraw());
            assert!(guard.sync_update_active);
        }
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;
        app.sessions[0].session.dirty = true;
        app.dispatch_user_event(UserEvent::RedrawNeeded(id));
        assert!(
            !app.sessions[0].session.dirty,
            "tras timeout el modo sigue activo pero ya no se difiere"
        );
    }

    #[test]
    fn redraw_needed_enfocada_limpia_dirty_sin_sync() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;
        app.sessions[0].session.dirty = true;
        app.dispatch_user_event(UserEvent::RedrawNeeded(id));
        assert!(!app.sessions[0].session.dirty);
    }

    #[test]
    fn focus_session_limpia_dirty_de_sesion_enfocada() {
        let session_a = test_session(Arc::new(Mutex::new(Term::new())));
        let session_b = test_session(Arc::new(Mutex::new(Term::new())));
        let id_b = session_b.id;
        let mut app = App::new(
            vec![SessionHost::test(session_a), SessionHost::test(session_b)],
            Config::default(),
            test_config_watch(),
            None,
            BlinkFocus::new(id_b),
            ConfigSource::Ok,
            EventLoopWatchdog::noop(),
        );
        app.sessions[0].session.dirty = true;
        app.focused = 1;
        app.focus_session(0);
        assert!(!app.sessions[0].session.dirty);
    }

    #[test]
    fn send_input_marca_pane_dirty_aunque_pty_no_eco() {
        let term = Arc::new(Mutex::new(Term::new()));
        let app = test_app(term.clone());
        let id = app.sessions[0].session.id;
        term.lock().expect("term mutex").take_dirty();
        assert!(
            !app.pane_is_dirty(id),
            "precondicion: sin dirty antes del input"
        );

        app.send_input(b" ".to_vec());

        assert!(
            app.pane_is_dirty(id),
            "send_input debe marcar el pane dirty aunque el PTY no genere eco"
        );
    }

    #[test]
    fn goto_tab_usa_indices_1_based() {
        use crate::input::actions::Action;
        let s0 = test_session(Arc::new(Mutex::new(Term::new())));
        let id0 = s0.id;
        let mut app = App::new(
            vec![
                SessionHost::test(s0),
                SessionHost::test(test_session(Arc::new(Mutex::new(Term::new())))),
                SessionHost::test(test_session(Arc::new(Mutex::new(Term::new())))),
            ],
            Config::default(),
            test_config_watch(),
            None,
            BlinkFocus::new(id0),
            ConfigSource::Ok,
            EventLoopWatchdog::noop(),
        );
        app.run_action(Action::GotoTab(2));
        assert_eq!(app.focused, 1);
        app.run_action(Action::GotoTab(0));
        assert_eq!(app.focused, 1);
    }

    #[test]
    fn test_config_reload_updates_render_cap() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let shared = Arc::new(AtomicU64::new(0));
        app.set_redraw_interval_handle(Arc::clone(&shared));

        let cfg: Config = toml::from_str("[render]\nmax_fps = 120\n").unwrap();
        app.dispatch_user_event(UserEvent::ConfigReloaded(Box::new(cfg)));

        assert_eq!(
            shared.load(Ordering::Relaxed),
            std::time::Duration::from_secs_f64(1.0 / 120.0).as_nanos() as u64
        );
    }

    #[test]
    fn test_effective_theme_usa_preview() {
        let mut app = test_app(Arc::new(Mutex::new(Term::new())));
        app.theme_picker = Some(ThemePickerState::open(
            &app.config.theme,
            Some("dracula"),
            None,
        ));
        let preview = app.effective_theme();
        assert_eq!(
            preview.background,
            crate::config::try_preset("dracula").unwrap().background
        );
    }

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
        let app = test_app(Arc::new(Mutex::new(Term::new())));
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
        let app = test_app(Arc::clone(&term));
        assert_eq!(
            app.try_should_forward_mouse_to_app(),
            Some(false),
            "shell: no reenviar mouse al PTY (seleccion local)"
        );

        term.lock().expect("term lock").mouse_reporting = MouseReporting {
            click: true,
            drag: true,
            any_motion: false,
            sgr: true,
        };
        let app_vim = test_app(term);
        assert_eq!(
            app_vim.try_should_forward_mouse_to_app(),
            Some(true),
            "vim: app captura mouse sin modificadores"
        );
    }

    #[test]
    fn try_should_forward_mouse_none_cuando_term_ocupado() {
        use crate::ansi::MouseReporting;

        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term lock");
            guard.mouse_reporting = MouseReporting {
                click: true,
                drag: true,
                any_motion: false,
                sgr: true,
            };
        }
        let app = test_app(Arc::clone(&term));
        let _hold = term.lock().expect("hold term");
        assert_eq!(
            app.try_should_forward_mouse_to_app(),
            None,
            "con Term ocupado no debe bloquear ni fingir seleccion local"
        );
        assert_eq!(app.watchdog.snapshot().term_lock_busy, 0);
        // El contador solo sube cuando el hot path anota busy.
        app.watchdog.note_term_lock_busy();
        assert_eq!(app.watchdog.snapshot().term_lock_busy, 1);
    }

    #[test]
    fn allowed_open_url_acepta_esquemas_conocidos() {
        assert!(allowed_open_url("https://example.com"));
        assert!(allowed_open_url("HTTP://EXAMPLE.COM"));
        assert!(allowed_open_url("ftp://files.example/resource"));
        assert!(allowed_open_url("file:///tmp/x"));
        assert!(allowed_open_url("mailto:user@example.com"));
    }

    #[test]
    fn allowed_open_url_rechaza_esquemas_peligrosos() {
        assert!(!allowed_open_url("javascript:alert(1)"));
        assert!(!allowed_open_url("data:text/html,hi"));
    }

    #[test]
    fn normalize_url_agrega_https_antes_de_abrir() {
        assert_eq!(
            crate::smart_select::normalize_url_for_open("karloz.dev").as_deref(),
            Some("https://karloz.dev")
        );
    }

    #[test]
    fn selection_qualifies_rechaza_clic_suelto() {
        let point = SelectionPoint { row: 0, col: 3 };
        let sel = Selection::new(point);
        assert!(!App::selection_qualifies(&sel));
    }

    #[test]
    fn selection_qualifies_acepta_arrastre_y_semantica() {
        let mut drag = Selection::new(SelectionPoint { row: 0, col: 1 });
        drag.update_end(SelectionPoint { row: 0, col: 5 });
        assert!(App::selection_qualifies(&drag));

        let mut word = Selection::new(SelectionPoint { row: 0, col: 0 });
        word.mode = SelectionMode::Word;
        assert!(App::selection_qualifies(&word));
    }
}
