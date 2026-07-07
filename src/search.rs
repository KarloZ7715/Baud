//! Busqueda de texto en filas (scrollback + grid).

use std::collections::HashMap;

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
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Rango de columnas resaltado en una fila visible.
#[derive(Debug, Clone, Copy)]
pub struct SearchColRange {
    pub start_col: usize,
    pub end_col: usize,
    pub is_current: bool,
}

/// Cache de rangos de resaltado por fila visible (una reconstruccion por frame).
#[derive(Debug, Clone, Default)]
pub struct SearchRenderCache {
    pub scrollback_offset: isize,
    pub rows_count: usize,
    pub match_count: usize,
    pub current: usize,
    pub visible_rows: Vec<Vec<SearchColRange>>,
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

/// Texto del formato de la barra inferior de busqueda.
pub fn format_bar(state: &SearchState) -> String {
    let case_lbl = if state.case_insensitive { "aa" } else { "Aa" };
    let counter = if state.matches.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.current + 1, state.matches.len())
    };
    format!(
        "/{}  {counter}  {case_lbl}  ↑↓ nav · Alt+C case · Ctrl+U limpiar · Esc salir",
        state.query
    )
}

/// Reconstruye el cache de resaltado para las filas visibles del viewport.
pub fn build_render_cache(term: &crate::ansi::Term, state: &SearchState) -> SearchRenderCache {
    let rows_count = term.grid.rows_count;
    let mut visible_rows = vec![Vec::new(); rows_count];
    if state.matches.is_empty() {
        return SearchRenderCache {
            scrollback_offset: term.scrollback_offset,
            rows_count,
            match_count: 0,
            current: state.current,
            visible_rows,
        };
    }

    let mut logical_cells: HashMap<usize, Vec<crate::grid::Cell>> = HashMap::new();
    for (i, m) in state.matches.iter().enumerate() {
        let Some(vis_row) = term.logical_to_visible_row(m.row) else {
            continue;
        };
        if vis_row >= rows_count {
            continue;
        }
        let cells = logical_cells
            .entry(m.row)
            .or_insert_with(|| term.row_cells_at_logical(m.row).unwrap_or_default());
        let (start_col, end_col) = char_range_to_cols(cells, m.col, m.len);
        visible_rows[vis_row].push(SearchColRange {
            start_col,
            end_col,
            is_current: i == state.current,
        });
    }

    SearchRenderCache {
        scrollback_offset: term.scrollback_offset,
        rows_count,
        match_count: state.matches.len(),
        current: state.current,
        visible_rows,
    }
}

/// Consulta el cache para saber si una celda visible es parte de un match.
pub fn hit_at(cache: &SearchRenderCache, visible_row: usize, col: usize) -> Option<bool> {
    let ranges = cache.visible_rows.get(visible_row)?;
    for range in ranges {
        if col >= range.start_col && col < range.end_col {
            return Some(range.is_current);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;

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

    #[test]
    fn char_range_to_cols_wide_char() {
        let mut cells = vec![Cell::default(); 5];
        cells[1].ch = '中';
        cells[1].width = 2;
        cells[2].width = 0;
        cells[3].ch = 'x';
        cells[3].width = 1;
        assert_eq!(char_range_to_cols(&cells, 0, 1), (0, 1));
        assert_eq!(char_range_to_cols(&cells, 1, 1), (1, 3));
        assert_eq!(char_range_to_cols(&cells, 2, 1), (3, 4));
    }

    #[test]
    fn format_bar_incluye_query_y_contador() {
        let state = SearchState {
            query: "error".into(),
            matches: vec![Match {
                row: 0,
                col: 0,
                len: 5,
            }],
            current: 0,
            ..Default::default()
        };
        let bar = format_bar(&state);
        assert!(bar.starts_with("/error"));
        assert!(bar.contains("1/1"));
        assert!(bar.contains("Aa"));
    }

    #[test]
    fn format_bar_case_insensitive() {
        let state = SearchState {
            query: "x".into(),
            case_insensitive: true,
            ..Default::default()
        };
        assert!(format_bar(&state).contains("aa"));
    }
}
