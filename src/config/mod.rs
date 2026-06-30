//! Sistema de configuración para Baud mediante archivos TOML.
//!
//! Las estructuras de este módulo utilizan `serde::Deserialize` con valores
//! por defecto tomados del tema **Catppuccin Mocha**. La configuración se
//! carga al inicio del programa (sin hot-reload) desde, por orden de
//! prioridad:
//!
//! 1. `$XDG_CONFIG_HOME/baud/config.toml` (o `~/.config/baud/config.toml` en Linux).
//! 2. `./baud.toml` en el directorio de trabajo.
//! 3. Valores por defecto (`Config::default()`).

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Estructuras principales
// ---------------------------------------------------------------------------

/// Configuración global del emulador.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub selection: SelectionConfig,
    #[serde(default)]
    pub copy_mode: CopyModeConfig,
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
#[derive(Debug, Clone, Deserialize)]
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
    // pony tail: Catppuccin Mocha no diferencia bright de los normales,
    // por eso los brillantes apuntan a los mismos valores.
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
}

/// Desplazamiento fino del glifo dentro de la celda.
#[derive(Debug, Clone, Copy, Deserialize)]
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
    #[serde(default = "default_win_width")]
    pub width: u32,
    #[serde(default = "default_win_height")]
    pub height: u32,
}

fn default_win_width() -> u32 {
    800
}

fn default_win_height() -> u32 {
    600
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
    "#3d3d3d".into()
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
    "MesloLGS Nerd Font Mono".into()
}
fn default_font_size() -> u16 {
    14
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
    "primary".into()
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
// Config::load()
// ---------------------------------------------------------------------------

impl Config {
    /// Construye la configuración del proceso hijo del PTY.
    ///
    /// Convierte [`Config`] en [`crate::pty::ProcessConfig`] (shell, args,
    /// directorio de arranque, variables de entorno y comando inicial).
    pub fn process_config(&self) -> crate::pty::ProcessConfig {
        let _ = self;
        crate::pty::ProcessConfig::default()
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
    pub fn load() -> Self {
        // 1. directorio de configuración del sistema
        let paths = [
            dirs::config_dir()
                .map(|d| d.join("baud").join("config.toml"))
                .unwrap_or_default(),
            std::path::PathBuf::from("baud.toml"),
        ];

        for path in &paths {
            if path.exists() {
                match std::fs::read_to_string(path) {
                    Ok(content) => match toml::from_str::<Config>(&content) {
                        Ok(config) => return config,
                        Err(e) => {
                            tracing::warn!(
                                "Config: error al parsear '{}': {}. Usando defaults.",
                                path.display(),
                                e
                            );
                            return Self::default();
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            "Config: no se pudo leer '{}': {}. Usando defaults.",
                            path.display(),
                            e
                        );
                        return Self::default();
                    }
                }
            }
        }

        Self::default()
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
        assert_eq!(config.font.family, "MesloLGS Nerd Font Mono");
        assert_eq!(config.font.size, 14);

        // Ventana
        assert_eq!(config.window.opacity, 1.0);

        // Selección por defecto: copy_on_select off, smart on, bypass=[shift]
        assert!(!config.selection.copy_on_select);
        assert!(config.selection.smart_selection);
        assert!(config.selection.bypass_contains("shift"));
        assert!(!config.selection.bypass_contains("alt"));
        // Copy mode habilitado por defecto
        assert!(config.copy_mode.enabled);
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
bypass_mouse_reporting_modifiers = ["shift", "alt"]
smart_selection = false
word_delimiters = " ,.;"
"##;
        let config: Config = toml::from_str(toml_str).expect("TOML selección");
        assert!(config.selection.copy_on_select);
        assert_eq!(config.selection.copy_on_select_target, "both");
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
        assert!(cfg.window.decorations);
        assert_eq!(cfg.window.startup, StartupState::Windowed);
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
        assert_eq!(config.font.family, "MesloLGS Nerd Font Mono");
        assert_eq!(config.font.size, 14);
        // Ventana por defecto
        assert_eq!(config.window.opacity, 1.0);
    }
}
