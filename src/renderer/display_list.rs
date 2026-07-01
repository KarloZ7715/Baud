//! Display list celda-determinista: fondos y glifos por coordenada de grid.

use crate::ansi::{Color, CursorStyle, Term, UnderlineStyle};
use crate::grid::{Cell, DamageSnapshot};

use super::decorations::cursor_glyph;
use super::glyph::{is_wide_continuation, resolve_glyph_key, GlyphKey};
use super::metrics::CellMetrics;
use super::palette::Palette;
use super::selection_bg_glyphon;

/// Factor de atenuacion RGB para SGR dim (2) cuando `dim_alpha` esta desactivado.
pub const DIM_FACTOR: f32 = 0.6;

/// Tipo de linea decorativa en una celda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Under,
    Strike,
    Over,
}

/// Linea decorativa (subrayado, tachado, overline).
#[derive(Debug, Clone, PartialEq)]
pub struct LineQuad {
    pub row: usize,
    pub col: usize,
    pub width_cells: u8,
    pub kind: LineKind,
    pub style: UnderlineStyle,
    pub color: glyphon::Color,
}

/// Rectangulo de fondo en coordenadas de celda.
#[derive(Debug, Clone, PartialEq)]
pub struct BgQuad {
    pub row: usize,
    pub col: usize,
    pub width_cells: u8,
    pub color: glyphon::Color,
}

/// Cursor DECSCUSR en coordenadas de grid.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorGlyph {
    pub row: usize,
    pub col: usize,
    pub style: CursorStyle,
    pub glyph_key: GlyphKey,
}

/// Glifo de texto posicionado por celda.
#[derive(Debug, Clone, PartialEq)]
pub struct TextGlyph {
    pub row: usize,
    pub col: usize,
    pub width_cells: u8,
    pub glyph_key: GlyphKey,
    pub fg: Color,
    pub bold: bool,
    pub dim: bool,
    pub custom_id: u16,
    pub selected: bool,
    /// True si se rasteriza con box_mask en vez de fuente.
    pub box_glyph: bool,
}

/// Lista de primitivas a pintar para un frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DisplayList {
    pub bg_quads: Vec<BgQuad>,
    pub line_quads: Vec<LineQuad>,
    pub text_glyphs: Vec<TextGlyph>,
    pub cursor: Option<CursorGlyph>,
    pub cursor_bars: Vec<(usize, usize)>,
}

impl DisplayList {
    pub fn clear(&mut self) {
        self.bg_quads.clear();
        self.line_quads.clear();
        self.text_glyphs.clear();
        self.cursor = None;
        self.cursor_bars.clear();
    }
}

