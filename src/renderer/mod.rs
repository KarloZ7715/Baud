//! Modulo de render GPU del grid dinamico.

mod blink;
mod builtin;
mod cell_renderer;
mod decorations;
mod display_list;
mod geometry;
mod glyph;
mod glyph_cache;
pub mod limits;
mod metrics;
mod palette;
mod terminal_fallback;

pub use blink::blink_on;
pub use palette::{ColorOverrides, Palette};

/// Base de ids reservados para box/block glyphs programaticos (sobre ids de cache).
pub const BOX_GLYPH_ID_BASE: u16 = 0xF000;
/// Slots reservados: cubre U+2500..=U+259F (box-drawing + block elements).
pub const BOX_GLYPH_ID_COUNT: u16 = 0xA0;

use limits::{custom_pixels, MAX_CUSTOM_GLYPH_PIXELS};

/// Alpha del clear de frame (0..=1), lineal con `window.opacity`.
pub fn frame_clear_alpha(window_opacity: f32) -> f64 {
    window_opacity.clamp(0.0, 1.0) as f64
}

/// Color de clear premultiplicado: fondo del tema con opacidad uniforme en toda la ventana.
pub fn frame_clear_color(bg: (u8, u8, u8), window_opacity: f32) -> wgpu::Color {
    let a = frame_clear_alpha(window_opacity);
    let r = bg.0 as f64 / 255.0;
    let g = bg.1 as f64 / 255.0;
    let b = bg.2 as f64 / 255.0;
    wgpu::Color {
        r: r * a,
        g: g * a,
        b: b * a,
        a,
    }
}

/// En debug, detecta CustomGlyph con dimensiones que harian crecer el atlas a 256GB+.
fn debug_assert_custom_glyphs_bounded(custom_glyphs: &[glyphon::CustomGlyph]) {
    for (i, g) in custom_glyphs.iter().enumerate() {
        let px = custom_pixels(g.width, g.height);
        if px > MAX_CUSTOM_GLYPH_PIXELS {
            panic!(
                "CustomGlyph[{i}] id={} size={}x{} px={px} excede limite",
                g.id, g.width, g.height
            );
        }
    }
}

pub use builtin::{
    box_mask, clear_cache as clear_builtin_cache, is_box_glyph, is_box_mask_supported,
    render_uncached as render_builtin_uncached, supports as supports_builtin_glyph,
};
pub use cell_renderer::CellRenderer;
pub use display_list::{DisplayList, DisplayListBuilder};
pub use geometry::CellGeometry;
pub use glyph::{is_wide_continuation, resolve_glyph_key, shape_glyph, GlyphKey, ShapedGlyph};
pub use glyph_cache::{CachedGlyph, CachedRaster, GlyphCache};
pub use metrics::CellMetrics;

use std::sync::Arc;
use std::time::Instant;

use crate::ansi::{Color, Term};
use crate::config::{parse_hex, FontConfig, GlyphOffset, ThemeConfig};
use crate::grid::{Cell, DamageSnapshot};
use glyphon::cosmic_text::Hinting;
use winit::window::Window;

/// Renderer GPU del terminal virtual.
///
/// Mantiene los recursos wgpu y glyphon necesarios para pintar el grid dinamico.
/// Los campos son privados: se inicializa via `Renderer::new` y se consume
/// via `render` y `resize`.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    wgpu_cache: glyphon::Cache,
    font_system: glyphon::FontSystem,
    atlas: glyphon::TextAtlas,
    viewport: glyphon::Viewport,
    text_renderer: glyphon::TextRenderer,
    swash_cache: glyphon::SwashCache,
    /// Buffer para overlay de status (renderizado encima del grid).
    overlay_buffer: glyphon::Buffer,
    /// Buffer vacio solo para custom_glyphs de fondo (evita doble dibujo de fila 0).
    bg_buffer: glyphon::Buffer,
    // ponytail: cell_w y cell_h se calculan en new() y se actualizan en resize().
    // El renderer los usa para posicionar cada TextArea.
    pub cell_w: f32,
    pub cell_h: f32,
    // ponytail: flag del overlay. Se activa con set_status(), se desactiva
    // cuando se llama con texto vacio o cuando se hace render() sin status.
    status_active: bool,
    /// Instant en que se activo el status overlay, para auto-desaparicion.
    status_start: Option<Instant>,
    frame_count: u64,
    /// Rango normalizado de seleccion del frame anterior (start_row, start_col,
    /// end_row, end_col). Cuando cambia, invalida damage en filas afectadas.
    prev_selection_bounds: Option<(usize, usize, usize, usize)>,
    /// Offset de scrollback del frame anterior (invalida cache si cambia con seleccion).
    prev_scrollback_offset: isize,
    /// Familia tipografica desde la configuracion.
    font_family: String,
    /// Tamano de fuente desde la configuracion (en puntos).
    font_size: f32,
    /// Metricas de celda (ancho, alto, offsets).
    cell_metrics: CellMetrics,
    /// Cache de glifos para el renderer celda-determinista.
    glyph_cache: GlyphCache,
    /// Display list reutilizada entre frames.
    display_list: DisplayList,
    line_height: f32,
    glyph_offset: GlyphOffset,
    builtin_box_drawing: bool,
}

impl Renderer {
    /// Fuerza avance horizontal uniforme (1 celda de grid = `cell_w` px).
    fn apply_monospace_grid(
        font_system: &mut glyphon::FontSystem,
        buffer: &mut glyphon::Buffer,
        cell_w: f32,
    ) {
        Self::configure_buffer(font_system, buffer, cell_w);
    }

    fn configure_buffer(
        font_system: &mut glyphon::FontSystem,
        buffer: &mut glyphon::Buffer,
        cell_w: f32,
    ) {
        buffer.set_monospace_width(font_system, Some(cell_w));
        buffer.set_hinting(font_system, Hinting::Enabled);
    }

    pub fn cell_w(&self) -> f32 {
        self.cell_w
    }

    pub fn cell_h(&self) -> f32 {
        self.cell_h
    }

