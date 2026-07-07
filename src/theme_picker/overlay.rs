//! Overlay GPU del theme picker: lista, paleta y muestras.

use glyphon::{Attrs, Buffer, Color, Family, FontSystem, Shaping, TextArea, TextBounds};

use super::style::{picker_dim, picker_footer, picker_foreground, picker_list_fg};
use super::ThemePickerState;
use crate::config::{parse_hex, GlyphOffset, ThemeConfig};
use crate::renderer::{resolve_family, CellMetrics, ContrastCache, SOLID_MASK_GLYPH_ID};

const LAYER_OVERLAY: usize = 3;

/// Tamaño fijo del TUI del picker (independiente del font del terminal).
pub const PICKER_FONT_SIZE: f32 = 14.0;
pub const PICKER_LINE_HEIGHT: f32 = 1.3;

/// Posiciones verticales de la paleta ANSI y del bloque de muestras.
pub struct PaletteLayout {
    pub text_top: f32,
    pub swatch_y0: f32,
    pub swatch_block_h: f32,
    /// Posición Y en px donde empiezan las muestras WYSIWYG.
    pub samples_y: f32,
}

/// Calcula layout de paleta y posición de muestras.
pub fn palette_layout(cell_h: f32) -> PaletteLayout {
    let text_top = cell_h * 0.5;
    // Líneas de texto en detail: nombre (1) + vacía (1) + etiqueta paleta (1)
    let label_lines = 3.0;
    let gap_after_label = cell_h * 0.35;
    let swatch_h = cell_h * 0.85;
    let swatch_row_gap = 4.0;
    let swatch_block_h = swatch_h * 2.0 + swatch_row_gap;
    let swatch_y0 = text_top + label_lines * cell_h + gap_after_label;
    let gap_after_swatches = cell_h * 0.75;
    let samples_y = swatch_y0 + swatch_block_h + gap_after_swatches;
    PaletteLayout {
        text_top,
        swatch_y0,
        swatch_block_h,
        samples_y,
    }
}

/// Métricas de celda dedicadas al theme picker.
pub fn picker_cell_metrics(font_system: &mut FontSystem, font_family: &str) -> CellMetrics {
    CellMetrics::measure(
        font_system,
        font_family,
        PICKER_FONT_SIZE,
        PICKER_LINE_HEIGHT,
        GlyphOffset { x: 0.0, y: 0.0 },
    )
}

/// Quads de color del overlay (fondo, swatches, resaltado).
pub fn build_custom_glyphs(
    picker: &ThemePickerState,
    theme: &ThemeConfig,
    cell_w: f32,
    cell_h: f32,
    surface_w: u32,
    surface_h: u32,
) -> Vec<glyphon::CustomGlyph> {
    let list_w = (surface_w as f32 * 0.30).max(cell_w * 12.0);
    let panel_w = surface_w as f32;
    let panel_h = surface_h as f32;
    let layout = palette_layout(cell_h);

    let (bg_r, bg_g, bg_b) = parse_hex(&theme.background);
    let (list_r, list_g, list_b) = parse_hex(&theme.black);
    let (sep_r, sep_g, sep_b) = parse_hex(&theme.bright_black);

    let mut custom_glyphs = Vec::new();
    custom_glyphs.push(solid_quad(
        0.0,
        0.0,
        panel_w,
        panel_h,
        Color::rgb(bg_r, bg_g, bg_b),
    ));
    custom_glyphs.push(solid_quad(
        0.0,
        0.0,
        list_w,
        panel_h,
        Color::rgb(list_r, list_g, list_b),
    ));
    custom_glyphs.push(solid_quad(
        list_w,
        0.0,
        cell_w * 0.25,
        panel_h,
        Color::rgb(sep_r, sep_g, sep_b),
    ));

    let swatch = cell_w * 1.5;
    let swatch_h = cell_h * 0.85;
    let swatch_x0 = list_w + cell_w;
    let colors = [
        &theme.black,
        &theme.red,
        &theme.green,
        &theme.yellow,
        &theme.blue,
        &theme.magenta,
        &theme.cyan,
        &theme.white,
        &theme.bright_black,
        &theme.bright_red,
        &theme.bright_green,
        &theme.bright_yellow,
        &theme.bright_blue,
        &theme.bright_magenta,
        &theme.bright_cyan,
        &theme.bright_white,
    ];
    for (i, hex) in colors.iter().enumerate() {
        let row = i / 8;
        let col = i % 8;
        let (r, g, b) = parse_hex(hex);
        custom_glyphs.push(solid_quad(
            swatch_x0 + col as f32 * (swatch + 2.0),
            layout.swatch_y0 + row as f32 * (swatch_h + 4.0),
            swatch,
            swatch_h,
            Color::rgb(r, g, b),
        ));
    }

    let highlight_y = layout.text_top + picker.index as f32 * cell_h;
    if picker.can_confirm() {
        let sel_hex = theme.selection_bg.as_deref().unwrap_or(&theme.bright_black);
        let (sel_r, sel_g, sel_b) = parse_hex(sel_hex);
        custom_glyphs.push(solid_quad(
            0.0,
            highlight_y,
            list_w,
            cell_h,
            Color::rgba(sel_r, sel_g, sel_b, 0xcc),
        ));
    }

    custom_glyphs
}

