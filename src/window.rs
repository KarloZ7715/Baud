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
use crate::color_scheme::{self, SchemeSource};
use crate::config::watch::WatchState;
use crate::config::{
    persist, preset_polarity, ColorMode, ColorScheme, Config, ConfigSource, DecorationsKind,
    ProcessSection, StartupState,
};
use crate::copy_mode::CopyModeState;
use crate::display_quirks::{self, DisplayQuirks};
use crate::event_loop::{should_redraw, BlinkFocus};
use crate::grid::Cell;
use crate::input::actions::{normalize_binding_key, Action, Keybindings};
use crate::input::keymap::{self, Key as KKey, KeyEventKind, KeyModes, Mods};
use crate::input::wheel::{self, WheelIntent, WheelOwnerHint};
use crate::layout::{Rect as LayoutRect, TabLayout};
use crate::pty::PtyCommand;
use crate::renderer::{
    compute_layout, PaneRender, PreeditState, Renderer, TabBarLayout, TitleBarHit, TitleBarLayout,
    TitleButtonKind,
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
#[cfg(windows)]
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, CursorIcon, Fullscreen, ResizeDirection, Window, WindowId};

#[cfg(all(unix, not(target_os = "macos")))]
use winit::platform::wayland::WindowAttributesExtWayland;
#[cfg(all(unix, not(target_os = "macos")))]
use winit::platform::x11::WindowAttributesExtX11;

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
    /// El sistema cambió de modo claro/oscuro (portal XDG o winit).
    SystemColorScheme(ColorScheme),
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

/// En Windows, `event.logical_key` puede llegar ya compuesto por el layout
/// activo (AltGr = Ctrl+Alt fisico compone un caracter de tercer/cuarto nivel
/// distinto de 't', p.ej. "Þ" en layouts islandeses), asi que el match por
/// caracter logico en `Keybindings` nunca ve 't'. Se resuelve con la tecla
/// fisica (independiente del layout) como via alterna, solo para este chord.
#[cfg(windows)]
fn is_theme_picker_physical_chord(physical_key: &PhysicalKey, mods: Mods) -> bool {
    matches!(physical_key, PhysicalKey::Code(KeyCode::KeyT)) && mods.ctrl && mods.alt
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
/// Cuánto esperar antes de volver a pedir imagen del swapchain tras un fallo.
/// Más corto que el timeout de wgpu (1000 ms) para que la recuperación sea
/// perceptiblemente inmediata, y más largo que un frame a 60 Hz para no
/// reintentar dentro del mismo vblank.
const ACQUIRE_BACKOFF: Duration = Duration::from_millis(200);
/// Cadencia del sondeo de proceso en primer plano para el titulo de tabs.
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Tasa de decaimiento exponencial del fondo de hover de tab (~120 ms).
const TAB_HOVER_FADE_RATE: f32 = 25.0;
/// Tasa de decaimiento exponencial del fondo de hover de boton de ventana (~100 ms).
const TITLE_BAR_HOVER_FADE_RATE: f32 = 30.0;

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
    /// Último botón del mouse presionado mientras el reenvío a la app está activo.
    /// Necesario para codificar motion events con el botón correcto en modo 1002.
    mouse_down_button: Option<MouseButton>,
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
    /// Momento del ultimo request_redraw disparado por drag/extend_selection
    /// (distinto de last_gui_redraw, que registra el redraw ya renderizado).
    last_selection_redraw: Option<Instant>,
    /// True cuando un update de seleccion quedo diferido por el throttle;
    /// exige un redraw garantizado al terminar el gesto (R3).
    selection_redraw_pending: bool,
    /// Momento en que debe ejecutarse copy-on-select pendiente (tras multi-clic).
    copy_on_select_deadline: Option<Instant>,
    /// Selector interactivo de temas (exclusivo con copy mode).
    theme_picker: Option<ThemePickerState>,
    /// Esquema de color del SO resuelto (None = sin señal, cae a oscuro).
    system_color_scheme: Option<ColorScheme>,
    /// Origen del esquema del SO (portal/winit/fallback) — info para el picker.
    system_scheme_source: SchemeSource,
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
    /// Tab que dibuja el fondo de hover (persiste durante el fade-out, igual
    /// que `tab_close_tab`).
    tab_hover_display: Option<usize>,
    /// Opacidad animada del fondo de hover de tab (0..1, ~120 ms).
    tab_hover_alpha: f32,
    /// Marca de tiempo para interpolar `tab_hover_alpha`.
    tab_hover_anim_last: Instant,
    /// Tab que renderiza el boton × (incluye fade-out).
    tab_close_tab: Option<usize>,
    /// Opacidad animada del boton × (0..1).
    tab_close_alpha: f32,
    /// Botón de la barra de título bajo hover (solo modo custom).
    title_bar_hover: Option<TitleButtonKind>,
    /// Boton que dibuja el fondo de hover (persiste durante el fade-out).
    title_bar_hover_display: Option<TitleButtonKind>,
    /// Opacidad animada del fondo de hover de boton (0..1, ~100 ms).
    title_bar_hover_alpha: f32,
    /// Marca de tiempo para interpolar `title_bar_hover_alpha`.
    title_bar_hover_anim_last: Instant,
    /// Marca de tiempo del último clic en zona de arrastre de la barra.
    title_bar_drag_last_click: Option<Instant>,
    /// Marca de tiempo para interpolar el fade del ×.
    tab_anim_last: Instant,
    /// Ultimo sondeo del proceso en primer plano de las tabs (cadencia de
    /// `PROCESS_POLL_INTERVAL`). `None` fuerza un sondeo en el primer tick.
    last_process_poll: Option<Instant>,
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
    /// Titulo inicial de ventana solicitado por CLI; OSC 0/2 lo puede sobreescribir.
    initial_title: Option<String>,
    /// app_id de Wayland / instancia de WM_CLASS en X11; solo se lee en unix.
    #[cfg_attr(windows, allow(dead_code))]
    app_id: Option<String>,
    /// Marca de tiempo al entrar a `resumed()`; se consume al loguear el primer frame.
    startup_instant: Option<Instant>,
    /// Factor de escala DPI de la ventana (1.0 = 96 DPI).
    scale_factor: f64,
    /// Refresco del monitor en Hz; None si no se pudo resolver.
    /// Se re-resuelve al crear la ventana y al moverla entre monitores.
    monitor_refresh_hz: Option<u32>,
    // --- Sonda de latencia tecla→present (diagnostics.latency_probe) ---
    /// Instant en que se envió la última entrada al PTY; None si no hay
    /// eco pendiente. Solo se mide cuando el drain dispara el redraw, no
    /// en el request_redraw inmediato del handler de teclado (ese frame
    /// no contiene el eco todavía).
    pending_echo: Option<Instant>,
    /// True cuando el último RedrawNeeded vino del drain (procesó salida).
    /// Distingue los frames que pueden contener el eco de los redraws
    /// inmediatos del handler de teclado.
    drain_triggered_redraw: bool,
    /// Acumulador de muestras de latencia; emite p50/p95/p99 al llenarse.
    latency_probe: LatencyProbeStats,
    /// True mientras el compositor reporta la ventana como no visible
    /// (`WindowEvent::Occluded`). Pedir imagenes del swapchain en ese estado
    /// bloquea el event loop hasta el timeout de wgpu sin que nadie vea el
    /// resultado.
    occluded: bool,
    /// Instante del último fallo de adquisición del swapchain. Durante
    /// `ACQUIRE_BACKOFF` no se vuelve a pedir imagen: cada intento fallido
    /// cuesta hasta 1000 ms de event loop bloqueado, y el timer de parpadeo
    /// pide redraws cada 500 ms, así que sin backoff el fallo se realimenta.
    last_acquire_failure: Option<Instant>,
    /// True cuando el último frame dejó algún pane sin repintar (el `Term`
    /// estaba tomado). Obliga a un redraw de seguimiento.
    followup_redraw: bool,
}

/// Recopila muestras de latencia (µs) y registra p50/p95/p99 cada N.
struct LatencyProbeStats {
    samples: Vec<u64>,
    capacity: usize,
}

impl LatencyProbeStats {
    const DEFAULT_CAPACITY: usize = 60;

    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(Self::DEFAULT_CAPACITY),
            capacity: Self::DEFAULT_CAPACITY,
        }
    }

    /// Añade una muestra; devuelve los percentiles (µs) cuando se llena.
    fn record(&mut self, latency_us: u64) -> Option<(u64, u64, u64)> {
        self.samples.push(latency_us);
        if self.samples.len() < self.capacity {
            return None;
        }
        self.samples.sort_unstable();
        let p = |pct: f64| -> u64 {
            let idx = ((pct / 100.0) * self.samples.len() as f64) as usize;
            self.samples[idx.min(self.samples.len() - 1)]
        };
        let result = (p(50.0), p(95.0), p(99.0));
        self.samples.clear();
        Some(result)
    }
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
    open_url_with_default_handler(&normalized);
}

#[cfg(not(windows))]
fn open_url_with_default_handler(normalized: &str) {
    if let Err(e) = std::process::Command::new("xdg-open")
        .arg(normalized)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        tracing::warn!("open_url: xdg-open fallo para {}: {e}", normalized);
    }
}

#[cfg(windows)]
fn open_url_with_default_handler(normalized: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = OsStr::new(normalized)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Devuelve un HINSTANCE que en la práctica es un código de error cuando
    // vale <= 32; ShellExecuteW no expone un HRESULT real aquí.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        tracing::warn!(
            "open_url: ShellExecuteW fallo para {}: code {}",
            normalized,
            result as isize
        );
    }
}

