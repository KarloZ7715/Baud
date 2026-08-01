//! Renderer celda-determinista via `CustomGlyph` + `prepare_with_custom`.

use glyphon::{
    ContentType, CustomGlyph, RasterizeCustomGlyphRequest, RasterizedCustomGlyph, TextArea,
    TextBounds, TextRenderer,
};

use crate::grid::DamageSnapshot;

use super::builtin;
use super::contrast::ContrastCache;
use super::decorations::{
    cursor_anchor_offset, line_quad, rasterize_line_mask, LINE_CURLY_GLYPH_ID,
    LINE_DASHED_GLYPH_ID, LINE_DOTTED_GLYPH_ID, LINE_DOUBLE_GLYPH_ID, SOLID_MASK_GLYPH_ID,
};
use super::display_list::{resolve_fg_glyphon, CursorGlyph, DisplayList, LineQuad, TextGlyph};
use super::geometry::cell_origin;
use super::glyph::{GlyphKey, GlyphStrings, ShapedGlyph};
use super::glyph_cache::GlyphCache;
use super::limits::{self, MAX_CUSTOM_GLYPH_PIXELS};
use super::metrics::CellMetrics;
use super::palette::Palette;
use super::selection_fg_glyphon;
use super::{builtin_custom_glyph_id, char_from_builtin_glyph_id};

fn line_quad_to_custom(line: &LineQuad, metrics: &CellMetrics) -> CustomGlyph {
    let mut glyph = line_quad(
        line.row,
        line.col,
        line.width_cells,
        line.kind,
        line.style,
        metrics,
        line.color,
    );
    glyph.metadata = LAYER_DECORATION;
    glyph.snap_to_physical_pixel = true;
    glyph
}

/// Convierte una display list en `CustomGlyph` y prepara el frame.
pub struct CellRenderer;

impl CellRenderer {
    /// Rota `[top, bottom]` del cache de custom glyphs por fila igual que
    /// `DisplayList::rotate_region`. A diferencia de la display list, cada
    /// `CustomGlyph` ya trae su posicion en pixeles horneada (`top`), asi
    /// que ademas de mover el slot hay que desplazar esa coordenada segun
    /// cuantas filas se movio (las filas recien expuestas por el scroll se
    /// sobrescriben de todos modos al reconstruirse, asi que el shift ahi
    /// es inofensivo aunque no se preserve).
    fn rotate_row_cache(
        row_cache: &mut [Vec<CustomGlyph>],
        region: (usize, usize),
        lines: i32,
        cell_h: f32,
    ) {
        let (top, bottom) = region;
        if top > bottom || bottom >= row_cache.len() {
            return;
        }
        let n = (lines.unsigned_abs() as usize).min(bottom - top + 1);
        if n == 0 {
            return;
        }
        let shift = if lines > 0 {
            row_cache[top..=bottom].rotate_left(n);
            -(n as f32) * cell_h
        } else {
            row_cache[top..=bottom].rotate_right(n);
            n as f32 * cell_h
        };
        for glyphs in &mut row_cache[top..=bottom] {
            for g in glyphs.iter_mut() {
                g.top += shift;
            }
        }
    }