/// Renderiza las muestras ANSI con el mismo pipeline celda-determinista que el terminal.
#[expect(clippy::too_many_arguments, reason = "sample grid needs GPU caches")]
pub fn build_sample_custom_glyphs(
    theme: &ThemeConfig,
    bold_is_bright: bool,
    metrics: &CellMetrics,
    font_family: &str,
    left: f32,
    top: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut glyphon::SwashCache,
    glyph_cache: &mut crate::renderer::GlyphCache,
    contrast_cache: &mut ContrastCache,
) -> Result<Vec<glyphon::CustomGlyph>, String> {
    use crate::grid::DamageSnapshot;
    use crate::renderer::{CellRenderer, ColorOverrides, DisplayList, DisplayListBuilder, Palette};

    let term = super::samples::build_sample_term();
    let active = term.active_grid();
    let rows = active.rows_count;
    let cols = active.cols_count;
    let row_sources: Vec<&[crate::grid::Cell]> = active.rows.iter().map(|r| r.as_slice()).collect();

    let overrides = ColorOverrides::default();
    let palette = Palette {
        theme,
        overrides: &overrides,
        bold_is_bright: bold_is_bright || theme.bold_is_bright,
    };

    let mut list = DisplayList::default();
    DisplayListBuilder::build(
        &mut list,
        &term,
        metrics,
        &palette,
        theme.dim_alpha,
        &row_sources,
        cols,
        rows,
        font_family,
        &DamageSnapshot::Full,
        false,
        true,
        true,
        false,
        &mut None,
        &mut None,
        contrast_cache,
    );

    let mut glyphs = Vec::new();
    CellRenderer::build_custom_glyphs(
        &list,
        metrics,
        &palette,
        theme.dim_alpha,
        font_family,
        glyph_cache,
        font_system,
        swash_cache,
        contrast_cache,
        &mut glyphs,
    )?;

    for glyph in &mut glyphs {
        glyph.left += left;
        glyph.top += top;
    }

    Ok(glyphs)
}

