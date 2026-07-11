//! Deteccion de fronteras de grafema (UAX #29) para agrupar secuencias
//! Unicode multi-codepoint en una sola celda de terminal.

use unicode_segmentation::UnicodeSegmentation;

/// `true` si `c` extiende el cluster en construccion `pending` (no debe
/// empezar una celda nueva). Con `pending` vacio siempre devuelve `false`.
pub fn extends_last_cluster(pending: &str, c: char) -> bool {
    if pending.is_empty() {
        return false;
    }
    let before = pending.graphemes(true).count();
    let mut probe = String::with_capacity(pending.len() + c.len_utf8());
    probe.push_str(pending);
    probe.push(c);
    probe.graphemes(true).count() == before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_nunca_extiende() {
        assert!(!extends_last_cluster("a", 'b'));
    }

    #[test]
    fn combining_accent_extiende_la_base() {
        assert!(extends_last_cluster("e", '\u{0301}'));
    }

    #[test]
    fn zwj_extiende_y_el_siguiente_emoji_tambien() {
        let mut pending = String::from("\u{1F9D1}");
        assert!(extends_last_cluster(&pending, '\u{200D}'));
        pending.push('\u{200D}');
        assert!(extends_last_cluster(&pending, '\u{1F33E}'));
    }

    #[test]
    fn pending_vacio_nunca_extiende() {
        assert!(!extends_last_cluster("", 'a'));
    }
}
