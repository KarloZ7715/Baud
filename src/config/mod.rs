//! Sistema de configuración para Baud mediante archivos TOML.
//!
//! Los valores por defecto de apariencia corresponden al preset `claude-dark`.
//! La configuración se carga al inicio y puede recargarse en caliente
//! desde, por orden de prioridad:
//!
//! 1. `$XDG_CONFIG_HOME/baud/config.toml` (o `~/.config/baud/config.toml` en Linux).
//! 2. `./baud.toml` en el directorio de trabajo.
//! 3. Valores por defecto (`Config::default()`).
//!
//! `bold_is_bright` puede declararse en la raíz del TOML o en `[theme]`; si
//! cualquiera de los dos es `true`, el renderer aplica el mapeo bold→bright.
//!
//! `[theme].minimum_contrast` (default `1.0`) ajusta dinámicamente el fg sobre
//! el bg efectivo de cada celda para cumplir contraste WCAG. Use `1.0` para
//! desactivar el ajuste y conservar colores crudos del tema; `3.0` piso de
//! texto grande; `4.5` piso de cuerpo de texto (WCAG AA). Rango útil 1.0–21.0,
//! valores fuera se ajustan al límite con un warning.

pub mod persist;
mod themes;
pub mod watch;

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

pub use crate::color::contrast_ratio_hex as contrast_ratio;
pub use themes::MIN_COMMENT_CONTRAST;
pub use themes::{available_presets, preset, try_preset, PresetError, MIN_LEGIBLE_CONTRAST};

// ---------------------------------------------------------------------------
// Estructuras principales
// ---------------------------------------------------------------------------

/// Configuración global del emulador.
#[derive(Debug, Clone, Deserialize)]
#[serde(from = "RawConfig")]
pub struct Config {
    #[serde(default)]
    pub theme: ThemeConfig,
    /// Preset embebido activo si la config lo declara por nombre.
    #[serde(skip)]
    pub theme_preset: Option<String>,
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub selection: SelectionConfig,
    #[serde(default)]
    pub copy_mode: CopyModeConfig,
    #[serde(default)]
    pub scrollback: ScrollbackConfig,
    #[serde(default)]
    pub cursor: CursorConfig,
    #[serde(default)]
    pub bold_is_bright: bool,
    #[serde(default = "default_true")]
    pub allow_osc52_read: bool,
    #[serde(default)]
    pub process: ProcessSection,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub panes: PanesConfig,
    #[serde(default)]
    pub status: StatusConfig,
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
    #[serde(default)]
    pub debug: DebugConfig,
    #[serde(default)]
    pub render: RenderConfig,
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

/// Opciones de depuración (off por defecto).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DebugConfig {
    /// Permite activar el contador de FPS con el atajo de teclado.
    #[serde(default)]
    pub fps_counter_enabled: bool,
}

/// Diagnósticos locales: watchdog y logging.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiagnosticsConfig {
    /// Activa el hilo watchdog del event loop. Requiere reinicio.
    #[serde(default)]
    pub watchdog: bool,
    /// Nivel de log por defecto del target `baud`. Solo aplica si no hay
    /// `RUST_LOG` en el entorno.
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub reporting: ReportingConfig,
}

/// Configuración de reporte remoto de errores (Sentry).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReportingConfig {
    /// `None` = no se ha decidido aún; `Some(true)` = aceptado; `Some(false)` = rechazado.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// DSN opcional. Si no se define, se usa `BAUD_SENTRY_DSN` de build.
    #[serde(default)]
    pub dsn: Option<String>,
}

/// Configuración de render de la GUI.
#[derive(Debug, Clone, Deserialize)]
pub struct RenderConfig {
    /// FPS máximo de redraw. `0` desactiva el límite.
    #[serde(default = "default_render_max_fps")]
    pub max_fps: u32,
}

fn default_render_max_fps() -> u32 {
    60
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            max_fps: default_render_max_fps(),
        }
    }
}

impl RenderConfig {
    /// Intervalo mínimo entre redraws según `max_fps`.
    pub fn redraw_interval(&self) -> Option<Duration> {
        if self.max_fps == 0 {
            return None;
        }
        Some(Duration::from_secs_f64(1.0 / self.max_fps as f64))
    }

    /// Intervalo mínimo en nanosegundos; `0` significa sin límite.
    pub fn redraw_interval_nanos(&self) -> u64 {
        self.redraw_interval()
            .map(|d| d.as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    }
}

/// Notificaciones de escritorio via OSC 9 / OSC 777. Off por defecto.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default)]
    pub enabled: bool,
}

/// Apariencia del overlay de status (duración y colores opcionales).
#[derive(Debug, Clone, Deserialize)]
pub struct StatusConfig {
    /// Duración del overlay en ms (`0` = sin auto-dismiss).
    #[serde(default = "default_status_duration")]
    pub duration_ms: u64,
    /// Color de fondo opcional. Si es `None`, usa `theme.black` semitransparente.
    #[serde(default)]
    pub bg_color: Option<String>,
    /// Color de foreground opcional. Si es `None`, usa `theme.foreground` ajustado.
    #[serde(default)]
    pub fg_color: Option<String>,
}

fn default_status_duration() -> u64 {
    2000
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            duration_ms: default_status_duration(),
            bg_color: None,
            fg_color: None,
        }
    }
}

/// Configuración de selección de texto.
#[derive(Debug, Clone, Deserialize)]
pub struct SelectionConfig {
    /// Copiar al soltar el boton izquierdo. Off por defecto.
    #[serde(default)]
    pub copy_on_select: bool,
    /// Destino al copiar por selección: "primary" | "clipboard" | "both".
    #[serde(default = "default_copy_on_select_target")]
    pub copy_on_select_target: String,
    /// Retardo tras soltar el botón izquierdo antes de copiar (ms).
    #[serde(default = "default_copy_on_select_delay_ms")]
    pub copy_on_select_delay_ms: u64,
    /// Modificadores que suprimen el mouse reporting de la app.
    /// `bypass_mouse_reporting_modifiers`). Valores: "shift", "alt", "ctrl".
    #[serde(default = "default_bypass_modifiers")]
    pub bypass_mouse_reporting_modifiers: Vec<String>,
    /// Doble clic semántico (URLs, paths, emails). On por defecto.
    #[serde(default = "default_true")]
    pub smart_selection: bool,
    /// Delimitadores de palabra para doble clic no-semantico.
    #[serde(default = "default_word_delimiters")]
    pub word_delimiters: String,
}