    /// Convierte una fila de la display list en `CustomGlyph`, escribiendo en
    /// el slot cacheado de esa fila (`row_out`). Ese slot se reutiliza tal
    /// cual mientras la fila no este sucia.
    #[expect(
        clippy::too_many_arguments,
        reason = "GPU glyph build needs font + cache handles"
    )]
    fn build_row_custom_glyphs(
        display_list: &DisplayList,
        row_idx: usize,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        cursor_color: glyphon::Color,
        glyph_cache: &mut GlyphCache,
        glyph_strings: &mut GlyphStrings,
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        contrast_cache: &mut ContrastCache,
        row_out: &mut Vec<CustomGlyph>,
    ) -> Result<(), String> {
        row_out.clear();

        for bg in &display_list.bg_quads[row_idx] {
            let cg = bg_quad_to_custom(bg, metrics);
            if limits::custom_pixels(cg.width, cg.height) <= MAX_CUSTOM_GLYPH_PIXELS {
                row_out.push(cg);
            }
        }

        for line in &display_list.line_quads[row_idx] {
            row_out.push(line_quad_to_custom(line, metrics));
        }

        for &(bar_row, col) in &display_list.cursor_bars[row_idx] {
            let mut bar = super::decorations::bar_quad(bar_row, col, metrics, cursor_color);
            bar.metadata = LAYER_DECORATION;
            row_out.push(bar);
        }

        for text in &display_list.text_glyphs[row_idx] {
            text_glyph_to_customs(
                text,
                metrics,
                palette,
                dim_alpha,
                glyph_cache,
                glyph_strings,
                font_system,
                swash_cache,
                contrast_cache,
                row_out,
            )?;
        }

        Ok(())
    }

    /// Convierte una display list en `CustomGlyph` y prepara el frame.
    ///
    /// `row_cache` guarda, por fila, los `CustomGlyph` ya resueltos de un
    /// frame anterior. Solo se reconvierten las filas que `damage` marca
    /// sucias (o todas si `row_cache` no coincide en tamano con la display
    /// list, lo que tambien cubre la invalidacion total forzada por el
    /// llamante vaciando el cache). El aplanado final concatena en orden de
    /// capa (fondos, decoraciones, texto) en vez de ordenar por `metadata`,
    /// porque la insercion por fila ya deja cada fila agrupada por capa.
    #[expect(
        clippy::too_many_arguments,
        reason = "GPU glyph build needs font + cache handles"
    )]
    pub fn build_custom_glyphs(
        display_list: &DisplayList,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        glyph_cache: &mut GlyphCache,
        glyph_strings: &mut GlyphStrings,
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        contrast_cache: &mut ContrastCache,
        row_cache: &mut Vec<Vec<CustomGlyph>>,
        damage: &DamageSnapshot,
        out: &mut Vec<CustomGlyph>,
    ) -> Result<(), String> {
        let rows = display_list.bg_quads.len();
        let force_full = row_cache.len() != rows;
        if force_full {
            row_cache.clear();
            row_cache.resize_with(rows, Vec::new);
        } else if let DamageSnapshot::Scrolled { lines, region, .. } = damage {
            Self::rotate_row_cache(row_cache, *region, *lines, metrics.cell_h);
        }

        let cursor_color = {
            let (r, g, b) = palette.cursor_rgb();
            glyphon::Color::rgb(r, g, b)
        };

        #[allow(
            clippy::needless_range_loop,
            reason = "row_idx indexa row_cache, damage y display_list a la vez"
        )]
        for row_idx in 0..rows {
            if !force_full && !damage.is_full() && !damage.is_row_dirty(row_idx) {
                continue;
            }
            Self::build_row_custom_glyphs(
                display_list,
                row_idx,
                metrics,
                palette,
                dim_alpha,
                cursor_color,
                glyph_cache,
                glyph_strings,
                font_system,
                swash_cache,
                contrast_cache,
                &mut row_cache[row_idx],
            )?;
        }

        out.clear();
        out.reserve(
            row_cache.iter().map(Vec::len).sum::<usize>()
                + usize::from(display_list.cursor.is_some()),
        );
        for row in row_cache.iter() {
            out.extend(row.iter().copied().filter(|g| g.metadata == LAYER_BG));
        }
        for row in row_cache.iter() {
            out.extend(
                row.iter()
                    .copied()
                    .filter(|g| g.metadata == LAYER_DECORATION),
            );
        }
        for row in row_cache.iter() {
            out.extend(row.iter().copied().filter(|g| g.metadata == LAYER_TEXT));
        }

        if let Some(cursor) = &display_list.cursor {
            if let Some(mut glyph) = cursor_glyph_to_custom(
                cursor,
                metrics,
                palette,
                glyph_cache,
                glyph_strings,
                font_system,
                swash_cache,
            )? {
                glyph.metadata = LAYER_TEXT;
                out.push(glyph);
            }
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "glyphon prepare mirrors wgpu resource bundle"
    )]
    pub fn prepare(
        custom_glyphs: &[CustomGlyph],
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        glyph_cache: &GlyphCache,
        text_renderer: &mut TextRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut glyphon::TextAtlas,
        viewport: &glyphon::Viewport,
        empty_buffer: &glyphon::Buffer,
        surface_width: u32,
        surface_height: u32,
        default_fg: glyphon::Color,
        extra_areas: &[TextArea<'_>],
    ) -> Result<(), String> {
        let grid_area = TextArea {
            buffer: empty_buffer,
            left: 0.0,
            top: 0.0,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: surface_width as i32,
                bottom: surface_height as i32,
            },
            default_color: default_fg,
            custom_glyphs,
        };

        let mut areas: Vec<TextArea<'_>> = Vec::with_capacity(1 + extra_areas.len());
        areas.push(grid_area);
        areas.extend_from_slice(extra_areas);

        text_renderer
            .prepare_with_custom(
                device,
                queue,
                font_system,
                atlas,
                viewport,
                areas,
                swash_cache,
                |request| rasterize_custom_glyph(request, glyph_cache),
            )
            .map_err(|e| format!("error al preparar cell renderer: {e}"))?;

        Ok(())
    }
}

fn bg_quad_to_custom(bg: &super::display_list::BgQuad, metrics: &CellMetrics) -> CustomGlyph {
    let gw = metrics.geometry.cell_w as f32;
    let gh = metrics.geometry.cell_h as f32;
    let width = limits::clamp_custom_dimension(gw * bg.width_cells.min(2) as f32, gw, 2);
    let height = limits::clamp_custom_dimension(gh, gh, 1);
    let (left, top) = cell_origin(
        bg.row,
        bg.col,
        metrics.geometry,
        metrics.padding_x,
        metrics.padding_y,
    );
    CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left,
        top,
        width,
        height,
        color: Some(bg.color),
        snap_to_physical_pixel: true,
        metadata: LAYER_BG,
    }
}

