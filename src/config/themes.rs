//! Catálogo de temas embebidos.

use super::ThemeConfig;

const CATPPUCCIN_MOCHA: &str = include_str!("themes/catppuccin-mocha.toml");
const TOKYO_NIGHT: &str = include_str!("themes/tokyo-night.toml");
const GRUVBOX_DARK: &str = include_str!("themes/gruvbox-dark.toml");
const NORD: &str = include_str!("themes/nord.toml");
const CLAUDE_DARK: &str = include_str!("themes/claude-dark.toml");

const PRESETS: &[(&str, &str)] = &[
    ("catppuccin-mocha", CATPPUCCIN_MOCHA),
    ("tokyo-night", TOKYO_NIGHT),
    ("gruvbox-dark", GRUVBOX_DARK),
    ("nord", NORD),
    ("claude-dark", CLAUDE_DARK),
];

/// Devuelve el `ThemeConfig` de un preset por nombre (`None` si no existe).
pub fn preset(name: &str) -> Option<ThemeConfig> {
    let (_, body) = PRESETS.iter().find(|(n, _)| *n == name)?;
    match toml::from_str::<ThemeConfig>(body) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("preset '{name}' inválido: {e}");
            None
        }
    }
}

/// Nombres de presets disponibles.
pub fn available_presets() -> Vec<&'static str> {
    PRESETS.iter().map(|(n, _)| *n).collect()
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
    }

    #[test]
    fn lista_de_presets_no_vacia() {
        assert!(available_presets().contains(&"catppuccin-mocha"));
        assert!(available_presets().len() >= 4);
    }
}
