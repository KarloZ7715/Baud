pub mod keymap;

/// Filtra bytes problematicos (ESC 0x1B, ETX 0x03) del texto pegado.
/// Retorna Vec<u8> con los bytes filtrados, listos para enviar al PTY.
// ponytail: solo filtra ESC y ETX por MVP. 0x1A y 0x04 por implementar si se requiere.
pub fn paste_text(text: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());
    for &byte in text.as_bytes() {
        if byte != 0x1B && byte != 0x03 {
            result.push(byte);
        }
    }
    result
}

/// Envuelve el texto en marcadores de bracketed paste si el flag esta activo.
/// Si no, retorna el texto sin cambios (solo filtrado).
// ponytail: wrapping para DEC 2004, anidamiento no permitido por xterm.
pub fn paste_with_bracketing(text: &str, bracketed: bool) -> Vec<u8> {
    let filtered = paste_text(text);
    if bracketed {
        let mut out = Vec::with_capacity(filtered.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend(filtered);
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paste_text_passes_normal_chars() {
        assert_eq!(paste_text("hello world"), b"hello world");
    }
    #[test]
    fn test_paste_text_filters_esc() {
        assert_eq!(paste_text("hello\x1besc"), b"helloesc");
    }
    #[test]
    fn test_paste_text_filters_etx() {
        assert_eq!(paste_text("hello\x03world"), b"helloworld");
    }

    #[test]
    fn test_paste_with_bracketing_wraps_when_active() {
        assert_eq!(paste_with_bracketing("hi", true), b"\x1b[200~hi\x1b[201~");
    }
    #[test]
    fn test_paste_with_bracketing_passthrough_when_inactive() {
        assert_eq!(paste_with_bracketing("hi", false), b"hi");
    }
}
