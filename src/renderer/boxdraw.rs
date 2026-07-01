//! Render propio de box-drawing y block elements para alineacion exacta.

/// True si el caracter pertenece a box-drawing o block elements.
pub fn is_box_glyph(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_rangos_box_y_block() {
        assert!(is_box_glyph('\u{2500}'));
        assert!(is_box_glyph('\u{250C}'));
        assert!(is_box_glyph('\u{2588}'));
        assert!(is_box_glyph('\u{2591}'));
        assert!(!is_box_glyph('A'));
        assert!(!is_box_glyph(' '));
    }
}
