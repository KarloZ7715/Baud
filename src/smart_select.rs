//! Selección semántica (smart selection).
//!
//! Detección por prioridad: URL, path, email, y fallback a palabra con
//! delimitadores configurables. Opera sobre una fila de celdas del grid.

use crate::grid::Cell;

/// Resultado de expansión smart: rango (start_col, end_col) inclusivo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmartRange {
    pub start: usize,
    pub end: usize,
}

/// Expande la selección en `col` aplicando reglas semánticas.
/// Devuelve None si `col` cae en un separador sin patrón reconocido.
pub fn expand_smart(row_cells: &[Cell], col: usize, delimiters: &str) -> Option<SmartRange> {
    if row_cells.is_empty() || col >= row_cells.len() {
        return None;
    }
    let chars: Vec<char> = row_cells.iter().map(|c| c.ch).collect();
    let line: String = chars.iter().collect();

    // 1. URL (esquema, www., dominio con TLD)
    if let Some(r) = find_url(&line, col) {
        return Some(r);
    }
    // 2. Email
    if let Some(r) = find_email(&line, col) {
        return Some(r);
    }
    // 3. Path (contiene '/' o empieza con ~ o ./ .. )
    if let Some(r) = find_path(&line, col) {
        return Some(r);
    }
    // 4. Fallback: palabra con delimitadores
    expand_word(&chars, col, delimiters).map(|(s, e)| SmartRange { start: s, end: e })
}

/// Devuelve la URL que contiene `col` en `line`, si la hay.
pub fn resolve_url_in_line(line: &str, col: usize) -> Option<String> {
    resolve_url_with_range(line, col).map(|(url, _)| url)
}

/// Devuelve URL y rango en un solo paso (evita doble escaneo de `find_url`).
pub fn resolve_url_with_range(line: &str, col: usize) -> Option<(String, SmartRange)> {
    let r = find_url(line, col)?;
    let url = line
        .chars()
        .skip(r.start)
        .take(r.end - r.start + 1)
        .collect();
    Some((url, r))
}

/// Rango de la URL que contiene `col` en `line`.
pub fn url_range_in_line(line: &str, col: usize) -> Option<SmartRange> {
    find_url(line, col)
}

/// Normaliza una URL detectada para abrirla con xdg-open.
/// Sin esquema web (`www.` o dominio con TLD) recibe prefijo `https://`.
pub fn normalize_url_for_open(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if ["http://", "https://", "ftp://", "file://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with("www.") || looks_like_bare_domain(trimmed) {
        return Some(format!("https://{trimmed}"));
    }
    None
}

/// Detecta una URL que contiene `col`.
fn find_url(line: &str, col: usize) -> Option<SmartRange> {
    find_scheme_url(line, col)
        .or_else(|| find_www_url(line, col))
        .or_else(|| find_bare_domain(line, col))
}

fn find_scheme_url(line: &str, col: usize) -> Option<SmartRange> {
    for scheme in ["https://", "http://", "ftp://", "file://"] {
        let bytes = scheme.as_bytes();
        let mut start = 0;
        while let Some(idx) = line[start..].find(scheme) {
            let abs = start + idx;
            let mut end = abs + bytes.len();
            let chars: Vec<char> = line.chars().collect();
            while end < chars.len() && !is_url_boundary(chars[end]) {
                end += 1;
            }
            if col >= abs && col < end {
                return Some(SmartRange {
                    start: abs,
                    end: end - 1,
                });
            }
            start = abs + bytes.len();
        }
    }
    None
}

fn find_www_url(line: &str, col: usize) -> Option<SmartRange> {
    const PREFIX: &str = "www.";
    let chars: Vec<char> = line.chars().collect();
    let mut start = 0;
    while let Some(idx) = line[start..].find(PREFIX) {
        let abs = start + idx;
        if abs > 0 {
            let prev = line[..abs].chars().next_back()?;
            if !is_url_token_boundary_before(prev) {
                start = abs + PREFIX.len();
                continue;
            }
        }
        let mut end = abs + PREFIX.len();
        while end < chars.len() && !is_url_boundary(chars[end]) {
            end += 1;
        }
        if col >= abs && col < end {
            let token: String = chars[abs..end].iter().collect();
            if looks_like_bare_domain(token.strip_prefix(PREFIX).unwrap_or(&token)) {
                return Some(SmartRange {
                    start: abs,
                    end: end - 1,
                });
            }
        }
        start = abs + PREFIX.len();
    }
    None
}

