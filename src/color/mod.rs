//! Utilidades de color compartidas (contraste WCAG).

/// Ratio de contraste WCAG 2.1 entre dos colores hex (#rrggbb).
pub fn contrast_ratio_hex(fg: &str, bg: &str) -> f64 {
    contrast_ratio_rgb(parse_hex_color(fg), parse_hex_color(bg))
}

/// Ratio de contraste WCAG 2.1 entre dos colores RGB.
pub fn contrast_ratio_rgb(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> f64 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Luminancia relativa WCAG 2.1 de un color sRGB.
pub fn relative_luminance(rgb: (u8, u8, u8)) -> f64 {
    let r = srgb_to_linear(f64::from(rgb.0) / 255.0);
    let g = srgb_to_linear(f64::from(rgb.1) / 255.0);
    let b = srgb_to_linear(f64::from(rgb.2) / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Convierte un canal sRGB codificado (0..=1) a su valor lineal (IEC 61966-2-1).
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    crate::config::parse_hex(hex)
}

/// Colores por defecto de una sesión: lo que Baud responde a una consulta OSC
/// mientras la aplicación no haya fijado nada, y aquello a lo que vuelve un
/// OSC de reset.
///
/// Sólo guarda los 16 primeros índices porque del 16 al 255 la paleta es fija
/// (cubo 6×6×6 y rampa de grises) y no depende del tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultColors {
    pub foreground: (u8, u8, u8),
    pub background: (u8, u8, u8),
    pub cursor: (u8, u8, u8),
    pub ansi: [(u8, u8, u8); 16],
}

impl Default for DefaultColors {
    /// Valores neutros estilo xterm. Sólo los ve un `Term::new()` sin config,
    /// es decir los tests: cada sesión real pasa por
    /// `Config::apply_to_term`, que los sobrescribe con el tema activo.
    fn default() -> Self {
        Self {
            foreground: (0xff, 0xff, 0xff),
            background: (0x00, 0x00, 0x00),
            cursor: (0xff, 0xff, 0xff),
            ansi: [
                (0x00, 0x00, 0x00),
                (0xcd, 0x00, 0x00),
                (0x00, 0xcd, 0x00),
                (0xcd, 0xcd, 0x00),
                (0x00, 0x00, 0xee),
                (0xcd, 0x00, 0xcd),
                (0x00, 0xcd, 0xcd),
                (0xe5, 0xe5, 0xe5),
                (0x7f, 0x7f, 0x7f),
                (0xff, 0x00, 0x00),
                (0x00, 0xff, 0x00),
                (0xff, 0xff, 0x00),
                (0x5c, 0x5c, 0xff),
                (0xff, 0x00, 0xff),
                (0x00, 0xff, 0xff),
                (0xff, 0xff, 0xff),
            ],
        }
    }
}

impl DefaultColors {
    /// Construye los defaults desde un tema resuelto.
    pub fn from_theme(theme: &crate::config::ThemeConfig) -> Self {
        let hex = crate::config::parse_hex;
        Self {
            foreground: hex(&theme.foreground),
            background: hex(&theme.background),
            cursor: hex(&theme.cursor),
            ansi: [
                hex(&theme.black),
                hex(&theme.red),
                hex(&theme.green),
                hex(&theme.yellow),
                hex(&theme.blue),
                hex(&theme.magenta),
                hex(&theme.cyan),
                hex(&theme.white),
                hex(&theme.bright_black),
                hex(&theme.bright_red),
                hex(&theme.bright_green),
                hex(&theme.bright_yellow),
                hex(&theme.bright_blue),
                hex(&theme.bright_magenta),
                hex(&theme.bright_cyan),
                hex(&theme.bright_white),
            ],
        }
    }

    /// Resuelve un índice 0-255 según ISO-8613-3: 0-15 del tema, 16-231 cubo
    /// 6×6×6, 232-255 rampa de 24 grises.
    pub fn indexed(&self, n: u8) -> (u8, u8, u8) {
        match n {
            0..=15 => self.ansi[n as usize],
            16..=231 => {
                let idx = n - 16;
                let r = idx / 36;
                let g = (idx % 36) / 6;
                let b = idx % 6;
                (r * 51, g * 51, b * 51)
            }
            232..=255 => {
                let nivel = n - 232;
                let gris = nivel * 10 + 8;
                (gris, gris, gris)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeConfig;

    #[test]
    fn black_on_white_high_contrast() {
        let ratio = contrast_ratio_rgb((0, 0, 0), (255, 255, 255));
        assert!(ratio >= 20.0);
    }

    #[test]
    fn hex_matches_rgb() {
        assert!(
            (contrast_ratio_hex("#000000", "#ffffff")
                - contrast_ratio_rgb((0, 0, 0), (255, 255, 255)))
            .abs()
                < 1e-6
        );
    }

    #[test]
    fn default_colors_toma_los_16_ansi_del_tema() {
        let theme = ThemeConfig::default();
        let d = DefaultColors::from_theme(&theme);
        assert_eq!(d.foreground, crate::config::parse_hex(&theme.foreground));
        assert_eq!(d.background, crate::config::parse_hex(&theme.background));
        assert_eq!(d.cursor, crate::config::parse_hex(&theme.cursor));
        assert_eq!(d.ansi[1], crate::config::parse_hex(&theme.red));
        assert_eq!(d.ansi[15], crate::config::parse_hex(&theme.bright_white));
    }

    #[test]
    fn default_colors_indexed_cubre_los_256() {
        let d = DefaultColors::from_theme(&ThemeConfig::default());
        // 0..16 salen del tema.
        assert_eq!(d.indexed(9), d.ansi[9]);
        // Cubo 6x6x6: el 16 es negro y el 231 blanco.
        assert_eq!(d.indexed(16), (0, 0, 0));
        assert_eq!(d.indexed(231), (255, 255, 255));
        // Rampa de grises.
        assert_eq!(d.indexed(232), (8, 8, 8));
        assert_eq!(d.indexed(255), (238, 238, 238));
    }

    #[test]
    fn default_colors_indexed_coincide_con_el_renderer() {
        // Dos caminos resuelven la paleta 256 (este y `ansi_256_to_rgb` del
        // renderer, que esta en el camino caliente por celda). Este test es lo
        // que impide que se separen.
        let theme = ThemeConfig::default();
        let d = DefaultColors::from_theme(&theme);
        for n in 0..=255u8 {
            assert_eq!(
                d.indexed(n),
                crate::renderer::ansi_256_to_rgb_for_test(n, &theme),
                "indice {n}"
            );
        }
    }
}
