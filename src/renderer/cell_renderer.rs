//! Renderer celda-determinista via `CustomGlyph` + `prepare_with_custom`.

use glyphon::{
    ContentType, CustomGlyph, RasterizeCustomGlyphRequest, RasterizedCustomGlyph, TextArea,
    TextBounds, TextRenderer,
};

use super::decorations::{
    cursor_anchor_offset, line_quad, rasterize_line_mask, LINE_CURLY_GLYPH_ID,
    LINE_DASHED_GLYPH_ID, LINE_DOTTED_GLYPH_ID, LINE_DOUBLE_GLYPH_ID, SOLID_MASK_GLYPH_ID,
};
use super::display_list::{resolve_fg_glyphon, CursorGlyph, DisplayList, LineQuad, TextGlyph};
use super::glyph_cache::GlyphCache;
use super::limits::{self, MAX_CUSTOM_GLYPH_PIXELS};
use super::metrics::CellMetrics;
use super::palette::Palette;
use super::selection_fg_glyphon;

fn line_quad_to_custom(line: &LineQuad, metrics: &CellMetrics) -> CustomGlyph {
    line_quad(
        line.row,
        line.col,
        line.width_cells,
        line.kind,
        line.style,
        metrics,
        line.color,
    )
}

/// Convierte una display list en `CustomGlyph` y prepara el frame.
pub struct CellRenderer;

