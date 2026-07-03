//! Agrupacion de celdas en runs ligables para shaping multi-caracter.

use glyphon::cosmic_text::{Metrics, Shaping, Style, Weight};
use glyphon::CacheKey;

use crate::grid::Cell;

use super::builtin::is_box_glyph;
use super::metrics::CellMetrics;
use super::resolve_family;

#[derive(Debug, Clone, PartialEq)]
pub struct RunGlyph {
    pub cache_key: CacheKey,
    /// Offset desde el inicio del run, en px.
    pub x: f32,
    pub top: f32,
    pub line_y: f32,
    pub width: f32,
    pub height: f32,
}

/// Shapea un run completo con ligaduras (sin ancho monospace forzado por char).
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
    buf.set_size(font_system, None, Some(metrics.cell_h));

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
            out.push(RunGlyph {
                cache_key: physical.cache_key,
                x: g.x,
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
    /// Columnas cubiertas (== text.chars().count() para runs no-wide).
    pub cols: usize,
}

/// True si la celda puede ir en un run ligable.
fn ligable(cell: &Cell) -> bool {
    cell.width == 1 && cell.ch != ' ' && cell.ch != '\0' && !is_box_glyph(cell.ch)
}

/// Mismos atributos visuales => mismo run.
fn same_style(a: &Cell, b: &Cell) -> bool {
    a.attrs == b.attrs && a.hyperlink == b.hyperlink
}

/// Agrupa runs ligables. `is_selected(col)` corta el run en fronteras de seleccion.
pub fn group_runs(row: &[Cell], cols: usize, is_selected: impl Fn(usize) -> bool) -> Vec<LigRun> {
    let mut runs = Vec::new();
    let mut cur: Option<LigRun> = None;
    let mut prev_idx: Option<usize> = None;
    for col in 0..cols.min(row.len()) {
        let cell = &row[col];
        let ok = ligable(cell)
            && prev_idx
                .map(|p| same_style(&row[p], cell) && is_selected(p) == is_selected(col))
                .unwrap_or(true);
        if ok {
            let run = cur.get_or_insert(LigRun {
                start_col: col,
                text: String::new(),
                cols: 0,
            });
            run.text.push(cell.ch);
            run.cols += 1;
            prev_idx = Some(col);
        } else {
            if let Some(r) = cur.take() {
                runs.push(r);
            }
            if ligable(cell) {
                cur = Some(LigRun {
                    start_col: col,
                    text: cell.ch.to_string(),
                    cols: 1,
                });
                prev_idx = Some(col);
            } else {
                prev_idx = None;
            }
        }
    }
    if let Some(r) = cur.take() {
        runs.push(r);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;

    #[test]
    fn agrupa_run_homogeneo_y_corta_en_cambio_de_color() {
        let mut row = vec![Cell::default(); 6];
        for (i, ch) in "a=>b".chars().enumerate() {
            row[i].ch = ch;
        }
        row[3].attrs.fg = crate::ansi::Color::Red;
        let runs = group_runs(&row, 4, |_| false);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].start_col, 0);
        assert_eq!(runs[0].text, "a=>");
        assert_eq!(runs[1].text, "b");
    }

    #[test]
    fn wide_y_box_rompen_run() {
        let mut row = vec![Cell::default(); 4];
        row[0].ch = '─';
        row[1].ch = 'a';
        row[2].ch = 'b';
        let runs = group_runs(&row, 3, |_| false);
        assert!(runs.iter().any(|r| r.text == "ab"));
        assert!(runs.iter().all(|r| !r.text.contains('─')));
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
    }
}
