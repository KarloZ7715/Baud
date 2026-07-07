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

    // 1. URL (http://, https://, ftp://, file://)
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
    let r = find_url(line, col)?;
    Some(
        line.chars()
            .skip(r.start)
            .take(r.end - r.start + 1)
            .collect(),
    )
}

/// Rango de la URL que contiene `col` en `line`.
pub fn url_range_in_line(line: &str, col: usize) -> Option<SmartRange> {
    find_url(line, col)
}

/// Detecta una URL que contiene `col`.
fn find_url(line: &str, col: usize) -> Option<SmartRange> {
    for scheme in ["https://", "http://", "ftp://", "file://"] {
        let bytes = scheme.as_bytes();
        let mut start = 0;
        while let Some(idx) = line[start..].find(scheme) {
            let abs = start + idx;
            // La URL va hasta el primer whitespace o delimitador de línea.
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
}
