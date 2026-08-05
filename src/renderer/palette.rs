//! Resolucion de color: tema base + overrides runtime (OSC) + toggles.

use std::sync::OnceLock;

use crate::ansi::{Color, Term};
use crate::config::ThemeConfig;

/// Overrides de color en runtime (provienen de `Term` via OSC 4/10/11/12),
/// mas la preferencia de cursor del usuario, que es un nivel de precedencia
/// intermedio entre el OSC de la aplicacion y el color del tema.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorOverrides {
    pub palette: [Option<(u8, u8, u8)>; 256],
    pub foreground: Option<(u8, u8, u8)>,
    pub background: Option<(u8, u8, u8)>,
    pub cursor: Option<(u8, u8, u8)>,
    pub config_cursor: Option<(u8, u8, u8)>,
}

impl Default for ColorOverrides {
    fn default() -> Self {
        Self {
            palette: [None; 256],
            foreground: None,
            background: None,
            cursor: None,
            config_cursor: None,
        }
    }
}

impl ColorOverrides {
    pub fn from_term(term: &Term) -> Self {
        Self {
            palette: term.runtime_palette,
            foreground: term.fg_override,
            background: term.bg_override,
            cursor: term.cursor_color_override,
            config_cursor: term.config_cursor_color,
        }
    }
}

/// Vista de resolucion de color para un frame.
pub struct Palette<'a> {
    pub theme: &'a ThemeConfig,
    pub overrides: &'a ColorOverrides,
    /// Colores base del tema que se esta pintando. Sale de `theme`, no del
    /// `Term`, para que la vista previa del theme picker siga funcionando: el
    /// `Term` lleva sembrado el tema guardado, no el de la vista previa.
    pub defaults: crate::color::DefaultColors,
    pub bold_is_bright: bool,
}

static EMPTY_OVERRIDES: OnceLock<ColorOverrides> = OnceLock::new();

impl<'a> Palette<'a> {
    /// Construye con overrides vacios (tests / camino sin OSC).
    pub fn from_theme(theme: &'a ThemeConfig) -> Self {
        Self {
            theme,
            overrides: EMPTY_OVERRIDES.get_or_init(ColorOverrides::default),
            defaults: crate::color::DefaultColors::from_theme(theme),
            bold_is_bright: false,
        }
    }

    fn ansi_index(color: Color) -> Option<u8> {
        Some(match color {
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::White => 7,
            Color::BrightBlack => 8,
            Color::BrightRed => 9,
            Color::BrightGreen => 10,
            Color::BrightYellow => 11,
            Color::BrightBlue => 12,
            Color::BrightMagenta => 13,
            Color::BrightCyan => 14,
            Color::BrightWhite => 15,
            Color::Indexed(n) => n,
            _ => return None,
        })
    }

    fn brighten(color: Color) -> Color {
        match color {
            Color::Black => Color::BrightBlack,
            Color::Red => Color::BrightRed,
            Color::Green => Color::BrightGreen,
            Color::Yellow => Color::BrightYellow,
            Color::Blue => Color::BrightBlue,
            Color::Magenta => Color::BrightMagenta,
            Color::Cyan => Color::BrightCyan,
            Color::White => Color::BrightWhite,
            other => other,
        }
    }

    /// Resuelve un color de foreground a RGB (aplica `bold_is_bright` si procede).
    pub fn rgb(&self, color: Color, bold: bool) -> (u8, u8, u8) {
        let color = if self.bold_is_bright && bold {
            Self::brighten(color)
        } else {
            color
        };
        if let Color::Default = color {
            return self
                .overrides
                .foreground
                .unwrap_or(self.defaults.foreground);
        }
        if let Color::Rgb(r, g, b) = color {
            return (r, g, b);
        }
        if let Some(idx) = Self::ansi_index(color) {
            return self.overrides.palette[idx as usize]
                .unwrap_or_else(|| self.defaults.indexed(idx));
        }
        super::color_rgb_from_theme(color, self.theme)
    }

    /// Resuelve background (`Default` -> color de fondo, no foreground).
    pub fn bg_rgb(&self, color: Color) -> (u8, u8, u8) {
        if let Color::Default = color {
            return self
                .overrides
                .background
                .unwrap_or(self.defaults.background);
        }
        if let Color::Rgb(r, g, b) = color {
            return (r, g, b);
        }
        if let Some(idx) = Self::ansi_index(color) {
            return self.overrides.palette[idx as usize]
                .unwrap_or_else(|| self.defaults.indexed(idx));
        }
        super::color_rgb_from_theme(color, self.theme)
    }

