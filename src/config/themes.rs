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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn claude_dark_coincide_con_default() {
        assert_eq!(
            preset("claude-dark").expect("claude-dark parsea"),
            ThemeConfig::default()
        );
    }
}
