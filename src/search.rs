//! Busqueda de texto en filas (scrollback + grid).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub row: usize,
    pub col: usize,
    pub len: usize,
}

/// Estado de busqueda activa en el terminal.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub case_insensitive: bool,
    pub matches: Vec<Match>,
    pub current: usize,
    /// Tras Enter el query queda fijado y n/N navegan sin capturar letras.
    pub committed: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Busca todas las ocurrencias de `query` en `rows` (cada string es una fila ya
/// renderizada a texto). `col`/`len` en caracteres (no bytes).
pub fn find_matches(rows: &[String], query: &str, case_insensitive: bool) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle = if case_insensitive {
        query.to_lowercase()
    } else {
        query.to_string()
    };
    let qlen = query.chars().count();
    let nchars: Vec<char> = needle.chars().collect();
    if nchars.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (row, line) in rows.iter().enumerate() {
        let hay = if case_insensitive {
            line.to_lowercase()
        } else {
            line.clone()
        };
        let chars: Vec<char> = hay.chars().collect();
        if chars.len() < nchars.len() {
            continue;
        }
        let mut i = 0;
        while i + nchars.len() <= chars.len() {
            if chars[i..i + nchars.len()] == nchars[..] {
                out.push(Match {
                    row,
                    col: i,
                    len: qlen,
                });
                i += nchars.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Convierte indices de caracteres en una fila de celdas a rango de columnas del grid.
pub fn char_range_to_cols(
    cells: &[crate::grid::Cell],
    char_start: usize,
    char_len: usize,
) -> (usize, usize) {
    let mut ci = 0usize;
    let mut start_col = 0usize;
    let mut end_col = 0usize;
    let mut found_start = false;
    for (col, cell) in cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        if !found_start && ci == char_start {
            start_col = col;
            found_start = true;
        }
        if found_start && ci == char_start.saturating_add(char_len).saturating_sub(1) {
            end_col = col + usize::from(cell.width);
            return (start_col, end_col);
        }
        ci += 1;
    }
    if found_start {
        (start_col, end_col.max(start_col + 1))
    } else {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_matches_en_varias_filas() {
        let filas = vec![
            "error: foo".to_string(),
            "ok".to_string(),
            "another error here".to_string(),
        ];
        let matches = find_matches(&filas, "error", false);
        assert_eq!(
            matches,
            vec![
                Match {
                    row: 0,
                    col: 0,
                    len: 5
                },
                Match {
                    row: 2,
                    col: 8,
                    len: 5
                },
            ]
        );
    }

    #[test]
    fn case_insensitive() {
        let filas = vec!["ERROR".to_string()];
        assert_eq!(find_matches(&filas, "error", true).len(), 1);
        assert_eq!(find_matches(&filas, "error", false).len(), 0);
    }

    #[test]
    fn query_vacia_no_matchea() {
        assert!(find_matches(&["abc".into()], "", false).is_empty());
    }
}