/// Elige negro o blanco para texto sobre un fondo RGB dado (luminancia).
pub fn contrast_text_color(bg: (u8, u8, u8)) -> (u8, u8, u8) {
    let lum = 0.299 * f64::from(bg.0) + 0.587 * f64::from(bg.1) + 0.114 * f64::from(bg.2);
    if lum >= 128.0 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

fn rgb_to_glyphon(rgb: (u8, u8, u8)) -> glyphon::Color {
    glyphon::Color::rgb(rgb.0, rgb.1, rgb.2)
}

/// Atenua un color RGB multiplicando por `DIM_FACTOR`.
pub fn attenuate_glyphon(color: glyphon::Color) -> glyphon::Color {
    glyphon::Color::rgba(
        (color.r() as f32 * DIM_FACTOR) as u8,
        (color.g() as f32 * DIM_FACTOR) as u8,
        (color.b() as f32 * DIM_FACTOR) as u8,
        color.a(),
    )
}

/// Resuelve fg a glyphon::Color, aplicando dim si corresponde.
pub fn resolve_fg_glyphon(
    fg: Color,
    dim: bool,
    bold: bool,
    palette: &Palette<'_>,
    dim_alpha: bool,
) -> glyphon::Color {
    let rgb = palette.rgb(fg, bold);
    let color = rgb_to_glyphon(rgb);
    if !dim {
        return color;
    }
    if dim_alpha {
        glyphon::Color::rgba(color.r(), color.g(), color.b(), (DIM_FACTOR * 255.0) as u8)
    } else {
        attenuate_glyphon(color)
    }
}

fn resolve_bg_glyphon(bg: Color, palette: &Palette<'_>, bg_alpha: u8) -> glyphon::Color {
    let (r, g, b) = palette.bg_rgb(bg);
    glyphon::Color::rgba(r, g, b, bg_alpha)
}

fn effective_underline_style(cell: &Cell) -> UnderlineStyle {
    if cell.attrs.underline_style != UnderlineStyle::None {
        cell.attrs.underline_style
    } else if cell.hyperlink.is_some() || cell.attrs.underline {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    }
}

fn underline_color_for_cell(
    fg: Color,
    bold: bool,
    cell: &Cell,
    palette: &Palette<'_>,
    dim_alpha: bool,
) -> glyphon::Color {
    let color = if cell.attrs.underline_color == Color::Default {
        fg
    } else {
        cell.attrs.underline_color
    };
    let mut resolved = resolve_fg_glyphon(color, cell.attrs.dim, bold, palette, dim_alpha);
    // ponytail: hover lo provee window.rs (mouse cell) en el plan de UX
    if cell.hyperlink.is_some() && cell.attrs.underline_color == Color::Default {
        resolved = attenuate_glyphon(resolved);
    }
    resolved
}

/// Construye o actualiza la display list recorriendo celdas visibles.
pub struct DisplayListBuilder;

impl DisplayListBuilder {
    #[expect(
        clippy::too_many_arguments,
        reason = "build context is one logical frame snapshot"
    )]
    pub fn build(
        list: &mut DisplayList,
        term: &Term,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        row_sources: &[&[Cell]],
        cols: usize,
        rows: usize,
        font_family: &str,
        damage: &DamageSnapshot,
        show_scrollback: bool,
        builtin_box_drawing: bool,
    ) {
        if damage.is_full() {
            for row in 0..rows {
                Self::build_row(
                    list,
                    term,
                    metrics,
                    palette,
                    dim_alpha,
                    row_sources,
                    cols,
                    row,
                    font_family,
                    show_scrollback,
                    builtin_box_drawing,
                );
            }
        } else {
            for row in 0..rows {
                if !damage.is_row_dirty(row) {
                    continue;
                }
                Self::clear_row(list, row);
                Self::build_row(
                    list,
                    term,
                    metrics,
                    palette,
                    dim_alpha,
                    row_sources,
                    cols,
                    row,
                    font_family,
                    show_scrollback,
                    builtin_box_drawing,
                );
            }
        }

        Self::build_cursor(
            list,
            term,
            metrics,
            palette,
            cols,
            rows,
            font_family,
            show_scrollback,
        );
    }

    fn cursor_color(palette: &Palette<'_>) -> glyphon::Color {
        rgb_to_glyphon(palette.cursor_rgb())
    }

    fn shell_cursor_here(term: &Term, row: usize, col: usize, show_scrollback: bool) -> bool {
        term.copy_mode.is_none()
            && !show_scrollback
            && term.cursor_visible
            && term.cursor.row == row
            && term.cursor.col == col
    }

    #[allow(clippy::too_many_arguments)]
    fn build_cursor(
        list: &mut DisplayList,
        term: &Term,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        cols: usize,
        rows: usize,
        font_family: &str,
        show_scrollback: bool,
    ) {
        list.cursor = None;
        if let Some(ref cm) = term.copy_mode {
            if let Some(vis_row) = crate::copy_mode::logical_to_visible_row(term, cm.row) {
                if vis_row < rows && cm.col < cols {
                    let color = Self::cursor_color(palette);
                    list.line_quads.push(LineQuad {
                        row: vis_row,
                        col: cm.col,
                        width_cells: 1,
                        kind: LineKind::Under,
                        style: UnderlineStyle::Single,
                        color,
                    });
                    let style = CursorStyle::Underline;
                    let ch = cursor_glyph(style, metrics);
                    list.cursor = Some(CursorGlyph {
                        row: vis_row,
                        col: cm.col,
                        style,
                        glyph_key: GlyphKey {
                            ch,
                            bold: false,
                            italic: false,
                            dim: false,
                            family: font_family.to_string(),
                        },
                    });
                }
            }
        }
        let _ = show_scrollback;
    }

    fn clear_row(list: &mut DisplayList, row: usize) {
        list.bg_quads.retain(|q| q.row != row);
        list.line_quads.retain(|q| q.row != row);
        list.text_glyphs.retain(|g| g.row != row);
        list.cursor_bars.retain(|(r, _)| *r != row);
    }

    #[allow(clippy::too_many_arguments)]
    fn build_row(
        list: &mut DisplayList,
        term: &Term,
        _metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        row_sources: &[&[Cell]],
        cols: usize,
        row: usize,
        font_family: &str,
        show_scrollback: bool,
        builtin_box_drawing: bool,
    ) {
        let source_row = row_sources.get(row).copied().unwrap_or(&[]);
        let cursor_on_row = !show_scrollback
            && term.copy_mode.is_none()
            && term.cursor_visible
            && term.cursor.row == row;
        let row_empty = source_row.is_empty() || source_row.iter().all(|c| *c == Cell::default());
        if row_empty && !cursor_on_row {
            return;
        }

        let max_col = if row_empty {
            term.cursor.col.saturating_add(1)
        } else {
            source_row.len()
        };
        for col in 0..cols.min(max_col.max(1)) {
            if col < source_row.len() && is_wide_continuation(source_row, col) {
                continue;
            }

            let default_cell = Cell::default();
            let cell = source_row.get(col).unwrap_or(&default_cell);
            let is_sel = term.is_selected(row, col);
            let is_cursor = Self::shell_cursor_here(term, row, col, show_scrollback);
            let bold = cell.attrs.bold;

            if is_cursor && matches!(term.cursor_style, CursorStyle::Bar) {
                list.cursor_bars.push((row, col));
            }

            let (mut fg, mut bg) = (cell.attrs.fg, cell.attrs.bg);
            if cell.attrs.reverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            let box_glyph = builtin_box_drawing && super::builtin::supports(cell.ch);

            if is_sel {
                list.bg_quads.push(BgQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    color: selection_bg_glyphon(palette.theme),
                });
            } else if is_cursor && matches!(term.cursor_style, CursorStyle::Block) {
                list.bg_quads.push(BgQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    color: Self::cursor_color(palette),
                });
            } else if bg != Color::Default {
                list.bg_quads.push(BgQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    color: resolve_bg_glyphon(bg, palette, 255),
                });
            }
            // Sin fondo explicito (Color::Default): no se pinta ningun bg_quad,
            // ni siquiera para box-drawing/block elements. Igual que las letras
            // normales, dejan ver el clear color (translucido si window.opacity
            // < 1). Una version anterior le daba a estas celdas un "backing"
            // propio a la misma opacidad de la ventana, pero al apilarse sobre
            // el clear color ya translucido, el resultado se veia opaco: un
            // recuadro negro solido justo donde habia box-drawing.

            if is_cursor && matches!(term.cursor_style, CursorStyle::Underline) {
                list.line_quads.push(LineQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    kind: LineKind::Under,
                    style: UnderlineStyle::Single,
                    color: Self::cursor_color(palette),
                });
            } else {
                let underline_style = effective_underline_style(cell);
                if underline_style != UnderlineStyle::None && cell.ch != ' ' {
                    list.line_quads.push(LineQuad {
                        row,
                        col,
                        width_cells: cell.width.max(1),
                        kind: LineKind::Under,
                        style: underline_style,
                        color: underline_color_for_cell(fg, bold, cell, palette, dim_alpha),
                    });
                }
            }

            if cell.attrs.strikethrough && cell.ch != ' ' {
                list.line_quads.push(LineQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    kind: LineKind::Strike,
                    style: UnderlineStyle::Single,
                    color: resolve_fg_glyphon(fg, cell.attrs.dim, bold, palette, dim_alpha),
                });
            }

            if cell.attrs.overline && cell.ch != ' ' {
                list.line_quads.push(LineQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    kind: LineKind::Over,
                    style: UnderlineStyle::Single,
                    color: resolve_fg_glyphon(fg, cell.attrs.dim, bold, palette, dim_alpha),
                });
            }

            let emit_text = !cell.attrs.invisible
                && (should_emit_text_glyph(cell)
                    || (is_cursor && matches!(term.cursor_style, CursorStyle::Block)));
            if !emit_text {
                continue;
            }

            let cursor_fg = if is_cursor && matches!(term.cursor_style, CursorStyle::Block) {
                let (r, g, b) = contrast_text_color(palette.cursor_rgb());
                Color::Rgb(r, g, b)
            } else {
                fg
            };

            let Some(glyph_key) = resolve_glyph_key(source_row, col, font_family) else {
                if is_cursor && cell.ch == ' ' {
                    let mut space_key = GlyphKey {
                        ch: ' ',
                        bold: false,
                        italic: false,
                        dim: false,
                        family: font_family.to_string(),
                    };
                    if bold {
                        space_key.bold = true;
                    }
                    list.text_glyphs.push(TextGlyph {
                        row,
                        col,
                        width_cells: 1,
                        glyph_key: space_key,
                        fg: cursor_fg,
                        bold,
                        dim: cell.attrs.dim,
                        custom_id: 0,
                        selected: is_sel,
                        box_glyph: false,
                    });
                }
                continue;
            };

            list.text_glyphs.push(TextGlyph {
                row,
                col,
                width_cells: cell.width.max(1),
                glyph_key,
                fg: cursor_fg,
                bold,
                dim: cell.attrs.dim,
                custom_id: 0,
                selected: is_sel,
                box_glyph,
            });
        }
    }
}