/// Capa de dibujo para ordenar custom glyphs (mayor = encima).
const LAYER_BG: usize = 0;
const LAYER_DECORATION: usize = 1;
const LAYER_TEXT: usize = 2;

#[expect(
    clippy::too_many_arguments,
    reason = "GPU glyph build needs palette + cache handles"
)]
/// Escribe los `CustomGlyph` de una celda en el buffer del llamante.
///
/// Recibe `out` para evitar una asignacion de `Vec` por glifo en el camino
/// caliente; el limite de pixeles se aplica dentro de cada rama.
fn text_glyph_to_customs(
    text: &TextGlyph,
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    dim_alpha: bool,
    glyph_cache: &mut GlyphCache,
    glyph_strings: &mut GlyphStrings,
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
    contrast_cache: &mut ContrastCache,
    out: &mut Vec<CustomGlyph>,
) -> Result<(), String> {
    if text.box_glyph {
        let Some(id) = builtin_custom_glyph_id(text.glyph_key.ch) else {
            return Ok(());
        };
        let gw = metrics.geometry.cell_w as f32;
        let gh = metrics.geometry.cell_h as f32;
        let width = limits::clamp_custom_dimension(gw * text.width_cells.min(2) as f32, gw, 2);
        let height = limits::clamp_custom_dimension(gh, gh, 1);
        if limits::custom_pixels(width, height) > MAX_CUSTOM_GLYPH_PIXELS {
            return Ok(());
        }
        let fg_color = if text.selected {
            selection_fg_glyphon(palette.theme)
        } else {
            resolve_fg_glyphon(
                text.fg,
                text.dim,
                text.bold,
                palette,
                dim_alpha,
                text.contrast_bg,
                text.skip_contrast,
                contrast_cache,
            )
        };
        let (left, top) = cell_origin(
            text.row,
            text.col,
            metrics.geometry,
            metrics.padding_x,
            metrics.padding_y,
        );
        out.push(CustomGlyph {
            id,
            left,
            top,
            width,
            height,
            color: Some(fg_color),
            snap_to_physical_pixel: true,
            metadata: LAYER_TEXT,
        });
        return Ok(());
    }

    let base_id = if let Some(shaped) = &text.run_shaped {
        glyph_cache.get_or_insert_shaped(font_system, swash_cache, metrics, text.glyph_key, shaped)
    } else {
        glyph_cache.get_or_insert(
            font_system,
            swash_cache,
            metrics,
            glyph_strings,
            text.glyph_key,
        )
    };

    if let Some(cached) = glyph_cache.get_by_custom_id(base_id) {
        if let Some(cg) =
            cached_text_to_custom(text, metrics, palette, dim_alpha, contrast_cache, cached)
        {
            out.push(cg);
        }
    }

    let line_y = glyph_cache
        .get_by_custom_id(base_id)
        .map(|c| c.shaped.line_y)
        .unwrap_or(0.0);
    let overlay_count = glyph_cache.overlays(base_id).len();
    for i in 0..overlay_count {
        let overlay = glyph_cache.overlays(base_id)[i];
        let tagged = format!("{}\u{0001}ov{i}", glyph_strings.extra(text.glyph_key.extra));
        let overlay_key = GlyphKey {
            ch: text.glyph_key.ch,
            extra: glyph_strings.intern_extra(&tagged),
            bold: text.glyph_key.bold,
            italic: text.glyph_key.italic,
            dim: text.glyph_key.dim,
            family: text.glyph_key.family,
        };
        let overlay_shaped = ShapedGlyph {
            cache_key: overlay.cache_key,
            bitmap_w: overlay.bitmap_w,
            bitmap_h: overlay.bitmap_h,
            left: overlay.left,
            top: overlay.top,
            line_y,
            advance: 0.0,
            used_bold_fallback: false,
            overlays: Vec::new(),
        };
        let overlay_id = glyph_cache.get_or_insert_shaped(
            font_system,
            swash_cache,
            metrics,
            overlay_key,
            &overlay_shaped,
        );
        if let Some(overlay_cached) = glyph_cache.get_by_custom_id(overlay_id) {
            if let Some(cg) = cached_text_to_custom(
                text,
                metrics,
                palette,
                dim_alpha,
                contrast_cache,
                overlay_cached,
            ) {
                out.push(cg);
            }
        }
    }

    Ok(())
}