    /// Tres niveles: el `OSC 12` de la aplicacion, luego `[cursor].color` del
    /// usuario, y por ultimo el cursor del tema que se esta pintando.
    pub fn cursor_rgb(&self) -> (u8, u8, u8) {
        self.overrides
            .cursor
            .or(self.overrides.config_cursor)
            .unwrap_or(self.defaults.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse_hex, ThemeConfig};

    #[test]
    fn test_palette_default_usa_tema() {
        let theme = ThemeConfig::default();
        let pal = Palette::from_theme(&theme);
        let (r, g, b) = parse_hex(&theme.red);
        assert_eq!(pal.rgb(Color::Red, false), (r, g, b));
    }

    #[test]
    fn test_palette_override_indexado_y_bg() {
        let theme = ThemeConfig::default();
        let mut overrides = ColorOverrides::default();
        overrides.palette[1] = Some((10, 20, 30));
        overrides.background = Some((1, 2, 3));
        let pal = Palette {
            theme: &theme,
            overrides: &overrides,
            defaults: crate::color::DefaultColors::from_theme(&theme),
            bold_is_bright: false,
        };
        assert_eq!(pal.rgb(Color::Indexed(1), false), (10, 20, 30));
        assert_eq!(pal.rgb(Color::Red, false), (10, 20, 30));
        assert_eq!(pal.bg_rgb(Color::Default), (1, 2, 3));
    }

    #[test]
    fn test_palette_bold_is_bright() {
        let theme = ThemeConfig::default();
        let overrides = ColorOverrides::default();
        let pal = Palette {
            theme: &theme,
            overrides: &overrides,
            defaults: crate::color::DefaultColors::from_theme(&theme),
            bold_is_bright: true,
        };
        assert_eq!(pal.rgb(Color::Red, true), parse_hex(&theme.bright_red));
    }

    #[test]
    fn palette_usa_defaults_para_color_default() {
        let theme = ThemeConfig::default();
        let overrides = ColorOverrides::default();
        let mut defaults = crate::color::DefaultColors::from_theme(&theme);
        defaults.foreground = (1, 2, 3);
        defaults.background = (4, 5, 6);
        let pal = Palette {
            theme: &theme,
            overrides: &overrides,
            defaults,
            bold_is_bright: false,
        };
        assert_eq!(pal.rgb(Color::Default, false), (1, 2, 3));
        assert_eq!(pal.bg_rgb(Color::Default), (4, 5, 6));
    }

    #[test]
    fn palette_cursor_respeta_los_tres_niveles() {
        let theme = ThemeConfig::default();
        let mut defaults = crate::color::DefaultColors::from_theme(&theme);
        defaults.cursor = (0xd9, 0x77, 0x57);

        // Nivel 3: sin nada mas, manda el tema que se esta pintando.
        let overrides = ColorOverrides::default();
        let pal = Palette {
            theme: &theme,
            overrides: &overrides,
            defaults,
            bold_is_bright: false,
        };
        assert_eq!(pal.cursor_rgb(), (0xd9, 0x77, 0x57));

        // Nivel 2: `[cursor].color` del usuario pisa al tema.
        let con_config = ColorOverrides {
            config_cursor: Some((0xff, 0xff, 0xff)),
            ..ColorOverrides::default()
        };
        let pal = Palette {
            theme: &theme,
            overrides: &con_config,
            defaults,
            bold_is_bright: false,
        };
        assert_eq!(pal.cursor_rgb(), (0xff, 0xff, 0xff));

        // Nivel 1: el OSC 12 de la aplicacion pisa a los dos.
        let mut con_osc = con_config.clone();
        con_osc.cursor = Some((9, 9, 9));
        let pal = Palette {
            theme: &theme,
            overrides: &con_osc,
            defaults,
            bold_is_bright: false,
        };
        assert_eq!(pal.cursor_rgb(), (9, 9, 9));
    }

    #[test]
    fn palette_indexado_sin_override_no_cambia_de_aspecto() {
        // Fija el aspecto por defecto: con el tema de fabrica, cada indice
        // 0..=255 tiene que resolver al mismo RGB que antes de este plan.
        let theme = ThemeConfig::default();
        let pal = Palette::from_theme(&theme);
        for n in 0..=255u8 {
            assert_eq!(
                pal.rgb(Color::Indexed(n), false),
                crate::renderer::ansi_256_to_rgb_for_test(n, &theme),
                "indice {n}"
            );
        }
    }
}
