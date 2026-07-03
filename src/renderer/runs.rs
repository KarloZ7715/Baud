//! Agrupacion de secuencias ligables y shaping multi-caracter.

use glyphon::cosmic_text::{Metrics, Shaping, Style, Weight};
use glyphon::CacheKey;

use crate::grid::Cell;

use super::builtin::is_box_glyph;
use super::metrics::CellMetrics;
use super::resolve_family;

/// Secuencias tipograficas habituales en fuentes con ligaduras (Fira Code, etc.).
/// Ordenadas de mayor a menor longitud para greedy match.
const LIGATURE_PATTERNS: &[&str] = &[
    "...", "!==", "===", "==", "!=", ">=", "<=", "=>", "->", "<-", "::", "&&", "||", "//", "/*",
    "*/", "..",
];

#[derive(Debug, Clone, PartialEq)]
pub struct RunGlyph {
    pub cache_key: CacheKey,
    /// Primera columna del cluster dentro del run (0..run.cols).
    pub col_in_run: usize,
    pub top: f32,
    pub line_y: f32,
    pub width: f32,
    pub height: f32,
}

/// Shapea una secuencia corta con ligaduras, ancho monospace por celda.
pub fn shape_run(
    font_system: &mut glyphon::FontSystem,
    metrics: &CellMetrics,
    family: &str,
    text: &str,
    bold: bool,
    italic: bool,
    dim: bool,
) -> Vec<RunGlyph> {
    let ct = Metrics::new(metrics.font_size, metrics.cell_h);
    let mut buf = glyphon::Buffer::new(font_system, ct);
    buf.set_monospace_width(font_system, Some(metrics.cell_w));
    let run_cols = text.chars().count().max(1) as f32;
    buf.set_size(
        font_system,
        Some(metrics.cell_w * run_cols),
        Some(metrics.cell_h),
    );

    let mut attrs = glyphon::Attrs::new().family(resolve_family(family));
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    } else if dim {
        attrs = attrs.weight(Weight::LIGHT);
    }
    if italic {
        attrs = attrs.style(Style::Italic);
    }

    buf.set_text(font_system, text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(font_system, false);

    let mut out = Vec::new();
    if let Some(run) = buf.layout_runs().next() {
        let line_y = run.line_y;
        for g in run.glyphs.iter() {
            let physical = g.physical((0.0, line_y), 1.0);
            let byte_start = g.start.min(text.len());
            let col_in_run = text[..byte_start].chars().count();
            out.push(RunGlyph {
                cache_key: physical.cache_key,
                col_in_run,
                top: physical.y as f32,
                line_y,
                width: g.w,
                height: run.line_height,
            });
        }
    }
    out
}

/// Convierte un glifo shaped de run al formato de cache por celda.
pub fn run_glyph_to_shaped(g: &RunGlyph) -> super::glyph::ShapedGlyph {
    super::glyph::ShapedGlyph {
        cache_key: g.cache_key,
        bitmap_w: g.width,
        bitmap_h: g.height,
        left: 0.0,
        top: g.top,
        line_y: g.line_y,
        advance: g.width,
        used_bold_fallback: false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LigRun {
    pub start_col: usize,
    pub text: String,
    /// Columnas cubiertas (== text.chars().count()).
    pub cols: usize,
}

/// True si la celda puede participar en una secuencia ligable.
pub fn is_ligable_cell(cell: &Cell) -> bool {
    if cell.width != 1 || cell.ch == ' ' || cell.ch == '\0' || is_box_glyph(cell.ch) {
        return false;
    }
    // Iconos Powerline/Nerd Font (PUA): siempre per-celda.
    let u = cell.ch as u32;
    !(0xE000..=0xF8FF).contains(&u)
}

fn same_style(a: &Cell, b: &Cell) -> bool {
    a.attrs == b.attrs && a.hyperlink == b.hyperlink
}

fn pattern_matches(
    row: &[Cell],
    start_col: usize,
    pattern: &str,
    is_selected: &impl Fn(usize) -> bool,
) -> bool {
    let sel = is_selected(start_col);
    for (i, ch) in pattern.chars().enumerate() {
        let col = start_col + i;
        let Some(cell) = row.get(col) else {
            return false;
        };
        if cell.ch != ch || !is_ligable_cell(cell) {
            return false;
        }
        if i > 0 && !same_style(&row[start_col], cell) {
            return false;
        }
        if is_selected(col) != sel {
            return false;
        }
    }
    true
}

/// Detecta solo secuencias que forman ligaduras tipograficas (no runs de estilo homogeneo).
pub fn group_ligature_runs(
    row: &[Cell],
    cols: usize,
    is_selected: impl Fn(usize) -> bool,
) -> Vec<LigRun> {
    let mut runs = Vec::new();
    let mut col = 0;
    let limit = cols.min(row.len());
    while col < limit {
        if !is_ligable_cell(&row[col]) {
            col += 1;
            continue;
        }
        let mut matched = false;
        for pattern in LIGATURE_PATTERNS {
            let plen = pattern.chars().count();
            if col + plen > limit {
                continue;
            }
            if pattern_matches(row, col, pattern, &is_selected) {
                runs.push(LigRun {
                    start_col: col,
                    text: pattern.to_string(),
                    cols: plen,
                });
                col += plen;
                matched = true;
                break;
            }
        }
        if !matched {
            col += 1;
        }
    }
    runs
}

/// True si `col` pertenece a alguna secuencia ligable detectada.
pub fn in_ligature_run(col: usize, runs: &[LigRun]) -> bool {
    runs.iter()
        .any(|r| (r.start_col..r.start_col + r.cols).contains(&col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;

    #[test]
    fn detecta_solo_secuencia_ligadura() {
        let mut row = vec![Cell::default(); 6];
        for (i, ch) in "a=>b".chars().enumerate() {
            row[i].ch = ch;
        }
        row[3].attrs.fg = crate::ansi::Color::Red;
        let runs = group_ligature_runs(&row, 4, |_| false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start_col, 1);
        assert_eq!(runs[0].text, "=>");
    }

    #[test]
    fn seleccion_corta_ligadura() {
        let mut row = vec![Cell::default(); 4];
        for (i, ch) in "x=>y".chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, 4, |col| col >= 2);
        assert!(runs.is_empty());
    }

    #[test]
    fn path_largo_no_genera_run() {
        let path = "~/Documentos/Dev/baud";
        let mut row = vec![Cell::default(); path.chars().count()];
        for (i, ch) in path.chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, row.len(), |_| false);
        assert!(runs.is_empty());
    }

    #[test]
    fn pua_no_es_ligable() {
        let cell = Cell {
            ch: '\u{E0B0}',
            ..Default::default()
        };
        assert!(!is_ligable_cell(&cell));
    }

    #[test]
    fn in_ligature_run_cubre_rango() {
        let runs = vec![LigRun {
            start_col: 2,
            text: "=>".into(),
            cols: 2,
        }];
        assert!(in_ligature_run(2, &runs));
        assert!(in_ligature_run(3, &runs));
        assert!(!in_ligature_run(4, &runs));
    }

    #[test]
    fn shape_run_de_fira_genera_glifos() {
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let fam = crate::config::FontConfig::default().family;
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            &fam,
            14.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let glyphs = shape_run(&mut fs, &m, &fam, "=>", false, false, false);
        assert!(!glyphs.is_empty());
        assert_eq!(glyphs[0].col_in_run, 0);
    }
}