fn find_bare_domain(line: &str, col: usize) -> Option<SmartRange> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_url_boundary(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !is_url_boundary(chars[i]) {
            i += 1;
        }
        let token: String = chars[start..i].iter().collect();
        if !token.starts_with("www.") && looks_like_bare_domain(&token) && col >= start && col < i {
            return Some(SmartRange { start, end: i - 1 });
        }
    }
    None
}

/// Heuristica de dominio con TLD (sin esquema): `karloz.dev`, `unicordoba.edu.co`.
fn looks_like_bare_domain(token: &str) -> bool {
    let host = url_token_host(token);
    if host.is_empty() || host.starts_with('-') {
        return false;
    }
    if host.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    if host.contains('@') {
        return false;
    }
    if !host.contains('.') {
        return false;
    }
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return false;
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    if labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let tld = labels.last().copied().unwrap_or("");
    if tld.len() < 2 || !tld.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    labels.iter().all(|label| {
        !label.starts_with('-')
            && !label.ends_with('-')
            && label.len() <= 63
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn domain_host_part(token: &str) -> &str {
    if let Some((host, port)) = token.rsplit_once(':') {
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return host;
        }
    }
    token
}

fn url_token_host(token: &str) -> &str {
    domain_host_part(token.split(&['/', '?', '#']).next().unwrap_or(token))
}

fn is_url_token_boundary_before(c: char) -> bool {
    is_url_boundary(c) || c == '/'
}

fn is_url_boundary(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '`'
        )
}

/// Detecta un email que contiene `col` (heurística: local@domain).
fn find_email(line: &str, col: usize) -> Option<SmartRange> {
    let chars: Vec<char> = line.chars().collect();
    // Buscar el @ más cercano a col.
    let mut at = None;
    for (i, &c) in chars.iter().enumerate() {
        if c == '@' && i.abs_diff(col) < 128 {
            at = Some(i);
            break;
        }
    }
    let at = at?;
    if at == 0 || at + 1 >= chars.len() {
        return None;
    }
    // Local parte: hacia atrás hasta whitespace/@.
    let mut start = at;
    while start > 0 && !chars[start - 1].is_whitespace() && chars[start - 1] != '@' {
        start -= 1;
    }
    // Dominio: hacia adelante hasta whitespace.
    let mut end = at + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    if start < at && end > at + 1 && col >= start && col < end {
        Some(SmartRange {
            start,
            end: end - 1,
        })
    } else {
        None
    }
}

/// Detecta un path que contiene `col`: secuencia sin espacios que incluye '/'
/// o empieza con '~', './', '../'.
fn find_path(line: &str, col: usize) -> Option<SmartRange> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() || col >= chars.len() {
        return None;
    }
    // Extender token sin espacios alrededor de col.
    let mut start = col;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && !chars[end + 1].is_whitespace() {
        end += 1;
    }
    let token: String = chars[start..=end].iter().collect();
    let is_path = token.contains('/')
        || token.starts_with('~')
        || token.starts_with("./")
        || token.starts_with("../");
    if is_path && col >= start && col <= end {
        Some(SmartRange { start, end })
    } else {
        None
    }
}