/// Configuración del copy mode.
#[derive(Debug, Clone, Deserialize)]
pub struct CopyModeConfig {
    /// Habilitar copy mode. On por defecto.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Colores del tema de terminal (ANSI de 16 colores + extras).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ThemeConfig {
    #[serde(default = "default_foreground")]
    pub foreground: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_cursor")]
    pub cursor: String,
    #[serde(default = "default_selection_bg_option")]
    pub selection_bg: Option<String>,
    #[serde(default = "default_selection_fg_option")]
    pub selection_fg: Option<String>,
    #[serde(default = "default_black")]
    pub black: String,
    #[serde(default = "default_red")]
    pub red: String,
    #[serde(default = "default_green")]
    pub green: String,
    #[serde(default = "default_yellow")]
    pub yellow: String,
    #[serde(default = "default_blue")]
    pub blue: String,
    #[serde(default = "default_magenta")]
    pub magenta: String,
    #[serde(default = "default_cyan")]
    pub cyan: String,
    #[serde(default = "default_white")]
    pub white: String,
    #[serde(default = "default_bright_black")]
    pub bright_black: String,
    #[serde(default = "default_bright_red")]
    pub bright_red: String,
    #[serde(default = "default_bright_green")]
    pub bright_green: String,
    #[serde(default = "default_bright_yellow")]
    pub bright_yellow: String,
    #[serde(default = "default_bright_blue")]
    pub bright_blue: String,
    #[serde(default = "default_bright_magenta")]
    pub bright_magenta: String,
    #[serde(default = "default_bright_cyan")]
    pub bright_cyan: String,
    #[serde(default = "default_bright_white")]
    pub bright_white: String,
    /// Bold ANSI 0-7 se mapea a bright 8-15.
    #[serde(default)]
    pub bold_is_bright: bool,
    /// SGR dim atenua alpha del glifo en vez de oscurecer RGB.
    #[serde(default)]
    pub dim_alpha: bool,
    /// Contraste mínimo WCAG fg/bg por celda (1.0 = desactivado, default 1.0).
    /// Precedencia: override de usuario > valor del tema > default.
    #[serde(default = "default_minimum_contrast")]
    pub minimum_contrast: f64,
}

/// Tabla `[theme]` con campos opcionales para distinguir ausencia de override.
macro_rules! define_theme_table {
    ($( $field:ident ),+ $(,)?) => {
        #[derive(Debug, Clone, Default, Deserialize)]
        struct ThemeTable {
            name: Option<String>,
            selection_bg: Option<String>,
            selection_fg: Option<String>,
            $( $field: Option<String>, )+
            bold_is_bright: Option<bool>,
            dim_alpha: Option<bool>,
            minimum_contrast: Option<f64>,
        }

        fn apply_theme_overrides(base: &mut ThemeConfig, table: &ThemeTable) {
            $( if let Some(v) = &table.$field {
                base.$field.clone_from(v);
            } )+
            if let Some(v) = &table.selection_bg {
                base.selection_bg = Some(v.clone());
            }
            if let Some(v) = &table.selection_fg {
                base.selection_fg = Some(v.clone());
            }
            if let Some(v) = table.bold_is_bright {
                base.bold_is_bright = v;
            }
            if let Some(v) = table.dim_alpha {
                base.dim_alpha = v;
            }
            if let Some(v) = table.minimum_contrast {
                base.minimum_contrast = clamp_minimum_contrast(v);
            }
        }
    };
}

define_theme_table!(
    foreground,
    background,
    cursor,
    black,
    red,
    green,
    yellow,
    blue,
    magenta,
    cyan,
    white,
    bright_black,
    bright_red,
    bright_green,
    bright_yellow,
    bright_blue,
    bright_magenta,
    bright_cyan,
    bright_white,
);

/// Representación cruda de `theme` en TOML: nombre o tabla inline.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawTheme {
    Named(String),
    Table(Box<ThemeTable>),
}

impl Default for RawTheme {
    fn default() -> Self {
        Self::Table(Box::default())
    }
}

/// Configuración sin resolver: `theme` queda en forma cruda hasta la conversión.
#[derive(Debug, Clone, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    theme: RawTheme,
    #[serde(default)]
    font: FontConfig,
    #[serde(default)]
    window: WindowConfig,
    #[serde(default)]
    selection: SelectionConfig,
    #[serde(default)]
    copy_mode: CopyModeConfig,
    #[serde(default)]
    scrollback: ScrollbackConfig,
    #[serde(default)]
    cursor: CursorConfig,
    #[serde(default)]
    bold_is_bright: bool,
    #[serde(default = "default_true")]
    allow_osc52_read: bool,
    #[serde(default)]
    process: ProcessSection,
    #[serde(default)]
    notifications: NotificationsConfig,
    #[serde(default)]
    panes: PanesConfig,
    #[serde(default)]
    status: StatusConfig,
    #[serde(default)]
    diagnostics: DiagnosticsConfig,
    #[serde(default)]
    debug: DebugConfig,
    #[serde(default)]
    render: RenderConfig,
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

fn theme_base_from_name(name: &str) -> ThemeConfig {
    match try_preset(name) {
        Ok(t) => t,
        Err(PresetError::NotFound) => {
            tracing::warn!("preset de tema desconocido: '{name}'");
            ThemeConfig::default()
        }
        Err(PresetError::InvalidToml(e)) => {
            tracing::warn!("preset '{name}' inválido: {e}");
            ThemeConfig::default()
        }
    }
}

fn resolve_theme(raw: RawTheme) -> (ThemeConfig, Option<String>) {
    match raw {
        RawTheme::Named(name) => (theme_base_from_name(&name), Some(name)),
        RawTheme::Table(table) => {
            let preset_name = table.name.clone();
            let mut base = table
                .name
                .as_deref()
                .map(theme_base_from_name)
                .unwrap_or_default();
            apply_theme_overrides(&mut base, table.as_ref());
            (base, preset_name)
        }
    }
}