impl App {
    /// Crea una nueva instancia de App con las sesiones dadas.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sessions: Vec<SessionHost>,
        config: Config,
        config_watch: Arc<Mutex<WatchState>>,
        proxy: Option<EventLoopProxy<UserEvent>>,
        blink_focus: Arc<BlinkFocus>,
        config_source: ConfigSource,
        watchdog: EventLoopWatchdog,
        initial_title: Option<String>,
        app_id: Option<String>,
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
            mouse_down_button: None,
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
            last_selection_redraw: None,
            selection_redraw_pending: false,
            copy_on_select_deadline: None,
            theme_picker: None,
            system_color_scheme: None,
            system_scheme_source: SchemeSource::default(),
            consent_prompt_active: false,
            config_watch,
            preedit: String::new(),
            preedit_cursor: None,
            proxy,
            pending_exit: false,
            detached_hosts: Vec::new(),
            tab_hover: None,
            tab_hover_display: None,
            tab_hover_alpha: 0.0,
            tab_hover_anim_last: Instant::now(),
            tab_close_tab: None,
            tab_close_alpha: 0.0,
            title_bar_hover: None,
            title_bar_hover_display: None,
            title_bar_hover_alpha: 0.0,
            title_bar_hover_anim_last: Instant::now(),
            title_bar_drag_last_click: None,
            tab_anim_last: Instant::now(),
            last_process_poll: None,
            pending_pane_sync: false,
            pending_config_source: Some(config_source),
            watchdog,
            redraw_interval_nanos,
            fps_overlay_visible: false,
            blink_focus,
            display_quirks: DisplayQuirks::DEFAULT,
            wheel_residual: 0.0,
            initial_title,
            app_id,
            startup_instant: None,
            scale_factor: 1.0,
            monitor_refresh_hz: None,
            pending_echo: None,
            drain_triggered_redraw: false,
            latency_probe: LatencyProbeStats::new(),
            occluded: false,
            last_acquire_failure: None,
            followup_redraw: false,
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
        let chrome_px = self.chrome_reserve_px(cell_h);
        let (rows, cols) = crate::renderer::limits::compute_grid_dims(
            width,
            height,
            cell_w,
            cell_h,
            self.config.window.padding_x,
            self.config.window.padding_y,
            0,
            chrome_px,
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
                self.drain_triggered_redraw = true;
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
                        // El dirty se limpia en `settle_frame_result`, cuando el
                        // frame ya se presento. Limpiarlo aqui lo perdia si el
                        // frame acababa saltandose (ventana ocluida, fallo de
                        // adquisicion) o sirviendose desde cache.
                        if let Some(i) = idx {
                            self.sessions[i].session.dirty = true;
                        }
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                } else if self.is_session_in_active_tab(id) {
                    if let Some(idx) = self.session_by_id(id) {
                        self.sessions[idx].session.dirty = true;
                        self.sessions[idx].session.has_activity = true;
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if let Some(idx) = self.session_by_id(id) {
                    self.sessions[idx].session.dirty = true;
                    self.sessions[idx].session.has_activity = true;
                }
            }
            UserEvent::PtyExited(id, code) => {
                let session_idx = self.session_by_id(id);
                let held = session_idx.is_some_and(|i| self.sessions[i].session.hold);
                let close_on_exit =
                    session_idx.is_some_and(|i| self.sessions[i].session.close_on_exit);

                if self.is_session_in_active_tab(id) {
                    if self.is_focused_session(id) {
                        if held {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.set_status(&format!(
                                    "[Proceso terminado: codigo {} (mantenido)]",
                                    code
                                ));
                            }
                        } else if close_on_exit {
                            self.pending_exit = true;
                            return;
                        } else if let Some(renderer) = &mut self.renderer {
                            renderer.set_status(&format!("[Proceso terminado: codigo {}]", code));
                        }
                    } else if self.tabs[self.focused].leaves().len() > 1 {
                        if held {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.set_status(&format!(
                                    "[Pane cerrado: codigo {} (mantenido)]",
                                    code
                                ));
                            }
                        } else {
                            if let Some(renderer) = &mut self.renderer {
                                renderer.set_status(&format!("[Pane cerrado: codigo {}]", code));
                            }
                            self.close_pane_session(id);
                        }
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
            UserEvent::SystemColorScheme(scheme) => {
                self.system_color_scheme = Some(scheme);
                self.system_scheme_source = SchemeSource::Portal;
                self.reconcile_theme();
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

    /// Toma el `Term` enfocado recuperando el guard si el mutex está
    /// envenenado. Un panic bajo el guard (p. ej. al construir el frame) lo
    /// envenena, y a partir de ahí un `expect` convertiría cada scroll en la
    /// muerte del proceso. El estado puede quedar raro un frame; morir es peor.
    fn lock_focused_term(&self) -> std::sync::MutexGuard<'_, Term> {
        match self.focused_term().lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("term mutex envenenado, recuperando guard");
                poisoned.into_inner()
            }
        }
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

        crate::diagnostics::logging::apply_log_level(new_cfg.diagnostics.log_level.as_deref());

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
        // Re-resolver el tema contra el esquema del SO conocido: una recarga
        // de disco trae `theme` resuelto a dark-fallback (esquema desconocido
        // al deserializar); aquí lo ajustamos al modo real del sistema.
        self.reconcile_theme();
        self.redraw_interval_nanos.store(
            self.config
                .render
                .redraw_interval_nanos_for_monitor(self.monitor_refresh_hz),
            Ordering::Relaxed,
        );

        // Sonda de latencia: limpiar estado pendiente si se desactiva.
        if !self.config.diagnostics.latency_probe {
            self.pending_echo = None;
        }

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

    /// Re-resuelve el tema activo contra el esquema del SO y redibuja.
    ///
    /// Usa el modelo (`theme_mode`/`theme_dark`/`theme_light`/overrides) de la
    /// config en memoria sin releer disco. Lo invocan: el watcher del portal
    /// (`UserEvent::SystemColorScheme`), el brazo `WindowEvent::ThemeChanged`
    /// de winit, y `apply_config` tras una recarga desde disco.
    fn reconcile_theme(&mut self) {
        let active = self.config.resolve_active_theme(self.system_color_scheme);
        self.config.theme = active.theme;
        self.config.theme_preset = active.preset;
        self.config.theme_import_label = active.import_label;
        self.config.theme_import_watch_paths = active.import_watch_paths;
        if let Ok(mut watch) = self.config_watch.lock() {
            watch.set_import_targets(self.config.theme_import_watch_paths.clone());
        }
        if let Ok(mut guard) = self.focused_term().try_lock() {
            guard.mark_dirty();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
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
        let chrome_px = self.chrome_reserve_px(cell_h);
        if let Some(renderer) = &mut self.renderer {
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

    /// Altura física reservada encima del grid para barra de tabs/título.
    fn chrome_reserve_px(&self, cell_h: f32) -> f32 {
        let has_custom_chrome = self.config.window.decorations.kind() == DecorationsKind::Custom;
        let bar_px = if has_custom_chrome && !self.is_fullscreen() {
            crate::renderer::title_bar_height_px(cell_h, self.scale_factor as f32)
        } else if self.tabs.len() > 1 {
            cell_h
        } else {
            0.0
        };
        if bar_px > 0.0 {
            bar_px + crate::renderer::TAB_CONTENT_GAP_PX
        } else {
            0.0
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

    fn title_bar_layout(&self, renderer: &Renderer) -> Option<TitleBarLayout> {
        if self.config.window.decorations.kind() != DecorationsKind::Custom || self.is_fullscreen()
        {
            return None;
        }
        let (pad_x, _) = renderer.content_padding();
        Some(crate::renderer::compute_title_bar_layout(
            self.window_width,
            pad_x,
            renderer.cell_h(),
            self.scale_factor as f32,
        ))
    }

    fn tab_bar_layout(
        &self,
        renderer: &Renderer,
        title_bar: Option<&TitleBarLayout>,
    ) -> Option<TabBarLayout> {
        if !self.tab_bar_visible() {
            return None;
        }
        let has_custom_chrome = self.config.window.decorations.kind() == DecorationsKind::Custom;
        let (titles, activities, icons): (Vec<String>, Vec<bool>, Vec<Option<char>>) =
            if self.tabs.len() == 1 && has_custom_chrome {
                let s = self.focused_session();
                let cwd = s.term.try_lock().ok().and_then(|t| t.cwd.clone());
                let process = s.foreground_cache.as_ref().map(|(_, n)| n.as_str());
                let (title, icon) =
                    crate::renderer::resolve_tab_title(&s.title, cwd.as_deref(), process);
                (vec![title], vec![false], vec![icon])
            } else {
                let mut titles = Vec::new();
                let mut activities = Vec::new();
                let mut icons = Vec::new();
                for tab in self.tabs.iter() {
                    if let Some(idx) = self.session_by_id(tab.focused()) {
                        let s = &self.sessions[idx].session;
                        let cwd = s.term.try_lock().ok().and_then(|t| t.cwd.clone());
                        let process = s.foreground_cache.as_ref().map(|(_, n)| n.as_str());
                        let (title, icon) =
                            crate::renderer::resolve_tab_title(&s.title, cwd.as_deref(), process);
                        titles.push(title);
                        activities.push(s.has_activity);
                        icons.push(icon);
                    }
                }
                (titles, activities, icons)
            };
        let (pad_x, _) = renderer.content_padding();
        let (bar_x, bar_w) = if let Some(tb) = title_bar {
            (tb.tab_area_x, tb.tab_area_width)
        } else {
            let w = crate::renderer::tab_bar_inner_width(self.window_width, pad_x);
            (pad_x, w)
        };
        let mut layout = compute_layout(&titles, self.focused, bar_x, bar_w, renderer.cell_w());
        for seg in &mut layout.segments {
            if seg.index < activities.len() {
                seg.activity = activities[seg.index];
            }
            if seg.index < icons.len() {
                seg.icon_candidate = icons[seg.index];
            }
        }
        Some(layout)
    }

    fn tab_bar_layout_with_mouse(
        &self,
        renderer: &Renderer,
        title_bar: Option<&TitleBarLayout>,
    ) -> Option<TabBarLayout> {
        let mut layout = self.tab_bar_layout(renderer, title_bar)?;
        layout.mouse.hover_index = self.tab_hover_display;
        layout.mouse.hover_alpha = self.tab_hover_alpha;
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

    /// Anima el fondo de hover de una tab inactiva hacia 1.0 mientras
    /// `tab_hover` apunte a una tab, y hacia 0.0 al salir (~120 ms). El
    /// indice mostrado (`tab_hover_display`) persiste durante el fade-out,
    /// igual que `tab_close_tab` para el boton ×.
    fn tick_tab_hover_fade(&mut self) -> bool {
        if self.tab_hover.is_some() {
            self.tab_hover_display = self.tab_hover;
        }
        let target = if self.tab_hover.is_some() { 1.0 } else { 0.0 };
        if (self.tab_hover_alpha - target).abs() < 0.005 {
            if target == 0.0 && self.tab_hover_alpha != 0.0 {
                self.tab_hover_alpha = 0.0;
                self.tab_hover_display = None;
                return true;
            }
            return false;
        }
        let dt = self.tab_hover_anim_last.elapsed().as_secs_f32().min(0.05);
        self.tab_hover_anim_last = Instant::now();
        let prev = self.tab_hover_alpha;
        self.tab_hover_alpha +=
            (target - self.tab_hover_alpha) * (TAB_HOVER_FADE_RATE * dt).min(1.0);
        if target == 0.0 && self.tab_hover_alpha < 0.02 {
            self.tab_hover_alpha = 0.0;
            self.tab_hover_display = None;
        }
        (self.tab_hover_alpha - prev).abs() > 0.005
    }

    /// Anima el fondo de hover de un boton de ventana hacia 1.0 mientras
    /// `title_bar_hover` apunte a un boton, y hacia 0.0 al salir (~100 ms).
    fn tick_title_bar_hover_fade(&mut self) -> bool {
        if self.title_bar_hover.is_some() {
            self.title_bar_hover_display = self.title_bar_hover;
        }
        let target = if self.title_bar_hover.is_some() {
            1.0
        } else {
            0.0
        };
        if (self.title_bar_hover_alpha - target).abs() < 0.005 {
            if target == 0.0 && self.title_bar_hover_alpha != 0.0 {
                self.title_bar_hover_alpha = 0.0;
                self.title_bar_hover_display = None;
                return true;
            }
            return false;
        }
        let dt = self
            .title_bar_hover_anim_last
            .elapsed()
            .as_secs_f32()
            .min(0.05);
        self.title_bar_hover_anim_last = Instant::now();
        let prev = self.title_bar_hover_alpha;
        self.title_bar_hover_alpha +=
            (target - self.title_bar_hover_alpha) * (TITLE_BAR_HOVER_FADE_RATE * dt).min(1.0);
        if target == 0.0 && self.title_bar_hover_alpha < 0.02 {
            self.title_bar_hover_alpha = 0.0;
            self.title_bar_hover_display = None;
        }
        (self.title_bar_hover_alpha - prev).abs() > 0.005
    }

    /// `true` si la barra de tabs/titulo se dibuja en el estado actual
    /// (misma condicion que `tab_bar_layout`, sin construir el layout).
    fn tab_bar_visible(&self) -> bool {
        let has_custom_chrome = self.config.window.decorations.kind() == DecorationsKind::Custom;
        !((self.tabs.len() <= 1 && !has_custom_chrome)
            || (has_custom_chrome && self.is_fullscreen()))
    }

    /// Sondea el proceso en primer plano de las sesiones visibles en la
    /// barra, como mucho cada `PROCESS_POLL_INTERVAL`. Vive en el tick del
    /// bucle de eventos, nunca en el camino de render (plan 011, verificacion
    /// 9). Devuelve si algun titulo cambio y, si la barra esta visible, el
    /// proximo instante en que hay que volver a sondear.
    fn tick_foreground_process_poll(&mut self) -> (bool, Option<Instant>) {
        if !self.tab_bar_visible() {
            return (false, None);
        }
        let now = Instant::now();
        if let Some(last) = self.last_process_poll {
            let deadline = last + PROCESS_POLL_INTERVAL;
            if now < deadline {
                return (false, Some(deadline));
            }
        }
        self.last_process_poll = Some(now);

        let indices: Vec<usize> = if self.tabs.len() == 1 {
            self.session_by_id(self.tabs[0].focused())
                .into_iter()
                .collect()
        } else {
            self.tabs
                .iter()
                .filter_map(|tab| self.session_by_id(tab.focused()))
                .collect()
        };
        let mut changed = false;
        for idx in indices {
            let session = &mut self.sessions[idx].session;
            if let Some(probe) = &session.foreground_probe {
                changed |= crate::pty::foreground::poll(probe, &mut session.foreground_cache);
            }
        }
        (changed, Some(now + PROCESS_POLL_INTERVAL))
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
            &cfg.process_config(),
            rows,
            cols,
            proxy.clone(),
            Arc::clone(&self.redraw_interval_nanos),
            false,
            false,
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
            &cfg.process_config(),
            new_rect.rows as u16,
            new_rect.cols as u16,
            proxy.clone(),
            Arc::clone(&self.redraw_interval_nanos),
            false,
            false,
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

    fn chrome_bar_metrics(&self, renderer: &Renderer) -> (f32, f32) {
        let (_, pad_y) = renderer.content_padding();
        let cell_h = renderer.cell_h();
        let bar_h = if self.config.window.decorations.kind() == DecorationsKind::Custom {
            crate::renderer::title_bar_height_px(cell_h, self.scale_factor as f32)
        } else {
            cell_h
        };
        (pad_y, bar_h)
    }

    fn tab_index_at(&self, x: f64, y: f64, renderer: &Renderer) -> Option<usize> {
        let title_bar = self.title_bar_layout(renderer);
        let layout = self.tab_bar_layout_with_mouse(renderer, title_bar.as_ref())?;
        let (bar_top, bar_h) = self.chrome_bar_metrics(renderer);
        crate::renderer::tab_index_at(&layout, x, y, bar_top, bar_h)
    }

    fn tab_close_at(&self, x: f64, y: f64, renderer: &Renderer) -> Option<usize> {
        let title_bar = self.title_bar_layout(renderer);
        let layout = self.tab_bar_layout_with_mouse(renderer, title_bar.as_ref())?;
        let (bar_top, bar_h) = self.chrome_bar_metrics(renderer);
        crate::renderer::tab_close_at(&layout, x, y, bar_top, bar_h, renderer.cell_w())
    }

    fn is_in_tab_bar_row(&self, y: f64, renderer: &Renderer) -> bool {
        if self.tabs.len() <= 1 && self.config.window.decorations.kind() != DecorationsKind::Custom
        {
            return false;
        }
        let (bar_top, bar_h) = self.chrome_bar_metrics(renderer);
        let top = f64::from(bar_top);
        let bottom = top + f64::from(bar_h);
        (top..bottom).contains(&y)
    }

    fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .is_some_and(|w| w.fullscreen().is_some())
    }

    fn title_bar_hit_at(&self, x: f64, y: f64, renderer: &Renderer) -> Option<TitleBarHit> {
        if self.config.window.decorations.kind() != DecorationsKind::Custom || self.is_fullscreen()
        {
            return None;
        }
        let layout = self.title_bar_layout(renderer)?;
        let (bar_top, _) = self.chrome_bar_metrics(renderer);
        crate::renderer::hit_test(&layout, x, y, bar_top)
    }

    fn resize_direction_at(&self, x: f64, y: f64) -> Option<ResizeDirection> {
        if self.config.window.decorations.kind() != DecorationsKind::Custom || self.is_fullscreen()
        {
            return None;
        }
        let border = 8.0 * self.scale_factor;
        let w = f64::from(self.window_width);
        let h = f64::from(self.window_height);
        let left = x < border;
        let right = x >= w - border;
        let top = y < border;
        let bottom = y >= h - border;
        match (top, bottom, left, right) {
            (true, _, _, true) => Some(ResizeDirection::NorthEast),
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (_, true, true, _) => Some(ResizeDirection::SouthWest),
            (true, _, _, _) => Some(ResizeDirection::North),
            (_, true, _, _) => Some(ResizeDirection::South),
            (_, _, true, _) => Some(ResizeDirection::West),
            (_, _, _, true) => Some(ResizeDirection::East),
            _ => None,
        }
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
        self.send_input(filtered);
    }

    /// Envia bytes de input al hilo PTY para escribirlos en el master fd.
    /// Para sesiones en hold el input se ignora (el proceso hijo ya termino).
    fn send_input(&self, bytes: Vec<u8>) {
        let session = self.focused_session();
        if session.hold {
            return;
        }
        // El flag se activa antes de intentar el lock: si el drain lo tiene
        // parseando output, el propio drain aplica el reset bajo su lock
        // antes de pintar el eco; si no, lo aplica about_to_wait. Un lock
        // bloqueante aqui congelaria el event loop compitiendo con el parseo.
        session.input_reset_pending.store(true, Ordering::Release);
        if let Ok(mut guard) = self.focused_term().try_lock() {
            guard.apply_input_reset();
            session.input_reset_pending.store(false, Ordering::Release);
        }
        // Marca que el proximo output de esta sesion lleva el eco de una
        // tecla: el drain lo usara para avisar sin esperar al intervalo de
        // max_fps.
        session.echo_pending.store(true, Ordering::Release);
        tracing::debug!("send_input: {} bytes: {:02x?}", bytes.len(), bytes);
        let _ = session.pty_tx.send(PtyCommand::Input(bytes));
    }

    /// Aplica el reset de vista diferido por `send_input` cuando el lock del
    /// term estaba ocupado. Si sigue ocupado el flag queda activo y el hilo
    /// de drain lo honra bajo su propio lock.
    fn apply_pending_input_reset(&self) {
        let session = self.focused_session();
        if !session.input_reset_pending.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut guard) = session.term.try_lock() {
            guard.apply_input_reset();
            session.input_reset_pending.store(false, Ordering::Release);
        }
    }

    /// Pide un redraw para un update de seleccion (drag o teclado) respetando
    /// el intervalo configurado. `mark_dirty()` ya debe haberse llamado antes;
    /// si el intervalo no elapsed, el request_redraw se difiere y queda
    /// `selection_redraw_pending` para forzarlo al terminar el gesto (R3).
    fn request_selection_redraw(&mut self, force: bool) {
        let now = Instant::now();
        let interval_nanos = self.redraw_interval_nanos.load(Ordering::Relaxed);
        let elapsed_enough = self
            .last_selection_redraw
            .is_none_or(|last| should_redraw(last, now, interval_nanos));
        if force || elapsed_enough {
            self.last_selection_redraw = Some(now);
            self.selection_redraw_pending = false;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else {
            self.selection_redraw_pending = true;
        }
    }

    /// Extiende la seleccion con teclado (Shift+arrow).
    /// Si no hay seleccion, crea una desde la posicion del cursor.
    fn extend_selection(&mut self, drow: isize, dcol: isize) {
        let mut changed = false;
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
            changed = true;
        }
        if changed {
            self.request_selection_redraw(false);
        }
    }

    /// Extiende la seleccion de teclado hacia una posicion absoluta (row, col),
    /// compartiendo con extend_selection la creacion-desde-cursor, el clamp de
    /// limites y el redraw diferido.
    fn extend_selection_to(&mut self, target_row: usize, target_col: usize) {
        let mut changed = false;
        if let Ok(mut guard) = self.focused_term().lock() {
            let cols_count = guard.grid.cols_count;
            let sb_len = if guard.alt_screen {
                0
            } else {
                guard.grid.scrollback.len()
            };
            let total_rows = sb_len + guard.grid.rows_count;
            let max_row = total_rows.saturating_sub(1);

            if guard.selection.is_none() {
                let abs_row = guard.cursor_logical_row();
                // El cursor descansa una columna despues del ultimo caracter
                // recien tecleado (celda en blanco aun no escrita). Anclar ahi
                // tal cual incluiria esa celda en blanco de mas una vez que
                // normalize() la trate como extremo "mayor" al extender hacia
                // atras. Anclar sobre el ultimo caracter real evita esa celda
                // fantasma sin afectar el caso en que el cursor ya esta sobre
                // contenido real (medio de linea).
                let content_len = guard.line_content_end_col(abs_row) + 1;
                let cur_col = if guard.cursor.col == content_len {
                    content_len.saturating_sub(1)
                } else {
                    guard.cursor.col
                };
                if abs_row < total_rows {
                    guard.selection = Some(Selection::new(SelectionPoint {
                        row: abs_row,
                        col: cur_col,
                    }));
                } else {
                    return;
                }
            }

            let new_row = target_row.min(max_row);
            let new_col = target_col.min(cols_count.saturating_sub(1));

            if let Some(ref mut sel) = guard.selection {
                sel.end.row = new_row;
                sel.end.col = new_col;
            }
            guard.scroll_to_show_logical_row(new_row);
            guard.mark_dirty();
            changed = true;
        }
        if changed {
            self.request_selection_redraw(false);
        }
    }

    /// Ctrl+Shift+Left/Right: extiende la seleccion al limite de palabra
    /// anterior/siguiente, reutilizando Term::word_boundary_left/right.
    fn extend_selection_word(&mut self, left: bool) {
        let target = {
            let Ok(guard) = self.focused_term().lock() else {
                return;
            };
            let (row, col) = guard
                .selection
                .as_ref()
                .map(|s| (s.end.row, s.end.col))
                .unwrap_or_else(|| (guard.cursor_logical_row(), guard.cursor.col));
            if left {
                guard.word_boundary_left(row, col)
            } else {
                guard.word_boundary_right(row, col)
            }
        };
        self.extend_selection_to(target.0, target.1);
    }

    /// Shift+Home/End: extiende la seleccion al inicio/fin de la fila logica
    /// actual (fin = ultimo caracter no-espacio, columna 0 si esta vacia).
    fn extend_selection_line_edge(&mut self, start: bool) {
        let target = {
            let Ok(guard) = self.focused_term().lock() else {
                return;
            };
            let row = guard
                .selection
                .as_ref()
                .map(|s| s.end.row)
                .unwrap_or_else(|| guard.cursor_logical_row());
            let col = if start {
                0
            } else {
                guard.line_content_end_col(row)
            };
            (row, col)
        };
        self.extend_selection_to(target.0, target.1);
    }

    /// Ctrl+Shift+Home/End: extiende la seleccion al borde superior/inferior
    /// del viewport visible actual (no todo el scrollback).
    fn extend_selection_viewport_edge(&mut self, start: bool) {
        let target = {
            let Ok(guard) = self.focused_term().lock() else {
                return;
            };
            let rows_count = guard.grid.rows_count;
            if start {
                (guard.visible_to_logical_row(0), 0)
            } else {
                let row = guard.visible_to_logical_row(rows_count.saturating_sub(1));
                (row, guard.line_content_end_col(row))
            }
        };
        self.extend_selection_to(target.0, target.1);
    }

    fn scroll_lines(&mut self, n: isize) {
        let mut guard = self.lock_focused_term();
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
        let mut guard = self.lock_focused_term();
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
        let mut guard = self.lock_focused_term();
        guard.scrollback_offset = 0;
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    fn jump_to_prev_prompt(&mut self) {
        let mut guard = self.lock_focused_term();
        guard.jump_to_prev_prompt();
        guard.mark_dirty();
        drop(guard);
        self.clear_link_hover_state();
    }

    fn jump_to_next_prompt(&mut self) {
        let mut guard = self.lock_focused_term();
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
            self.config.theme_mode,
            self.system_scheme_source,
            self.system_color_scheme,
            self.config.active_import_label().map(str::to_string),
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
        // La fila de import no es un preset que persistir: confirmar sobre
        // ella deja el import (ya activo) tal cual, igual que cancelar.
        if picker.is_import_selected() {
            self.cancel_theme_picker(picker);
            return;
        }
        let Some(name) = picker.try_selected_name() else {
            self.theme_picker = Some(picker);
            if let Some(renderer) = &mut self.renderer {
                renderer.set_status("[Theme picker: sin coincidencias para aplicar]");
            }
            return;
        };
        let name = name.to_string();
        let polarity = preset_polarity(&name);
        let had_import = self.config.theme_import_label.is_some();
        match persist::write_theme_variant(&name, polarity, had_import) {
            Ok(outcome) => {
                if let Ok(mut watch) = self.config_watch.lock() {
                    watch.sync(persist::file_mtime(&outcome.path));
                    if had_import {
                        watch.set_import_targets(Vec::new());
                    }
                }
                // Aplicar el preset elegido y actualizar el modelo en memoria:
                // la variante correspondiente + el modo fijado a su polaridad
                // (coincide con lo escrito a disco por `write_theme_variant`).
                self.config.theme = picker.preview_theme();
                self.config.theme_preset = Some(name.clone());
                self.config.theme_mode = match polarity {
                    ColorScheme::Dark => ColorMode::Dark,
                    ColorScheme::Light => ColorMode::Light,
                };
                match polarity {
                    ColorScheme::Dark => self.config.theme_dark = Some(name.clone()),
                    ColorScheme::Light => self.config.theme_light = Some(name.clone()),
                }
                if had_import {
                    self.config.disable_theme_import();
                }
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
                    "theme picker: preset '{name}' ({:?}) persistido en {}",
                    polarity,
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
            ExtendSelectionWordLeft => self.extend_selection_word(true),
            ExtendSelectionWordRight => self.extend_selection_word(false),
            ExtendSelectionLineStart => self.extend_selection_line_edge(true),
            ExtendSelectionLineEnd => self.extend_selection_line_edge(false),
            ExtendSelectionViewportStart => self.extend_selection_viewport_edge(true),
            ExtendSelectionViewportEnd => self.extend_selection_viewport_edge(false),
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

    /// Actualiza el estado de oclusión. Al volver a ser visible se pide un
    /// redraw: mientras estuvo oculta se acumuló dirty sin pintar.
    pub(crate) fn set_occluded(&mut self, hidden: bool) {
        if self.occluded == hidden {
            return;
        }
        self.occluded = hidden;
        tracing::debug!("ventana {}", if hidden { "ocluida" } else { "visible" });
        if !hidden {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    /// True si este frame no debe intentar adquirir imagen del swapchain.
    pub(crate) fn should_skip_frame(&self) -> bool {
        self.occluded || self.acquire_backoff_active()
    }

    /// Anota un fallo de adquisición para abrir la ventana de backoff.
    pub(crate) fn note_acquire_failure(&mut self) {
        self.last_acquire_failure = Some(Instant::now());
    }

    /// True mientras el backoff siga vigente.
    pub(crate) fn acquire_backoff_active(&self) -> bool {
        self.last_acquire_failure
            .is_some_and(|t| t.elapsed() < ACQUIRE_BACKOFF)
    }

    #[cfg(test)]
    pub(crate) fn expire_acquire_backoff_for_test(&mut self) {
        self.last_acquire_failure = Some(Instant::now() - ACQUIRE_BACKOFF);
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

    /// Aplica el resultado de un frame: limpia el dirty de los panes que sí
    /// se repintaron (`updated`) y lo conserva en los que se sirvieron desde
    /// caché porque el `Term` estaba ocupado (`stale`). Los panes con frame
    /// sincronizado en curso no llegan aquí: el llamador los filtra antes.
    pub(crate) fn settle_frame_result(&mut self, updated: Vec<SessionId>, stale: Vec<SessionId>) {
        for id in updated {
            let Some(idx) = self.session_by_id(id) else {
                continue;
            };
            self.sessions[idx].session.dirty = false;
            if let Ok(mut guard) = self.sessions[idx].session.term.try_lock() {
                guard.take_dirty();
            }
        }
        for id in stale {
            if let Some(idx) = self.session_by_id(id) {
                self.sessions[idx].session.dirty = true;
            }
            self.followup_redraw = true;
        }
    }

    /// True si el último frame dejó trabajo sin pintar.
    #[cfg(test)]
    pub(crate) fn needs_followup_redraw(&self) -> bool {
        self.followup_redraw
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
            // X10: todas las liberaciones se reportan como boton 3.
            let b = (if release { 3 } else { button }) + 0x20;
            let cx = (x.min(223) + 0x20) as u8;
            let cy = (y.min(223) + 0x20) as u8;
            Some(vec![0x1b, b'M', b, cx, cy])
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
        self.apply_pending_input_reset();
        let close_fade_changed = self.tick_tab_close_fade();
        let tab_hover_changed = self.tick_tab_hover_fade();
        let title_hover_changed = self.tick_title_bar_hover_fade();
        if close_fade_changed || tab_hover_changed || title_hover_changed {
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
        let (process_titles_changed, process_poll_wake) = self.tick_foreground_process_poll();
        if process_titles_changed {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if let Some(deadline) = process_poll_wake {
            wake_at = Some(deadline);
        }
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

        let t_start = Instant::now();
        self.startup_instant = Some(t_start);
        self.display_quirks = display_quirks::snapshot_for_event_loop(event_loop);

        // El escaneo de fuentes del sistema no depende de la GPU: arrancar el
        // hilo ya mismo lo solapa con la negociacion de adapter/device de wgpu
        // en vez de esperar a que termine antes de tocar wgpu.
        let font_fallback = self.config.font.fallback.clone();
        let font_thread = std::thread::spawn(move || {
            crate::renderer::create_font_system_with_fallback(&font_fallback)
        });

        // 1. Crear ventana.
        let t_window = Instant::now();
        let wcfg = &self.config.window;
        let initial_title = self.initial_title.as_deref().unwrap_or("baud");
        let decorations_kind = wcfg.decorations.kind();
        let mut attrs = Window::default_attributes()
            .with_title(initial_title)
            .with_inner_size(winit::dpi::LogicalSize::new(wcfg.width, wcfg.height))
            .with_decorations(decorations_kind != DecorationsKind::None);
        #[cfg(windows)]
        if decorations_kind == DecorationsKind::Custom {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_undecorated_shadow(true);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(app_id) = &self.app_id {
            // Los dos traits escriben en el mismo campo platform_specific.name,
            // asi que el orden no importa; con valores iguales ambos lados quedan
            // app_id para Wayland (general) y X11 (instance/class).
            attrs = WindowAttributesExtWayland::with_name(attrs, app_id.clone(), app_id.clone());
            attrs = WindowAttributesExtX11::with_name(attrs, app_id.clone(), app_id.clone());
        }
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
        // En Windows el alfa por framebuffer no basta para que el escritorio
        // se vea a traves: se pide el material Mica al DWM. Ambas ramas leen
        // el mismo umbral, y `restart_required_fields` ya marca el cruce.
        #[cfg(windows)]
        let attrs = match select_windows_backdrop(opacity) {
            WindowsBackdropChoice::Mica => {
                use winit::platform::windows::{BackdropType, WindowAttributesExtWindows};
                attrs.with_system_backdrop(BackdropType::MainWindow)
            }
            WindowsBackdropChoice::None => attrs,
        };
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("no se pudo crear la ventana"),
        );
        self.window = Some(window.clone());
        self.scale_factor = window.scale_factor();
        // Esquema de color del SO vía winit (Win/Mac). En Linux winit devuelve
        // None y el portal lo resuelve aparte; el `apply_config` final de
        // `resumed` re-resuelve con lo que ya se conozca aquí.
        if let Some(scheme) = color_scheme::system_color_scheme(&window) {
            self.system_color_scheme = Some(scheme);
            self.system_scheme_source = SchemeSource::Winit;
        }
        // Resolver el refresco del monitor para `max_fps` automático.
        // current_monitor() devuelve None en algunos compositores Wayland y
        // en headless; refresh_rate_millihertz() puede devolver None igualmente.
        // El fallback a 60 Hz cubre ambos casos.
        let monitor_hz = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .map(|millihz| millihz / 1000);
        if let Some(hz) = monitor_hz {
            tracing::info!("startup: monitor refresco {} Hz", hz);
        } else {
            tracing::info!("startup: monitor refresco desconocido, fallback 60 Hz");
        }
        self.monitor_refresh_hz = monitor_hz;
        self.redraw_interval_nanos.store(
            self.config
                .render
                .redraw_interval_nanos_for_monitor(monitor_hz),
            Ordering::Relaxed,
        );
        window.set_ime_allowed(true);
        tracing::info!(
            "startup: ventana creada en {}ms",
            t_window.elapsed().as_millis()
        );

        // 2. Obtener display handle para wgpu (evita el lifetime de ActiveEventLoop).
        let display_handle = event_loop.owned_display_handle();

        // 3. Inicializar wgpu: instance, adapter, device, queue, surface, config.
        //    wgpu 29 tiene API async (request_adapter, request_device retornan Future).
        //    Usamos block_on() local (sin pollster) para bloquear en nativo.
        // El mut solo se usa en la rama cfg(windows) de abajo.
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut instance_desc =
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle));
        // En DX12 la swapchain desde HWND solo ofrece alpha Opaque; el visual
        // de DirectComposition es el unico camino a transparencia real (y a
        // que el material Mica se vea). Solo aplica con opacidad < 1.0.
        #[cfg(windows)]
        if opacity < 1.0 {
            instance_desc.backend_options.dx12.presentation_system =
                wgpu::Dx12SwapchainKind::DxgiFromVisual;
        }
        let instance = wgpu::Instance::new(instance_desc);

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
        // Performance es el default de wgpu; MemoryUsage pide bloques de
        // suballocacion menores y el buffer de glifos se recrea a menudo.
        let make_desc = |limits: wgpu::Limits| wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        };
        // downlevel_defaults() es el piso nativo garantizado; si una GPU vieja
        // no lo satisface se degrada al piso WebGL2 en vez de paniquear.
        let (device, queue) = match block_on(adapter.request_device(&make_desc(
            wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ))) {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(
                    "wgpu: device con downlevel_defaults fallo ({err}); \
                     reintentando con el piso WebGL2"
                );
                block_on(adapter.request_device(&make_desc(
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                )))
                .expect("no se pudo crear el device GPU")
            }
        };
        tracing::info!(
            "wgpu: device listo en {}ms (init GPU total {}ms)",
            t_device.elapsed().as_millis(),
            t_gpu_init.elapsed().as_millis()
        );

        let t_surface_cfg = Instant::now();
        let size = window.inner_size();
        let surface_w = size.width.clamp(1, 16_384);
        let surface_h = size.height.clamp(1, 16_384);
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, surface_w, surface_h)
            .expect("no se encontro formato de surface compatible");
        // get_default_config toma el primer formato en el orden del driver,
        // que varia entre backends y servidores graficos; se fija uno de la
        // lista de preferencia para que el pipeline de color sea estable.
        config.format = pick_surface_format(&caps.formats);
        if !matches!(
            config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        ) {
            tracing::warn!(
                "wgpu: surface sin formato no-sRGB de 8 bits; elegido {:?} \
                 (soportados: {:?}). El pipeline de color queda en el camino \
                 degradado",
                config.format,
                caps.formats,
            );
        }
        // Las variantes Auto* degradan solas si el backend no soporta
        // Mailbox/Immediate; las crudas pueden paniquear en configure.
        config.present_mode = if self.config.render.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        // wgpu traduce esto a `min_image_count = valor + 1`
        // (wgpu-hal/src/vulkan/swapchain/native.rs). Con 1 el swapchain queda
        // en 2 imagenes, que es el minimo absoluto para FIFO: cada `acquire`
        // bloquea hasta que el compositor libera la anterior, y si tarda
        // (workspace oculto, direct scanout de otra ventana, VRR) se agota el
        // timeout interno de wgpu — 1000 ms con el event loop entero parado.
        // 2 da 3 imagenes: un frame de cola a cambio de que el hilo GUI no se
        // bloquee. El frame de latencia se recupera en el plan 002, que quita
        // el throttle de max_fps del camino del eco.
        config.desired_maximum_frame_latency = 2;
        // Si hay transparencia, asegurar que el alpha mode sea compatible
        if opacity < 1.0 {
            match select_alpha_mode(opacity, &caps.alpha_modes) {
                Some(mode) => {
                    config.alpha_mode = mode;
                    config.view_formats = vec![config.format.add_srgb_suffix()];
                }
                // Sin swapchain translucida disponible la ventana queda
                // opaca; mejor degradar que paniquear en configure.
                None => tracing::warn!(
                    "window.opacity < 1.0 pero el backend GPU no soporta \
                     swapchain translucida (alpha modes: {:?}); \
                     la ventana sera opaca",
                    caps.alpha_modes
                ),
            }
        }
        surface.configure(&device, &config);
        tracing::info!(
            "startup: surface config lista en {}ms",
            t_surface_cfg.elapsed().as_millis()
        );
        // El formato de surface decide todo el pipeline de color del frame;
        // se registra con las capacidades crudas para diagnosticar diferencias
        // entre servidores graficos (Wayland vs X11) y drivers.
        let adapter_info = adapter.get_info();
        tracing::info!(
            "wgpu: surface formato={:?} alpha_mode={:?} present_mode={:?} \
             frame_latency={} backend={:?} adaptador={:?} \
             formatos_soportados={:?}",
            config.format,
            config.alpha_mode,
            config.present_mode,
            config.desired_maximum_frame_latency,
            adapter_info.backend,
            adapter_info.name,
            caps.formats,
        );

        // Pintar el fondo del tema y presentar ya: la ventana no queda vacia
        // mientras el hilo de fuentes sigue escaneando en segundo plano.
        let t_early_present = Instant::now();
        let (bg_r, bg_g, bg_b) = crate::config::parse_hex(&self.config.theme.background);
        let clear_color = crate::renderer::frame_clear_color(
            (bg_r, bg_g, bg_b),
            opacity,
            config.format.is_srgb(),
        );
        if let wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = surface.get_current_texture()
        {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("early background clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    ..Default::default()
                });
            }
            queue.submit(std::iter::once(encoder.finish()));
            frame.present();
            tracing::info!(
                "startup: ventana pintada (fondo) en {}ms",
                t_early_present.elapsed().as_millis()
            );
        }

        let t_font_join = Instant::now();
        let font_system = match font_thread.join() {
            Ok(font_system) => font_system,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        tracing::info!(
            "startup: join del hilo de fonts esperado {}ms",
            t_font_join.elapsed().as_millis()
        );

        // 4. Crear Renderer.
        let t_renderer = Instant::now();
        self.renderer = Some(Renderer::new(
            window.clone(),
            device,
            queue,
            surface,
            config,
            &self.config.font,
            font_system,
            self.scale_factor as f32,
        ));
        tracing::info!(
            "startup: renderer construido en {}ms",
            t_renderer.elapsed().as_millis()
        );
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
            WindowEvent::Occluded(hidden) => {
                self.set_occluded(hidden);
            }
            WindowEvent::Focused(focused) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_window_focused(focused);
                }
                self.blink_focus.set_window_focused(focused);
                // Invalidar el pane enfocado para que el cursor cambie de
                // forma (bloque ↔ contorno) en el siguiente frame.
                if let Some(idx) = self.session_by_id(self.tabs[self.focused].focused()) {
                    self.sessions[idx].session.dirty = true;
                    if let Ok(mut guard) = self.sessions[idx].session.term.try_lock() {
                        guard.mark_dirty();
                    }
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                let Ok(guard) = self.focused_term().try_lock() else {
                    self.watchdog.note_term_lock_busy();
                    return;
                };
                if guard.mouse_reporting.focus {
                    let seq = if focused {
                        b"\x1b[I".to_vec()
                    } else {
                        b"\x1b[O".to_vec()
                    };
                    drop(guard);
                    self.send_pty_bytes(seq);
                }
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                inner_size_writer: _,
            } => {
                self.scale_factor = scale_factor;
                let window = self.window.clone();
                let Some(window) = window else {
                    return;
                };
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                // Re-resolver el refresco del monitor: ScaleFactorChanged
                // se dispara al mover la ventana entre monitores.
                let monitor_hz = window
                    .current_monitor()
                    .and_then(|m| m.refresh_rate_millihertz())
                    .map(|millihz| millihz / 1000);
                if monitor_hz != self.monitor_refresh_hz {
                    self.monitor_refresh_hz = monitor_hz;
                    self.redraw_interval_nanos.store(
                        self.config
                            .render
                            .redraw_interval_nanos_for_monitor(monitor_hz),
                        Ordering::Relaxed,
                    );
                }
                let size = window.inner_size();
                renderer.set_scale_factor(scale_factor as f32);
                renderer.resize(size.width, size.height, 0);
                let cell_w = renderer.cell_w;
                let cell_h = renderer.cell_h;
                let (_old_rows, _old_cols, new_rows, new_cols, deferred) =
                    self.sync_grid_to_window(size.width, size.height, cell_w, cell_h, true, true);
                self.pending_pane_sync = deferred;
                tracing::debug!(
                    "[SCALE] factor={:.2} win={}x{} -> grid={}x{}",
                    scale_factor,
                    size.width,
                    size.height,
                    new_rows,
                    new_cols,
                );
                window.request_redraw();
                self.update_ime_area();
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
                // El resumen de grid toma un lock bloqueante del term y construye
                // strings por fila; solo se justifica si el log debug esta activo.
                if tracing::enabled!(tracing::Level::DEBUG) {
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
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                self.update_ime_area();
            }
            WindowEvent::RedrawRequested => {
                if self.should_skip_frame() {
                    tracing::debug!("RedrawRequested: skip (ventana ocluida)");
                    return;
                }
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
                let title_bar_layout = self
                    .renderer
                    .as_ref()
                    .and_then(|r| self.title_bar_layout(r));
                let tab_layout = self
                    .renderer
                    .as_ref()
                    .and_then(|r| self.tab_bar_layout_with_mouse(r, title_bar_layout.as_ref()));
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
                // Al enfocar la sesion se apaga su indicador de actividad.
                if let Some(idx) = self.session_by_id(focused_id) {
                    self.sessions[idx].session.has_activity = false;
                }

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
                let frame_count_before = renderer.frame_count();
                let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
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
                    title_bar_layout.as_ref(),
                    self.title_bar_hover_display,
                    self.title_bar_hover_alpha,
                    maximized,
                ) {
                    Ok(updated) => {
                        // Ok(_) tambien lo devuelven los early-return de render()
                        // que no llegan a dibujar (Timeout/Occluded/Outdated/Lost);
                        // frame_count solo sube en los paths que si presentan.
                        if renderer.frame_count() > frame_count_before {
                            if let Some(t_start) = self.startup_instant.take() {
                                tracing::info!(
                                    "startup: time-to-first-frame {}ms",
                                    t_start.elapsed().as_millis()
                                );
                            }
                        }
                        let stale = renderer.take_stale_panes();
                        let (kept, settled): (Vec<_>, Vec<_>) = updated
                            .into_iter()
                            .partition(|id| deferred_at_schedule.contains(id));
                        // Frames diferidos por sync: no limpiar dirty, se
                        // reintenta al cerrar el sync.
                        self.settle_frame_result(settled, stale);
                        for id in kept {
                            if let Some(idx) = self.session_by_id(id) {
                                self.sessions[idx].session.dirty = true;
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
                let acquire_failure = self
                    .renderer
                    .as_mut()
                    .and_then(|r| r.take_acquire_failure());
                if acquire_failure.is_some() {
                    self.note_acquire_failure();
                }
                let render_ms = t_render.elapsed().as_millis();
                if render_ms > 250 {
                    match acquire_failure {
                        Some(failure) => tracing::warn!(
                            "render lento: {}ms — el compositor no libero imagen del swapchain \
                             ({}, espera {}ms). El frame no se pinto.",
                            render_ms,
                            failure.kind,
                            failure.waited_ms,
                        ),
                        None => tracing::warn!(
                            "render lento: {}ms ({} panes, status_present={}, search={})",
                            render_ms,
                            panes.len(),
                            status_needs_present,
                            search_active
                        ),
                    }
                }

                // Sonda de latencia: medir tecla→present solo en frames
                // disparados por el drain (contienen el eco), no en el
                // request_redraw inmediato del handler de teclado.
                if self.config.diagnostics.latency_probe {
                    if let Some(t_echo) = self.pending_echo.take() {
                        if self.drain_triggered_redraw {
                            let us = t_echo.elapsed().as_micros() as u64;
                            if let Some((p50, p95, p99)) = self.latency_probe.record(us) {
                                tracing::info!(
                                    "[LATENCY] p50={}µs p95={}µs p99={}µs",
                                    p50,
                                    p95,
                                    p99
                                );
                            }
                        } else {
                            // El frame no vino del drain: reponer para la
                            // próxima presentación que sí contenga el eco.
                            self.pending_echo = Some(t_echo);
                        }
                    }
                }
                if std::mem::take(&mut self.followup_redraw) {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                self.drain_triggered_redraw = false;
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
                    if let Some(hit) = self.title_bar_hit_at(position.x, position.y, renderer) {
                        if let TitleBarHit::Button(kind) = hit {
                            let changed = self.title_bar_hover != Some(kind);
                            if changed {
                                self.title_bar_hover = Some(kind);
                                if let Some(window) = &self.window {
                                    window.set_cursor(CursorIcon::Pointer);
                                    window.request_redraw();
                                }
                            }
                            if self.tab_hover.is_some() {
                                self.tab_hover = None;
                                self.tab_anim_last = Instant::now();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                            return;
                        }
                        if hit != TitleBarHit::TabArea && self.tab_hover.is_some() {
                            self.tab_hover = None;
                            self.tab_anim_last = Instant::now();
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        if self.title_bar_hover.is_some() {
                            self.title_bar_hover = None;
                            if let Some(window) = &self.window {
                                window.set_cursor(CursorIcon::Default);
                                window.request_redraw();
                            }
                        }
                    } else if let Some(dir) = self.resize_direction_at(position.x, position.y) {
                        let icon = match dir {
                            ResizeDirection::North => CursorIcon::NResize,
                            ResizeDirection::South => CursorIcon::SResize,
                            ResizeDirection::East => CursorIcon::EResize,
                            ResizeDirection::West => CursorIcon::WResize,
                            ResizeDirection::NorthEast => CursorIcon::NeResize,
                            ResizeDirection::NorthWest => CursorIcon::NwResize,
                            ResizeDirection::SouthEast => CursorIcon::SeResize,
                            ResizeDirection::SouthWest => CursorIcon::SwResize,
                        };
                        if let Some(window) = &self.window {
                            window.set_cursor(icon);
                        }
                        if self.title_bar_hover.is_some() {
                            self.title_bar_hover = None;
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        if self.tab_hover.is_some() {
                            self.tab_hover = None;
                            self.tab_anim_last = Instant::now();
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        return;
                    } else {
                        if self.title_bar_hover.is_some() {
                            self.title_bar_hover = None;
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                        if let Some(window) = &self.window {
                            window.set_cursor(CursorIcon::Default);
                        }
                    }
                }

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
                        let held = self.mouse_down_button;
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
                            if held.is_some() && reporting.drag {
                                drop(guard);
                                if self.last_reported_cell != Some(cell) {
                                    self.last_reported_cell = Some(cell);
                                    let btn = match held {
                                        Some(MouseButton::Left) => 0,
                                        Some(MouseButton::Middle) => 1,
                                        Some(MouseButton::Right) => 2,
                                        _ => 0,
                                    };
                                    self.forward_mouse_motion(32 + btn);
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
                    self.request_selection_redraw(false);
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

                // Interacciones de la barra de título propia: botones, arrastre,
                // doble clic y menú de sistema. Solo en modo custom y no fullscreen.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(renderer) = &self.renderer {
                        if let Some(hit) =
                            self.title_bar_hit_at(self.mouse_x, self.mouse_y, renderer)
                        {
                            match hit {
                                TitleBarHit::Button(kind) => {
                                    let Some(window) = self.window.clone() else {
                                        return;
                                    };
                                    match kind {
                                        TitleButtonKind::Minimize => {
                                            window.set_minimized(true);
                                        }
                                        TitleButtonKind::Maximize => {
                                            window.set_maximized(!window.is_maximized());
                                        }
                                        TitleButtonKind::Close => {
                                            for host in &self.sessions {
                                                let _ =
                                                    host.session.pty_tx.send(PtyCommand::Shutdown);
                                            }
                                            event_loop.exit();
                                        }
                                    }
                                    return;
                                }
                                TitleBarHit::Drag => {
                                    let Some(window) = self.window.clone() else {
                                        return;
                                    };
                                    let now = Instant::now();
                                    let is_double =
                                        self.title_bar_drag_last_click.is_some_and(|t| {
                                            now.duration_since(t) < MULTI_CLICK_INTERVAL
                                        });
                                    if is_double {
                                        self.title_bar_drag_last_click = None;
                                        if window.is_maximized() {
                                            window.set_maximized(false);
                                        } else {
                                            window.set_maximized(true);
                                        }
                                    } else {
                                        self.title_bar_drag_last_click = Some(now);
                                        let _ = window.drag_window();
                                    }
                                    return;
                                }
                                TitleBarHit::TabArea => {
                                    // Dejar que el manejo de tabs lo procese.
                                }
                            }
                        }
                        if let Some(idx) = self.tab_close_at(self.mouse_x, self.mouse_y, renderer) {
                            self.close_tab_at(idx);
                            return;
                        }
                        if let Some(idx) = self.tab_index_at(self.mouse_x, self.mouse_y, renderer) {
                            self.focus_session(idx);
                            return;
                        }
                        if let Some(dir) = self.resize_direction_at(self.mouse_x, self.mouse_y) {
                            if let Some(window) = &self.window {
                                let _ = window.drag_resize_window(dir);
                            }
                            return;
                        }
                    }
                }

                if button == MouseButton::Right && state == ElementState::Pressed {
                    if let Some(renderer) = &self.renderer {
                        if let Some(TitleBarHit::Drag) =
                            self.title_bar_hit_at(self.mouse_x, self.mouse_y, renderer)
                        {
                            if let Some(window) = &self.window {
                                window.show_window_menu(winit::dpi::PhysicalPosition::new(
                                    self.mouse_x,
                                    self.mouse_y,
                                ));
                            }
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
                    // Fin del gesto: garantiza el frame final aunque el
                    // ultimo update de seleccion haya quedado diferido (R3).
                    if self.selection_redraw_pending {
                        self.request_selection_redraw(true);
                    }
                }

                match self.try_should_forward_mouse_to_app() {
                    None => {
                        self.watchdog.note_term_lock_busy();
                        return;
                    }
                    Some(true) => {
                        let Some(renderer) = &self.renderer else {
                            return;
                        };
                        if self.is_in_tab_bar_row(self.mouse_y, renderer) {
                            return;
                        }
                        let focused_id = self.tabs[self.focused].focused();
                        if let Some((id, _, _)) =
                            self.pixel_to_pane_cell(self.mouse_x, self.mouse_y, renderer)
                        {
                            if id != focused_id {
                                // Click en otro pane: enfocarlo y no reenviar el evento
                                // a la sesion que tenia el foco.
                                self.focus_pane_by_id(id);
                                return;
                            }
                        } else {
                            // Fuera de cualquier pane: no reenviar.
                            return;
                        }
                        let btn = match button {
                            MouseButton::Left => 0,
                            MouseButton::Middle => 1,
                            MouseButton::Right => 2,
                            _ => return,
                        };
                        let release = state == ElementState::Released;
                        self.mouse_down_button = if release { None } else { Some(button) };
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
                    #[cfg(windows)]
                    if is_theme_picker_physical_chord(&event.physical_key, mods) {
                        self.run_action(Action::ToggleThemePicker);
                        return;
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
                #[cfg(windows)]
                if is_theme_picker_physical_chord(&event.physical_key, mods) {
                    self.run_action(Action::ToggleThemePicker);
                    return;
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
                // Sonda de latencia: marcar el instante de envío justo antes
                // de encode_key. Solo se mide cuando el drain dispara el redraw
                // (drain_triggered_redraw), no en el request_redraw inmediato
                // de abajo: ese frame no contiene el eco todavía.
                if self.config.diagnostics.latency_probe && self.pending_echo.is_none() {
                    self.pending_echo = Some(Instant::now());
                }
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
            WindowEvent::ThemeChanged(theme) => {
                // Windows/macOS: winit avisa cuando el escritorio cambia de
                // modo. Re-resolvemos el tema por el mismo camino que el portal.
                let scheme = match theme {
                    winit::window::Theme::Dark => Some(ColorScheme::Dark),
                    winit::window::Theme::Light => Some(ColorScheme::Light),
                };
                if let Some(scheme) = scheme {
                    self.system_color_scheme = Some(scheme);
                    self.system_scheme_source = SchemeSource::Winit;
                    self.reconcile_theme();
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
            UserEvent::SystemColorScheme(_) => "UserEvent::SystemColorScheme",
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
// Material de ventana nativo en Windows
// ---------------------------------------------------------------------------

/// Elección de material DWM para la ventana, derivada solo de la opacidad.
///
/// Tipo propio (no de winit) para que la decisión sea testeable en cualquier
/// SO; el mapeo al `BackdropType` de winit vive en el `cfg(windows)` de
/// `resumed()`. Fuera de Windows solo existe en tests.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsBackdropChoice {
    None,
    Mica,
}

/// Mismo umbral `< 1.0` que `with_transparent`: con opacidad plena la ventana
/// queda opaca; por debajo, Mica da el fondo translúcido nativo.
#[cfg(any(windows, test))]
fn select_windows_backdrop(opacity: f32) -> WindowsBackdropChoice {
    if opacity < 1.0 {
        WindowsBackdropChoice::Mica
    } else {
        WindowsBackdropChoice::None
    }
}

/// Formato de surface segun una lista de preferencia, limitado a lo que el
/// backend soporta. El primer formato no-sRGB de 8 bits deja correctos a la
/// vez el clear, el atlas de color y la mezcla del antialiasing en espacio
/// codificado; sin el, cada consumidor tendria que linealizar por su cuenta.
/// Si ninguno de la lista esta soportado se cae al primer formato del
/// backend, asumiendo el camino degradado (avisa el llamante con `warn`).
fn pick_surface_format(supported: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    const PREFERRED: &[wgpu::TextureFormat] = &[
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ];
    PREFERRED
        .iter()
        .copied()
        .find(|f| supported.contains(f))
        .unwrap_or(supported[0])
}

/// Alpha mode de la swapchain para una opacidad dada, limitado a lo que el
/// backend soporta. `None` = dejar el default del surface config (ventana
/// opaca): con opacidad plena es lo deseado, y con opacidad < 1.0 evita un
/// panic en `configure` en backends sin swapchain translúcida. El clear es
/// premultiplicado, asi que `PreMultiplied` es la unica eleccion exacta;
/// `Auto` queda como degradado delegando la eleccion a wgpu.
fn select_alpha_mode(
    opacity: f32,
    supported: &[wgpu::CompositeAlphaMode],
) -> Option<wgpu::CompositeAlphaMode> {
    if opacity >= 1.0 {
        return None;
    }
    [
        wgpu::CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::Auto,
    ]
    .into_iter()
    .find(|mode| supported.contains(mode))
}

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

    #[test]
    fn formato_surface_prefiere_8bits_no_srgb() {
        use wgpu::TextureFormat as F;
        // Con ambos disponibles gana el no-sRGB: deja correctos a la vez el
        // clear, el atlas de color y la mezcla del antialiasing.
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]),
            F::Bgra8Unorm
        );
        // Bgra tiene prioridad sobre Rgba dentro del mismo grupo.
        assert_eq!(
            pick_surface_format(&[F::Rgba8Unorm, F::Bgra8Unorm]),
            F::Bgra8Unorm
        );
        // Sin opcion no-sRGB se acepta el sRGB de 8 bits (camino degradado).
        assert_eq!(pick_surface_format(&[F::Bgra8UnormSrgb]), F::Bgra8UnormSrgb);
        // Sin ninguno de la lista se cae al primer formato soportado.
        assert_eq!(pick_surface_format(&[F::Rgba16Unorm]), F::Rgba16Unorm);
    }

    #[test]
    fn alpha_mode_respeta_lo_soportado_por_el_backend() {
        use wgpu::CompositeAlphaMode as A;
        // Opacidad plena: no se toca el alpha mode del default config.
        assert_eq!(select_alpha_mode(1.0, &[A::PreMultiplied]), None);
        // Backend con soporte (Linux/Vulkan, DX12 con visual): PreMultiplied.
        assert_eq!(
            select_alpha_mode(0.9, &[A::Opaque, A::PreMultiplied]),
            Some(A::PreMultiplied)
        );
        // DX12 desde HWND (solo Opaque): sin panico, la ventana queda opaca.
        assert_eq!(select_alpha_mode(0.9, &[A::Opaque]), None);
        // Sin PreMultiplied pero con Auto: se delega la eleccion a wgpu.
        assert_eq!(select_alpha_mode(0.9, &[A::Opaque, A::Auto]), Some(A::Auto));
    }

    #[test]
    fn backdrop_mica_solo_con_opacidad_menor_a_1() {
        assert_eq!(select_windows_backdrop(1.0), WindowsBackdropChoice::None);
        assert_eq!(select_windows_backdrop(0.9), WindowsBackdropChoice::Mica);
        assert_eq!(select_windows_backdrop(0.0), WindowsBackdropChoice::Mica);
        // Mismo umbral `< 1.0` que `with_transparent`: ambos se fijan al crear
        // la ventana y deben moverse juntos.
        assert_eq!(select_windows_backdrop(0.999), WindowsBackdropChoice::Mica);
    }

    #[test]
    fn restart_flag_solo_al_cruzar_el_umbral_de_opacidad() {
        // El backdrop y `with_transparent` se fijan al crear la ventana con el
        // mismo umbral `< 1.0`; cruzarlo exige reinicio y quedarse del mismo
        // lado hot-aplica. Este test fija ese acoplamiento para que un
        // refactor no separe ambas lecturas sin aviso.
        let mut prev = Config::default();
        let mut next = Config::default();
        prev.window.opacity = 0.9;
        next.window.opacity = 1.0;
        let fields = App::restart_required_fields(&prev, &next);
        assert!(
            fields.contains(&"window.opacity"),
            "cruzar el umbral debe pedir reinicio: {fields:?}"
        );

        prev.window.opacity = 0.5;
        next.window.opacity = 0.8;
        let fields = App::restart_required_fields(&prev, &next);
        assert!(
            !fields.contains(&"window.opacity"),
            "mismo lado del umbral hot-aplica, sin reinicio: {fields:?}"
        );
    }

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
            hold: false,
            close_on_exit: false,
            has_activity: false,
            foreground_probe: None,
            foreground_cache: None,
            input_reset_pending: Arc::new(AtomicBool::new(false)),
            echo_pending: Arc::new(AtomicBool::new(false)),
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
            None,
            None,
        )
    }

    #[test]
    fn scroll_sobrevive_a_un_term_envenenado() {
        let term = Arc::new(Mutex::new(Term::new()));

        // Envenena el mutex igual que lo haria un panic bajo el guard.
        let poisoner = Arc::clone(&term);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("term mutex");
            panic!("simula el panic de build_custom_glyphs bajo el guard");
        })
        .join();
        assert!(term.is_poisoned(), "el mutex quedo envenenado");

        let mut app = test_app(Arc::clone(&term));
        // No debe paniquear: un scroll sobre un Term envenenado degrada, no mata.
        app.scroll_lines(-1);

        let guard = app.lock_focused_term();
        assert_eq!(guard.scrollback_offset, 0);
    }

    #[test]
    fn tick_tab_hover_fade_sube_hacia_1_y_baja_hacia_0() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        app.tab_hover = Some(0);
        let mut risen = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.tick_tab_hover_fade();
            if app.tab_hover_alpha > 0.3 {
                risen = true;
                break;
            }
        }
        assert!(risen, "el fade de hover de tab nunca subio");
        assert_eq!(app.tab_hover_display, Some(0));

        app.tab_hover = None;
        let mut settled = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick_tab_hover_fade();
            if app.tab_hover_alpha == 0.0 {
                settled = true;
                break;
            }
        }
        assert!(settled, "el fade de hover de tab nunca se asento");
        assert_eq!(app.tab_hover_display, None);
    }

    #[test]
    fn tick_title_bar_hover_fade_sube_hacia_1_y_baja_hacia_0() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        app.title_bar_hover = Some(TitleButtonKind::Close);
        let mut risen = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.tick_title_bar_hover_fade();
            if app.title_bar_hover_alpha > 0.3 {
                risen = true;
                break;
            }
        }
        assert!(risen, "el fade de hover de boton nunca subio");
        assert_eq!(app.title_bar_hover_display, Some(TitleButtonKind::Close));

        app.title_bar_hover = None;
        let mut settled = false;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick_title_bar_hover_fade();
            if app.title_bar_hover_alpha == 0.0 {
                settled = true;
                break;
            }
        }
        assert!(settled, "el fade de hover de boton nunca se asento");
        assert_eq!(app.title_bar_hover_display, None);
    }

    #[test]
    fn tab_bar_visible_falso_con_una_tab_y_decoraciones_de_sistema() {
        let term = Arc::new(Mutex::new(Term::new()));
        let app = test_app(term);
        assert!(!app.tab_bar_visible());
    }

    #[test]
    fn tab_bar_visible_verdadero_con_varias_tabs() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term.clone());
        let second = test_session(term);
        let second_id = second.id;
        app.sessions.push(SessionHost::test(second));
        app.tabs.push(TabLayout::new(second_id));
        assert!(app.tab_bar_visible());
    }

    #[test]
    fn tick_foreground_process_poll_no_hace_nada_con_la_barra_oculta() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let (changed, wake) = app.tick_foreground_process_poll();
        assert!(!changed);
        assert!(wake.is_none());
    }

    #[test]
    fn tick_foreground_process_poll_respeta_la_cadencia() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term.clone());
        let second = test_session(term);
        let second_id = second.id;
        app.sessions.push(SessionHost::test(second));
        app.tabs.push(TabLayout::new(second_id));

        let (_, first_wake) = app.tick_foreground_process_poll();
        assert!(
            first_wake.is_some(),
            "la barra esta visible, debe programar el proximo sondeo"
        );

        let (changed_again, second_wake) = app.tick_foreground_process_poll();
        assert!(
            !changed_again,
            "no debe volver a sondear antes de PROCESS_POLL_INTERVAL"
        );
        assert_eq!(first_wake, second_wake);
    }

    #[cfg(unix)]
    #[test]
    fn tick_foreground_process_poll_actualiza_el_cache_de_la_sesion() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term.clone());
        let mut second = test_session(term);
        let second_id = second.id;
        let master = crate::pty::spawn("sleep", &["2"]).expect("spawn");
        second.foreground_probe = crate::pty::foreground::make_probe(&master).ok();
        app.sessions.push(SessionHost::test(second));
        app.tabs.push(TabLayout::new(second_id));

        let mut resolved = false;
        for _ in 0..50 {
            let (changed, _) = app.tick_foreground_process_poll();
            if changed {
                resolved = true;
                break;
            }
            app.last_process_poll = None; // fuerza el siguiente sondeo sin esperar 500ms reales
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            resolved,
            "tick_foreground_process_poll nunca reporto el proceso"
        );
        let idx = app.session_by_id(second_id).unwrap();
        assert_eq!(
            app.sessions[idx]
                .session
                .foreground_cache
                .as_ref()
                .map(|(_, n)| n.as_str()),
            Some("sleep")
        );
    }

    #[test]
    fn system_color_scheme_conmuta_tema_en_modo_auto() {
        use crate::config::ColorScheme;
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let toml = r##"[theme]
mode = "auto"
dark = "claude-dark"
light = "catppuccin-latte"
import = false
"##;
        app.config = toml::from_str(toml).unwrap();
        let light_bg = crate::config::try_preset("catppuccin-latte")
            .unwrap()
            .background
            .clone();
        // El portal (o winit) reporta modo claro => el tema re-resuelve a la
        // variante clara sin releer disco.
        app.dispatch_user_event(UserEvent::SystemColorScheme(ColorScheme::Light));
        assert_eq!(app.config.theme.background, light_bg);
        assert_eq!(app.config.theme_preset.as_deref(), Some("catppuccin-latte"));
        assert_eq!(app.system_scheme_source, SchemeSource::Portal);

        // Vuelve a oscuro.
        let dark_bg = crate::config::try_preset("claude-dark")
            .unwrap()
            .background
            .clone();
        app.dispatch_user_event(UserEvent::SystemColorScheme(ColorScheme::Dark));
        assert_eq!(app.config.theme.background, dark_bg);
        assert_eq!(app.config.theme_preset.as_deref(), Some("claude-dark"));
    }

    #[test]
    fn reconcile_theme_respeta_modo_fijo_dark() {
        use crate::config::ColorScheme;
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let toml = r##"[theme]
mode = "dark"
dark = "nord"
light = "catppuccin-latte"
import = false
"##;
        app.config = toml::from_str(toml).unwrap();
        let nord_bg = crate::config::try_preset("nord")
            .unwrap()
            .background
            .clone();
        // mode=dark ignora el esquema del SO.
        app.dispatch_user_event(UserEvent::SystemColorScheme(ColorScheme::Light));
        assert_eq!(app.config.theme.background, nord_bg);
        assert_eq!(app.config.theme_preset.as_deref(), Some("nord"));
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
            None,
            None,
        );
        app.focused = 1;

        app.dispatch_user_event(UserEvent::RedrawNeeded(id_a));
        assert!(app.sessions[0].session.dirty);
        assert!(!app.sessions[1].session.dirty);
    }

    #[test]
    fn ventana_ocluida_salta_el_frame_sin_perder_dirty() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);

        assert!(!app.should_skip_frame(), "visible: se pinta normalmente");

        app.set_occluded(true);
        assert!(
            app.should_skip_frame(),
            "ocluida: no se pide imagen al swapchain"
        );

        // Hay contenido pendiente de pintar mientras esta oculta. El skip del
        // frame no debe tocarlo: al reaparecer hay que pintar lo acumulado, no
        // un frame viejo.
        app.sessions[0].session.dirty = true;
        assert!(
            app.sessions[0].session.dirty,
            "ocluida: el dirty pendiente sobrevive al frame saltado"
        );

        app.set_occluded(false);
        assert!(
            !app.should_skip_frame(),
            "visible de nuevo: se vuelve a pintar"
        );
        assert!(
            app.sessions[0].session.dirty,
            "el dirty acumulado sigue pendiente"
        );
    }

    #[test]
    fn backoff_tras_fallo_de_swapchain_ignora_redraws_de_parpadeo() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);

        assert!(!app.acquire_backoff_active(), "sin fallos no hay backoff");

        app.note_acquire_failure();
        assert!(
            app.should_skip_frame(),
            "justo tras el fallo no se reintenta: el timer de parpadeo pediria \
             un frame cada 500ms y cada intento cuesta 1000ms de event loop"
        );

        // El backoff es temporal, no permanente: expira solo.
        app.expire_acquire_backoff_for_test();
        assert!(
            !app.should_skip_frame(),
            "pasado el backoff se vuelve a intentar"
        );
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
    fn redraw_needed_tras_esu_conserva_dirty() {
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
            app.sessions[0].session.dirty,
            "tras ESU se pide el redraw final, pero el dirty se limpia al presentar"
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
            app.sessions[0].session.dirty,
            "tras timeout el modo sigue activo pero ya no se difiere: dirty \
             sobrevive hasta presentar"
        );
    }

    #[test]
    fn redraw_needed_enfocada_conserva_dirty_hasta_presentar() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;
        app.sessions[0].session.dirty = true;
        app.dispatch_user_event(UserEvent::RedrawNeeded(id));
        assert!(
            app.sessions[0].session.dirty,
            "pedir el redraw no es haberlo pintado: el dirty se limpia en \
             settle_frame_result, cuando el frame ya se presento"
        );
    }

