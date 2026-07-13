//! Sanitización de datos sensibles antes de enviar a Sentry.
//!
//! Reglas:
//! - Reemplaza rutas del home del usuario por `<HOME>`.
//! - Truncado duro de bytes para eventos muy largos.

const MAX_EVENT_BYTES: usize = 4096;
const MAX_BACKTRACE_BYTES: usize = 8192;

/// Reemplaza la ruta del home del usuario por `<HOME>`.
pub fn sanitize_message(msg: &str) -> String {
    let sanitized = sanitize_home_paths(msg);
    truncate_bytes(&sanitized, MAX_EVENT_BYTES)
}

/// Igual que `sanitize_message` pero con un límite mayor para backtraces.
pub fn sanitize_backtrace(bt: &str) -> String {
    let sanitized = sanitize_home_paths(bt);
    truncate_bytes(&sanitized, MAX_BACKTRACE_BYTES)
}

/// Reemplaza apariciones de `$HOME` o su valor real por `<HOME>`.
fn sanitize_home_paths(input: &str) -> String {
    static CACHED_HOME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    let home_str = CACHED_HOME.get_or_init(|| {
        dirs::home_dir()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "/home/unknown".to_string())
    });

    input.replace(home_str.as_str(), "<HOME>")
}

/// Devuelve la posición del carácter UTF-8 más cercano antes o en `max`.
pub fn floor_char_boundary(s: &str, max: usize) -> usize {
    if s.is_char_boundary(max) {
        return max;
    }
    (0..max).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0)
}

/// Trunca una cadena a `max_bytes` bytes en una frontera de carácter válida.
fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let end = floor_char_boundary(s, max_bytes);

    if end == 0 {
        return String::new();
    }

    let truncated = &s[..end];
    format!("{truncated}…[truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reemplaza_home_path() {
        let home = dirs::home_dir()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/home/testuser".to_string());
        let msg = format!("Error en {}/.config/baud/config.toml", home);
        let sanitized = sanitize_message(&msg);
        assert!(sanitized.contains("<HOME>/.config/baud/config.toml"));
        assert!(!sanitized.contains(&home));
    }

    #[test]
    fn trunca_mensaje_largo() {
        let long = "a".repeat(5000);
        let truncated = sanitize_message(&long);
        assert!(truncated.len() <= MAX_EVENT_BYTES + "[truncated]".len() + 3);
        assert!(truncated.ends_with("…[truncated]"));
    }

    #[test]
    fn no_trunca_mensaje_corto() {
        let short = "error simple";
        let result = sanitize_message(short);
        assert_eq!(result, short);
    }

    #[test]
    fn backtrace_tiene_limite_mayor() {
        let long = "a".repeat(10000);
        let truncated = sanitize_backtrace(&long);
        assert!(truncated.len() <= MAX_BACKTRACE_BYTES + "[truncated]".len() + 3);
    }
}