/// Rellena los buffers de texto del overlay.
#[expect(
    clippy::too_many_arguments,
    reason = "GPU overlay layout needs surface and buffer handles"
)]
pub fn fill_buffers(
    picker: &ThemePickerState,
    font_system: &mut FontSystem,
    font_family: &str,
    cell_w: f32,
    cell_h: f32,
    surface_w: u32,
    surface_h: u32,
    list_buffer: &mut Buffer,
    detail_buffer: &mut Buffer,
    footer_buffer: &mut Buffer,
    contrast_cache: &mut ContrastCache,
) {
    let family = resolve_family(font_family);
    let list_w = (surface_w as f32 * 0.30).max(cell_w * 12.0);
    let panel_w = surface_w as f32;
    let panel_h = surface_h as f32;

    let presets = picker.filtered_presets();
    let mut list_lines = String::new();
    if presets.is_empty() {
        list_lines.push_str("(sin coincidencias)\n");
        if picker.is_search_mode() || !picker.filter().is_empty() {
            list_lines.push_str(&format!("/{}", picker.filter()));
        }
    } else {
        let selected = picker.try_selected_name();
        for name in &presets {
            if Some(*name) == selected {
                list_lines.push_str("▸ ");
            } else {
                list_lines.push_str("  ");
            }
            list_lines.push_str(name);
            list_lines.push('\n');
        }
        if picker.is_search_mode() {
            list_lines.push_str(&format!("\n/{}", picker.filter()));
        } else if picker.has_active_filter() {
            list_lines.push_str(&format!("\nfiltro: {}", picker.filter()));
        }
    }

    let preview_theme = picker.preview_theme();
    let list_fg = picker_list_fg(&preview_theme, contrast_cache);

    fill_buffer(
        font_system,
        list_buffer,
        &list_lines,
        family,
        list_fg,
        list_w,
        panel_h,
    );

    let header_attrs = Attrs::new()
        .family(family)
        .color(picker_foreground(&preview_theme, contrast_cache));
    let label_attrs = Attrs::new()
        .family(family)
        .color(picker_dim(&preview_theme, contrast_cache));

    let mut detail_spans: Vec<(String, Attrs<'_>)> = Vec::new();
    if let Some(name) = picker.try_selected_name() {
        detail_spans.push((format!("{name}\n\n"), header_attrs));
        detail_spans.push((String::from("Paleta ANSI 0-7 / 8-15\n"), label_attrs));
    } else {
        detail_spans.push((
            String::from("(sin coincidencias)\n\nEnter deshabilitado\n\n"),
            header_attrs,
        ));
    }
    fill_buffer_spans(
        font_system,
        detail_buffer,
        &detail_spans,
        family,
        panel_w - list_w - cell_w,
        panel_h,
    );

    let footer = if picker.is_search_mode() {
        "↑/↓ navegar · enter confirmar filtro · esc cancelar búsqueda"
    } else if !picker.can_confirm() {
        "sin coincidencias · / buscar · Esc cancelar"
    } else if picker.has_active_filter() {
        "↑/↓ navegar resultados · / buscar · Enter aplicar · Esc cancelar"
    } else {
        "↑/↓ navegar · / buscar · Enter aplicar · Esc cancelar"
    };

    fill_buffer(
        font_system,
        footer_buffer,
        footer,
        family,
        picker_footer(&preview_theme, contrast_cache),
        panel_w,
        cell_h * 2.0,
    );
}

fn fill_buffer_spans<'a>(
    font_system: &mut FontSystem,
    buffer: &mut Buffer,
    spans: &[(String, Attrs<'a>)],
    family: Family<'_>,
    width: f32,
    height: f32,
) {
    let default_attrs = Attrs::new().family(family);
    let rich: Vec<(&str, Attrs)> = spans
        .iter()
        .map(|(text, attrs)| (text.as_str(), attrs.clone()))
        .collect();
    buffer.set_rich_text(font_system, rich, &default_attrs, Shaping::Advanced, None);
    buffer.set_size(font_system, Some(width), Some(height));
    buffer.shape_until_scroll(font_system, false);
}

fn fill_buffer(
    font_system: &mut FontSystem,
    buffer: &mut Buffer,
    text: &str,
    family: Family<'_>,
    color: Color,
    width: f32,
    height: f32,
) {
    let default_attrs = Attrs::new().family(family);
    let attrs = Attrs::new().family(family).color(color);
    buffer.set_rich_text(
        font_system,
        [(text, attrs)],
        &default_attrs,
        Shaping::Advanced,
        None,
    );
    buffer.set_size(font_system, Some(width), Some(height));
    buffer.shape_until_scroll(font_system, false);
}

/// Aplica métricas fijas del picker a los buffers de texto del overlay.
pub fn configure_picker_buffers(
    font_system: &mut FontSystem,
    font_family: &str,
    list_buffer: &mut Buffer,
    detail_buffer: &mut Buffer,
    footer_buffer: &mut Buffer,
) {
    let m = picker_cell_metrics(font_system, font_family);
    let metrics = glyphon::Metrics::new(m.font_size, m.cell_h);
    for buffer in [list_buffer, detail_buffer, footer_buffer] {
        buffer.set_metrics(font_system, metrics);
        buffer.set_monospace_width(font_system, Some(m.cell_w));
        buffer.set_hinting(font_system, glyphon::cosmic_text::Hinting::Enabled);
        buffer.set_wrap(font_system, glyphon::cosmic_text::Wrap::None);
    }
}

/// Añade `TextArea`s del overlay a `extra_areas`.
#[expect(
    clippy::too_many_arguments,
    reason = "three overlay buffers plus layout metrics"
)]
pub fn push_text_areas<'a>(
    list_buffer: &'a Buffer,
    detail_buffer: &'a Buffer,
    footer_buffer: &'a Buffer,
    extra_areas: &mut Vec<TextArea<'a>>,
    list_w: f32,
    surface_w: u32,
    surface_h: u32,
    cell_h: f32,
    default_fg: Color,
) {
    let layout = palette_layout(cell_h);
    let bounds = TextBounds {
        left: 0,
        top: 0,
        right: surface_w as i32,
        bottom: surface_h as i32,
    };
    extra_areas.push(TextArea {
        buffer: list_buffer,
        left: cell_h * 0.5,
        top: layout.text_top,
        scale: 1.0,
        bounds,
        default_color: default_fg,
        custom_glyphs: &[],
    });
    extra_areas.push(TextArea {
        buffer: detail_buffer,
        left: list_w + cell_h,
        top: layout.text_top,
        scale: 1.0,
        bounds,
        default_color: default_fg,
        custom_glyphs: &[],
    });
    extra_areas.push(TextArea {
        buffer: footer_buffer,
        left: cell_h * 0.5,
        top: surface_h as f32 - cell_h * 1.8,
        scale: 1.0,
        bounds,
        default_color: default_fg,
        custom_glyphs: &[],
    });
}