impl CellRenderer {
    #[expect(
        clippy::too_many_arguments,
        reason = "GPU glyph build needs font + cache handles"
    )]
    pub fn build_custom_glyphs(
        display_list: &DisplayList,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        dim_alpha: bool,
        font_family: &str,
        glyph_cache: &mut GlyphCache,
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        out: &mut Vec<CustomGlyph>,
    ) -> Result<(), String> {
        out.clear();
        out.reserve(
            display_list.bg_quads.len()
                + display_list.line_quads.len()
                + display_list.text_glyphs.len()
                + usize::from(display_list.cursor.is_some()),
        );

        for bg in &display_list.bg_quads {
            let cg = bg_quad_to_custom(bg, metrics);
            if limits::custom_pixels(cg.width, cg.height) <= MAX_CUSTOM_GLYPH_PIXELS {
                out.push(cg);
            }
        }

        for line in &display_list.line_quads {
            out.push(line_quad_to_custom(line, metrics));
        }

        let cursor_color = {
            let (r, g, b) = palette.cursor_rgb();
            glyphon::Color::rgb(r, g, b)
        };
        for &(row, col) in &display_list.cursor_bars {
            out.push(super::decorations::bar_quad(
                row,
                col,
                metrics,
                cursor_color,
            ));
        }

        for text in &display_list.text_glyphs {
            if let Some(glyph) = text_glyph_to_custom(
                text,
                metrics,
                palette,
                dim_alpha,
                font_family,
                glyph_cache,
                font_system,
                swash_cache,
            )? {
                if limits::custom_pixels(glyph.width, glyph.height) <= MAX_CUSTOM_GLYPH_PIXELS {
                    out.push(glyph);
                }
            }
        }

        if let Some(cursor) = &display_list.cursor {
            if let Some(glyph) = cursor_glyph_to_custom(
                cursor,
                metrics,
                palette,
                font_family,
                glyph_cache,
                font_system,
                swash_cache,
            )? {
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
    let width = limits::clamp_custom_dimension(
        metrics.cell_w * bg.width_cells.min(2) as f32,
        metrics.cell_w,
        2,
    );
    let height = limits::clamp_custom_dimension(metrics.cell_h, metrics.cell_h, 1);
    CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: bg.col as f32 * metrics.cell_w + metrics.padding_x,
        top: bg.row as f32 * metrics.cell_h + metrics.padding_y,
        width,
        height,
        color: Some(bg.color),
        snap_to_physical_pixel: true,
        metadata: 0,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "GPU glyph build needs palette + cache handles"
)]
fn text_glyph_to_custom(
    text: &TextGlyph,
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    dim_alpha: bool,
    font_family: &str,
    glyph_cache: &mut GlyphCache,
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
) -> Result<Option<CustomGlyph>, String> {
    let cached = glyph_cache.get_or_insert(
        font_system,
        swash_cache,
        metrics,
        font_family,
        text.glyph_key.clone(),
    );

    if cached.raster.missing {
        return Ok(None);
    }

    // Usar dimensiones del bitmap; deben coincidir con lo que rasterize_custom_glyph devuelve.
    let width = f32::from(cached.raster.width).max(1.0);
    let height = f32::from(cached.raster.height).max(1.0);
    if limits::custom_pixels(width, height) > MAX_CUSTOM_GLYPH_PIXELS {
        return Ok(None);
    }

    let left = text.col as f32 * metrics.cell_w
        + metrics.padding_x
        + metrics.glyph_offset_x
        + cached.raster.placement_left as f32;
    let top = text.row as f32 * metrics.cell_h
        + metrics.padding_y
        + metrics.glyph_offset_y
        + cached.shaped.line_y.round()
        + cached.shaped.top
        - cached.raster.placement_top as f32;

    let fg_color = if text.selected {
        selection_fg_glyphon(palette.theme)
    } else {
        resolve_fg_glyphon(text.fg, text.dim, text.bold, palette, dim_alpha)
    };

    // glyphon: bitmaps a color (emoji) no deben llevar tinte de foreground.
    let glyph_color = if cached.raster.content_type == ContentType::Color {
        None
    } else {
        Some(fg_color)
    };

    Ok(Some(CustomGlyph {
        id: cached.custom_glyph_id,
        left,
        top,
        width,
        height,
        color: glyph_color,
        snap_to_physical_pixel: true,
        metadata: 0,
    }))
}

fn cursor_glyph_to_custom(
    cursor: &CursorGlyph,
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    font_family: &str,
    glyph_cache: &mut GlyphCache,
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
) -> Result<Option<CustomGlyph>, String> {
    let cached = glyph_cache.get_or_insert(
        font_system,
        swash_cache,
        metrics,
        font_family,
        cursor.glyph_key.clone(),
    );

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
        + metrics.glyph_offset_x
        + cached.raster.placement_left as f32;
    let top = cursor.row as f32 * metrics.cell_h
        + metrics.padding_y
        + anchor_dy
        + metrics.glyph_offset_y
        + cached.shaped.line_y.round()
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
    fn bg_quad_uses_solid_glyph_id() {
        let metrics = CellMetrics {
            cell_w: 10.0,
            cell_h: 20.0,
            font_size: 14.0,
            baseline_y: 14.0,
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
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);

        let text = TextGlyph {
            row: 0,
            col: 0,
            width_cells: 1,
            glyph_key: GlyphKey {
                ch: 'A',
                bold: false,
                italic: false,
                dim: false,
                family: font_config.family.clone(),
            },
            fg: Color::Default,
            bold: false,
            dim: false,
            custom_id: 0,
            selected: false,
        };

        let cg = text_glyph_to_custom(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &font_config.family,
            &mut cache,
            &mut font_system,
            &mut swash_cache,
        )
        .expect("ok")
        .expect("Some glyph");

        assert!(
            cg.id >= 8,
            "ids de texto empiezan en 8 (0-7 reservados para decoracion)"
        );
        assert!(cg.width >= 1.0);
        assert!(cg.height >= 1.0);
        assert!(cg.color.is_some(), "glifo mask lleva tinte de foreground");
    }

    #[test]
    fn emoji_custom_glyph_sin_tinte_de_foreground() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let theme = crate::config::ThemeConfig::default();
        let palette = Palette::from_theme(&theme);

        let text = TextGlyph {
            row: 0,
            col: 0,
            width_cells: 2,
            glyph_key: GlyphKey {
                ch: '😀',
                bold: false,
                italic: false,
                dim: false,
                family: font_config.family.clone(),
            },
            fg: Color::Default,
            bold: false,
            dim: false,
            custom_id: 0,
            selected: false,
        };

        let cg = text_glyph_to_custom(
            &text,
            &metrics,
            &palette,
            theme.dim_alpha,
            &font_config.family,
            &mut cache,
            &mut font_system,
            &mut swash_cache,
        )
        .expect("ok")
        .expect("emoji rasterizado");

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
        let key = GlyphKey {
            ch: '😀',
            bold: false,
            italic: false,
            dim: false,
            family: font_config.family.clone(),
        };
        let cached = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics,
            &font_config.family,
            key,
        );
        assert!(!cached.raster.missing);
        let raster_w = cached.raster.width;
        let raster_h = cached.raster.height;
        let glyph_id = cached.custom_glyph_id;
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
    fn rasterize_cjk_usa_dimensiones_del_bitmap() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let font_config = FontConfig::default();
        let mut cache = GlyphCache::new();
        let key = GlyphKey {
            ch: '中',
            bold: false,
            italic: false,
            dim: false,
            family: font_config.family.clone(),
        };
        let cached = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics,
            &font_config.family,
            key,
        );
        assert!(!cached.raster.missing);
        let raster_w = cached.raster.width;
        let raster_h = cached.raster.height;
        let glyph_id = cached.custom_glyph_id;
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
}