fn should_emit_text_glyph(cell: &Cell) -> bool {
    if cell.attrs.invisible {
        return false;
    }
    if cell.width == 0 {
        return false;
    }
    if cell.ch != ' ' {
        return true;
    }
    cell.attrs.bold || cell.attrs.italic || cell.attrs.dim || cell.attrs.fg != Color::Default
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::CursorStyle;
    use crate::config::{FontConfig, ThemeConfig};
    use crate::grid::GridDamage;
    use crate::renderer::color_to_glyphon;

    fn test_palette(theme: &ThemeConfig) -> Palette<'_> {
        Palette::from_theme(theme)
    }

    fn test_metrics() -> CellMetrics {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let font_config = FontConfig::default();
        CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        )
    }

    fn build_full(
        term: &Term,
        metrics: &CellMetrics,
        theme: &ThemeConfig,
        row_sources: &[&[Cell]],
        cols: usize,
        rows: usize,
        family: &str,
    ) -> DisplayList {
        let palette = test_palette(theme);
        let mut list = DisplayList::default();
        DisplayListBuilder::build(
            &mut list,
            term,
            metrics,
            &palette,
            theme.dim_alpha,
            row_sources,
            cols,
            rows,
            family,
            &DamageSnapshot::Full,
            false,
            true,
        );
        list
    }

    fn row_with_box_top() -> Vec<Cell> {
        let chars = ['┌', '─', '─', '┐'];
        let mut row = vec![Cell::default(); chars.len()];
        for (col, ch) in chars.iter().enumerate() {
            row[col].ch = *ch;
        }
        row
    }

    #[test]
    fn doble_linea_usa_box_glyph() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = '\u{2550}';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert_eq!(list.text_glyphs.len(), 1);
        assert!(list.text_glyphs[0].box_glyph);
    }

    #[test]
    fn box_drawing_row_marca_box_glyph() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let row = row_with_box_top();
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 4, 1, &family);

        assert_eq!(list.text_glyphs.len(), 4);
        assert!(list.text_glyphs.iter().all(|g| g.box_glyph));
    }

    #[test]
    fn box_drawing_row_emits_four_text_glyphs() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let row = row_with_box_top();
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 4, 1, &family);

        assert_eq!(list.text_glyphs.len(), 4);
        for (i, glyph) in list.text_glyphs.iter().enumerate() {
            assert_eq!(glyph.col, i);
            assert_eq!(glyph.row, 0);
            assert_eq!(glyph.width_cells, 1);
        }
    }

    #[test]
    fn box_drawing_con_fondo_por_defecto_no_emite_bg_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let row = row_with_box_top();
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor_visible = false;

        let list = build_full(&term, &metrics, &theme, &row_sources, 4, 1, &family);

        assert!(
            list.bg_quads.is_empty(),
            "box-drawing con bg por defecto no deberia generar bg_quad"
        );
    }

    #[test]
    fn block_cursor_emits_bg_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 5];
        row[3].ch = 'X';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor.move_to(0, 3);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Block;

        let list = build_full(&term, &metrics, &theme, &row_sources, 5, 1, &family);

        assert!(list.bg_quads.iter().any(|q| q.row == 0 && q.col == 3));
    }

    #[test]
    fn block_cursor_text_uses_contrast_fg() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 5];
        row[3].ch = 'X';
        row[3].attrs.fg = Color::Red;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor.move_to(0, 3);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Block;

        let list = build_full(&term, &metrics, &theme, &row_sources, 5, 1, &family);
        let palette = test_palette(&theme);
        let expected = contrast_text_color(palette.cursor_rgb());

        let glyph = list
            .text_glyphs
            .iter()
            .find(|g| g.row == 0 && g.col == 3)
            .expect("glifo bajo cursor block");
        assert_eq!(glyph.fg, Color::Rgb(expected.0, expected.1, expected.2));
    }

    #[test]
    fn strikethrough_emits_strike_line_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'z';
        row[0].attrs.strikethrough = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor_visible = false;

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert!(
            list.line_quads
                .iter()
                .any(|q| q.kind == LineKind::Strike && q.row == 0 && q.col == 0),
            "strikethrough debe emitir LineKind::Strike"
        );
    }

    #[test]
    fn overline_emits_over_line_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'z';
        row[0].attrs.overline = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor_visible = false;

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert!(
            list.line_quads
                .iter()
                .any(|q| q.kind == LineKind::Over && q.row == 0 && q.col == 0),
            "overline debe emitir LineKind::Over"
        );
    }

    #[test]
    fn hyperlink_underline_es_mas_tenue_que_sgr() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let palette = test_palette(&theme);

        let mut link_row = vec![Cell::default(); 1];
        link_row[0].ch = 'L';
        link_row[0].attrs.fg = Color::Green;
        link_row[0].hyperlink = Some(0);

        let mut sgr_row = vec![Cell::default(); 1];
        sgr_row[0].ch = 'L';
        sgr_row[0].attrs.fg = Color::Green;
        sgr_row[0].attrs.underline = true;

        let mut term = Term::default();
        term.cursor_visible = false;

        let link_list = build_full(
            &term,
            &metrics,
            &theme,
            &[link_row.as_slice()],
            1,
            1,
            &family,
        );
        let sgr_list = build_full(
            &term,
            &metrics,
            &theme,
            &[sgr_row.as_slice()],
            1,
            1,
            &family,
        );

        let link_color = link_list.line_quads[0].color;
        let sgr_color = sgr_list.line_quads[0].color;
        let full = resolve_fg_glyphon(Color::Green, false, false, &palette, theme.dim_alpha);

        assert_eq!(sgr_color, full);
        assert_eq!(link_color, attenuate_glyphon(full));
    }

    #[test]
    fn wide_emoji_row_one_glyph_skips_continuation() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 4];
        row[0].ch = '\u{1F600}';
        row[0].width = 2;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 4, 1, &family);

        assert_eq!(list.text_glyphs.len(), 1);
        assert_eq!(list.text_glyphs[0].col, 0);
        assert_eq!(list.text_glyphs[0].width_cells, 2);
    }

    #[test]
    fn reverse_dim_attenuates_swapped_foreground() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'X';
        row[0].attrs.fg = Color::Red;
        row[0].attrs.bg = Color::Blue;
        row[0].attrs.reverse = true;
        row[0].attrs.dim = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor_visible = false;
        let palette = test_palette(&theme);

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        let expected = attenuate_glyphon(color_to_glyphon(Color::Blue, &theme));
        let actual = resolve_fg_glyphon(
            list.text_glyphs[0].fg,
            list.text_glyphs[0].dim,
            list.text_glyphs[0].bold,
            &palette,
            theme.dim_alpha,
        );
        assert_eq!(actual, expected);
        assert_eq!(list.text_glyphs[0].fg, Color::Blue);
    }

    #[test]
    fn underline_emits_quad_with_dimmed_fg() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'a';
        row[0].attrs.fg = Color::Green;
        row[0].attrs.underline = true;
        row[0].attrs.dim = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();
        let palette = test_palette(&theme);

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert_eq!(list.line_quads.len(), 1);
        let expected = attenuate_glyphon(color_to_glyphon(Color::Green, &theme));
        assert_eq!(list.line_quads[0].color, expected);
        let _ = palette;
    }

    #[test]
    fn incremental_build_preserves_clean_rows() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let row0 = row_with_box_top();
        let row1: Vec<Cell> = (0..4)
            .map(|i| Cell {
                ch: char::from_u32(b'a' as u32 + i as u32).unwrap(),
                ..Default::default()
            })
            .collect();
        let row_sources: Vec<&[Cell]> = vec![row0.as_slice(), row1.as_slice()];
        let term = Term::default();
        let palette = test_palette(&theme);
        let mut list = DisplayList::default();

        DisplayListBuilder::build(
            &mut list,
            &term,
            &metrics,
            &palette,
            theme.dim_alpha,
            &row_sources,
            4,
            2,
            &family,
            &DamageSnapshot::Full,
            false,
            true,
        );
        let full_glyphs = list.text_glyphs.len();
        assert_eq!(full_glyphs, 8);

        let mut damage = GridDamage::new(2, 4);
        let _ = damage.take();
        damage.mark_cell(1, 0);
        let snap = damage.take();

        DisplayListBuilder::build(
            &mut list,
            &term,
            &metrics,
            &palette,
            theme.dim_alpha,
            &row_sources,
            4,
            2,
            &family,
            &snap,
            false,
            true,
        );

        assert_eq!(list.text_glyphs.len(), full_glyphs);
        assert_eq!(list.text_glyphs.iter().filter(|g| g.row == 0).count(), 4);
    }

    #[test]
    fn invisible_no_emite_glifo_de_texto() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'x';
        row[0].attrs.invisible = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert!(list.text_glyphs.is_empty(), "invisible no dibuja glifo");
    }

    #[test]
    fn contrast_fg_elige_negro_o_blanco() {
        assert_eq!(contrast_text_color((255, 255, 255)), (0, 0, 0));
        assert_eq!(contrast_text_color((0, 0, 0)), (255, 255, 255));
    }

    #[test]
    fn undercurl_usa_underline_color() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'a';
        row[0].attrs.underline_style = UnderlineStyle::Curly;
        row[0].attrs.underline_color = Color::Red;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();
        let palette = test_palette(&theme);

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        let under = list
            .line_quads
            .iter()
            .find(|q| q.kind == LineKind::Under)
            .expect("underline quad");
        assert_eq!(under.style, UnderlineStyle::Curly);
        let expected = resolve_fg_glyphon(Color::Red, false, false, &palette, theme.dim_alpha);
        assert_eq!(under.color, expected);
    }

    #[test]
    fn dim_alpha_attenuates_via_alpha_channel() {
        let theme = ThemeConfig {
            dim_alpha: true,
            ..ThemeConfig::default()
        };
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'a';
        row[0].attrs.fg = Color::Red;
        row[0].attrs.dim = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::default();
        term.cursor_visible = false;
        let palette = test_palette(&theme);

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        let actual = resolve_fg_glyphon(
            list.text_glyphs[0].fg,
            list.text_glyphs[0].dim,
            list.text_glyphs[0].bold,
            &palette,
            theme.dim_alpha,
        );
        assert_eq!(actual.a(), (DIM_FACTOR * 255.0) as u8);
        assert_eq!(actual.r(), palette.rgb(Color::Red, false).0);
    }

    #[test]
    fn celda_con_hyperlink_emite_underline() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'L';
        row[0].hyperlink = Some(0);
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let term = Term::default();

        let list = build_full(&term, &metrics, &theme, &row_sources, 1, 1, &family);

        assert_eq!(
            list.line_quads
                .iter()
                .filter(|q| q.kind == LineKind::Under)
                .count(),
            1
        );
    }
}