fn solid_quad(left: f32, top: f32, width: f32, height: f32, color: Color) -> glyphon::CustomGlyph {
    glyphon::CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left,
        top,
        width,
        height,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: LAYER_OVERLAY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ThemeConfig;

    #[test]
    fn swatches_cubren_16_colores() {
        let picker = ThemePickerState::open(&ThemeConfig::default(), None, None);
        let glyphs = build_custom_glyphs(&picker, &ThemeConfig::default(), 10.0, 20.0, 800, 600);
        assert_eq!(glyphs.len(), 20);
    }

    #[test]
    fn paleta_queda_debajo_de_etiqueta() {
        let cell_h = 20.0;
        let layout = palette_layout(cell_h);
        let label_bottom = layout.text_top + 3.0 * cell_h;
        assert!(
            layout.swatch_y0 > label_bottom,
            "swatch_y0={} debe quedar bajo la etiqueta (bottom={label_bottom})",
            layout.swatch_y0
        );
        assert!(layout.samples_y > layout.swatch_y0 + layout.swatch_block_h);
    }

    #[test]
    fn picker_font_mas_grande_que_defecto_terminal() {
        let mut fs = FontSystem::new();
        let m = picker_cell_metrics(&mut fs, "monospace");
        assert!(m.font_size >= PICKER_FONT_SIZE);
        assert!(m.cell_h > m.font_size);
    }

    #[test]
    fn sample_glyphs_nord_cobalt2_solarized() {
        use crate::config::try_preset;
        use crate::renderer::GlyphCache;

        let mut fs = FontSystem::new();
        let mut swash = glyphon::SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut contrast_cache = ContrastCache::default();
        let metrics = picker_cell_metrics(&mut fs, "monospace");

        for name in ["nord", "cobalt2", "solarized-dark"] {
            let theme = try_preset(name).unwrap();
            build_sample_custom_glyphs(
                &theme,
                theme.bold_is_bright,
                &metrics,
                "monospace",
                100.0,
                200.0,
                &mut fs,
                &mut swash,
                &mut glyph_cache,
                &mut contrast_cache,
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
