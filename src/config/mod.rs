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
    #[serde(default)]
    pub selection_bg: Option<String>,
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
}

/// Configuración de la fuente (tipografía y tamaño).
///
/// La funcionalidad real de renderizado de fuente se implementará en
/// sprints posteriores (Sprint A2).
#[derive(Debug, Clone, Deserialize)]
pub struct FontConfig {
    #[serde(default = "default_font_family")]
    pub family: String,
    #[serde(default = "default_font_size")]
    pub size: u16,
}

/// Configuración de la ventana (opacidad, etc.).
///
/// La opacidad real se implementará en sprints posteriores.
#[derive(Debug, Clone, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_opacity")]
    pub opacity: f32,
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
            selection_bg: None,
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
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: default_opacity(),
        }
    }
}

// ---------------------------------------------------------------------------
// Funciones default — tema Catppuccin Mocha
// ---------------------------------------------------------------------------

fn default_foreground() -> String {
    "#cdd6f4".into()
}
fn default_background() -> String {
    "#1e1e2e".into()
}
fn default_cursor() -> String {
    "#f5e0dc".into()
}
fn default_black() -> String {
    "#45475a".into()
}
fn default_red() -> String {
    "#f38ba8".into()
}
fn default_green() -> String {
    "#a6e3a1".into()
}
fn default_yellow() -> String {
    "#f9e2af".into()
}
fn default_blue() -> String {
    "#89b4fa".into()
}
fn default_magenta() -> String {
    "#f5c2e7".into()
}
fn default_cyan() -> String {
    "#94e2d5".into()
}
fn default_white() -> String {
    "#bac2de".into()
}
// pony tail: Catppuccin Mocha no diferencia bright; reutilizamos los valores
// normales para los brillantes.
fn default_bright_black() -> String {
    default_black()
}
fn default_bright_red() -> String {
    default_red()
}
fn default_bright_green() -> String {
    default_green()
}
fn default_bright_yellow() -> String {
    default_yellow()
}
fn default_bright_blue() -> String {
    default_blue()
}
fn default_bright_magenta() -> String {
    default_magenta()
}
fn default_bright_cyan() -> String {
    default_cyan()
}
fn default_bright_white() -> String {
    default_foreground()
}

fn default_font_family() -> String {
    "monospace".into()
}
fn default_font_size() -> u16 {
    14
}
fn default_opacity() -> f32 {
    1.0
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

    /// Verifica que `Config::default()` use los colores de Catppuccin Mocha.
    #[test]
    fn test_config_default_values() {
        let config = Config::default();

        // Tema — valores representativos de Catppuccin Mocha
        assert_eq!(config.theme.foreground, "#cdd6f4");
        assert_eq!(config.theme.background, "#1e1e2e");
        assert_eq!(config.theme.cursor, "#f5e0dc");
        assert_eq!(config.theme.selection_bg, None);
        assert_eq!(config.theme.black, "#45475a");
        assert_eq!(config.theme.red, "#f38ba8");
        assert_eq!(config.theme.green, "#a6e3a1");
        assert_eq!(config.theme.yellow, "#f9e2af");
        assert_eq!(config.theme.blue, "#89b4fa");
        assert_eq!(config.theme.magenta, "#f5c2e7");
        assert_eq!(config.theme.cyan, "#94e2d5");
        assert_eq!(config.theme.white, "#bac2de");

        // Brillantes — mismos valores que los normales (Catppuccin no diferencia)
        assert_eq!(config.theme.bright_black, config.theme.black);
        assert_eq!(config.theme.bright_red, config.theme.red);
        assert_eq!(config.theme.bright_green, config.theme.green);
        assert_eq!(config.theme.bright_yellow, config.theme.yellow);
        assert_eq!(config.theme.bright_blue, config.theme.blue);
        assert_eq!(config.theme.bright_magenta, config.theme.magenta);
        assert_eq!(config.theme.bright_cyan, config.theme.cyan);
        assert_eq!(config.theme.bright_white, config.theme.foreground);

        // Fuente
        assert_eq!(config.font.family, "monospace");
        assert_eq!(config.font.size, 14);

        // Ventana
        assert_eq!(config.window.opacity, 1.0);
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
        // El resto debe ser default (Catppuccin Mocha)
        assert_eq!(config.theme.cursor, "#f5e0dc");
        assert_eq!(config.theme.selection_bg, None);
        assert_eq!(config.theme.red, "#f38ba8");
        // Fuente por defecto
        assert_eq!(config.font.family, "monospace");
        assert_eq!(config.font.size, 14);
        // Ventana por defecto
        assert_eq!(config.window.opacity, 1.0);
    }
}
