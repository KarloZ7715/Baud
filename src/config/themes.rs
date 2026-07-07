//! Catálogo de temas embebidos.

use super::ThemeConfig;

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

/// Contraste mínimo exigido para texto legible sobre el fondo del tema.
pub const MIN_LEGIBLE_CONTRAST: f64 = 3.0;

/// Contraste mínimo para comentarios (`bright_black`) sobre el fondo.
pub const MIN_COMMENT_CONTRAST: f64 = 4.5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::contrast_ratio_hex as contrast_ratio;

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
    fn minimum_contrast_default_es_tres() {
        assert!((ThemeConfig::default().minimum_contrast - 3.0).abs() < f64::EPSILON);
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

    #[test]
    fn presets_tienen_contraste_legible() {
        for name in available_presets() {
            let theme = try_preset(name).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let bg = &theme.background;
            for field in ANSI_COLOR_FIELDS {
                let hex = theme_color_hex(&theme, field);
                let min = if *field == "bright_black" {
                    MIN_COMMENT_CONTRAST
                } else {
                    MIN_LEGIBLE_CONTRAST
                };
                let ratio = contrast_ratio(hex, bg);
                assert!(
                    ratio >= min,
                    "preset '{name}' campo '{field}' ({hex} sobre {bg}) ratio={ratio:.2} < {min}"
                );
            }
            let fg_ratio = contrast_ratio(&theme.foreground, bg);
            assert!(
                fg_ratio >= MIN_LEGIBLE_CONTRAST,
                "preset '{name}' foreground ratio={fg_ratio:.2} < {MIN_LEGIBLE_CONTRAST}"
            );
        }
    }

    #[test]
    fn presets_ajustados_cumplen_minimo() {
        use crate::color::contrast_ratio_rgb;
        use crate::config::parse_hex;
        use crate::renderer::adjust_fg;

        for name in available_presets() {
            let theme = try_preset(name).unwrap();
            let bg = parse_hex(&theme.background);
            let min = theme.minimum_contrast;
            for field in ANSI_COLOR_FIELDS {
                let fg = parse_hex(theme_color_hex(&theme, field));
                let adjusted = adjust_fg(fg, bg, min);
                assert!(
                    contrast_ratio_rgb(adjusted, bg) >= min,
                    "preset {name} campo {field}: ratio tras ajuste < {min}"
                );
            }
        }
    }
}