    /// Inicializa wgpu, glyphon y la surface configuration.
    pub fn new(
        _window: Arc<Window>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        font_config: &FontConfig,
    ) -> Self {
        let mut font_system =
            terminal_fallback::create_font_system_with_fallback(&font_config.fallback);
        // Cache necesario para glyphon 0.11
        let wgpu_cache = glyphon::Cache::new(&device);
        let mut atlas = glyphon::TextAtlas::new(&device, &queue, &wgpu_cache, config.format);
        // Inicializar viewport con la resolución REAL de la surface.
        // glyphon::Viewport::new() por defecto crea resolución (0, 0),
        // lo que clipea todo el texto. Sin este update, el primer frame
        // antes del evento Resized no renderiza ningún glyph.
        let mut viewport = glyphon::Viewport::new(&device, &wgpu_cache);
        viewport.update(
            &queue,
            glyphon::Resolution {
                width: config.width,
                height: config.height,
            },
        );
        let text_renderer = glyphon::TextRenderer::new(
            &mut atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        let swash_cache = glyphon::SwashCache::new();

        let font_size = font_config.size as f32;
        let font_family = font_config.family.clone();
        let line_height = font_config.line_height;
        let glyph_offset = font_config.glyph_offset;
        let builtin_box_drawing = font_config.builtin_box_drawing;

        let cell_metrics = CellMetrics::measure(
            &mut font_system,
            &font_family,
            font_size,
            line_height,
            glyph_offset,
        );
        let cell_w = cell_metrics.cell_w;
        let cell_h = cell_metrics.cell_h;
        let metrics = glyphon::Metrics::new(font_size, cell_h);

        let mut overlay_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut overlay_buffer, cell_w);
        let mut bg_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut bg_buffer, cell_w);

