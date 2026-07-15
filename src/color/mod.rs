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
    fn channel(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = channel(rgb.0);
    let g = channel(rgb.1);
    let b = channel(rgb.2);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    crate::config::parse_hex(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