    #[test]
    fn redraw_needed_no_limpia_dirty_antes_de_pintar() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term);
        let id = app.sessions[0].session.id;

        app.dispatch_user_event(UserEvent::RedrawNeeded(id));

        assert!(
            app.sessions[0].session.dirty,
            "pedir el redraw no es haberlo pintado: el dirty se limpia en \
             settle_frame_result, cuando el frame ya se presento"
        );
    }

    #[test]
    fn pane_obsoleto_conserva_dirty_para_repintarse() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(Arc::clone(&term));
        let id = app.sessions[0].session.id;

        // El drain tiene el Term: la GUI no podra leerlo en el frame.
        let _held = term.lock().expect("term mutex");

        app.sessions[0].session.dirty = true;
        app.settle_frame_result(vec![], vec![id]);

        assert!(
            app.sessions[0].session.dirty,
            "el pane no se repinto: su dirty debe sobrevivir al frame"
        );
        assert!(
            app.needs_followup_redraw(),
            "hay que pedir otro redraw o el eco no aparece hasta el evento siguiente"
        );
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
            None,
            None,
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
    fn send_input_ignora_sesiones_en_hold() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term.clone());
        let id = app.sessions[0].session.id;
        term.lock().expect("term mutex").take_dirty();
        app.sessions[0].session.hold = true;

        app.send_input(b" ".to_vec());

        assert!(
            !app.pane_is_dirty(id),
            "send_input no debe marcar dirty en sesion held"
        );
    }

    #[test]
    fn send_input_con_lock_ocupado_difiere_el_reset() {
        let term = Arc::new(Mutex::new(Term::new()));
        let app = test_app(term.clone());
        let id = app.sessions[0].session.id;
        term.lock().expect("term mutex").take_dirty();
        let guard = term.lock().expect("term mutex");

        app.send_input(b"a".to_vec());

        assert!(
            app.sessions[0]
                .session
                .input_reset_pending
                .load(Ordering::Relaxed),
            "con el lock ocupado el reset queda pendiente"
        );
        assert!(
            !guard.dirty,
            "el dirty se difiere mientras el lock esta ocupado"
        );

        drop(guard);
        app.apply_pending_input_reset();

        assert!(
            !app.sessions[0]
                .session
                .input_reset_pending
                .load(Ordering::Relaxed),
            "el flag se consume al aplicar el reset"
        );
        assert!(
            app.pane_is_dirty(id),
            "el reset diferido marca dirty al aplicarse"
        );
    }

    #[test]
    fn pty_exited_con_close_on_exit_cierra_app() {
        let mut app = test_app(Arc::new(Mutex::new(Term::new())));
        let id = app.sessions[0].session.id;
        app.sessions[0].session.close_on_exit = true;

        app.dispatch_user_event(UserEvent::PtyExited(id, 0));

        assert!(app.pending_exit);
    }

    #[test]
    fn pty_exited_con_hold_no_cierra_app() {
        let mut app = test_app(Arc::new(Mutex::new(Term::new())));
        let id = app.sessions[0].session.id;
        app.sessions[0].session.hold = true;
        app.sessions[0].session.close_on_exit = true;

        app.dispatch_user_event(UserEvent::PtyExited(id, 0));

        assert!(!app.pending_exit);
    }

    #[test]
    fn request_selection_redraw_respeta_intervalo_y_fuerza_al_final_del_gesto() {
        let mut app = test_app(Arc::new(Mutex::new(Term::new())));
        app.redraw_interval_nanos
            .store(1_000_000_000, Ordering::Relaxed);

        app.request_selection_redraw(false);
        assert!(
            !app.selection_redraw_pending,
            "el primer request nunca debe diferirse"
        );
        assert!(app.last_selection_redraw.is_some());

        app.request_selection_redraw(false);
        assert!(
            app.selection_redraw_pending,
            "un segundo request dentro del intervalo debe diferirse"
        );

        app.request_selection_redraw(true);
        assert!(
            !app.selection_redraw_pending,
            "force=true debe emitir el redraw pendiente"
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
            None,
            None,
        );
        app.run_action(Action::GotoTab(2));
        assert_eq!(app.focused, 1);
        app.run_action(Action::GotoTab(0));
        assert_eq!(app.focused, 1);
    }

    #[test]
    fn extend_selection_word_ae2_uno_dos_tres() {
        use crate::input::actions::Action;
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"uno dos tres");
        }
        let mut app = test_app(term.clone());

        app.run_action(Action::ExtendSelectionWordLeft);
        app.run_action(Action::ExtendSelectionWordLeft);

        let guard = term.lock().expect("term mutex");
        assert_eq!(guard.selected_text(), "dos tres");
    }

    #[test]
    fn extend_selection_line_start_end_desde_mitad_de_linea() {
        use crate::input::actions::Action;
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"uno dos tres");
            guard.cursor.col = 5; // mitad de "dos"
        }
        let mut app = test_app(term.clone());

        app.run_action(Action::ExtendSelectionLineEnd);
        {
            let guard = term.lock().expect("term mutex");
            assert_eq!(guard.selected_text(), "os tres");
            assert_eq!(guard.selection.as_ref().unwrap().end.col, 11);
        }

        // Reiniciar y probar Home desde la misma posicion.
        {
            let mut guard = term.lock().expect("term mutex");
            guard.selection = None;
            guard.cursor.col = 5;
        }
        app.run_action(Action::ExtendSelectionLineStart);
        let guard = term.lock().expect("term mutex");
        assert_eq!(guard.selected_text(), "uno do");
        assert_eq!(guard.selection.as_ref().unwrap().end.col, 0);
    }

    #[test]
    fn extend_selection_line_end_en_linea_vacia_no_invierte_ni_panica() {
        use crate::input::actions::Action;
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(term.clone());

        app.run_action(Action::ExtendSelectionLineEnd);

        let guard = term.lock().expect("term mutex");
        let sel = guard.selection.as_ref().expect("seleccion creada");
        assert_eq!(
            sel.end.col, 0,
            "fila vacia: no-op, no debe invertir el rango"
        );
    }

    #[test]
    fn extend_selection_viewport_start_end_no_panica() {
        use crate::input::actions::Action;
        let term = Arc::new(Mutex::new(Term::new()));
        {
            let mut guard = term.lock().expect("term mutex");
            feed_term(&mut guard, b"uno dos tres");
        }
        let mut app = test_app(term.clone());

        app.run_action(Action::ExtendSelectionViewportStart);
        app.run_action(Action::ExtendSelectionViewportEnd);

        let guard = term.lock().expect("term mutex");
        assert!(guard.selection.is_some());
    }

    #[test]
    fn extend_selection_word_override_de_config_remapea_chord() {
        use crate::input::actions::Action;
        use crate::input::keymap::{Key, Mods};
        let overrides = vec![(
            "alt+shift+j".to_string(),
            "extend_selection_word_left".to_string(),
        )];
        let kb = Keybindings::from_overrides(&overrides);
        let alt_shift = Mods {
            alt: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Char('j'), alt_shift),
            Some(Action::ExtendSelectionWordLeft)
        );
        // El chord por defecto (Ctrl+Shift+Left) se conserva ademas del override.
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Left, cs),
            Some(Action::ExtendSelectionWordLeft)
        );
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
            crate::config::ColorMode::Dark,
            SchemeSource::Fallback,
            None,
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
            focus: false,
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
                focus: false,
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
    fn test_encode_mouse_report_sgr_press_release() {
        use crate::ansi::MouseReporting;

        let reporting = MouseReporting {
            click: true,
            sgr: true,
            ..Default::default()
        };
        assert_eq!(
            App::encode_mouse_report(&reporting, 0, 9, 4, false),
            Some(b"\x1b[<0;10;5M".to_vec())
        );
        assert_eq!(
            App::encode_mouse_report(&reporting, 0, 9, 4, true),
            Some(b"\x1b[<0;10;5m".to_vec())
        );
    }

    #[test]
    fn test_encode_mouse_report_x10_release_uses_button_three() {
        use crate::ansi::MouseReporting;

        let reporting = MouseReporting {
            click: true,
            ..Default::default()
        };
        let press =
            App::encode_mouse_report(&reporting, 2, 0, 0, false).expect("press codificable");
        let release =
            App::encode_mouse_report(&reporting, 2, 0, 0, true).expect("release codificable");
        // Boton derecho (2) presionado -> 0x22; cualquier liberacion -> 0x23.
        assert_eq!(press[2], 0x22);
        assert_eq!(release[2], 0x23);
    }

    #[test]
    fn test_encode_mouse_report_x10_clamps_at_223() {
        use crate::ansi::MouseReporting;

        let reporting = MouseReporting {
            click: true,
            ..Default::default()
        };
        let bytes = App::encode_mouse_report(&reporting, 0, 222, 222, false)
            .expect("coordenada limite codificable");
        assert_eq!(bytes[3], 255);
        assert_eq!(bytes[4], 255);

        let bytes_clamped = App::encode_mouse_report(&reporting, 0, 500, 500, false)
            .expect("coordenada excedida debe clamp");
        assert_eq!(bytes_clamped[3], 255);
        assert_eq!(bytes_clamped[4], 255);
    }

    #[test]
    fn test_clamp_mouse_to_grid_rejects_sentinel() {
        assert_eq!(
            App::clamp_mouse_to_grid(usize::MAX, 0, 24, 80),
            None,
            "coordenada fuera del pane debe rechazarse, no clamp al borde"
        );
        assert_eq!(App::clamp_mouse_to_grid(0, usize::MAX, 24, 80), None);
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