        Self {
            device,
            queue,
            surface,
            config,
            wgpu_cache,
            font_system,
            atlas,
            viewport,
            text_renderer,
            swash_cache,
            overlay_buffer,
            bg_buffer,
            cell_w,
            cell_h,
            status_active: false,
            status_start: None,
            frame_count: 0,
            prev_selection_bounds: None,
            prev_scrollback_offset: 0,
            font_family,
            font_size,
            cell_metrics,
            glyph_cache: GlyphCache::new(),
            display_list: DisplayList::default(),
            line_height,
            glyph_offset,
            builtin_box_drawing,
        }
    }

    fn refresh_cell_metrics(&mut self) {
        let pad_x = self.cell_metrics.padding_x;
        let pad_y = self.cell_metrics.padding_y;
        self.cell_metrics = CellMetrics::measure(
            &mut self.font_system,
            &self.font_family,
            self.font_size,
            self.line_height,
            self.glyph_offset,
        );
        self.cell_metrics.padding_x = pad_x;
        self.cell_metrics.padding_y = pad_y;
        self.cell_w = self.cell_metrics.cell_w;
        self.cell_h = self.cell_metrics.cell_h;
    }

    /// Margen interior del área de celdas (único origen del offset de render).
    pub fn set_content_padding(&mut self, padding_x: u16, padding_y: u16) {
        self.cell_metrics.padding_x = padding_x as f32;
        self.cell_metrics.padding_y = padding_y as f32;
    }

    pub fn content_padding(&self) -> (f32, f32) {
        (self.cell_metrics.padding_x, self.cell_metrics.padding_y)
    }

    /// Aplica un nuevo tamano de fuente y recalcula metricas de celda.
    pub fn set_font_size(&mut self, size: u16) -> (f32, f32) {
        self.font_size = size as f32;
        self.refresh_cell_metrics();
        self.reset_glyph_pipeline();
        let metrics = glyphon::Metrics::new(self.font_size, self.cell_h);
        self.overlay_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.overlay_buffer, self.cell_w);
        self.bg_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.bg_buffer, self.cell_w);
        (self.cell_w, self.cell_h)
    }

    /// Invalida caches GPU tras cambio de metricas (resize).
    fn reset_glyph_pipeline(&mut self) {
        self.glyph_cache.clear();
        builtin::clear_cache();
        self.swash_cache = glyphon::SwashCache::new();
        self.display_list.clear();
        self.atlas = glyphon::TextAtlas::new(
            &self.device,
            &self.queue,
            &self.wgpu_cache,
            self.config.format,
        );
        self.text_renderer = glyphon::TextRenderer::new(
            &mut self.atlas,
            &self.device,
            wgpu::MultisampleState::default(),
            None,
        );
    }

    /// Cambia el tamano de la surface y recrea buffers auxiliares.
    pub fn resize(&mut self, width: u32, height: u32, _rows_count: usize) {
        self.config.width = width.clamp(1, 16_384);
        self.config.height = height.clamp(1, 16_384);
        self.surface.configure(&self.device, &self.config);
        self.viewport
            .update(&self.queue, glyphon::Resolution { width, height });

        self.refresh_cell_metrics();
        self.reset_glyph_pipeline();
        let metrics = glyphon::Metrics::new(self.font_size, self.cell_h);

        self.overlay_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.overlay_buffer, self.cell_w);
        self.bg_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.bg_buffer, self.cell_w);
    }

    /// Renderiza el estado del `term` en la surface.
    #[tracing::instrument(skip(self, term))]
    pub fn render(
        &mut self,
        term: &mut Term,
        theme: &ThemeConfig,
        bold_is_bright: bool,
        window_opacity: f32,
    ) -> Result<(), String> {
        let t0 = Instant::now();

        // 1. Obtener frame de la surface
        let t_frame_start = Instant::now();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("error: validacion de surface fallo".to_string());
            }
        };
        let get_frame_us = t_frame_start.elapsed().as_secs_f64() * 1_000_000.0;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let overrides = ColorOverrides::from_term(term);
        let palette = Palette {
            theme,
            overrides: &overrides,
            bold_is_bright: bold_is_bright || theme.bold_is_bright,
        };
        let (fg_r, fg_g, fg_b) = palette.rgb(Color::Default, false);
        let default_fg_color = glyphon::Color::rgb(fg_r, fg_g, fg_b);
        let grid_damage = term.take_active_grid_damage();
        let active = term.active_grid();
        let cols_count = limits::clamp_grid_dimension(active.cols_count);
        let rows_count = limits::clamp_grid_dimension(active.rows_count);
        if active.cols_count > limits::MAX_GRID_DIM || active.rows_count > limits::MAX_GRID_DIM {
            tracing::warn!(
                raw_cols = active.cols_count,
                raw_rows = active.rows_count,
                "grid dimensions clamped to prevent OOM"
            );
        }
        let show_scrollback = term.scrollback_offset > 0;

        // Pre-computar filas del scrollback si es necesario.
        let sb_rows: Vec<&[Cell]> = if show_scrollback {
            let sb_offset = term.scrollback_offset as usize;
            let sb_len = term.grid.scrollback.len();
            let sb_start = sb_len.saturating_sub(sb_offset);
            term.grid
                .scrollback
                .range(sb_start..)
                .map(|r| r.as_slice())
                .collect()
        } else {
            Vec::new()
        };

        // Modelo: viewport sobre buffer virtual [scrollback + grid].
        let row_sources: Vec<&[Cell]> = (0..rows_count)
            .map(|row| {
                if show_scrollback {
                    let sb_len = term.grid.scrollback.len();
                    let offset = term.scrollback_offset as usize;
                    let viewport_start = sb_len.saturating_sub(offset);
                    let virtual_row = viewport_start + row;
                    if virtual_row < sb_len {
                        sb_rows[virtual_row - viewport_start]
                    } else {
                        let grid_row = virtual_row - sb_len;
                        &term.grid.rows[grid_row]
                    }
                } else {
                    &active.rows[row]
                }
            })
            .collect();

        self.render_cell_path(
            term,
            grid_damage,
            theme,
            &palette,
            frame,
            &view,
            encoder,
            &row_sources,
            cols_count,
            rows_count,
            show_scrollback,
            default_fg_color,
            window_opacity,
            t0,
            get_frame_us,
        )
    }

    /// Renderiza via display list + CustomGlyph.
    #[allow(clippy::too_many_arguments)]
    fn render_cell_path(
        &mut self,
        term: &Term,
        mut damage: DamageSnapshot,
        theme: &ThemeConfig,
        palette: &Palette<'_>,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        mut encoder: wgpu::CommandEncoder,
        row_sources: &[&[Cell]],
        cols_count: usize,
        rows_count: usize,
        show_scrollback: bool,
        default_fg_color: glyphon::Color,
        window_opacity: f32,
        t0: Instant,
        get_frame_us: f64,
    ) -> Result<(), String> {
        if show_scrollback {
            damage = DamageSnapshot::Full;
        }

        if !damage.is_full() && !damage.has_any_dirty() {
            damage = DamageSnapshot::Full;
        }

        let new_bounds = term.selection.as_ref().map(|s| s.normalize());
        let old_bounds = self.prev_selection_bounds;
        self.prev_selection_bounds = new_bounds;

        if new_bounds != old_bounds {
            let mut inv_min = rows_count;
            let mut inv_max = 0usize;
            for bounds in [old_bounds, new_bounds].into_iter().flatten() {
                let (sr, _, er, _) = bounds;
                for logical in sr.min(er)..=sr.max(er) {
                    if let Some(vis) = term.logical_to_visible_row(logical) {
                        inv_min = inv_min.min(vis);
                        inv_max = inv_max.max(vis);
                    }
                }
            }
            if inv_min <= inv_max {
                for row in inv_min..=inv_max.min(rows_count.saturating_sub(1)) {
                    damage.mark_row_dirty(row, cols_count);
                }
            }
        }

        if term.selection.is_some() && term.scrollback_offset != self.prev_scrollback_offset {
            damage = DamageSnapshot::Full;
        }
        self.prev_scrollback_offset = term.scrollback_offset;

        let t_build = Instant::now();
        let blink_on = crate::renderer::blink_on(
            term.last_blink_reset.elapsed(),
            std::time::Duration::from_millis(term.blink_interval_ms),
        );
        let bg_cap = self.display_list.bg_quads.capacity();
        let line_cap = self.display_list.line_quads.capacity();
        let glyph_cap = self.display_list.text_glyphs.capacity();
        if damage.is_full() {
            self.display_list.clear();
        }
        self.display_list
            .bg_quads
            .reserve(bg_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));
        self.display_list
            .line_quads
            .reserve(line_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));
        self.display_list
            .text_glyphs
            .reserve(glyph_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));

        DisplayListBuilder::build(
            &mut self.display_list,
            term,
            &self.cell_metrics,
            palette,
            theme.dim_alpha,
            row_sources,
            cols_count,
            rows_count,
            &self.font_family,
            &damage,
            show_scrollback,
            self.builtin_box_drawing,
            blink_on,
        );

        let mut custom_glyphs = Vec::new();
        CellRenderer::build_custom_glyphs(
            &self.display_list,
            &self.cell_metrics,
            palette,
            theme.dim_alpha,
            &self.font_family,
            &mut self.glyph_cache,
            &mut self.font_system,
            &mut self.swash_cache,
            &mut custom_glyphs,
        )?;
        debug_assert_custom_glyphs_bounded(&custom_glyphs);
        tracing::debug!(
            cols = cols_count,
            rows = rows_count,
            cell_w = self.cell_w,
            cell_h = self.cell_h,
            bg_quads = self.display_list.bg_quads.len(),
            text_glyphs = self.display_list.text_glyphs.len(),
            custom_glyphs = custom_glyphs.len(),
            "render build complete"
        );
        let build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;

        if let Some(start) = self.status_start {
            if start.elapsed() > std::time::Duration::from_secs(2) {
                self.status_active = false;
                self.status_start = None;
            }
        }

        let cell_w = self.cell_w;
        let mut extra_areas: Vec<glyphon::TextArea<'_>> = Vec::with_capacity(1);
        if self.status_active {
            let overlay_left = self.config.width as f32 - (23.0 * cell_w) - 10.0;
            extra_areas.push(glyphon::TextArea {
                buffer: &self.overlay_buffer,
                left: overlay_left.max(0.0),
                top: 0.0,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: self.config.width as i32,
                    bottom: self.config.height as i32,
                },
                default_color: glyphon::Color::rgb(0xf3, 0x8b, 0xa8),
                custom_glyphs: &[],
            });
        }

        let t_prepare = Instant::now();
        CellRenderer::prepare(
            &custom_glyphs,
            &mut self.font_system,
            &mut self.swash_cache,
            &self.glyph_cache,
            &mut self.text_renderer,
            &self.device,
            &self.queue,
            &mut self.atlas,
            &self.viewport,
            &self.bg_buffer,
            self.config.width,
            self.config.height,
            default_fg_color,
            &extra_areas,
        )?;
        let prepare_us = t_prepare.elapsed().as_secs_f64() * 1_000_000.0;

        let t_gpu = Instant::now();
        let (bg_r, bg_g, bg_b) = palette.bg_rgb(Color::Default);
        let clear_color = frame_clear_color((bg_r, bg_g, bg_b), window_opacity);
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cell renderer pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut render_pass)
                .map_err(|e| format!("error al renderizar cell renderer: {e}"))?;
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        let gpu_us = t_gpu.elapsed().as_secs_f64() * 1_000_000.0;

        let total_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(30) {
            tracing::info!(
                "[RENDER_PERF] frame={} mode=cell total={:.0}us get_frame={:.0}us build={:.0}us prepare={:.0}us gpu={:.0}us rows={} cols={}",
                self.frame_count,
                total_us,
                get_frame_us,
                build_us,
                prepare_us,
                gpu_us,
                rows_count,
                cols_count,
            );
        }

        Ok(())
    }

    /// El overlay de status esta activo (requiere frame aunque el term no cambie).
    pub fn status_overlay_active(&self) -> bool {
        self.status_active
    }

    /// Establece el texto del overlay de status.
    ///
    /// Si `text` esta vacio, desactiva el overlay. Si no, llena el
    /// overlay_buffer con el texto y activa el flag `status_active`.
    /// El overlay se renderiza encima del grid en el proximo render().
    pub fn set_status(&mut self, text: &str) {
        if text.is_empty() {
            self.status_active = false;
            self.status_start = None;
            return;
        }

        let default_attrs = glyphon::Attrs::new().family(resolve_family(&self.font_family));
        let mut attrs = glyphon::Attrs::new().family(resolve_family(&self.font_family));
        // ponytail: color rojo para status. Refinable con theme en el futuro.
        attrs = attrs.color(glyphon::Color::rgb(0xf3, 0x8b, 0xa8));
        let spans = [(text, attrs)];

        self.overlay_buffer.set_rich_text(
            &mut self.font_system,
            spans,
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        self.overlay_buffer.set_size(
            &mut self.font_system,
            Some(self.config.width as f32),
            Some(self.config.height as f32),
        );
        Self::apply_monospace_grid(&mut self.font_system, &mut self.overlay_buffer, self.cell_w);
        self.overlay_buffer
            .shape_until_scroll(&mut self.font_system, false);

        self.status_start = Some(Instant::now());
        self.status_active = true;
    }
}

