//! Display list celda-determinista: fondos y glifos por coordenada de grid.

use std::collections::HashSet;

use crate::ansi::{Color, CursorStyle, Term, UnderlineStyle};
use crate::config::parse_hex;
use crate::grid::{Cell, DamageSnapshot};

use super::contrast::ContrastCache;
use super::decorations::cursor_glyph;
use super::glyph::{is_wide_continuation, resolve_glyph_key, GlyphKey};
use super::metrics::CellMetrics;
use super::palette::Palette;
use super::runs::{group_ligature_runs, shape_run};
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
    /// Fondo efectivo para ajuste de contraste WCAG.
    pub contrast_bg: (u8, u8, u8),
    /// True cuando fg == bg (bloques) o el color ya esta fijado (seleccion/cursor).
    pub skip_contrast: bool,
    pub custom_id: u16,
    pub selected: bool,
    /// True si se rasteriza con box_mask en vez de fuente.
    pub box_glyph: bool,
    /// Posicion horizontal desde el origen de fila (px). `Some` = glifo de run ligable.
    pub x_offset: Option<f32>,
    /// Shaping precomputado para glifos de run (evita reshape por celda).
    pub run_shaped: Option<super::glyph::ShapedGlyph>,
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

/// Resuelve fg a glyphon::Color, aplicando contraste y dim si corresponde.
#[expect(
    clippy::too_many_arguments,
    reason = "color resolution needs palette, contrast bg and cache"
)]
pub fn resolve_fg_glyphon(
    fg: Color,
    dim: bool,
    bold: bool,
    palette: &Palette<'_>,
    dim_alpha: bool,
    contrast_bg: (u8, u8, u8),
    skip_contrast: bool,
    cache: &mut ContrastCache,
) -> glyphon::Color {
    let mut rgb = palette.rgb(fg, bold);
    if !skip_contrast {
        rgb = cache.adjust(rgb, contrast_bg, palette.theme.minimum_contrast);
    }
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

fn cell_contrast_context(
    fg: Color,
    bg: Color,
    is_sel: bool,
    cursor_block: bool,
    palette: &Palette<'_>,
) -> ((u8, u8, u8), bool) {
    if fg == bg {
        return (palette.bg_rgb(bg), true);
    }
    if is_sel {
        let sel_bg = palette
            .theme
            .selection_bg
            .as_deref()
            .map(parse_hex)
            .unwrap_or_else(|| parse_hex("#c4704a"));
        return (sel_bg, true);
    }
    if cursor_block {
        return (palette.cursor_rgb(), true);
    }
    (palette.bg_rgb(bg), false)
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

#[expect(
    clippy::too_many_arguments,
    reason = "underline color shares cell contrast context with fg resolution"
)]
fn underline_color_for_cell(
    fg: Color,
    bg: Color,
    bold: bool,
    cell: &Cell,
    palette: &Palette<'_>,
    dim_alpha: bool,
    is_sel: bool,
    cursor_block: bool,
    is_link_hover: bool,
    cache: &mut ContrastCache,
) -> glyphon::Color {
    let color = if cell.attrs.underline_color == Color::Default {
        fg
    } else {
        cell.attrs.underline_color
    };
    let (contrast_bg, skip_contrast) =
        cell_contrast_context(color, bg, is_sel, cursor_block, palette);
    let mut resolved = resolve_fg_glyphon(
        color,
        cell.attrs.dim,
        bold,
        palette,
        dim_alpha,
        contrast_bg,
        skip_contrast,
        cache,
    );
    if cell.hyperlink.is_some() && !is_link_hover && cell.attrs.underline_color == Color::Default {
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
        blink_on: bool,
        ligatures: bool,
        font_system: &mut Option<&mut glyphon::FontSystem>,
        swash_cache: &mut Option<&mut glyphon::SwashCache>,
        contrast_cache: &mut ContrastCache,
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
                    blink_on,
                    ligatures,
                    font_system,
                    swash_cache,
                    contrast_cache,
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
                    blink_on,
                    ligatures,
                    font_system,
                    swash_cache,
                    contrast_cache,
                );
            }
        }

        // El cursor de copy mode es de navegacion: siempre visible, no parpadea.
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
            && term.search.is_none()
            && !show_scrollback
            && term.cursor_visible
            && term.cursor.row == row
            && term.cursor.col == col
    }

    /// Cursor de copy mode. Es de navegacion: se emite siempre, sin importar la
    /// fase de parpadeo (`build` no le pasa `blink_on`), a diferencia del shell
    /// cursor que vive en `build_row` y se oculta en la fase "off".
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
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        row_sources: &[&[Cell]],
        cols: usize,
        row: usize,
        font_family: &str,
        show_scrollback: bool,
        builtin_box_drawing: bool,
        blink_on: bool,
        ligatures: bool,
        font_system: &mut Option<&mut glyphon::FontSystem>,
        swash_cache: &mut Option<&mut glyphon::SwashCache>,
        contrast_cache: &mut ContrastCache,
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
        let lig_runs = if ligatures {
            group_ligature_runs(source_row, cols, |col| term.is_selected(row, col))
        } else {
            Vec::new()
        };
        let lig_handled = if ligatures {
            match (font_system.as_deref_mut(), swash_cache.as_deref_mut()) {
                (Some(fs), Some(swash)) => Self::build_row_text_runs(
                    list,
                    term,
                    metrics,
                    palette,
                    &lig_runs,
                    source_row,
                    cols,
                    row,
                    font_family,
                    fs,
                    swash,
                    show_scrollback,
                    builtin_box_drawing,
                    blink_on,
                    contrast_cache,
                ),
                _ => HashSet::new(),
            }
        } else {
            HashSet::new()
        };

        for col in 0..cols.min(max_col.max(1)) {
            if col < source_row.len() && is_wide_continuation(source_row, col) {
                continue;
            }

            let default_cell = Cell::default();
            let cell = source_row.get(col).unwrap_or(&default_cell);
            let is_sel = term.is_selected(row, col);
            let is_link_hover = term.is_hovered_link(row, col);
            let search_hit = term.search_hit_at(row, col);
            let is_search = search_hit.is_some();
            let is_cursor = Self::shell_cursor_here(term, row, col, show_scrollback);
            // El shell cursor se suprime en la fase "off" del parpadeo cuando
            // el parpadeo del cursor esta habilitado en config. Si el usuario
            // lo desactivo, el cursor siempre es visible (independiente de
            // SGR 5 en el texto).
            let cursor_rendered = is_cursor && (blink_on || !term.cursor_blink_enabled);
            let bold = cell.attrs.bold;

            if cursor_rendered && matches!(term.cursor_style, CursorStyle::Bar) {
                list.cursor_bars.push((row, col));
            }

            let (mut fg, mut bg) = (cell.attrs.fg, cell.attrs.bg);
            if cell.attrs.reverse {
                std::mem::swap(&mut fg, &mut bg);
            }

            let cursor_block = cursor_rendered && matches!(term.cursor_style, CursorStyle::Block);
            let (contrast_bg, skip_contrast) =
                cell_contrast_context(fg, bg, is_sel || is_search, cursor_block, palette);

            let box_glyph = builtin_box_drawing && super::builtin::supports(cell.ch);

            if is_sel {
                list.bg_quads.push(BgQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    color: selection_bg_glyphon(palette.theme),
                });
            } else if let Some(current) = search_hit {
                let base = selection_bg_glyphon(palette.theme);
                let color = if current {
                    base
                } else {
                    attenuate_glyphon(base)
                };
                list.bg_quads.push(BgQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    color,
                });
            } else if cursor_rendered && matches!(term.cursor_style, CursorStyle::Block) {
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

            if cursor_rendered && matches!(term.cursor_style, CursorStyle::Underline) {
                list.line_quads.push(LineQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    kind: LineKind::Under,
                    style: UnderlineStyle::Single,
                    color: Self::cursor_color(palette),
                });
            } else {
                let underline_style = if is_link_hover {
                    UnderlineStyle::Single
                } else {
                    effective_underline_style(cell)
                };
                if underline_style != UnderlineStyle::None && cell.ch != ' ' {
                    list.line_quads.push(LineQuad {
                        row,
                        col,
                        width_cells: cell.width.max(1),
                        kind: LineKind::Under,
                        style: underline_style,
                        color: underline_color_for_cell(
                            fg,
                            bg,
                            bold,
                            cell,
                            palette,
                            dim_alpha,
                            is_sel,
                            cursor_block,
                            is_link_hover,
                            contrast_cache,
                        ),
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
                    color: resolve_fg_glyphon(
                        fg,
                        cell.attrs.dim,
                        bold,
                        palette,
                        dim_alpha,
                        contrast_bg,
                        skip_contrast,
                        contrast_cache,
                    ),
                });
            }

            if cell.attrs.overline && cell.ch != ' ' {
                list.line_quads.push(LineQuad {
                    row,
                    col,
                    width_cells: cell.width.max(1),
                    kind: LineKind::Over,
                    style: UnderlineStyle::Single,
                    color: resolve_fg_glyphon(
                        fg,
                        cell.attrs.dim,
                        bold,
                        palette,
                        dim_alpha,
                        contrast_bg,
                        skip_contrast,
                        contrast_cache,
                    ),
                });
            }

            // SGR 5 (blink): oculta el glifo de texto en la fase "off".
            // El fondo y las decoraciones (subrayado, tachado, overline) se
            // mantienen, igual que en xterm.
            let text_hidden_by_blink = cell.attrs.blink && !blink_on;
            let emit_text = !cell.attrs.invisible
                && !text_hidden_by_blink
                && (should_emit_text_glyph(cell)
                    || (cursor_rendered && matches!(term.cursor_style, CursorStyle::Block)));
            if !emit_text {
                continue;
            }

            let cursor_fg = if cursor_block {
                let (r, g, b) = contrast_text_color(palette.cursor_rgb());
                Color::Rgb(r, g, b)
            } else {
                fg
            };
            let (text_contrast_bg, text_skip_contrast) = if cursor_block {
                (contrast_bg, true)
            } else {
                (contrast_bg, skip_contrast)
            };

            if lig_handled.contains(&col) {
                continue;
            }

            let Some(glyph_key) = resolve_glyph_key(source_row, col, font_family) else {
                if cursor_rendered && cell.ch == ' ' {
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
                        contrast_bg: text_contrast_bg,
                        skip_contrast: text_skip_contrast,
                        custom_id: 0,
                        selected: is_sel,
                        box_glyph: false,
                        x_offset: None,
                        run_shaped: None,
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
                contrast_bg: text_contrast_bg,
                skip_contrast: text_skip_contrast,
                custom_id: 0,
                selected: is_sel,
                box_glyph,
                x_offset: None,
                run_shaped: None,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_row_text_runs(
        list: &mut DisplayList,
        term: &Term,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        lig_runs: &[super::runs::LigRun],
        source_row: &[Cell],
        cols: usize,
        row: usize,
        font_family: &str,
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        show_scrollback: bool,
        _builtin_box_drawing: bool,
        blink_on: bool,
        _contrast_cache: &mut ContrastCache,
    ) -> HashSet<usize> {
        use super::glyph_cache::cache_key_rasterizes;

        let mut handled = HashSet::new();
        for run in lig_runs {
            if run.text.is_empty() {
                continue;
            }
            let start = run.start_col;
            let end = start + run.cols;
            let cell = &source_row[start];

            let blink_hidden = (start..end).any(|c| {
                source_row
                    .get(c)
                    .is_some_and(|cell| cell.attrs.blink && !blink_on)
            });
            if blink_hidden || cell.attrs.invisible {
                continue;
            }

            let bold = cell.attrs.bold;
            let shaped_glyphs = shape_run(
                font_system,
                metrics,
                font_family,
                &run.text,
                bold,
                cell.attrs.italic,
                cell.attrs.dim,
            );

            let rasterizes: Vec<bool> = shaped_glyphs
                .iter()
                .map(|g| cache_key_rasterizes(font_system, swash_cache, g.cache_key))
                .collect();
            if !rasterizes.iter().any(|&ok| ok) {
                continue;
            }

            for c in run.start_col..run.start_col + run.cols {
                handled.insert(c);
            }

            let run_x = run.start_col as f32 * metrics.cell_w;
            for (gi, g) in shaped_glyphs.iter().enumerate() {
                let col = run.start_col + g.col_in_run;
                if col >= cols {
                    continue;
                }
                if !rasterizes[gi] {
                    continue;
                }
                let Some(cell_at) = source_row.get(col) else {
                    continue;
                };
                let is_cursor = Self::shell_cursor_here(term, row, col, show_scrollback)
                    && (blink_on || !term.cursor_blink_enabled);
                let mut fg = cell_at.attrs.fg;
                let mut bg = cell_at.attrs.bg;
                if cell_at.attrs.reverse {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let is_sel = term.is_selected(row, col);
                let is_search = term.search_hit_at(row, col).is_some();
                let cursor_block = is_cursor && matches!(term.cursor_style, CursorStyle::Block);
                let (contrast_bg, skip_contrast) =
                    cell_contrast_context(fg, bg, is_sel || is_search, cursor_block, palette);
                let fg_color = if cursor_block {
                    let (r, g, b) = contrast_text_color(palette.cursor_rgb());
                    Color::Rgb(r, g, b)
                } else {
                    fg
                };
                let (text_contrast_bg, text_skip_contrast) = if cursor_block {
                    (contrast_bg, true)
                } else {
                    (contrast_bg, skip_contrast)
                };
                let glyph_key = GlyphKey {
                    ch: run.text.chars().nth(g.col_in_run).unwrap_or(' '),
                    bold,
                    italic: cell.attrs.italic,
                    dim: cell.attrs.dim,
                    family: format!("{font_family}#lig:{}:{gi}", run.text),
                };
                list.text_glyphs.push(TextGlyph {
                    row,
                    col,
                    width_cells: g.cluster_cols.min(255) as u8,
                    glyph_key,
                    fg: fg_color,
                    bold: cell_at.attrs.bold,
                    dim: cell_at.attrs.dim,
                    contrast_bg: text_contrast_bg,
                    skip_contrast: text_skip_contrast,
                    custom_id: 0,
                    selected: is_sel,
                    box_glyph: false,
                    x_offset: Some(run_x),
                    run_shaped: Some(super::runs::run_glyph_to_shaped(g)),
                });
            }
        }
        handled
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
    use crate::config::{parse_hex, FontConfig, ThemeConfig};
    use crate::grid::GridDamage;
    use crate::renderer::color_to_glyphon;

    fn test_palette(theme: &ThemeConfig) -> Palette<'_> {
        Palette::from_theme(theme)
    }

    fn test_resolve(
        fg: Color,
        dim: bool,
        bold: bool,
        palette: &Palette<'_>,
        dim_alpha: bool,
        cache: &mut ContrastCache,
    ) -> glyphon::Color {
        let bg = parse_hex(&palette.theme.background);
        resolve_fg_glyphon(fg, dim, bold, palette, dim_alpha, bg, false, cache)
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
        let mut contrast_cache = ContrastCache::default();
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
            true,
            false,
            &mut None,
            &mut None,
            &mut contrast_cache,
        );
        list
    }

    /// Como `build_full` pero permite fijar la fase de parpadeo (`blink_on`).
    #[allow(clippy::too_many_arguments)]
    fn build_full_blink(
        term: &Term,
        metrics: &CellMetrics,
        theme: &ThemeConfig,
        row_sources: &[&[Cell]],
        cols: usize,
        rows: usize,
        family: &str,
        blink_on: bool,
    ) -> DisplayList {
        let palette = test_palette(theme);
        let mut list = DisplayList::default();
        let mut contrast_cache = ContrastCache::default();
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
            blink_on,
            false,
            &mut None,
            &mut None,
            &mut contrast_cache,
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
        let mut cache = ContrastCache::default();
        let full = test_resolve(
            Color::Green,
            false,
            false,
            &palette,
            theme.dim_alpha,
            &mut cache,
        );

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
        let theme = ThemeConfig {
            minimum_contrast: 1.0,
            ..ThemeConfig::default()
        };
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
        let mut cache = ContrastCache::default();
        let actual = resolve_fg_glyphon(
            list.text_glyphs[0].fg,
            list.text_glyphs[0].dim,
            list.text_glyphs[0].bold,
            &palette,
            theme.dim_alpha,
            list.text_glyphs[0].contrast_bg,
            list.text_glyphs[0].skip_contrast,
            &mut cache,
        );
        assert_eq!(actual, expected);
        assert_eq!(list.text_glyphs[0].fg, Color::Blue);
    }

    #[test]
    fn underline_emits_quad_with_dimmed_fg() {
        let theme = ThemeConfig {
            minimum_contrast: 1.0,
            ..ThemeConfig::default()
        };
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
        let mut contrast_cache = ContrastCache::default();

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
            true,
            false,
            &mut None,
            &mut None,
            &mut contrast_cache,
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
            true,
            false,
            &mut None,
            &mut None,
            &mut contrast_cache,
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
        let theme = ThemeConfig {
            minimum_contrast: 1.0,
            ..ThemeConfig::default()
        };
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
        let expected = {
            let mut cache = ContrastCache::default();
            let bg = parse_hex(&theme.background);
            resolve_fg_glyphon(
                Color::Red,
                false,
                false,
                &palette,
                theme.dim_alpha,
                bg,
                false,
                &mut cache,
            )
        };
        assert_eq!(under.color, expected);
    }

    #[test]
    fn dim_alpha_attenuates_via_alpha_channel() {
        let theme = ThemeConfig {
            dim_alpha: true,
            minimum_contrast: 1.0,
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

        let mut cache = ContrastCache::default();
        let actual = resolve_fg_glyphon(
            list.text_glyphs[0].fg,
            list.text_glyphs[0].dim,
            list.text_glyphs[0].bold,
            &palette,
            theme.dim_alpha,
            list.text_glyphs[0].contrast_bg,
            list.text_glyphs[0].skip_contrast,
            &mut cache,
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

    #[test]
    fn hovered_link_sin_osc8_emite_underline() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 20];
        let text = "see https://ex.com";
        for (i, ch) in text.chars().enumerate() {
            row[i].ch = ch;
        }
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.hovered_link = Some(crate::ansi::LinkRange {
            row: 0,
            start_col: 4,
            end_col: 17,
        });

        let list = build_full(&term, &metrics, &theme, &row_sources, 20, 1, &family);

        assert!(
            list.line_quads
                .iter()
                .any(|q| q.kind == LineKind::Under && q.col >= 4 && q.col <= 17),
            "hovered_link debe subrayar el rango sin atributo OSC 8"
        );
    }

    // -----------------------------------------------------------------
    // Parpadeo: cursor y texto SGR 5 se ocultan en fase off.
    // -----------------------------------------------------------------

    /// Cursor block emitido en fase on; suprimido en fase off (blink activado).
    #[test]
    fn cursor_block_blink_off_oculta_bg_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 3];
        row[1].ch = 'X';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor.move_to(0, 1);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Block;
        term.cursor_blink_enabled = true;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, false);

        assert!(
            on.bg_quads.iter().any(|q| q.row == 0 && q.col == 1),
            "fase on: el cursor block emite bg_quad"
        );
        assert!(
            !off.bg_quads.iter().any(|q| q.row == 0 && q.col == 1),
            "fase off: no hay bg_quad del cursor"
        );
        assert!(
            !off.cursor_bars.iter().any(|&(r, _)| r == 0),
            "fase off: no hay cursor_bars"
        );
    }

    /// Con cursor_blink desactivado, el cursor se mantiene en ambas fases.
    #[test]
    fn cursor_blink_disabled_no_suprime_en_off() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 3];
        row[1].ch = 'X';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor.move_to(0, 1);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Block;
        term.cursor_blink_enabled = false;

        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, false);

        assert!(
            off.bg_quads.iter().any(|q| q.row == 0 && q.col == 1),
            "blink desactivado: el cursor sigue visible en fase off"
        );
    }

    /// Texto con SGR 5 (blink): glifo oculto en fase off, bg y decoraciones
    /// se mantienen.
    #[test]
    fn texto_blink_oculto_en_fase_off_mantiene_bg() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'x';
        row[0].attrs.blink = true;
        row[0].attrs.bg = Color::Red;
        row[0].attrs.underline = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor_visible = false;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, false);

        assert_eq!(on.text_glyphs.len(), 1, "fase on: glifo de texto visible");
        assert!(
            off.text_glyphs.is_empty(),
            "fase off: glifo de texto blink suprimido"
        );
        assert_eq!(
            off.bg_quads.len(),
            on.bg_quads.len(),
            "bg de celda blink se mantiene en fase off"
        );
        assert_eq!(
            off.line_quads
                .iter()
                .filter(|q| q.kind == LineKind::Under)
                .count(),
            1,
            "underline de celda blink se mantiene en fase off"
        );
    }

    /// Cursor `Bar`: emite `cursor_bars` en fase on, ninguna en fase off
    /// (cuando blink del cursor activo).
    #[test]
    fn cursor_bar_blink_off_no_emite_cursor_bars() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 3];
        row[1].ch = 'X';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor.move_to(0, 1);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Bar;
        term.cursor_blink_enabled = true;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, false);

        assert!(
            on.cursor_bars.iter().any(|&(r, _)| r == 0),
            "fase on: bar emite cursor_bar"
        );
        assert!(
            !off.cursor_bars.iter().any(|&(r, _)| r == 0),
            "fase off: no hay cursor_bars para Bar"
        );
    }

    /// Cursor `Underline`: emite `LineQuad` de subrayado (cursor) en on,
    /// ninguna en off (blink del cursor activo).
    #[test]
    fn cursor_underline_blink_off_no_emite_line_quad() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 3];
        row[1].ch = 'X';
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor.move_to(0, 1);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Underline;
        term.cursor_blink_enabled = true;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 3, 1, &family, false);

        let cursor_under = |l: &DisplayList| {
            l.line_quads.iter().any(|q| {
                q.row == 0
                    && q.col == 1
                    && q.color == rgb_to_glyphon(Palette::from_theme(&theme).cursor_rgb())
            })
        };
        assert!(cursor_under(&on), "fase on: underline del cursor presente");
        assert!(
            !cursor_under(&off),
            "fase off: underline del cursor ausente"
        );
    }

    /// Cursor block sobre una celda con SGR 5 (blink): ambos ocultos en fase
    /// off; el glifo de texto blink y el bg del cursor se suprimen.
    #[test]
    fn cursor_block_sobre_celda_blink_ambos_ocultos_off() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'X';
        row[0].attrs.blink = true;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor.move_to(0, 0);
        term.cursor_visible = true;
        term.cursor_style = CursorStyle::Block;
        term.cursor_blink_enabled = true;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, false);

        assert_eq!(
            on.text_glyphs.len(),
            1,
            "fase on: glifo text + cursor block"
        );
        assert!(
            off.text_glyphs.is_empty(),
            "fase off: ni cursor block ni glifo blink"
        );
        assert!(
            !off.bg_quads.iter().any(|q| q.row == 0 && q.col == 0),
            "fase off: sin bg_quad del cursor block"
        );
    }

    /// Celda SGR 5 con reverse: el glifo blink se oculta en fase off; el bg
    /// (color invertido efectivo) se mantiene como en xterm.
    #[test]
    fn celda_blink_con_reverse_mantiene_bg_off() {
        let theme = ThemeConfig::default();
        let metrics = test_metrics();
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'x';
        row[0].attrs.blink = true;
        row[0].attrs.reverse = true;
        // reverse intercambia fg<->bg en build_row: tras swap, el bg efectivo
        // es el fg original; por eso fg != Default para que haya bg_quad.
        row[0].attrs.fg = Color::Red;
        row[0].attrs.bg = Color::Default;
        let row_sources: Vec<&[Cell]> = vec![row.as_slice()];
        let mut term = Term::new();
        term.cursor_visible = false;

        let on = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, true);
        let off = build_full_blink(&term, &metrics, &theme, &row_sources, 1, 1, &family, false);

        assert_eq!(on.text_glyphs.len(), 1, "fase on: glifo blink visible");
        assert!(
            off.text_glyphs.is_empty(),
            "fase off: glifo blink oculto con reverse"
        );
        assert_eq!(
            off.bg_quads.len(),
            on.bg_quads.len(),
            "bg efectivo (swap por reverse) se mantiene en fase off"
        );
        assert!(
            !off.bg_quads.is_empty(),
            "hay un bg_quad por el reverse efectivo"
        );
    }
}