fn cached_text_to_custom(
    text: &TextGlyph,
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    dim_alpha: bool,
    contrast_cache: &mut ContrastCache,
    cached: &super::glyph_cache::CachedGlyph,
) -> Option<CustomGlyph> {
    if cached.raster.missing {
        return None;
    }

    // width/height DEBEN coincidir con el bitmap cacheado: rasterize_custom_glyph
    // rechaza el glifo si request y raster difieren (caracter invisible con hueco).
    let width = f32::from(cached.raster.width).max(1.0);
    let height = f32::from(cached.raster.height).max(1.0);
    if limits::custom_pixels(width, height) > MAX_CUSTOM_GLYPH_PIXELS {
        return None;
    }

    let left = if let Some(x_offset) = text.x_offset {
        x_offset + metrics.padding_x + cached.shaped.left + cached.raster.placement_left as f32
    } else {
        text.col as f32 * metrics.cell_w
            + metrics.padding_x
            + cached.shaped.left
            + cached.raster.placement_left as f32
    };
    let top = text.row as f32 * metrics.cell_h
        + metrics.padding_y
        + metrics.glyph_offset_y
        + cached.shaped.line_y
        + cached.shaped.top
        - cached.raster.placement_top as f32;

    let fg_color = if text.selected {
        selection_fg_glyphon(palette.theme)
    } else {
        resolve_fg_glyphon(
            text.fg,
            text.dim,
            text.bold,
            palette,
            dim_alpha,
            text.contrast_bg,
            text.skip_contrast,
            contrast_cache,
        )
    };

    let glyph_color = if cached.raster.content_type == ContentType::Color {
        None
    } else {
        Some(fg_color)
    };

    Some(CustomGlyph {
        id: cached.custom_glyph_id,
        left,
        top,
        width,
        height,
        color: glyph_color,
        snap_to_physical_pixel: true,
        metadata: LAYER_TEXT,
    })
}
fn cursor_glyph_to_custom(
    cursor: &CursorGlyph,
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    glyph_cache: &mut GlyphCache,
    glyph_strings: &GlyphStrings,
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
) -> Result<Option<CustomGlyph>, String> {
    let cached_id = glyph_cache.get_or_insert(
        font_system,
        swash_cache,
        metrics,
        glyph_strings,
        cursor.glyph_key,
    );

    let Some(cached) = glyph_cache.get_by_custom_id(cached_id) else {
        return Ok(None);
    };

    if cached.raster.missing {
        return Ok(None);
    }

    let width = limits::clamp_custom_dimension(f32::from(cached.raster.width), metrics.cell_w, 2);
    let height = limits::clamp_custom_dimension(f32::from(cached.raster.height), metrics.cell_h, 1);
    if limits::custom_pixels(width, height) > MAX_CUSTOM_GLYPH_PIXELS {
        return Ok(None);
    }

    let (anchor_dx, anchor_dy) = cursor_anchor_offset(cursor.style, metrics, width, height);
    let left = cursor.col as f32 * metrics.cell_w
        + metrics.padding_x
        + anchor_dx
        + cached.shaped.left
        + cached.raster.placement_left as f32;
    let top = cursor.row as f32 * metrics.cell_h
        + metrics.padding_y
        + anchor_dy
        + metrics.glyph_offset_y
        + cached.shaped.line_y
        + cached.shaped.top
        - cached.raster.placement_top as f32;

    let (r, g, b) = palette.cursor_rgb();
    let fg_color = glyphon::Color::rgb(r, g, b);

    Ok(Some(CustomGlyph {
        id: cached.custom_glyph_id,
        left,
        top,
        width,
        height,
        color: Some(fg_color),
        snap_to_physical_pixel: true,
        metadata: 0,
    }))
}