/// Mapea un `Color` a RGB usando solo el tema (sin overrides runtime).
pub(crate) fn color_rgb_from_theme(color: Color, theme: &ThemeConfig) -> (u8, u8, u8) {
    match color {
        Color::Default => parse_hex(&theme.foreground),
        Color::Black => parse_hex(&theme.black),
        Color::Red => parse_hex(&theme.red),
        Color::Green => parse_hex(&theme.green),
        Color::Yellow => parse_hex(&theme.yellow),
        Color::Blue => parse_hex(&theme.blue),
        Color::Magenta => parse_hex(&theme.magenta),
        Color::Cyan => parse_hex(&theme.cyan),
        Color::White => parse_hex(&theme.white),
        Color::BrightBlack => parse_hex(&theme.bright_black),
        Color::BrightRed => parse_hex(&theme.bright_red),
        Color::BrightGreen => parse_hex(&theme.bright_green),
        Color::BrightYellow => parse_hex(&theme.bright_yellow),
        Color::BrightBlue => parse_hex(&theme.bright_blue),
        Color::BrightMagenta => parse_hex(&theme.bright_magenta),
        Color::BrightCyan => parse_hex(&theme.bright_cyan),
        Color::BrightWhite => parse_hex(&theme.bright_white),
        Color::Indexed(n) => ansi_256_to_rgb(n, theme),
        Color::Rgb(r, g, b) => (r, g, b),
    }
}

/// Convierte un Color ANSI a `glyphon::Color` usando los valores del tema.
#[cfg_attr(not(test), expect(dead_code, reason = "usado en tests del renderer"))]
pub(crate) fn color_to_glyphon(color: Color, theme: &ThemeConfig) -> glyphon::Color {
    let (r, g, b) = color_rgb_from_theme(color, theme);
    glyphon::Color::rgb(r, g, b)
}

/// Convierte un Color ANSI a `glyphon::Color` usando los valores del tema,
/// pero mapea `Color::Default` al color de BACKGROUND del tema (no foreground).
#[cfg_attr(not(test), expect(dead_code, reason = "usado en tests del renderer"))]
pub(crate) fn color_to_glyphon_bg(color: Color, theme: &ThemeConfig) -> glyphon::Color {
    let (r, g, b) = if let Color::Default = color {
        parse_hex(&theme.background)
    } else {
        color_rgb_from_theme(color, theme)
    };
    glyphon::Color::rgb(r, g, b)
}

/// Color de fondo para celdas seleccionadas.
pub(crate) fn selection_bg_glyphon(theme: &ThemeConfig) -> glyphon::Color {
    let hex = theme.selection_bg.as_deref().unwrap_or("#c4704a");
    let (r, g, b) = parse_hex(hex);
    glyphon::Color::rgba(r, g, b, 255)
}

/// Color de texto sobre celdas seleccionadas.
pub(crate) fn selection_fg_glyphon(theme: &ThemeConfig) -> glyphon::Color {
    let hex = theme.selection_fg.as_deref().unwrap_or("#0a0a0a");
    let (r, g, b) = parse_hex(hex);
    glyphon::Color::rgb(r, g, b)
}