impl From<RawConfig> for Config {
    fn from(raw: RawConfig) -> Self {
        let (theme, theme_preset) = resolve_theme(raw.theme);
        Self {
            theme,
            theme_preset,
            font: raw.font,
            window: raw.window,
            selection: raw.selection,
            copy_mode: raw.copy_mode,
            scrollback: raw.scrollback,
            cursor: raw.cursor,
            bold_is_bright: raw.bold_is_bright,
            allow_osc52_read: raw.allow_osc52_read,
            process: raw.process,
            notifications: raw.notifications,
            panes: raw.panes,
            status: raw.status,
            debug: raw.debug,
            diagnostics: raw.diagnostics,
            render: raw.render,
            keys: raw.keys,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        RawConfig::default().into()
    }
}

/// Configuración de la fuente (tipografía y tamaño).
#[derive(Debug, Clone, Deserialize)]
pub struct FontConfig {
    #[serde(default = "default_font_family")]
    pub family: String,
    #[serde(default = "default_font_size")]
    pub size: u16,
    #[serde(default = "default_glyph_offset")]
    pub glyph_offset: GlyphOffset,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// Dibujar U+2500..U+259F y separadores Powerline U+E0B0..U+E0B3
    /// de forma programatica. Si false, usa la fuente.
    #[serde(default = "default_true")]
    pub builtin_box_drawing: bool,
    /// Shaping multi-caracter con ligaduras tipograficas (off por defecto).
    #[serde(default)]
    pub ligatures: bool,
    /// Familias de fallback en orden de preferencia (emoji, CJK, símbolos).
    #[serde(default)]
    pub fallback: Vec<String>,
}

/// Desplazamiento fino del glifo dentro de la celda.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct GlyphOffset {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

/// Estado inicial de la ventana al arrancar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupState {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// Configuración de la ventana (opacidad, padding, decoraciones, tamaño).
#[derive(Debug, Clone, Deserialize)]
pub struct WindowConfig {
    /// 0..=1. Valores menores a 1 dejan ver el escritorio a través del fondo por defecto.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub padding_x: u16,
    #[serde(default)]
    pub padding_y: u16,
    #[serde(default = "default_true")]
    pub decorations: bool,
    #[serde(default)]
    pub startup: StartupState,
    /// Ancho inicial en píxeles lógicos. Solo aplica con `startup = "windowed"`.
    #[serde(default = "default_win_width")]
    pub width: u32,
    /// Alto inicial en píxeles lógicos. Solo aplica con `startup = "windowed"`.
    #[serde(default = "default_win_height")]
    pub height: u32,
}

fn default_win_width() -> u32 {
    800
}

fn default_win_height() -> u32 {
    600
}

/// Layout de panes (dwindle Hyprland).
#[derive(Debug, Clone, Deserialize)]
pub struct PanesConfig {
    /// Máximo de panes por tab. 0 = sin límite.
    #[serde(default = "default_max_panes")]
    pub max: usize,
    /// Hyprland dwindle:split_width_multiplier
    #[serde(default = "default_split_width_multiplier")]
    pub split_width_multiplier: f32,
    /// Split según posición del cursor (triángulos Hyprland).
    #[serde(default)]
    pub smart_split: bool,
    /// No recalcular orient al resize. Se activa también con smart_split.
    #[serde(default)]
    pub preserve_split: bool,
}

fn default_max_panes() -> usize {
    12
}

fn default_split_width_multiplier() -> f32 {
    1.0
}

impl Default for PanesConfig {
    fn default() -> Self {
        Self {
            max: default_max_panes(),
            split_width_multiplier: default_split_width_multiplier(),
            smart_split: false,
            preserve_split: false,
        }
    }
}

/// Límite de líneas en scrollback (configurable; ver `unlimited`).
#[derive(Debug, Clone, Deserialize)]
pub struct ScrollbackConfig {
    /// Máximo de líneas guardadas. Con `0` no se almacena scrollback.
    #[serde(default = "default_scrollback_lines")]
    pub lines: usize,
    #[serde(default)]
    pub unlimited: bool,
    /// Multiplicador de velocidad para scroll local con rueda (líneas por tick).
    /// Por defecto 3.0.
    #[serde(default = "default_multiplier")]
    pub multiplier: f32,
    /// Multiplicador para scroll sintético en pantalla alterna (flechas por tick).
    /// Por defecto 3.0.
    #[serde(default = "default_faux_multiplier")]
    pub faux_multiplier: f32,
}

fn default_scrollback_lines() -> usize {
    10_000
}

fn default_multiplier() -> f32 {
    3.0
}

fn default_faux_multiplier() -> f32 {
    3.0
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self {
            lines: default_scrollback_lines(),
            unlimited: false,
            multiplier: default_multiplier(),
            faux_multiplier: default_faux_multiplier(),
        }
    }
}

/// Apariencia del cursor (color, forma, parpadeo).
#[derive(Debug, Clone, Deserialize)]
pub struct CursorConfig {
    /// `None` usa `theme.cursor`.
    #[serde(default)]
    pub color: Option<String>,
    /// `"block"` | `"bar"` | `"underline"`.
    #[serde(default = "default_cursor_style")]
    pub style: String,
    /// Si `true` el cursor parpadea (independiente del SGR 5 del texto).
    #[serde(default = "default_true")]
    pub blink: bool,
    /// Intervalo del parpadeo de cursor y texto SGR 5 en milisegundos. `0`
    /// desactiva ambos: el cursor queda fijo y SGR 5 no se oculta. Lo cablea
    /// `Config::apply_to_term` a `Term::blink_interval_ms`.
    #[serde(default = "default_blink_ms")]
    pub blink_interval_ms: u64,
}

fn default_cursor_style() -> String {
    "block".into()
}

fn default_blink_ms() -> u64 {
    530
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            color: None,
            style: default_cursor_style(),
            blink: true,
            blink_interval_ms: default_blink_ms(),
        }
    }
}

/// Proceso hijo del PTY (`[process]` en TOML).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProcessSection {
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub startup_command: Option<String>,
    #[serde(default)]
    pub login: bool,
    #[serde(default)]
    pub kind: crate::pty::SessionKind,
    pub distro: Option<String>,
    pub wsl_cwd: Option<String>,
}

// ---------------------------------------------------------------------------
// Implementaciones de Default para structs con valores no estándar
// ---------------------------------------------------------------------------

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            foreground: default_foreground(),
            background: default_background(),
            cursor: default_cursor(),
            selection_bg: Some(default_selection_bg()),
            selection_fg: Some(default_selection_fg()),
            black: default_black(),
            red: default_red(),
            green: default_green(),
            yellow: default_yellow(),
            blue: default_blue(),
            magenta: default_magenta(),
            cyan: default_cyan(),
            white: default_white(),
            bright_black: default_bright_black(),
            bright_red: default_bright_red(),
            bright_green: default_bright_green(),
            bright_yellow: default_bright_yellow(),
            bright_blue: default_bright_blue(),
            bright_magenta: default_bright_magenta(),
            bright_cyan: default_bright_cyan(),
            bright_white: default_bright_white(),
            bold_is_bright: false,
            dim_alpha: false,
            minimum_contrast: default_minimum_contrast(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
            glyph_offset: default_glyph_offset(),
            line_height: default_line_height(),
            builtin_box_drawing: true,
            ligatures: false,
            fallback: Vec::new(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: default_opacity(),
            padding_x: 0,
            padding_y: 0,
            decorations: true,
            startup: StartupState::Windowed,
            width: default_win_width(),
            height: default_win_height(),
        }
    }
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            copy_on_select: false,
            copy_on_select_target: default_copy_on_select_target(),
            copy_on_select_delay_ms: default_copy_on_select_delay_ms(),
            bypass_mouse_reporting_modifiers: default_bypass_modifiers(),
            smart_selection: default_true(),
            word_delimiters: default_word_delimiters(),
        }
    }
}

impl Default for CopyModeConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
        }
    }
}

impl SelectionConfig {
    /// Retardo configurado antes de ejecutar copy-on-select.
    pub fn copy_on_select_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.copy_on_select_delay_ms)
    }

    /// True si `modifier` ("shift"|"alt"|"ctrl") está en la lista de bypass.
    pub fn bypass_contains(&self, modifier: &str) -> bool {
        self.bypass_mouse_reporting_modifiers
            .iter()
            .any(|m| m.eq_ignore_ascii_case(modifier))
    }
}