fn rasterize_custom_glyph(
    request: RasterizeCustomGlyphRequest,
    glyph_cache: &GlyphCache,
) -> Option<RasterizedCustomGlyph> {
    if let Some(ch) = char_from_builtin_glyph_id(request.id) {
        let data = builtin::render(ch, u32::from(request.width), u32::from(request.height))?;
        return Some(RasterizedCustomGlyph {
            data,
            content_type: ContentType::Mask,
        });
    }

    if request.id == SOLID_MASK_GLYPH_ID {
        if request.width == 0 || request.height == 0 {
            return None;
        }
        if request.height <= 4 {
            let data = rasterize_line_mask(request.width, request.height, SOLID_MASK_GLYPH_ID)?;
            return Some(RasterizedCustomGlyph {
                data,
                content_type: ContentType::Mask,
            });
        }
        let len = limits::safe_mask_len(request.width, request.height)?;
        return Some(RasterizedCustomGlyph {
            data: vec![255u8; len],
            content_type: ContentType::Mask,
        });
    }

    if matches!(
        request.id,
        LINE_DOUBLE_GLYPH_ID | LINE_DOTTED_GLYPH_ID | LINE_DASHED_GLYPH_ID | LINE_CURLY_GLYPH_ID
    ) {
        let data = rasterize_line_mask(request.width, request.height, request.id)?;
        return Some(RasterizedCustomGlyph {
            data,
            content_type: ContentType::Mask,
        });
    }

    let cached = glyph_cache.get_by_custom_id(request.id)?;
    if cached.raster.missing {
        let bpp = ContentType::Mask.bytes_per_pixel();
        let len = request.width as usize * request.height as usize * bpp;
        if len == 0 {
            return None;
        }
        return Some(RasterizedCustomGlyph {
            data: vec![0u8; len],
            content_type: ContentType::Mask,
        });
    }

    let content_type = cached.raster.content_type;
    let rw = cached.raster.width;
    let rh = cached.raster.height;
    let (data, norm_w, norm_h) = super::glyph_cache::normalize_raster_bytes(
        &cached.raster.data,
        rw as u32,
        rh as u32,
        content_type,
    );
    let expected = norm_w as usize * norm_h as usize * content_type.bytes_per_pixel();
    if expected == 0 || data.len() != expected {
        return None;
    }

    let req_w = request.width as usize;
    let req_h = request.height as usize;
    if norm_w as usize != req_w || norm_h as usize != req_h {
        tracing::debug!(
            id = request.id,
            req_w,
            req_h,
            norm_w,
            norm_h,
            "CustomGlyph y bitmap raster tienen dimensiones distintas"
        );
        return None;
    }

    Some(RasterizedCustomGlyph { data, content_type })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Color;
    use crate::config::FontConfig;
    use crate::renderer::{BOX_GLYPH_ID_BASE, POWERLINE_GLYPH_ID_BASE};

    use super::super::display_list::BgQuad;
    use super::super::glyph::GlyphKey;
    use super::super::terminal_fallback::create_font_system;

    fn test_metrics() -> (glyphon::FontSystem, CellMetrics) {
        let mut font_system = create_font_system();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        (font_system, metrics)
    }

    #[test]
    fn rasterize_box_glyph_usa_box_mask() {
        let request = RasterizeCustomGlyphRequest {
            id: BOX_GLYPH_ID_BASE,
            width: 10,
            height: 20,
            x_bin: glyphon::SubpixelBin::Zero,
            y_bin: glyphon::SubpixelBin::Zero,
            scale: 1.0,
        };
        let cache = GlyphCache::new();
        let out = rasterize_custom_glyph(request, &cache).expect("box glyph");
        assert_eq!(out.content_type, ContentType::Mask);
        assert_eq!(out.data.len(), 200);
        assert!(out.data[100] > 0);
    }

    #[test]
    fn rasterize_box_id_roundtrip_junction() {
        let ch = '\u{253C}';
        let id = BOX_GLYPH_ID_BASE + (ch as u32 - 0x2500) as u16;
        let request = RasterizeCustomGlyphRequest {
            id,
            width: 12,
            height: 24,
            x_bin: glyphon::SubpixelBin::Zero,
            y_bin: glyphon::SubpixelBin::Zero,
            scale: 1.0,
        };
        let out = rasterize_custom_glyph(request, &GlyphCache::new()).expect("junction");
        assert_eq!(out.data.len(), 12 * 24);
        assert!(out.data[12 * 12 + 6] > 0);
    }

    #[test]
    fn text_glyph_to_custom_box_sin_glyph_cache() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = CellMetrics {
            geometry: super::super::geometry::CellGeometry::from_u32(10, 20),
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 14.0,
            underline_position: 1.0,
            underline_thickness: 1.0,
            glyph_offset_x: 4.0,
            glyph_offset_y: 2.0,
            padding_x: 2.0,
            padding_y: 3.0,
        };
        let ch = '\u{250C}';
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();
        let text = TextGlyph {
            row: 2,
            col: 1,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch,
                extra: 0,
                bold: false,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Green,
            bold: false,
            dim: false,
            contrast_bg: bg,
            skip_contrast: false,
            custom_id: 0,
            selected: false,
            box_glyph: true,
            x_offset: None,
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("box glyph");

        assert!(cache.is_empty(), "box_glyph no debe insertar en GlyphCache");
        assert_eq!(cg.id, builtin_custom_glyph_id(ch).expect("id"));
        assert_eq!(cg.width, 10.0);
        assert_eq!(cg.height, 20.0);
        assert_eq!(cg.left, 12.0);
        assert_eq!(cg.top, 43.0);
        assert!(cg.color.is_some());
    }

    #[test]
    fn text_glyph_powerline_usa_id_y_sin_cache() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = CellMetrics {
            geometry: super::super::geometry::CellGeometry::from_u32(10, 20),
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 14.0,
            underline_position: 1.0,
            underline_thickness: 1.0,
            glyph_offset_x: 4.0,
            glyph_offset_y: 2.0,
            padding_x: 2.0,
            padding_y: 3.0,
        };
        let ch = '\u{E0B0}';
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();
        let text = TextGlyph {
            row: 0,
            col: 0,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch,
                extra: 0,
                bold: false,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Green,
            bold: false,
            dim: false,
            contrast_bg: bg,
            skip_contrast: true,
            custom_id: 0,
            selected: false,
            box_glyph: true,
            x_offset: None,
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("powerline");

        assert!(cache.is_empty());
        assert_eq!(cg.id, POWERLINE_GLYPH_ID_BASE);
        let request = RasterizeCustomGlyphRequest {
            id: cg.id,
            width: 10,
            height: 20,
            x_bin: glyphon::SubpixelBin::Zero,
            y_bin: glyphon::SubpixelBin::Zero,
            scale: 1.0,
        };
        let out = rasterize_custom_glyph(request, &GlyphCache::new()).expect("raster");
        assert_eq!(out.data.len(), 200);
        assert!(out.data.iter().any(|&p| p > 0));
    }

    #[test]
    fn bg_quad_uses_solid_glyph_id() {
        let metrics = CellMetrics {
            geometry: super::super::geometry::CellGeometry::from_u32(10, 20),
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 14.0,
            underline_position: 1.0,
            underline_thickness: 1.0,
            glyph_offset_x: 0.0,
            glyph_offset_y: 0.0,
            padding_x: 0.0,
            padding_y: 0.0,
        };
        let bg = BgQuad {
            row: 1,
            col: 2,
            width_cells: 1,
            color: glyphon::Color::rgb(255, 0, 0),
        };
        let cg = bg_quad_to_custom(&bg, &metrics);
        assert_eq!(cg.id, SOLID_MASK_GLYPH_ID);
        assert_eq!(cg.left, 20.0);
        assert_eq!(cg.top, 20.0);
        assert_eq!(cg.width, 10.0);
        assert_eq!(cg.height, 20.0);
    }

    #[test]
    fn rasterize_solid_bg_produces_mask() {
        let request = RasterizeCustomGlyphRequest {
            id: SOLID_MASK_GLYPH_ID,
            width: 4,
            height: 20,
            x_bin: glyphon::SubpixelBin::Zero,
            y_bin: glyphon::SubpixelBin::Zero,
            scale: 1.0,
        };
        let cache = GlyphCache::new();
        let out = rasterize_custom_glyph(request, &cache).expect("solid bg");
        assert_eq!(out.content_type, ContentType::Mask);
        assert_eq!(out.data.len(), 80);
        assert!(out.data.iter().all(|&b| b == 255));
    }

    #[test]
    fn text_glyph_to_custom_resolves_cache_id() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();

        let text = TextGlyph {
            row: 0,
            col: 0,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch: 'A',
                extra: 0,
                bold: false,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Default,
            bold: false,
            dim: false,
            contrast_bg: bg,
            skip_contrast: false,
            custom_id: 0,
            selected: false,
            box_glyph: false,
            x_offset: None,
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("Some glyph");

        assert!(
            cg.id >= 8,
            "ids de texto empiezan en 8 (0-7 reservados para decoracion)"
        );
        assert!(cg.width >= 1.0);
        assert!(cg.height >= 1.0);
        assert!(cg.color.is_some(), "glifo mask lleva tinte de foreground");
    }

    #[test]
    fn bold_text_glyph_quad_matches_raster_dims() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();

        let text = TextGlyph {
            row: 1,
            col: 3,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch: 'W',
                extra: 0,
                bold: true,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Default,
            bold: true,
            dim: false,
            contrast_bg: bg,
            skip_contrast: false,
            custom_id: 0,
            selected: false,
            box_glyph: false,
            x_offset: None,
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("bold W");

        let cached = cache.get_by_custom_id(cg.id).expect("en cache");
        assert_eq!(
            cg.width,
            f32::from(cached.raster.width),
            "clampear el quad rompe rasterize (glifo invisible)"
        );
        assert_eq!(cg.height, f32::from(cached.raster.height));

        let out = rasterize_custom_glyph(
            RasterizeCustomGlyphRequest {
                id: cg.id,
                width: cached.raster.width,
                height: cached.raster.height,
                x_bin: glyphon::SubpixelBin::Zero,
                y_bin: glyphon::SubpixelBin::Zero,
                scale: 1.0,
            },
            &cache,
        );
        assert!(
            out.is_some(),
            "rasterize con dims del bitmap debe funcionar"
        );

        // Si el quad se clampea a la celda, rasterize falla y el caracter desaparece.
        let cell_w = metrics.cell_w.round().max(1.0) as u16;
        let cell_h = metrics.cell_h.round().max(1.0) as u16;
        if cached.raster.width > cell_w || cached.raster.height > cell_h {
            let mismatched = rasterize_custom_glyph(
                RasterizeCustomGlyphRequest {
                    id: cg.id,
                    width: cell_w.min(cached.raster.width),
                    height: cell_h.min(cached.raster.height),
                    x_bin: glyphon::SubpixelBin::Zero,
                    y_bin: glyphon::SubpixelBin::Zero,
                    scale: 1.0,
                },
                &cache,
            );
            assert!(
                mismatched.is_none(),
                "dims != raster deben rechazarse (contrato actual)"
            );
        }
    }

    #[test]
    fn ligature_x_offset_keeps_run_based_left() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();

        let run_x = 1.0 * metrics.cell_w;
        let text = TextGlyph {
            row: 0,
            col: 3,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch: 'A',
                extra: 0,
                bold: false,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Default,
            bold: false,
            dim: false,
            contrast_bg: bg,
            skip_contrast: false,
            custom_id: 0,
            selected: false,
            box_glyph: false,
            x_offset: Some(run_x),
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("glyph");

        let cached = cache.get_by_custom_id(cg.id).expect("cache");
        let expected_left =
            run_x + metrics.padding_x + cached.shaped.left + cached.raster.placement_left as f32;
        assert!(
            (cg.left - expected_left).abs() < 0.01,
            "left {} != run-based {} (no reclavar a col)",
            cg.left,
            expected_left
        );
    }

    #[test]
    fn emoji_custom_glyph_sin_tinte_de_foreground() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let bg = crate::config::parse_hex(&theme.background);
        let mut contrast_cache = ContrastCache::default();

        let text = TextGlyph {
            row: 0,
            col: 0,
            width_cells: 2,
            glyph_key: GlyphKey {
                ch: '😀',
                extra: 0,
                bold: false,
                italic: false,
                dim: false,
                family: strings.intern_family(&font_config.family),
            },
            fg: Color::Default,
            bold: false,
            dim: false,
            contrast_bg: bg,
            skip_contrast: false,
            custom_id: 0,
            selected: false,
            box_glyph: false,
            x_offset: None,
            run_shaped: None,
        };

        let mut glyphs = Vec::new();
        text_glyph_to_customs(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut glyphs,
        )
        .expect("ok");
        let cg = glyphs.into_iter().next().expect("emoji rasterizado");

        assert!(
            cg.color.is_none(),
            "emoji a color no debe llevar tinte de foreground"
        );
    }

    #[test]
    fn rasterize_emoji_usa_dimensiones_del_bitmap() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let key = GlyphKey {
            ch: '😀',
            extra: 0,
            bold: false,
            italic: false,
            dim: false,
            family: strings.intern_family(&font_config.family),
        };
        let glyph_id =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
        let cached = cache.get_by_custom_id(glyph_id).expect("glifo cacheado");
        assert!(!cached.raster.missing);
        let raster_w = cached.raster.width;
        let raster_h = cached.raster.height;
        let out = rasterize_custom_glyph(
            RasterizeCustomGlyphRequest {
                id: glyph_id,
                width: raster_w,
                height: raster_h,
                x_bin: glyphon::SubpixelBin::Zero,
                y_bin: glyphon::SubpixelBin::Zero,
                scale: 1.0,
            },
            &cache,
        );
        assert!(
            out.is_some(),
            "emoji raster {}x{} (celda {}x{})",
            raster_w,
            raster_h,
            metrics.cell_w,
            metrics.cell_h
        );
    }

    #[test]
    #[ignore = "requiere fuente CJK (no disponible en CI)"]
    fn rasterize_cjk_usa_dimensiones_del_bitmap() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let key = GlyphKey {
            ch: '中',
            extra: 0,
            bold: false,
            italic: false,
            dim: false,
            family: strings.intern_family(&font_config.family),
        };
        let glyph_id =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
        let cached = cache.get_by_custom_id(glyph_id).expect("glifo cacheado");
        assert!(!cached.raster.missing);
        let raster_w = cached.raster.width;
        let raster_h = cached.raster.height;
        let out = rasterize_custom_glyph(
            RasterizeCustomGlyphRequest {
                id: glyph_id,
                width: raster_w,
                height: raster_h,
                x_bin: glyphon::SubpixelBin::Zero,
                y_bin: glyphon::SubpixelBin::Zero,
                scale: 1.0,
            },
            &cache,
        );
        assert!(
            out.is_some(),
            "CJK raster {}x{} (celda {}x{})",
            raster_w,
            raster_h,
            metrics.cell_w,
            metrics.cell_h
        );
    }

    fn row_cache_test_metrics() -> CellMetrics {
        CellMetrics {
            geometry: super::super::geometry::CellGeometry::from_u32(10, 20),
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 14.0,
            underline_position: 1.0,
            underline_thickness: 1.0,
            glyph_offset_x: 0.0,
            glyph_offset_y: 0.0,
            padding_x: 0.0,
            padding_y: 0.0,
        }
    }

    fn build_two_row_list(bg_color: glyphon::Color) -> DisplayList {
        use super::super::display_list::{LineKind, LineQuad};
        use crate::ansi::UnderlineStyle;

        let mut list = DisplayList::default();
        list.ensure_rows(2);
        list.bg_quads[0].push(BgQuad {
            row: 0,
            col: 0,
            width_cells: 1,
            color: bg_color,
        });
        list.line_quads[1].push(LineQuad {
            row: 1,
            col: 0,
            width_cells: 1,
            kind: LineKind::Under,
            style: UnderlineStyle::Single,
            color: glyphon::Color::rgb(0, 255, 0),
        });
        list
    }

    #[test]
    fn build_custom_glyphs_no_reconvierte_filas_no_sucias() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = row_cache_test_metrics();
        let mut contrast_cache = ContrastCache::default();

        let original_color = glyphon::Color::rgb(255, 0, 0);
        let mut list = build_two_row_list(original_color);
        let mut row_cache = Vec::new();
        let mut out = Vec::new();

        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &DamageSnapshot::Full,
            &mut out,
        )
        .expect("build inicial");
        assert_eq!(row_cache[0][0].color, Some(original_color));

        // Cambia el color de la fila 0 en la display list, pero el damage
        // solo marca sucia la fila 1: la fila 0 debe seguir sirviendo el
        // valor cacheado, no el nuevo.
        list.bg_quads[0][0].color = glyphon::Color::rgb(0, 0, 255);
        let damage = DamageSnapshot::Cells(vec![vec![0], vec![1]]);
        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &damage,
            &mut out,
        )
        .expect("build incremental");

        assert_eq!(
            row_cache[0][0].color,
            Some(original_color),
            "fila 0 no esta sucia: debe conservar el custom glyph cacheado"
        );
    }

    #[test]
    fn build_custom_glyphs_agrupa_por_capa_sin_ordenar() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = row_cache_test_metrics();
        let mut contrast_cache = ContrastCache::default();

        let list = build_two_row_list(glyphon::Color::rgb(255, 0, 0));
        let mut row_cache = Vec::new();
        let mut out = Vec::new();

        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &DamageSnapshot::Full,
            &mut out,
        )
        .expect("build");

        assert_eq!(out.len(), 2, "un bg (fila 0) y una decoracion (fila 1)");
        assert_eq!(
            out[0].metadata, LAYER_BG,
            "capa de fondo va primero aunque este en la fila 0"
        );
        assert_eq!(
            out[1].metadata, LAYER_DECORATION,
            "decoracion de la fila 1 va despues del fondo, no por orden de fila"
        );
    }

    #[test]
    fn build_custom_glyphs_invalida_todo_si_cambia_el_numero_de_filas() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = row_cache_test_metrics();
        let mut contrast_cache = ContrastCache::default();

        let original_color = glyphon::Color::rgb(255, 0, 0);
        let mut list = build_two_row_list(original_color);
        let mut row_cache = Vec::new();
        let mut out = Vec::new();

        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &DamageSnapshot::Full,
            &mut out,
        )
        .expect("build inicial");

        let new_color = glyphon::Color::rgb(0, 0, 255);
        list.bg_quads[0][0].color = new_color;
        list.ensure_rows(3);
        // Damage vacio (ninguna fila marcada), pero el cache tiene 2 filas y
        // la display list ahora tiene 3: el cambio de tamano debe forzar
        // reconversion total, no dejar la fila 0 con el color viejo.
        let damage = DamageSnapshot::Cells(Vec::new());
        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &damage,
            &mut out,
        )
        .expect("build tras resize");

        assert_eq!(row_cache.len(), 3);
        assert_eq!(row_cache[0][0].color, Some(new_color));
    }

    /// `CustomGlyph.top` viene horneado en pixeles (a diferencia de
    /// `BgQuad.row`, que es logico). `rotate_row_cache` debe desplazar ese
    /// valor ademas de mover el slot, o el contenido reciclado por scroll
    /// se pintaria en su fila vieja. Compara contra un rebuild completo del
    /// estado ya desplazado.
    #[test]
    fn scroll_desplaza_el_top_horneado_del_row_cache() {
        let (mut font_system, _) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let metrics = row_cache_test_metrics();
        let mut contrast_cache = ContrastCache::default();
        let color = glyphon::Color::rgb(255, 0, 0);

        let mut list = DisplayList::default();
        list.ensure_rows(3);
        list.bg_quads[1].push(BgQuad {
            row: 1,
            col: 0,
            width_cells: 1,
            color,
        });
        let mut row_cache = Vec::new();
        let mut out = Vec::new();
        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &DamageSnapshot::Full,
            &mut out,
        )
        .expect("build inicial");

        // Scroll up de 1 sobre [0,2]: la fila 1 (con el quad) pasa a la 0.
        list.rotate_region((0, 2), 1);
        let damage = DamageSnapshot::Scrolled {
            lines: 1,
            region: (0, 2),
            rest: vec![vec![0]; 3],
        };
        CellRenderer::build_custom_glyphs(
            &list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut row_cache,
            &damage,
            &mut out,
        )
        .expect("build incremental tras scroll");

        let mut expected_list = DisplayList::default();
        expected_list.ensure_rows(3);
        expected_list.bg_quads[0].push(BgQuad {
            row: 0,
            col: 0,
            width_cells: 1,
            color,
        });
        let mut expected_row_cache = Vec::new();
        let mut expected_out = Vec::new();
        CellRenderer::build_custom_glyphs(
            &expected_list,
            &metrics,
            &palette,
            theme.dim_alpha,
            &mut glyph_cache,
            &mut strings,
            &mut font_system,
            &mut swash_cache,
            &mut contrast_cache,
            &mut expected_row_cache,
            &DamageSnapshot::Full,
            &mut expected_out,
        )
        .expect("build de referencia");

        assert_eq!(row_cache[0], expected_row_cache[0]);
    }
}