/// Mapea un color indexado 0-255 a RGB segun el estandar ISO-8613-3.
///
/// Los indices 0-15 usan los colores ANSI del tema; 16-231 usan un cubo 6x6x6;
/// 232-255 son 24 tonos de gris.
/// ponytail: formula estandar, sin crate de paleta de color.
fn ansi_256_to_rgb(index: u8, theme: &ThemeConfig) -> (u8, u8, u8) {
    match index {
        0 => parse_hex(&theme.black),
        1 => parse_hex(&theme.red),
        2 => parse_hex(&theme.green),
        3 => parse_hex(&theme.yellow),
        4 => parse_hex(&theme.blue),
        5 => parse_hex(&theme.magenta),
        6 => parse_hex(&theme.cyan),
        7 => parse_hex(&theme.white),
        8 => parse_hex(&theme.bright_black),
        9 => parse_hex(&theme.bright_red),
        10 => parse_hex(&theme.bright_green),
        11 => parse_hex(&theme.bright_yellow),
        12 => parse_hex(&theme.bright_blue),
        13 => parse_hex(&theme.bright_magenta),
        14 => parse_hex(&theme.bright_cyan),
        15 => parse_hex(&theme.bright_white),
        16..=231 => {
            let idx = index - 16;
            let r = idx / 36;
            let g = (idx % 36) / 6;
            let b = idx % 6;
            (r * 51, g * 51, b * 51)
        }
        232..=255 => {
            let nivel = index - 232;
            let gris = nivel * 10 + 8;
            (gris, gris, gris)
        }
    }
}

/// Construye la familia tipografica desde el nombre en configuracion.
/// Delega la resolución al nombre concreto. fontdb resuelve alias como "monospace".
pub fn resolve_family(name: &str) -> glyphon::Family<'_> {
    glyphon::Family::Name(name)
}