// ---------------------------------------------------------------------------
// Funciones default — tema oscuro
// ---------------------------------------------------------------------------

fn default_foreground() -> String {
    "#ececec".into()
}
fn default_minimum_contrast() -> f64 {
    1.0
}

/// Rango útil de contraste WCAG: 1.0 (sin ajuste) ..= 21.0 (negro/blanco puros).
pub const MIN_CONTRAST_FLOOR: f64 = 1.0;
pub const MIN_CONTRAST_CEIL: f64 = 21.0;

/// Acota `minimum_contrast` al rango útil avisando cuando se excede.
fn clamp_minimum_contrast(v: f64) -> f64 {
    if v.is_finite() && (MIN_CONTRAST_FLOOR..=MIN_CONTRAST_CEIL).contains(&v) {
        v
    } else {
        let clamped = if v.is_nan() || v < MIN_CONTRAST_FLOOR {
            MIN_CONTRAST_FLOOR
        } else {
            MIN_CONTRAST_CEIL
        };
        tracing::warn!(
            "minimum_contrast {v} fuera de rango [{MIN_CONTRAST_FLOOR}, {MIN_CONTRAST_CEIL}], ajustado a {clamped}"
        );
        clamped
    }
}
fn default_background() -> String {
    "#0a0a0a".into()
}
fn default_cursor() -> String {
    "#d97757".into()
}
fn default_black() -> String {
    "#3d3d3d".into()
}
fn default_red() -> String {
    "#e85d5d".into()
}
fn default_green() -> String {
    "#6bbf8a".into()
}
fn default_yellow() -> String {
    "#d4a574".into()
}
fn default_blue() -> String {
    "#6b9fd4".into()
}
fn default_magenta() -> String {
    "#c47ad4".into()
}
fn default_cyan() -> String {
    "#5eb8b8".into()
}
fn default_white() -> String {
    "#ececec".into()
}
fn default_bright_black() -> String {
    "#797979".into()
}
fn default_bright_red() -> String {
    "#f07070".into()
}
fn default_bright_green() -> String {
    "#7ed49a".into()
}
fn default_bright_yellow() -> String {
    "#e8b888".into()
}
fn default_bright_blue() -> String {
    "#82b4e8".into()
}
fn default_bright_magenta() -> String {
    "#d494e8".into()
}
fn default_bright_cyan() -> String {
    "#72d0d0".into()
}
fn default_bright_white() -> String {
    "#ffffff".into()
}

fn default_selection_bg() -> String {
    "#c4704a".into()
}

fn default_selection_fg() -> String {
    "#0a0a0a".into()
}

fn default_selection_bg_option() -> Option<String> {
    Some(default_selection_bg())
}

fn default_selection_fg_option() -> Option<String> {
    Some(default_selection_fg())
}

fn default_font_family() -> String {
    // NOTA: Usar "monospace" delega en fontdb la resolución a Family::Monospace,
    // que por defecto busca "Courier New" (no disponible en Linux). En su lugar,
    // se usa una fuente concreta con soporte garantizado de box-drawing Unicode
    // y glifos TUI. El usuario puede sobrescribir esto en ~/.config/baud/config.toml.
    "FiraCode Nerd Font Mono".into()
}
fn default_font_size() -> u16 {
    12
}
fn default_glyph_offset() -> GlyphOffset {
    GlyphOffset { x: 0.0, y: 0.0 }
}
fn default_line_height() -> f32 {
    1.0
}
fn default_opacity() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_copy_on_select_target() -> String {
    "both".into()
}

fn default_copy_on_select_delay_ms() -> u64 {
    500
}
fn default_bypass_modifiers() -> Vec<String> {
    vec!["shift".into()]
}
fn default_word_delimiters() -> String {
    ",│`|:\"' ()[]{}<>\t".into()
}

// ---------------------------------------------------------------------------
// parse_hex — conversor de color hexadecimal
// ---------------------------------------------------------------------------

/// Convierte un string hexadecimal `"#RRGGBB"` a una tupla `(R, G, B)`.
///
/// Si el string no tiene el formato esperado (7 caracteres, iniciando con `#`),
/// devuelve `(0, 0, 0)` y emite una advertencia via `tracing::warn!`.
///
/// # Ejemplos
///
/// ```
/// # use baud::config::parse_hex;
/// assert_eq!(parse_hex("#ff0000"), (255, 0, 0));
/// assert_eq!(parse_hex("#00ff00"), (0, 255, 0));
/// assert_eq!(parse_hex("#0000ff"), (0, 0, 255));
/// ```
///
/// # Nota
///
/// pony tail: sin crate externo de color, tres líneas con la stdlib bastan.
pub fn parse_hex(s: &str) -> (u8, u8, u8) {
    if s.len() == 7 && s.starts_with('#') {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[1..3], 16),
            u8::from_str_radix(&s[3..5], 16),
            u8::from_str_radix(&s[5..7], 16),
        ) {
            return (r, g, b);
        }
    }
    tracing::warn!("parse_hex: formato inválido '{}', usando negro", s);
    (0, 0, 0)
}

// ---------------------------------------------------------------------------
// Carga de config con metadata de origen
// ---------------------------------------------------------------------------

/// Origen de la configuración cargada desde disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Archivo encontrado y parseado correctamente.
    Ok,
    /// Ningún archivo de config encontrado en los paths, usando defaults.
    NotFound,
    /// Archivo encontrado pero el TOML tiene errores de sintaxis o no se pudo leer.
    ParseError { path: String, message: String },
}

/// Resultado de carga: config resuelta más metadata de origen.
#[derive(Debug, Clone)]
pub struct LoadResult {
    pub config: Config,
    pub source: ConfigSource,
}

impl Config {
    /// Límite efectivo de scrollback en líneas (`usize::MAX` si `unlimited`).
    pub fn scrollback_max_lines(&self) -> usize {
        if self.scrollback.unlimited {
            usize::MAX
        } else {
            self.scrollback.lines
        }
    }

    /// Límite de panes por tab (`None` si `panes.max == 0`).
    pub fn panes_max(&self) -> Option<usize> {
        if self.panes.max == 0 {
            None
        } else {
            Some(self.panes.max)
        }
    }

    /// `preserve_split` efectivo (smart_split lo activa como en Hyprland).
    pub fn effective_preserve_split(&self) -> bool {
        self.panes.preserve_split || self.panes.smart_split
    }

