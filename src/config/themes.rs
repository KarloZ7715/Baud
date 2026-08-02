//! Catálogo de temas embebidos.

use super::{ColorScheme, ThemeConfig};

macro_rules! presets {
    ($( ($name:literal, $body:expr) ),+ $(,)?) => {
        const PRESETS: &[(&str, &str)] = &[ $( ($name, $body) ),+ ];
        const PRESET_NAMES: &[&str] = &[ $( $name ),+ ];
    };
}

presets!(
    (
        "catppuccin-mocha",
        include_str!("themes/catppuccin-mocha.toml")
    ),
    ("tokyo-night", include_str!("themes/tokyo-night.toml")),
    ("gruvbox-dark", include_str!("themes/gruvbox-dark.toml")),
    ("nord", include_str!("themes/nord.toml")),
    ("claude-dark", include_str!("themes/claude-dark.toml")),
    ("dracula", include_str!("themes/dracula.toml")),
    ("rose-pine", include_str!("themes/rose-pine.toml")),
    ("monokai", include_str!("themes/monokai.toml")),
    ("one-dark", include_str!("themes/one-dark.toml")),
    ("solarized-dark", include_str!("themes/solarized-dark.toml")),
    (
        "everforest-dark",
        include_str!("themes/everforest-dark.toml")
    ),
    ("kanagawa-wave", include_str!("themes/kanagawa-wave.toml")),
    ("ayu-dark", include_str!("themes/ayu-dark.toml")),
    ("github-dark", include_str!("themes/github-dark.toml")),
    ("cobalt2", include_str!("themes/cobalt2.toml")),
    ("flexoki-dark", include_str!("themes/flexoki-dark.toml")),
    (
        "catppuccin-latte",
        include_str!("themes/catppuccin-latte.toml")
    ),
    ("gruvbox-light", include_str!("themes/gruvbox-light.toml")),
    (
        "solarized-light",
        include_str!("themes/solarized-light.toml")
    ),
    ("rose-pine-dawn", include_str!("themes/rose-pine-dawn.toml")),
    ("github-light", include_str!("themes/github-light.toml")),
    (
        "everforest-light",
        include_str!("themes/everforest-light.toml")
    ),
);

/// Error al resolver un preset embebido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    NotFound,
    InvalidToml(String),
}

/// Resuelve un preset por nombre con error tipado.
pub fn try_preset(name: &str) -> Result<ThemeConfig, PresetError> {
    let (_, body) = PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or(PresetError::NotFound)?;
    toml::from_str::<ThemeConfig>(body).map_err(|e| PresetError::InvalidToml(e.to_string()))
}

/// Devuelve el `ThemeConfig` de un preset por nombre (`None` si no existe o no parsea).
pub fn preset(name: &str) -> Option<ThemeConfig> {
    match try_preset(name) {
        Ok(t) => Some(t),
        Err(PresetError::NotFound) => None,
        Err(PresetError::InvalidToml(e)) => {
            tracing::warn!("preset '{name}' inválido: {e}");
            None
        }
    }
}

/// Nombres de presets disponibles.
pub fn available_presets() -> &'static [&'static str] {
    PRESET_NAMES
}

/// Catálogo de presets resueltos: nombre y su `ThemeConfig` parseado. Fuente
/// única para el generador de referencia (R3): itera esto, no duplica la
/// lista de nombres ni transcribe paletas a mano.
pub fn preset_entries() -> Vec<(&'static str, ThemeConfig)> {
    PRESET_NAMES
        .iter()
        .map(|&name| {
            let theme = try_preset(name)
                .unwrap_or_else(|e| panic!("preset embebido '{name}' invalido: {e:?}"));
            (name, theme)
        })
        .collect()
}

/// Polaridad de un preset (oscura/clara) según la luminancia de su fondo.
///
/// Usa el mismo criterio que el motor de contraste: un fondo claro
/// (`relative_luminance > 0.5`) clasifica el preset como [`ColorScheme::Light`].
/// El theme picker agrupa los presets por este valor y decide a qué variante
/// (`theme.dark`/`theme.light`) escribir al confirmar.
pub fn preset_polarity(name: &str) -> ColorScheme {
    let theme = preset(name).unwrap_or_default();
    let (r, g, b) = super::parse_hex(&theme.background);
    if crate::color::relative_luminance((r, g, b)) > 0.5 {
        ColorScheme::Light
    } else {
        ColorScheme::Dark
    }
}

/// Piso de contraste del chrome de Baud (theme picker, barra de estado, tabs,
/// title bar): fijo, no depende de `theme.minimum_contrast` — ese ajuste es
/// del usuario para el texto de sus aplicaciones, no para la interfaz del
/// terminal. Ya no valida la paleta cruda de los presets.
pub const MIN_LEGIBLE_CONTRAST: f64 = 3.0;

