//! Resolucion de color: tema base + overrides runtime (OSC) + toggles.

use std::sync::OnceLock;

use crate::ansi::{Color, Term};
use crate::config::{parse_hex, ThemeConfig};

/// Overrides de color en runtime (provienen de `Term` via OSC 4/10/11/12).
#[derive(Debug, Clone)]
pub struct ColorOverrides {
    pub palette: [Option<(u8, u8, u8)>; 256],
    pub foreground: Option<(u8, u8, u8)>,
    pub background: Option<(u8, u8, u8)>,
    pub cursor: Option<(u8, u8, u8)>,
}

impl Default for ColorOverrides {
    fn default() -> Self {
        Self {
            palette: [None; 256],
            foreground: None,
            background: None,
            cursor: None,
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
        }
    }
}

/// Vista de resolucion de color para un frame.
pub struct Palette<'a> {
    pub theme: &'a ThemeConfig,
    pub overrides: &'a ColorOverrides,
    pub bold_is_bright: bool,
}

static EMPTY_OVERRIDES: OnceLock<ColorOverrides> = OnceLock::new();

impl<'a> Palette<'a> {
    /// Construye con overrides vacios (tests / camino sin OSC).
    pub fn from_theme(theme: &'a ThemeConfig) -> Self {
        Self {
            theme,
            overrides: EMPTY_OVERRIDES.get_or_init(ColorOverrides::default),
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
            if let Some(fg) = self.overrides.foreground {
                return fg;
            }
            return parse_hex(&self.theme.foreground);
        }
        if let Some(idx) = Self::ansi_index(color) {
            if let Some(rgb) = self.overrides.palette[idx as usize] {
                return rgb;
            }
        }
        super::color_rgb_from_theme(color, self.theme)
    }

    /// Resuelve background (`Default` -> color de fondo, no foreground).
    pub fn bg_rgb(&self, color: Color) -> (u8, u8, u8) {
        if let Color::Default = color {
            if let Some(bg) = self.overrides.background {
                return bg;
            }
            return parse_hex(&self.theme.background);
        }
        if let Some(idx) = Self::ansi_index(color) {
            if let Some(rgb) = self.overrides.palette[idx as usize] {
                return rgb;
            }
        }
        super::color_rgb_from_theme(color, self.theme)
    }

    pub fn cursor_rgb(&self) -> (u8, u8, u8) {
        self.overrides
            .cursor
            .unwrap_or_else(|| parse_hex(&self.theme.cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeConfig;

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
            bold_is_bright: true,
        };
        assert_eq!(pal.rgb(Color::Red, true), parse_hex(&theme.bright_red));
    }
}
