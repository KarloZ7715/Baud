//! Deteccion de fronteras de grafema (UAX #29) para agrupar secuencias
//! Unicode multi-codepoint en una sola celda de terminal.

use unicode_segmentation::GraphemeCursor;

/// `true` si `c` extiende el cluster en construccion `pending` (no debe
/// empezar una celda nueva). Con `pending` vacio siempre devuelve `false`.
pub fn extends_last_cluster(pending: &str, c: char) -> bool {
    if pending.is_empty() {
        return false;
    }
    // Ningun codepoint ASCII es Extend/ZWJ/SpacingMark/Prepend/Regional_Indicator
    // ni modificador de emoji, asi que nunca puede continuar un cluster. Cubre
    // la inmensa mayoria del texto de una terminal (logs, comandos, codigo).
    if c.is_ascii() {
        return false;
    }
    // El resto de Latin-1 (acentos precompuestos, simbolos) tampoco continua
    // un cluster que empieza en ASCII salvo que sea un acento combinante
    // (>= U+0300), asi que se resuelve sin pasar por el segmentador.
    if c < '\u{0300}'
        && pending
            .chars()
            .next_back()
            .is_some_and(|last| last.is_ascii())
    {
        return false;
    }

    let mut probe = String::with_capacity(pending.len() + c.len_utf8());
    probe.push_str(pending);
    probe.push(c);
    let mut cursor = GraphemeCursor::new(pending.len(), probe.len(), true);
    // El chunk cubre desde el inicio del string, asi que el cursor siempre
    // tiene contexto suficiente para resolver y nunca pide mas (PreContext).
    let is_boundary = cursor.is_boundary(&probe, 0).unwrap_or(true);
    !is_boundary
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

    #[test]
    fn latin1_no_combinante_tras_ascii_no_extiende() {
        assert!(!extends_last_cluster("a", '\u{00E9}'));
    }
}