/// Fallback: palabra delimitada por `delimiters` o whitespace.
fn expand_word(chars: &[char], col: usize, delimiters: &str) -> Option<(usize, usize)> {
    let is_delim = |c: char| c.is_whitespace() || delimiters.contains(c);
    if col >= chars.len() || is_delim(chars[col]) {
        return None;
    }
    let mut start = col;
    while start > 0 && !is_delim(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && !is_delim(chars[end + 1]) {
        end += 1;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(s: &str) -> Vec<Cell> {
        s.chars()
            .map(|c| Cell {
                ch: c,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn test_smart_url() {
        let s = "see https://example.com/path?x=1 now";
        let cells = cells(s);
        // col dentro de la URL
        let r = expand_smart(&cells, 10, " ").unwrap();
        assert_eq!(&s[r.start..=r.end], "https://example.com/path?x=1");
    }

    #[test]
    fn test_smart_path() {
        let s = "open /home/user/foo/bar.txt please";
        let cells = cells(s);
        let r = expand_smart(&cells, 10, " ").unwrap();
        assert_eq!(&s[r.start..=r.end], "/home/user/foo/bar.txt");
    }

    #[test]
    fn test_smart_relative_path() {
        let s = "run ./src/main.rs now";
        let cells = cells(s);
        let r = expand_smart(&cells, 6, " ").unwrap();
        assert_eq!(&s[r.start..=r.end], "./src/main.rs");
    }

    #[test]
    fn test_smart_email() {
        let s = "mail me at user@example.com thanks";
        let cells = cells(s);
        let r = expand_smart(&cells, 15, " ").unwrap();
        assert_eq!(&s[r.start..=r.end], "user@example.com");
    }

    #[test]
    fn test_smart_fallback_word() {
        let s = "hello world foo";
        let cells = cells(s);
        let r = expand_smart(&cells, 1, " ").unwrap();
        assert_eq!(&s[r.start..=r.end], "hello");
    }

    #[test]
    fn test_smart_on_delimiter_returns_none() {
        let s = "a b c";
        let cells = cells(s);
        // col 1 es espacio
        assert!(expand_smart(&cells, 1, " ").is_none());
    }

    #[test]
    fn test_smart_empty_and_oob() {
        let empty: Vec<Cell> = vec![];
        assert!(expand_smart(&empty, 0, " ").is_none());
        let cells = cells("hi");
        assert!(expand_smart(&cells, 100, " ").is_none());
    }

    #[test]
    fn resuelve_url_por_smart_select() {
        let line = "ver https://example.com/p?x=1 ahora";
        let r = resolve_url_in_line(line, 10).unwrap();
        assert_eq!(r, "https://example.com/p?x=1");
    }

    #[test]
    fn fuera_de_url_es_none() {
        assert!(resolve_url_in_line("hola mundo", 2).is_none());
    }

    #[test]
    fn detecta_www_sin_esquema() {
        let line = "visita www.example.com/path hoy";
        let r = resolve_url_in_line(line, 10).unwrap();
        assert_eq!(r, "www.example.com/path");
    }

    #[test]
    fn detecta_dominio_suelto_con_tld() {
        let line = "ver karloz.dev y unicordoba.edu.co";
        assert_eq!(resolve_url_in_line(line, 5).unwrap(), "karloz.dev");
        assert_eq!(resolve_url_in_line(line, 20).unwrap(), "unicordoba.edu.co");
    }

    #[test]
    fn dominio_con_puerto() {
        let line = "api karloz.dev:8080/health";
        assert_eq!(
            resolve_url_in_line(line, 6).unwrap(),
            "karloz.dev:8080/health"
        );
    }

    #[test]
    fn no_confunde_version_con_dominio() {
        assert!(resolve_url_in_line("version 1.2.3 ok", 8).is_none());
    }

    #[test]
    fn no_confunde_palabra_sin_tld() {
        assert!(resolve_url_in_line("hello world", 1).is_none());
    }

    #[test]
    fn normalize_url_for_open_agrega_https() {
        assert_eq!(
            normalize_url_for_open("karloz.dev").as_deref(),
            Some("https://karloz.dev")
        );
        assert_eq!(
            normalize_url_for_open("www.example.com").as_deref(),
            Some("https://www.example.com")
        );
        assert_eq!(
            normalize_url_for_open("https://ok.dev").as_deref(),
            Some("https://ok.dev")
        );
        assert!(normalize_url_for_open("javascript:alert(1)").is_none());
    }
}