/// Piso de contraste del chrome para texto secundario/tenue (comentarios de
/// UI, no de la paleta ANSI). Mismo motivo que `MIN_LEGIBLE_CONTRAST`.
pub const MIN_COMMENT_CONTRAST: f64 = 4.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_polarity_clasifica_los_22_presets() {
        let light: &[&str] = &[
            "catppuccin-latte",
            "gruvbox-light",
            "solarized-light",
            "rose-pine-dawn",
            "github-light",
            "everforest-light",
        ];
        for name in available_presets() {
            let got = preset_polarity(name);
            if light.contains(name) {
                assert_eq!(got, ColorScheme::Light, "{name} debería ser Light");
            } else {
                assert_eq!(got, ColorScheme::Dark, "{name} debería ser Dark");
            }
        }
    }

    const ANSI_COLOR_FIELDS: &[&str] = &[
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    ];

    fn theme_color_hex<'a>(theme: &'a ThemeConfig, field: &str) -> &'a str {
        match field {
            "foreground" => &theme.foreground,
            "black" => &theme.black,
            "red" => &theme.red,
            "green" => &theme.green,
            "yellow" => &theme.yellow,
            "blue" => &theme.blue,
            "magenta" => &theme.magenta,
            "cyan" => &theme.cyan,
            "white" => &theme.white,
            "bright_black" => &theme.bright_black,
            "bright_red" => &theme.bright_red,
            "bright_green" => &theme.bright_green,
            "bright_yellow" => &theme.bright_yellow,
            "bright_blue" => &theme.bright_blue,
            "bright_magenta" => &theme.bright_magenta,
            "bright_cyan" => &theme.bright_cyan,
            "bright_white" => &theme.bright_white,
            _ => unreachable!("campo ANSI desconocido: {field}"),
        }
    }

    #[test]
    fn preset_conocido_devuelve_theme() {
        let t = preset("catppuccin-mocha").expect("preset existe");
        assert!(t.background.starts_with('#'));
        assert_eq!(t.background.len(), 7);
    }

    #[test]
    fn preset_desconocido_es_none() {
        assert!(preset("no-existe").is_none());
        assert_eq!(try_preset("no-existe"), Err(PresetError::NotFound));
    }

    #[test]
    fn lista_de_presets_completa() {
        assert_eq!(available_presets().len(), PRESET_NAMES.len());
        assert!(available_presets().contains(&"catppuccin-mocha"));
    }

    #[test]
    fn todos_los_presets_parsean() {
        for name in available_presets() {
            try_preset(name).unwrap_or_else(|e| panic!("preset '{name}' falló: {e:?}"));
        }
    }

    #[test]
    fn minimum_contrast_default_es_uno_punto_cinco() {
        assert!((ThemeConfig::default().minimum_contrast - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn minimum_contrast_uno_desactiva_ajuste() {
        let theme = ThemeConfig {
            minimum_contrast: 1.0,
            ..ThemeConfig::default()
        };
        let fg = (0x58, 0x6e, 0x75);
        let bg = (0x00, 0x2b, 0x36);
        assert_eq!(
            crate::renderer::adjust_fg(fg, bg, theme.minimum_contrast),
            fg
        );
    }

    #[test]
    fn claude_dark_coincide_con_default() {
        assert_eq!(
            preset("claude-dark").expect("claude-dark parsea"),
            ThemeConfig::default()
        );
    }

    /// Cada archivo de tema empieza por una línea `# source: http...` que cita
    /// su origen upstream. Guardarraíl contra que alguien vuelva a retocar un
    /// hex a mano para pasar un test de contraste: cualquier cambio de color
    /// exige tocar también la fuente citada.
    #[test]
    fn todo_preset_declara_su_fuente() {
        const FILES: &[(&str, &str)] = &[
            ("ayu-dark", include_str!("themes/ayu-dark.toml")),
            (
                "catppuccin-mocha",
                include_str!("themes/catppuccin-mocha.toml"),
            ),
            (
                "catppuccin-latte",
                include_str!("themes/catppuccin-latte.toml"),
            ),
            ("claude-dark", include_str!("themes/claude-dark.toml")),
            ("cobalt2", include_str!("themes/cobalt2.toml")),
            ("dracula", include_str!("themes/dracula.toml")),
            (
                "everforest-dark",
                include_str!("themes/everforest-dark.toml"),
            ),
            (
                "everforest-light",
                include_str!("themes/everforest-light.toml"),
            ),
            ("flexoki-dark", include_str!("themes/flexoki-dark.toml")),
            ("github-dark", include_str!("themes/github-dark.toml")),
            ("github-light", include_str!("themes/github-light.toml")),
            ("gruvbox-dark", include_str!("themes/gruvbox-dark.toml")),
            ("gruvbox-light", include_str!("themes/gruvbox-light.toml")),
            ("kanagawa-wave", include_str!("themes/kanagawa-wave.toml")),
            ("monokai", include_str!("themes/monokai.toml")),
            ("nord", include_str!("themes/nord.toml")),
            ("one-dark", include_str!("themes/one-dark.toml")),
            ("rose-pine", include_str!("themes/rose-pine.toml")),
            ("rose-pine-dawn", include_str!("themes/rose-pine-dawn.toml")),
            ("solarized-dark", include_str!("themes/solarized-dark.toml")),
            (
                "solarized-light",
                include_str!("themes/solarized-light.toml"),
            ),
            ("tokyo-night", include_str!("themes/tokyo-night.toml")),
        ];
        assert_eq!(
            FILES.len(),
            available_presets().len(),
            "la lista de archivos del test no cubre todos los presets registrados"
        );
        for (name, body) in FILES {
            assert!(
                body.starts_with("# source: http"),
                "preset '{name}' no declara su fuente en la primera línea"
            );
        }
    }

    /// Valida la salida (lo que ve el usuario al pintar), no el insumo: cada
    /// preset y cada color ANSI, ajustado al piso por defecto (1.5), alcanza
    /// ese piso sobre su fondo. Sustituye a la vieja `presets_tienen_contraste_legible`,
    /// que exigía el piso sobre el hex crudo del tema y por eso forzaba a
    /// retocar paletas a mano.
    #[test]
    fn presets_ajustados_cumplen_piso_por_defecto() {
        use crate::color::contrast_ratio_rgb;
        use crate::config::parse_hex;
        use crate::renderer::adjust_fg;

        let ajuste = ThemeConfig::default().minimum_contrast;
        for name in available_presets() {
            let theme = try_preset(name).unwrap();
            let bg = parse_hex(&theme.background);
            for field in ANSI_COLOR_FIELDS {
                let fg = parse_hex(theme_color_hex(&theme, field));
                if fg == bg {
                    continue;
                }
                let adjusted = adjust_fg(fg, bg, ajuste);
                assert!(
                    contrast_ratio_rgb(adjusted, bg) >= ajuste,
                    "preset {name} campo {field}: ratio tras ajuste al piso por defecto < {ajuste}"
                );
            }
        }
    }

    /// Verifica el algoritmo de ajuste OKLab a un piso de 3.0 (texto grande
    /// WCAG), independientemente del default de `minimum_contrast`.
    #[test]
    fn presets_ajustados_cumplen_piso_tres() {
        use crate::color::contrast_ratio_rgb;
        use crate::config::parse_hex;
        use crate::renderer::adjust_fg;

        const AJUSTE: f64 = 3.0;
        for name in available_presets() {
            let theme = try_preset(name).unwrap();
            let bg = parse_hex(&theme.background);
            for field in ANSI_COLOR_FIELDS {
                let fg = parse_hex(theme_color_hex(&theme, field));
                // `adjust_fg` no puede separar un color de un fondo idéntico
                // (no hay dirección en la que moverlo): solarized-dark define
                // `bright_black` igual a su propio fondo (`base03`), a
                // propósito, en la especificación upstream.
                if fg == bg {
                    continue;
                }
                let adjusted = adjust_fg(fg, bg, AJUSTE);
                assert!(
                    contrast_ratio_rgb(adjusted, bg) >= AJUSTE,
                    "preset {name} campo {field}: ratio tras ajuste a {AJUSTE} < {AJUSTE}"
                );
            }
        }
    }

    /// El motor de contraste distingue polaridad de fondo (`contrast.rs`,
    /// `light_bg = bg_lab.l > 0.6`); hasta este batch ningún preset claro lo
    /// ejercitaba. `catppuccin-latte` tiene fondo claro y un ANSI de bajo
    /// contraste (`yellow`) que demuestra que el ajuste sube el ratio sin
    /// invertir la dirección de la búsqueda binaria.
    #[test]
    fn adjust_fg_funciona_sobre_preset_claro() {
        use crate::color::contrast_ratio_rgb;
        use crate::config::parse_hex;
        use crate::renderer::adjust_fg;

        let theme = try_preset("catppuccin-latte").unwrap();
        let bg = parse_hex(&theme.background);
        let fg = parse_hex(&theme.yellow);
        let before = contrast_ratio_rgb(fg, bg);
        let adjusted = adjust_fg(fg, bg, 4.5);
        assert!(
            contrast_ratio_rgb(adjusted, bg) >= 4.5,
            "catppuccin-latte yellow ajustado no alcanza 4.5:1"
        );
        assert!(contrast_ratio_rgb(adjusted, bg) >= before);
    }
}