    /// Construye la configuración del proceso hijo del PTY.
    ///
    /// Convierte [`Config`] en [`crate::pty::ProcessConfig`] (shell, args,
    /// directorio de arranque, variables de entorno y comando inicial).
    pub fn process_config(&self) -> crate::pty::ProcessConfig {
        let shell = self
            .process
            .program
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| {
                #[cfg(windows)]
                {
                    std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
                }
                #[cfg(not(windows))]
                {
                    "bash".into()
                }
            });
        let env = self
            .process
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        crate::pty::ProcessConfig {
            shell,
            args: self.process.args.clone(),
            working_directory: self.process.working_directory.clone(),
            env,
            startup_command: self.process.startup_command.clone(),
            login_shell: self.process.login,
            kind: self.process.kind,
            distro: self.process.distro.clone(),
            wsl_cwd: self.process.wsl_cwd.clone(),
        }
    }

    /// Atajos de teclado: defaults del emulador + overrides de `[keys]`.
    pub fn keybindings(&self) -> crate::input::actions::Keybindings {
        let overrides: Vec<(String, String)> = self
            .keys
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        crate::input::actions::Keybindings::from_overrides(&overrides)
    }

    /// Nombre del preset embebido si la config lo declara (`theme = "…"` o `[theme].name`).
    pub fn active_preset_name(&self) -> Option<&str> {
        self.theme_preset.as_deref()
    }

    /// Aplica al `Term` los campos de config que tienen efecto al arrancar.
    pub fn apply_to_term(&self, term: &mut crate::ansi::Term) {
        use crate::ansi::CursorStyle;

        term.allow_osc52_read = self.allow_osc52_read;
        term.notifications_enabled = self.notifications.enabled;
        term.cursor_style = match self.cursor.style.as_str() {
            "bar" => CursorStyle::Bar,
            "underline" => CursorStyle::Underline,
            "block" => CursorStyle::Block,
            other => {
                tracing::warn!("cursor.style desconocido '{other}', usando block");
                CursorStyle::Block
            }
        };
        if let Some(ref color) = self.cursor.color {
            let (r, g, b) = parse_hex(color);
            term.cursor_color_override = Some((r, g, b));
        } else {
            term.cursor_color_override = None;
        }
        term.cursor_blink_enabled = self.cursor.blink;
        term.blink_interval_ms = self.cursor.blink_interval_ms;
    }

    /// Carga la configuración desde disco o devuelve los valores por defecto.
    ///
    /// El orden de búsqueda es:
    /// 1. `$XDG_CONFIG_HOME/baud/config.toml` (resuelto con [`dirs::config_dir`]).
    /// 2. `./baud.toml` en el directorio de trabajo actual.
    /// 3. [`Config::default()`] si ninguno de los anteriores existe o es válido.
    ///
    /// Si el archivo existe pero no puede parsearse, se emite una advertencia
    /// con `tracing::warn!` y se retorna la configuración por defecto.
    pub fn load() -> LoadResult {
        let paths = [
            dirs::config_dir()
                .map(|d| d.join("baud").join("config.toml"))
                .unwrap_or_default(),
            std::path::PathBuf::from("baud.toml"),
        ];
        Self::load_from_paths(&paths)
    }

    /// Carga config desde una lista de paths en orden de prioridad.
    pub fn load_from_paths(paths: &[std::path::PathBuf]) -> LoadResult {
        for path in paths {
            if path.as_os_str().is_empty() {
                continue;
            }
            if !path.exists() {
                continue;
            }
            let path_display = path.display().to_string();
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str::<Config>(&content) {
                    Ok(config) => {
                        return LoadResult {
                            config,
                            source: ConfigSource::Ok,
                        };
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Config: error al parsear '{path_display}': {e}. Usando defaults."
                        );
                        return LoadResult {
                            config: Self::default(),
                            source: ConfigSource::ParseError {
                                path: path_display,
                                message: e.to_string(),
                            },
                        };
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Config: no se pudo leer '{path_display}': {e}. Usando defaults."
                    );
                    return LoadResult {
                        config: Self::default(),
                        source: ConfigSource::ParseError {
                            path: path_display,
                            message: e.to_string(),
                        },
                    };
                }
            }
        }

        LoadResult {
            config: Self::default(),
            source: ConfigSource::NotFound,
        }
    }

    /// Carga config desde disco para hot-reload.
    ///
    /// Si el archivo existe pero falla lectura o parseo, devuelve error y la
    /// config en memoria debe conservarse. Si no hay archivo, devuelve defaults.
    pub fn try_load_from_disk() -> Result<Self, String> {
        let paths = [
            dirs::config_dir()
                .map(|d| d.join("baud").join("config.toml"))
                .unwrap_or_default(),
            std::path::PathBuf::from("baud.toml"),
        ];

        for path in &paths {
            if path.exists() {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| format!("no se pudo leer '{}': {e}", path.display()))?;
                return toml::from_str::<Config>(&content)
                    .map_err(|e| format!("error al parsear '{}': {e}", path.display()));
            }
        }

        Ok(Self::default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_process_config_usa_defaults() {
        let config = Config::default();
        let process = config.process_config();
        assert!(process.args.is_empty());
        assert!(process.working_directory.is_none());
        assert!(process.env.is_empty());
        assert!(process.startup_command.is_none());
        assert!(!process.login_shell);
        assert!(!process.shell.is_empty());
    }

    /// Verifica que `Config::default()` use el tema oscuro.
    #[test]
    fn test_config_default_values() {
        let config = Config::default();

        assert_eq!(config.theme.foreground, "#ececec");
        assert_eq!(config.theme.background, "#0a0a0a");
        assert_eq!(config.theme.cursor, "#d97757");
        assert_eq!(config.theme.selection_bg, Some("#c4704a".into()));
        assert_eq!(config.theme.selection_fg, Some("#0a0a0a".into()));
        assert_eq!(config.theme.black, "#3d3d3d");
        assert_eq!(config.theme.red, "#e85d5d");
        assert_eq!(config.theme.green, "#6bbf8a");
        assert_eq!(config.theme.yellow, "#d4a574");
        assert_eq!(config.theme.blue, "#6b9fd4");
        assert_eq!(config.theme.magenta, "#c47ad4");
        assert_eq!(config.theme.cyan, "#5eb8b8");
        assert_eq!(config.theme.white, "#ececec");

        assert_eq!(config.theme.bright_white, "#ffffff");

        // Fuente
        assert_eq!(config.font.family, "FiraCode Nerd Font Mono");
        assert_eq!(config.font.size, 12);

        // Ventana
        assert_eq!(config.window.opacity, 1.0);

        // Selección por defecto: copy_on_select off, smart on, bypass=[shift]
        assert!(!config.selection.copy_on_select);
        assert_eq!(config.selection.copy_on_select_delay_ms, 500);
        assert!(config.selection.smart_selection);
        assert!(config.selection.bypass_contains("shift"));
        assert!(!config.selection.bypass_contains("alt"));
        // Copy mode habilitado por defecto
        assert!(config.copy_mode.enabled);
    }

    #[test]
    fn minimum_contrast_se_acota_al_rango_util() {
        assert_eq!(clamp_minimum_contrast(0.5), MIN_CONTRAST_FLOOR);
        assert_eq!(clamp_minimum_contrast(25.0), MIN_CONTRAST_CEIL);
        assert_eq!(clamp_minimum_contrast(f64::NAN), MIN_CONTRAST_FLOOR);
        assert!((clamp_minimum_contrast(3.0) - 3.0).abs() < f64::EPSILON);
    }

    /// Precedencia: override de usuario > valor del preset > default.
    #[test]
    fn minimum_contrast_precedencia_usuario_tema_default() {
        let default_cfg: Config = toml::from_str("").unwrap();
        assert!((default_cfg.theme.minimum_contrast - 1.0).abs() < f64::EPSILON);

        let named: Config = toml::from_str(r#"theme = "solarized-dark""#).unwrap();
        assert!((named.theme.minimum_contrast - 3.0).abs() < f64::EPSILON);

        let user_overrides_named: Config = toml::from_str(
            r#"
            [theme]
            name = "solarized-dark"
            minimum_contrast = 4.5
            "#,
        )
        .unwrap();
        assert!((user_overrides_named.theme.minimum_contrast - 4.5).abs() < f64::EPSILON);

        let user_overrides_default: Config = toml::from_str(
            r#"
            [theme]
            minimum_contrast = 2.0
            "#,
        )
        .unwrap();
        assert!((user_overrides_default.theme.minimum_contrast - 2.0).abs() < f64::EPSILON);
    }

    /// Verifica que un TOML con todos los campos se parsea correctamente.
    #[test]
    fn test_config_load_from_toml() {
        let toml_str = r##"
[theme]
foreground = "#ffffff"
background = "#000000"
cursor = "#00ff00"
selection_bg = "#333333"
black = "#111111"
red = "#ff0000"
green = "#00ff00"
yellow = "#ffff00"
blue = "#0000ff"
magenta = "#ff00ff"
cyan = "#00ffff"
white = "#eeeeee"
bright_black = "#222222"
bright_red = "#ff4444"
bright_green = "#44ff44"
bright_yellow = "#ffff44"
bright_blue = "#4444ff"
bright_magenta = "#ff44ff"
bright_cyan = "#44ffff"
bright_white = "#ffffff"

[font]
family = "Fira Code"
size = 16

[window]
opacity = 0.85
"##;
        let config: Config = toml::from_str(toml_str).expect("TOML válido");
        assert_eq!(config.theme.foreground, "#ffffff");
        assert_eq!(config.theme.background, "#000000");
        assert_eq!(config.theme.cursor, "#00ff00");
        assert_eq!(config.theme.selection_bg, Some("#333333".into()));
        assert_eq!(config.theme.black, "#111111");
        assert_eq!(config.theme.red, "#ff0000");
        assert_eq!(config.theme.bright_white, "#ffffff");
        assert_eq!(config.font.family, "Fira Code");
        assert_eq!(config.font.size, 16);
        assert!((config.window.opacity - 0.85).abs() < f32::EPSILON);
    }

    /// Verifica que la sección [selection] se parsea y respeta overrides.
    #[test]
    fn test_config_selection_section() {
        let toml_str = r##"
[selection]
copy_on_select = true
copy_on_select_target = "both"
copy_on_select_delay_ms = 300
bypass_mouse_reporting_modifiers = ["shift", "alt"]
smart_selection = false
word_delimiters = " ,.;"
"##;
        let config: Config = toml::from_str(toml_str).expect("TOML selección");
        assert!(config.selection.copy_on_select);
        assert_eq!(config.selection.copy_on_select_target, "both");
        assert_eq!(config.selection.copy_on_select_delay_ms, 300);
        assert!(config.selection.bypass_contains("shift"));
        assert!(config.selection.bypass_contains("alt"));
        assert!(!config.selection.smart_selection);
        assert_eq!(config.selection.word_delimiters, " ,.;");
    }

    /// Verifica que `parse_hex` convierte correctamente colores válidos.
    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex("#ff0000"), (255, 0, 0));
        assert_eq!(parse_hex("#00ff00"), (0, 255, 0));
        assert_eq!(parse_hex("#0000ff"), (0, 0, 255));
        assert_eq!(parse_hex("#ffffff"), (255, 255, 255));
        assert_eq!(parse_hex("#000000"), (0, 0, 0));
    }

    /// Verifica que `parse_hex` maneja entradas inválidas sin panic.
    #[test]
    fn test_parse_hex_invalid() {
        assert_eq!(parse_hex(""), (0, 0, 0));
        assert_eq!(parse_hex("xyz"), (0, 0, 0));
        assert_eq!(parse_hex("#gg0000"), (0, 0, 0));
        assert_eq!(parse_hex("#ff000"), (0, 0, 0)); // 6 caracteres
        assert_eq!(parse_hex("#ff00000"), (0, 0, 0)); // 8 caracteres
        assert_eq!(parse_hex("ff0000"), (0, 0, 0)); // sin #
        assert_eq!(parse_hex("#-10000"), (0, 0, 0)); // signo negativo
    }

    #[test]
    fn test_scrollback_max_lines() {
        let mut cfg = Config::default();
        cfg.scrollback.lines = 500;
        assert_eq!(cfg.scrollback_max_lines(), 500);

        cfg.scrollback.unlimited = true;
        assert_eq!(cfg.scrollback_max_lines(), usize::MAX);

        cfg.scrollback.unlimited = false;
        cfg.scrollback.lines = 0;
        assert_eq!(cfg.scrollback_max_lines(), 0);
    }

    #[test]
    fn test_process_config_overrides() {
        let toml = r#"
[process]
program = "/usr/bin/zsh"
args = ["-l"]
working_directory = "/tmp/wd"
login = true
startup_command = "echo hi"

[process.env]
FOO = "bar"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let pc = cfg.process_config();
        assert_eq!(pc.shell, "/usr/bin/zsh");
        assert_eq!(pc.args, vec!["-l"]);
        assert_eq!(pc.working_directory.as_deref(), Some("/tmp/wd"));
        assert!(pc.login_shell);
        assert_eq!(pc.startup_command.as_deref(), Some("echo hi"));
        assert!(pc.env.iter().any(|(k, v)| k == "FOO" && v == "bar"));
    }

    #[test]
    fn test_apply_to_term() {
        use crate::ansi::{CursorStyle, Term};

        let toml = r##"
allow_osc52_read = false
[cursor]
color = "#aabbcc"
style = "underline"
"##;
        let cfg: Config = toml::from_str(toml).unwrap();
        let mut term = Term::new();
        cfg.apply_to_term(&mut term);
        assert!(!term.allow_osc52_read);
        assert_eq!(term.cursor_style, CursorStyle::Underline);
        assert_eq!(term.cursor_color_override, Some((0xaa, 0xbb, 0xcc)));
    }

    #[test]
    fn test_apply_to_term_estilo_invalido_usa_block() {
        use crate::ansi::{CursorStyle, Term};

        let toml = r#"[cursor]
style = "hologram"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let mut term = Term::new();
        cfg.apply_to_term(&mut term);
        assert_eq!(term.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn test_toggles_process_keys() {
        let toml = r#"
bold_is_bright = true
allow_osc52_read = false

[process]
program = "/usr/bin/zsh"
args = ["-l"]
login = true

[keys]
"ctrl+shift+v" = "paste_primary"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.bold_is_bright);
        assert!(!cfg.allow_osc52_read);
        assert_eq!(cfg.process.program.as_deref(), Some("/usr/bin/zsh"));
        assert_eq!(cfg.process.args, vec!["-l"]);
        assert!(cfg.process.login);
        assert_eq!(
            cfg.keys.get("ctrl+shift+v").map(String::as_str),
            Some("paste_primary")
        );
        let kb = cfg.keybindings();
        use crate::input::keymap::{Key, Mods};
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Char('v'), cs),
            Some(crate::input::actions::Action::PastePrimary)
        );
    }

    #[test]
    fn test_cursor_config() {
        let cfg = Config::default();
        assert!(cfg.cursor.blink);
        assert_eq!(cfg.cursor.style, "block");

        let toml = "[cursor]\ncolor = \"#ff8800\"\nstyle = \"bar\"\nblink = false\n";
        let p: Config = toml::from_str(toml).unwrap();
        assert_eq!(p.cursor.color.as_deref(), Some("#ff8800"));
        assert_eq!(p.cursor.style, "bar");
        assert!(!p.cursor.blink);
    }

    #[test]
    fn test_panes_config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.panes.max, 12);
        assert!((cfg.panes.split_width_multiplier - 1.0).abs() < f32::EPSILON);
        assert!(!cfg.panes.smart_split);
        assert!(!cfg.panes.preserve_split);
        assert_eq!(cfg.panes_max(), Some(12));
    }

    #[test]
    fn test_panes_max_zero_unlimited() {
        let cfg: Config = toml::from_str("[panes]\nmax = 0").unwrap();
        assert_eq!(cfg.panes_max(), None);
    }

    #[test]
    fn test_scrollback_config() {
        let cfg = Config::default();
        assert_eq!(cfg.scrollback.lines, 10000);
        assert!(!cfg.scrollback.unlimited);
        assert!((cfg.scrollback.multiplier - 3.0).abs() < f32::EPSILON);
        assert!((cfg.scrollback.faux_multiplier - 3.0).abs() < f32::EPSILON);

        let toml = "[scrollback]\nlines = 5000\nunlimited = true\n";
        let parsed: Config = toml::from_str(toml).unwrap();
        assert_eq!(parsed.scrollback.lines, 5000);
        assert!(parsed.scrollback.unlimited);
        // Valores por defecto se mantienen si no se sobreescriben
        assert!((parsed.scrollback.multiplier - 3.0).abs() < f32::EPSILON);
        assert!((parsed.scrollback.faux_multiplier - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_scrollback_multipliers_from_toml() {
        let toml = "[scrollback]\nmultiplier = 5.0\nfaux_multiplier = 2.5\n";
        let parsed: Config = toml::from_str(toml).unwrap();
        assert!((parsed.scrollback.multiplier - 5.0).abs() < f32::EPSILON);
        assert!((parsed.scrollback.faux_multiplier - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_window_config_extendido() {
        let toml = r#"
[window]
opacity = 0.9
padding_x = 8
padding_y = 6
decorations = false
startup = "maximized"
width = 1200
height = 800
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!((cfg.window.opacity - 0.9).abs() < f32::EPSILON);
        assert_eq!(cfg.window.padding_x, 8);
        assert_eq!(cfg.window.padding_y, 6);
        assert!(!cfg.window.decorations);
        assert_eq!(cfg.window.startup, StartupState::Maximized);
        assert_eq!(cfg.window.width, 1200);
        assert_eq!(cfg.window.height, 800);
    }

    #[test]
    fn test_window_config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.window.padding_x, 0);
        // Decorations activas por defecto en ambos SO: la sensacion compacta
        // viene de materiales/tema/fuentes, no de chrome sin bordes.
        assert!(cfg.window.decorations);
        assert_eq!(cfg.window.startup, StartupState::Windowed);
        // Opacidad plena por defecto: la misma clave gobierna el alpha del
        // compositor en Linux y el material nativo en Windows.
        assert_eq!(cfg.window.opacity, 1.0);
    }

    #[test]
    fn test_font_ligatures_default_false() {
        assert!(!FontConfig::default().ligatures);
        let toml = "[font]\nligatures = true\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.font.ligatures);
    }

    #[test]
    fn test_font_fallback_default_y_parse() {
        let cfg = FontConfig::default();
        assert!(cfg.fallback.is_empty(), "sin fallback por defecto");

        let toml = r#"
[font]
family = "Fira Code"
fallback = ["Noto Color Emoji", "Noto Sans CJK SC"]
"#;
        let parsed: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            parsed.font.fallback,
            vec!["Noto Color Emoji", "Noto Sans CJK SC"]
        );
    }

    /// Verifica que un TOML parcial usa defaults para los campos faltantes.
    #[test]
    fn test_config_partial_toml() {
        let toml_str = r##"
[theme]
foreground = "#aabbcc"
background = "#ddeeff"
"##;
        let config: Config = toml::from_str(toml_str).expect("TOML parcial");
        // Campos explícitos
        assert_eq!(config.theme.foreground, "#aabbcc");
        assert_eq!(config.theme.background, "#ddeeff");
        // El resto debe ser default
        assert_eq!(config.theme.cursor, "#d97757");
        assert_eq!(config.theme.selection_bg, Some("#c4704a".into()));
        assert_eq!(config.theme.selection_fg, Some("#0a0a0a".into()));
        assert_eq!(config.theme.red, "#e85d5d");
        // Fuente por defecto
        assert_eq!(config.font.family, "FiraCode Nerd Font Mono");
        assert_eq!(config.font.size, 12);
        // Ventana por defecto
        assert_eq!(config.window.opacity, 1.0);
    }

    #[test]
    fn theme_por_nombre_resuelve_preset() {
        let cfg: Config = toml::from_str("theme = \"nord\"").unwrap();
        let nord = crate::config::themes::preset("nord").unwrap();
        assert_eq!(cfg.theme.background, nord.background);
    }

    #[test]
    fn theme_nombre_mas_override_inline() {
        let toml = r##"
[theme]
name = "nord"
background = "#000000"
"##;
        let cfg: Config = toml::from_str(toml).unwrap();
        let nord = crate::config::themes::preset("nord").unwrap();
        assert_eq!(cfg.theme.background, "#000000");
        assert_eq!(cfg.theme.foreground, nord.foreground);
    }

    #[test]
    fn theme_inline_sin_nombre_sigue_funcionando() {
        let cfg: Config = toml::from_str("[theme]\nbackground = \"#123456\"").unwrap();
        assert_eq!(cfg.theme.background, "#123456");
    }

    #[test]
    fn theme_preset_desconocido_usa_default() {
        let cfg: Config = toml::from_str("theme = \"bogus\"").unwrap();
        assert_eq!(cfg.theme, ThemeConfig::default());
    }

    #[test]
    fn theme_nombre_desconocido_mas_override_usa_default_base() {
        let toml = r##"
[theme]
name = "bogus"
background = "#000000"
"##;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.theme.background, "#000000");
        assert_eq!(cfg.theme.foreground, ThemeConfig::default().foreground);
    }

    #[test]
    fn theme_preset_override_bold_is_bright_y_dim_alpha() {
        let toml = r##"
[theme]
name = "nord"
bold_is_bright = true
dim_alpha = true
"##;
        let cfg: Config = toml::from_str(toml).unwrap();
        let nord = crate::config::themes::preset("nord").unwrap();
        assert_eq!(cfg.theme.background, nord.background);
        assert!(cfg.theme.bold_is_bright);
        assert!(cfg.theme.dim_alpha);
    }

    #[test]
    fn test_apply_to_term_notifications() {
        use crate::ansi::Term;

        let mut term = Term::new();
        assert!(!term.notifications_enabled);

        let toml = "[notifications]\nenabled = true\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        cfg.apply_to_term(&mut term);
        assert!(term.notifications_enabled);
    }

    #[test]
    fn test_notifications_config() {
        let cfg = Config::default();
        assert!(!cfg.notifications.enabled);

        let toml = "[notifications]\nenabled = true\n";
        let p: Config = toml::from_str(toml).unwrap();
        assert!(p.notifications.enabled);
    }

    #[test]
    fn test_debug_config_default_off() {
        let cfg = Config::default();
        assert!(!cfg.debug.fps_counter_enabled);
        let toml = "[debug]\nfps_counter_enabled = true\n";
        let p: Config = toml::from_str(toml).unwrap();
        assert!(p.debug.fps_counter_enabled);
    }

    #[test]
    fn test_render_config_default_and_parse() {
        let cfg = Config::default();
        assert_eq!(cfg.render.max_fps, 60);
        assert_eq!(
            cfg.render.redraw_interval_nanos(),
            Duration::from_secs_f64(1.0 / 60.0).as_nanos() as u64
        );

        let p: Config = toml::from_str("[render]\nmax_fps = 120\n").unwrap();
        assert_eq!(p.render.max_fps, 120);
        assert_eq!(
            p.render.redraw_interval_nanos(),
            Duration::from_secs_f64(1.0 / 120.0).as_nanos() as u64
        );

        let uncapped: Config = toml::from_str("[render]\nmax_fps = 0\n").unwrap();
        assert_eq!(uncapped.render.max_fps, 0);
        assert_eq!(uncapped.render.redraw_interval_nanos(), 0);
        assert!(uncapped.render.redraw_interval().is_none());
    }

    #[test]
    fn test_status_config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.status.duration_ms, 2000);
        assert!(cfg.status.bg_color.is_none());
        assert!(cfg.status.fg_color.is_none());
        let toml = "[status]\nduration_ms = 4000\n";
        let p: Config = toml::from_str(toml).unwrap();
        assert_eq!(p.status.duration_ms, 4000);
    }

    #[test]
    fn test_load_result_describe_failure() {
        let r = Config::load_from_paths(&["/tmp/baud_no_existe.toml".into()]);
        assert!(matches!(r.source, ConfigSource::NotFound));

        use std::io::Write;
        let dir = std::env::temp_dir().join("baud_test_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("invalid.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "theme = [invalid").unwrap();
        let r = Config::load_from_paths(&[path]);
        assert!(matches!(r.source, ConfigSource::ParseError { .. }));

        let valid = dir.join("valid.toml");
        let mut f = std::fs::File::create(&valid).unwrap();
        writeln!(f, "theme = \"claude-dark\"").unwrap();
        let r = Config::load_from_paths(&[valid]);
        assert!(matches!(r.source, ConfigSource::Ok));
    }

    #[test]
    fn apply_to_term_limpia_color_cursor_si_ausente() {
        use crate::ansi::{CursorStyle, Term};

        let mut term = Term::new();
        term.cursor_color_override = Some((1, 2, 3));
        let cfg: Config = toml::from_str("[cursor]\nstyle = \"bar\"\n").unwrap();
        cfg.apply_to_term(&mut term);
        assert_eq!(term.cursor_style, CursorStyle::Bar);
        assert_eq!(term.cursor_color_override, None);
    }

    #[test]
    fn test_diagnostics_defaults() {
        let cfg = Config::default();
        assert!(!cfg.diagnostics.watchdog);
        assert!(cfg.diagnostics.log_level.is_none());
        assert!(cfg.diagnostics.reporting.enabled.is_none());
        assert!(cfg.diagnostics.reporting.dsn.is_none());
    }

    #[test]
    fn test_diagnostics_parse() {
        let toml = r#"
[diagnostics]
watchdog = true
log_level = "info"

[diagnostics.reporting]
enabled = true
dsn = "https://key@o0.ingest.sentry.io/123"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.diagnostics.watchdog);
        assert_eq!(cfg.diagnostics.log_level.as_deref(), Some("info"));
        assert_eq!(cfg.diagnostics.reporting.enabled, Some(true));
        assert_eq!(
            cfg.diagnostics.reporting.dsn.as_deref(),
            Some("https://key@o0.ingest.sentry.io/123")
        );
    }

    #[test]
    fn test_diagnostics_reporting_disabled() {
        let toml = r#"
[diagnostics.reporting]
enabled = false
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.diagnostics.reporting.enabled, Some(false));
        assert!(cfg.diagnostics.reporting.dsn.is_none());
    }

    #[test]
    fn test_process_kind_default_native() {
        let cfg = Config::default();
        assert_eq!(cfg.process.kind, crate::pty::SessionKind::Native);
        let pc = cfg.process_config();
        assert_eq!(pc.kind, crate::pty::SessionKind::Native);
        assert!(pc.distro.is_none());
        assert!(pc.wsl_cwd.is_none());
    }

    #[test]
    fn test_process_kind_wsl_sin_distro() {
        let toml = r#"
[process]
kind = "wsl"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.process.kind, crate::pty::SessionKind::Wsl);
        assert!(cfg.process.distro.is_none());
        let pc = cfg.process_config();
        assert_eq!(pc.kind, crate::pty::SessionKind::Wsl);
    }

    #[test]
    fn test_process_kind_wsl_con_distro_y_cwd() {
        let toml = r#"
[process]
kind = "wsl"
distro = "Ubuntu"
wsl_cwd = "~"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.process.kind, crate::pty::SessionKind::Wsl);
        assert_eq!(cfg.process.distro.as_deref(), Some("Ubuntu"));
        assert_eq!(cfg.process.wsl_cwd.as_deref(), Some("~"));
        let pc = cfg.process_config();
        assert_eq!(pc.distro.as_deref(), Some("Ubuntu"));
        assert_eq!(pc.wsl_cwd.as_deref(), Some("~"));
    }

    #[test]
    fn test_process_kind_desconocido_da_error() {
        let toml = r#"
[process]
kind = "docker"
"#;
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_err(), "kind desconocido debe fallar al parsear");
    }
}