// ---------------------------------------------------------------------------
// Tests unitarios
// ---------------------------------------------------------------------------
//
// Tests de color mapping: verifican que color_to_glyphon mapea los colores
// ANSI a los valores del tema, propagacion SGR, e inversion de seleccion.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::{Color, Term};

    fn feed(term: &mut Term, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, data);
    }

    #[test]
    fn test_frame_clear_alpha_clamps() {
        assert!((frame_clear_alpha(0.99) - 0.99).abs() < 1e-6);
        assert_eq!(frame_clear_alpha(1.0), 1.0);
        assert_eq!(frame_clear_alpha(0.0), 0.0);
    }

    #[test]
    fn test_frame_clear_color_escala_linealmente() {
        let opaque = frame_clear_color((100, 50, 25), 1.0);
        assert!((opaque.a - 1.0).abs() < 1e-6);
        assert!((opaque.r - 100.0 / 255.0).abs() < 1e-6);

        let half = frame_clear_color((200, 0, 0), 0.5);
        assert!((half.a - 0.5).abs() < 1e-6);
        assert!((half.r - 0.5 * 200.0 / 255.0).abs() < 1e-6);
    }

    // -----------------------------------------------------------------------
    // color_to_glyphon (helper puro, sin GPU)
    // -----------------------------------------------------------------------

    #[test]
    fn test_color_mapping_all_nine() {
        let theme = ThemeConfig::default();
        let cases = [
            (Color::Default, (0xec, 0xec, 0xec)),
            (Color::Black, (0x3d, 0x3d, 0x3d)),
            (Color::Red, (0xe8, 0x5d, 0x5d)),
            (Color::Green, (0x6b, 0xbf, 0x8a)),
            (Color::Yellow, (0xd4, 0xa5, 0x74)),
            (Color::Blue, (0x6b, 0x9f, 0xd4)),
            (Color::Magenta, (0xc4, 0x7a, 0xd4)),
            (Color::Cyan, (0x5e, 0xb8, 0xb8)),
            (Color::White, (0xec, 0xec, 0xec)),
        ];
        for (color, (r, g, b)) in cases {
            let c = color_to_glyphon(color, &theme);
            assert_eq!(c.r(), r, "r para {color:?}");
            assert_eq!(c.g(), g, "g para {color:?}");
            assert_eq!(c.b(), b, "b para {color:?}");
        }
    }

    // -----------------------------------------------------------------------
    // Propagacion SGR: el parser ANSI alimenta los attrs de celda.
    // -----------------------------------------------------------------------

    #[test]
    fn test_renderer_sgr_red_text_propagates_to_cell() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31mR\x1b[0m");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.ch, 'R', "caracter en (0,0)");
        assert_eq!(cell.attrs.fg, Color::Red, "fg de celda (0,0)");
    }

    #[test]
    fn test_renderer_sgr_bold_plus_color_propagates() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[1;31mB");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.ch, 'B', "caracter en (0,0)");
        assert!(cell.attrs.bold, "bold activo");
        assert_eq!(cell.attrs.fg, Color::Red, "fg = Red");
    }

    // =====================================================================
    // Tests adversariales de seleccion e inversion de color
    // =====================================================================

    /// Default y BrightWhite deben mapear a colores distintos del tema.
    #[test]
    fn test_color_to_glyphon_default_differs_from_white() {
        let theme = ThemeConfig::default();
        let c_default = color_to_glyphon(Color::Default, &theme);
        let c_bright = color_to_glyphon(Color::BrightWhite, &theme);
        assert_ne!(c_default.r(), c_bright.r());
        assert_ne!(c_default.g(), c_bright.g());
        assert_ne!(c_default.b(), c_bright.b());
    }

    /// Verifica que todos los colores sean visibles sobre fondo negro.
    /// El renderer usa Clear(BLACK) como fondo. Si algun color mapea
    /// a valores muy oscuros, es invisible para el usuario.
    ///
    /// BUG CONOCIDO: Color::Black mapea a #45475a que es visible sobre
    /// #1e1e2e, pero podria no serlo sobre fondos mas oscuros.
    #[test]
    fn test_color_to_glyphon_all_visible_on_black() {
        let theme = ThemeConfig::default();
        let cases = [
            (Color::Default, "Default"),
            (Color::Black, "Black"),
            (Color::Red, "Red"),
            (Color::Green, "Green"),
            (Color::Yellow, "Yellow"),
            (Color::Blue, "Blue"),
            (Color::Magenta, "Magenta"),
            (Color::Cyan, "Cyan"),
            (Color::White, "White"),
        ];
        let mut all_visible = true;
        for (color, name) in cases {
            let c = color_to_glyphon(color, &theme);
            let is_visible = c.r() > 50 || c.g() > 50 || c.b() > 50;
            if !is_visible {
                all_visible = false;
                eprintln!(
                    "BUG: {name} -> RGB({},{},{}) INVISIBLE sobre fondo negro",
                    c.r(),
                    c.g(),
                    c.b()
                );
            }
        }
        assert!(
            all_visible,
            "Al menos un color mapea a valores RGB invisibles sobre fondo negro"
        );
    }

    /// Verifica que la inversion fg<->bg en seleccion produce un color
    /// diferente al original cuando fg != bg. Si produjeran el mismo color,
    /// la seleccion seria indistinguible visualmente.
    ///
    /// Replica la logica de render(): cuando selected=true, effective = bg,
    /// Verifica que el fg de celdas seleccionadas usa el background del tema.
    #[test]
    fn test_inversion_produces_different_color() {
        let theme = ThemeConfig::default();
        fn selected_fg(theme: &ThemeConfig, selected: bool, fg: Color) -> (u8, u8, u8) {
            let c = if selected {
                selection_fg_glyphon(theme)
            } else {
                color_to_glyphon(fg, theme)
            };
            (c.r(), c.g(), c.b())
        }

        let normal = selected_fg(&theme, false, Color::Red);
        let selected = selected_fg(&theme, true, Color::Red);
        assert_ne!(normal, selected, "fg seleccionado debe contrastar");
        assert_eq!(selected, (0x0a, 0x0a, 0x0a));
    }

    /// Verifica que un CellStyle con selected=true produce color diferente
    /// que con selected=false para cualquier par fg != bg.
    #[test]
    fn test_selected_cell_style_changes_color() {
        let theme = ThemeConfig::default();
        fn span_color(theme: &ThemeConfig, fg: Color, selected: bool) -> (u8, u8, u8) {
            let c = if selected {
                selection_fg_glyphon(theme)
            } else {
                color_to_glyphon(fg, theme)
            };
            (c.r(), c.g(), c.b())
        }

        let test_pairs = [
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::White,
            Color::Default,
            Color::Black,
            Color::Blue,
            Color::Cyan,
        ];
        for fg in test_pairs {
            let normal = span_color(&theme, fg, false);
            let sel = span_color(&theme, fg, true);
            assert_ne!(
                normal, sel,
                "fg={fg:?} produce mismo color con y sin seleccion"
            );
        }
    }

    // -----------------------------------------------------------------------
    // ansi_256_to_rgb
    // -----------------------------------------------------------------------

    #[test]
    fn test_ansi_256_to_rgb_standard() {
        let theme = ThemeConfig::default();
        assert_eq!(ansi_256_to_rgb(16, &theme), (0, 0, 0));
        assert_eq!(ansi_256_to_rgb(231, &theme), (255, 255, 255));
        assert_eq!(ansi_256_to_rgb(232, &theme), (8, 8, 8));
        assert_eq!(ansi_256_to_rgb(255, &theme), (238, 238, 238));
        assert_eq!(ansi_256_to_rgb(17, &theme), (0, 0, 51));
        assert_eq!(ansi_256_to_rgb(88, &theme), (102, 0, 0));
    }

    #[test]
    fn test_ansi_256_to_rgb_theme_colors() {
        let theme = ThemeConfig::default();
        let (r, g, b) = ansi_256_to_rgb(0, &theme);
        assert_eq!(
            (r, g, b),
            (0x3d, 0x3d, 0x3d),
            "indice 0 debe mapear al black del tema"
        );
    }

    #[test]
    fn test_color_to_glyphon_with_theme() {
        let theme = ThemeConfig::default();
        let c = color_to_glyphon(Color::Default, &theme);
        assert_eq!(c.r(), 0xec, "Default R debe ser foreground del tema");
        assert_eq!(c.g(), 0xec, "Default G debe ser foreground del tema");
        assert_eq!(c.b(), 0xec, "Default B debe ser foreground del tema");
    }

    /// Invariante de metricas que usa `Renderer::set_font_size` (sin GPU).
    #[test]
    fn set_font_size_aumenta_celda() {
        let mut fs = terminal_fallback::create_font_system();
        let fam = FontConfig::default().family;
        let offset = GlyphOffset { x: 0.0, y: 0.0 };
        let small = CellMetrics::measure(&mut fs, &fam, 12.0, 1.0, offset);
        let big = CellMetrics::measure(&mut fs, &fam, 24.0, 1.0, offset);
        assert!(big.cell_w > small.cell_w);
        assert!(big.cell_h > small.cell_h);
    }

    #[test]
    fn test_font_config_defaults() {
        let fc = FontConfig::default();
        assert_eq!(fc.family, "MesloLGS Nerd Font Mono");
        assert_eq!(fc.size, 14);
    }

    #[test]
    fn test_resolve_family_known() {
        // Ahora todos los nombres se resuelven como Family::Name,
        // delegando la resolucion de alias a fontdb.
        assert!(matches!(
            resolve_family("monospace"),
            glyphon::Family::Name("monospace")
        ));
        assert!(matches!(
            resolve_family("sans-serif"),
            glyphon::Family::Name("sans-serif")
        ));
        assert!(matches!(
            resolve_family("serif"),
            glyphon::Family::Name("serif")
        ));
        assert!(matches!(
            resolve_family("Fira Code"),
            glyphon::Family::Name(_)
        ));
        assert!(matches!(
            resolve_family("Meslo LG M"),
            glyphon::Family::Name(_)
        ));
    }

    #[test]
    fn test_selection_bg_override() {
        let theme = ThemeConfig {
            selection_bg: Some("#ff0000".into()),
            ..ThemeConfig::default()
        };
        let (r, g, b) = parse_hex(theme.selection_bg.as_ref().unwrap());
        assert_eq!((r, g, b), (255, 0, 0), "selection_bg=#ff0000 debe ser rojo");
    }

    #[test]
    fn test_selection_bg_default() {
        let theme = ThemeConfig::default();
        assert_eq!(
            theme.selection_bg,
            Some("#c4704a".into()),
            "selection_bg por defecto debe ser naranja suave"
        );
        let c = selection_bg_glyphon(&theme);
        assert_eq!(c.r(), 0xc4);
        assert_eq!(c.g(), 0x70);
        assert_eq!(c.b(), 0x4a);
        let fg = selection_fg_glyphon(&theme);
        assert_eq!(fg.r(), 0x0a);
    }

    // -----------------------------------------------------------------------
    // Background quads via glyphon (full-block chars)
    // -----------------------------------------------------------------------

    /// Verifica que color_to_glyphon_bg mapea Color::Default al background
    /// del tema (#1e1e2e), no al foreground.
    #[test]
    fn test_color_to_glyphon_bg_default_is_background() {
        let theme = ThemeConfig::default();
        let c = color_to_glyphon_bg(Color::Default, &theme);
        assert_eq!(c.r(), 0x0a, "Default bg R debe ser background del tema");
        assert_eq!(c.g(), 0x0a, "Default bg G debe ser background del tema");
        assert_eq!(c.b(), 0x0a, "Default bg B debe ser background del tema");
    }

    /// Verifica que color_to_glyphon_bg mapea colores ANSI igual que
    /// color_to_glyphon (solo difiere en Color::Default).
    #[test]
    fn test_color_to_glyphon_bg_red_maps_to_theme_red() {
        let theme = ThemeConfig::default();
        let c = color_to_glyphon_bg(Color::Red, &theme);
        assert_eq!(c.r(), 0xe8, "Red bg R");
        assert_eq!(c.g(), 0x5d, "Red bg G");
        assert_eq!(c.b(), 0x5d, "Red bg B");
    }

    /// Verifica que color_to_glyphon_bg y color_to_glyphon producen
    /// colores diferentes para Color::Default pero iguales para colores concretos.
    #[test]
    fn test_color_to_glyphon_bg_differs_from_fg_for_default() {
        let theme = ThemeConfig::default();
        let bg = color_to_glyphon_bg(Color::Default, &theme);
        let fg = color_to_glyphon(Color::Default, &theme);
        assert_ne!(
            (bg.r(), bg.g(), bg.b()),
            (fg.r(), fg.g(), fg.b()),
            "color_to_glyphon_bg(Default) debe diferir de color_to_glyphon(Default)"
        );
        let bg_red = color_to_glyphon_bg(Color::Red, &theme);
        let fg_red = color_to_glyphon(Color::Red, &theme);
        assert_eq!(
            (bg_red.r(), bg_red.g(), bg_red.b()),
            (fg_red.r(), fg_red.g(), fg_red.b()),
            "color_to_glyphon_bg(Red) debe coincidir con color_to_glyphon(Red)"
        );
    }

    /// Verifica que el parser ANSI propaga bg al Cell.attrs.bg (SGR 41-47).
    #[test]
    fn test_sgr_bg_propagates_to_cell_attrs() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[41mR\x1b[0m");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.ch, 'R');
        assert_eq!(cell.attrs.bg, Color::Red, "SGR 41 debe setear bg=Red");
    }

    /// Verifica que celdas con diferente bg producen spans separados.
    /// Si el renderer no rompiera el span al cambiar bg, ambos caracteres
    /// se renderizarian con el mismo color de fondo.
    #[test]
    fn test_sgr_bg_change_breaks_span() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[41mA\x1b[44mB");
        let cell_a = &term.active_grid().rows[0][0];
        let cell_b = &term.active_grid().rows[0][1];
        assert_eq!(cell_a.attrs.bg, Color::Red, "A debe tener bg=Red");
        assert_eq!(cell_b.attrs.bg, Color::Blue, "B debe tener bg=Blue");
        assert_ne!(
            cell_a.attrs.bg, cell_b.attrs.bg,
            "A y B deben tener bg diferente -> spans separados"
        );
    }

    /// Verifica que la inversion fg<->bg por seleccion tambien afecta al bg
    /// efectivo: cuando selected=true, effective_bg debe ser el fg original.
    #[test]
    fn test_selection_inverts_effective_bg() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31;44mX");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.attrs.fg, Color::Red);
        assert_eq!(cell.attrs.bg, Color::Blue);

        let effective_fg_selected = cell.attrs.bg;
        let effective_bg_selected = cell.attrs.fg;
        assert_eq!(effective_fg_selected, Color::Blue);
        assert_eq!(effective_bg_selected, Color::Red);
        assert_ne!(
            effective_fg_selected, effective_bg_selected,
            "Con seleccion, effective_fg != effective_bg"
        );
    }

    // -----------------------------------------------------------------------
    // CustomGlyph background quads (reemplazo de bg_buffers)
    // -----------------------------------------------------------------------

    /// Verifica que la generacion de CustomGlyph para fondos de celda
    /// produce los quads correctos: posicion, tamano, color y cantidad.
    #[test]
    fn test_custom_glyph_bg_quads_basic() {
        let theme = ThemeConfig::default();
        let cell_w = 10.0;
        let cell_h = 20.0;

        // Crear 2 filas x 3 columnas de celdas con bg conocidos.
        // Fila 0: bg=Red, bg=Default, bg=Blue
        // Fila 1: bg=Default, bg=Green, bg=Default
        let mut row0 = vec![Cell::default(); 3];
        let mut row1 = vec![Cell::default(); 3];
        row0[0].attrs.bg = Color::Red;
        row0[2].attrs.bg = Color::Blue;
        row1[1].attrs.bg = Color::Green;

        let row_sources: Vec<&[Cell]> = vec![&row0, &row1];
        let row_empty: Vec<bool> = vec![false, false];
        let cols_count = 3;

        // Replicar la logica de generacion de bg_quads de render()
        let mut bg_quads: Vec<glyphon::CustomGlyph> = Vec::new();
        for (row, source_row) in row_sources.iter().enumerate() {
            if row_empty[row] {
                continue;
            }
            for col in 0..cols_count {
                let default_cell = Cell::default();
                let cell = source_row.get(col).unwrap_or(&default_cell);
                if cell.attrs.bg != Color::Default {
                    let bg_color = color_to_glyphon_bg(cell.attrs.bg, &theme);
                    bg_quads.push(glyphon::CustomGlyph {
                        id: 0,
                        left: col as f32 * cell_w,
                        top: row as f32 * cell_h,
                        width: cell_w,
                        height: cell_h,
                        color: Some(glyphon::Color::rgba(
                            bg_color.r(),
                            bg_color.g(),
                            bg_color.b(),
                            255,
                        )),
                        snap_to_physical_pixel: true,
                        metadata: 0,
                    });
                }
            }
        }

        assert_eq!(bg_quads.len(), 3, "3 celdas con bg != Default => 3 quads");

        // Quad 0: fila 0, col 0 -> bg=Red
        assert_eq!(bg_quads[0].left, 0.0, "Quad0.left");
        assert_eq!(bg_quads[0].top, 0.0, "Quad0.top");
        assert_eq!(bg_quads[0].width, cell_w, "Quad0.width");
        assert_eq!(bg_quads[0].height, cell_h, "Quad0.height");
        let bg0 = color_to_glyphon_bg(Color::Red, &theme);
        assert_eq!(
            bg_quads[0].color,
            Some(glyphon::Color::rgba(bg0.r(), bg0.g(), bg0.b(), 255)),
            "Quad0.color = Red"
        );

        // Quad 1: fila 0, col 2 -> bg=Blue
        assert_eq!(bg_quads[1].left, 2.0 * cell_w, "Quad1.left");
        assert_eq!(bg_quads[1].top, 0.0, "Quad1.top");
        let bg1 = color_to_glyphon_bg(Color::Blue, &theme);
        assert_eq!(
            bg_quads[1].color,
            Some(glyphon::Color::rgba(bg1.r(), bg1.g(), bg1.b(), 255)),
            "Quad1.color = Blue"
        );

        // Quad 2: fila 1, col 1 -> bg=Green
        assert_eq!(bg_quads[2].left, 1.0 * cell_w, "Quad2.left");
        assert_eq!(bg_quads[2].top, 1.0 * cell_h, "Quad2.top");
        let bg2 = color_to_glyphon_bg(Color::Green, &theme);
        assert_eq!(
            bg_quads[2].color,
            Some(glyphon::Color::rgba(bg2.r(), bg2.g(), bg2.b(), 255)),
            "Quad2.color = Green"
        );

        // Verificar metadata e id de fondo solido
        for (i, q) in bg_quads.iter().enumerate() {
            assert!(q.snap_to_physical_pixel, "Quad{i}: snap enabled");
            assert_eq!(q.metadata, 0, "Quad{i}: metadata=0");
            assert_eq!(q.id, 0, "Quad{i}: id=0");
        }
    }

    /// Verifica que celdas seleccionadas con bg=Default generan quad de seleccion.
    #[test]
    fn test_selection_bg_quad_on_default_cell() {
        let theme = ThemeConfig::default();
        let cell_w = 10.0;
        let cell_h = 20.0;
        let row = [Cell::default(); 3];
        let is_selected = |_: usize, col: usize| col == 1;

        let mut bg_quads: Vec<glyphon::CustomGlyph> = Vec::new();
        for (col, cell) in row.iter().enumerate().take(3) {
            if is_selected(0, col) {
                bg_quads.push(glyphon::CustomGlyph {
                    id: 0,
                    left: col as f32 * cell_w,
                    top: 0.0,
                    width: cell_w,
                    height: cell_h,
                    color: Some(selection_bg_glyphon(&theme)),
                    snap_to_physical_pixel: true,
                    metadata: 0,
                });
            } else if cell.attrs.bg != Color::Default {
                let bg_color = color_to_glyphon_bg(cell.attrs.bg, &theme);
                bg_quads.push(glyphon::CustomGlyph {
                    id: 0,
                    left: col as f32 * cell_w,
                    top: 0.0,
                    width: cell_w,
                    height: cell_h,
                    color: Some(glyphon::Color::rgba(
                        bg_color.r(),
                        bg_color.g(),
                        bg_color.b(),
                        255,
                    )),
                    snap_to_physical_pixel: true,
                    metadata: 0,
                });
            }
        }

        assert_eq!(bg_quads.len(), 1, "solo col 1 seleccionada");
        let sel = selection_bg_glyphon(&theme);
        assert_eq!(bg_quads[0].left, cell_w);
        assert_eq!(bg_quads[0].color, Some(sel));
    }

    /// Verifica que filas vacias NO generan CustomGlyph.
    #[test]
    fn test_custom_glyph_empty_rows_produce_no_quads() {
        let theme = ThemeConfig::default();
        let cell_w = 10.0;
        let cell_h = 20.0;

        let row0 = vec![Cell::default(); 3]; // todas Default
        let row_sources: Vec<&[Cell]> = vec![&row0];
        let row_empty: Vec<bool> = vec![true]; // marcada como vacia
        let cols_count = 3;

        let mut bg_quads: Vec<glyphon::CustomGlyph> = Vec::new();
        for (row, source_row) in row_sources.iter().enumerate() {
            if row_empty[row] {
                continue;
            }
            for col in 0..cols_count {
                let default_cell = Cell::default();
                let cell = source_row.get(col).unwrap_or(&default_cell);
                if cell.attrs.bg != Color::Default {
                    let bg_color = color_to_glyphon_bg(cell.attrs.bg, &theme);
                    bg_quads.push(glyphon::CustomGlyph {
                        id: 0,
                        left: col as f32 * cell_w,
                        top: row as f32 * cell_h,
                        width: cell_w,
                        height: cell_h,
                        color: Some(glyphon::Color::rgba(
                            bg_color.r(),
                            bg_color.g(),
                            bg_color.b(),
                            255,
                        )),
                        snap_to_physical_pixel: true,
                        metadata: 0,
                    });
                }
            }
        }

        assert!(
            bg_quads.is_empty(),
            "Filas vacias no deben generar CustomGlyph"
        );
    }
}
