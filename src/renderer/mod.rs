//! Modulo de render GPU del grid dinamico.

mod blink;
mod builtin;
mod cell_renderer;
mod contrast;
mod decorations;
mod display_list;
mod geometry;
mod glyph;
mod glyph_cache;
pub mod limits;
mod metrics;
mod palette;
mod runs;
mod tab_bar;
mod terminal_fallback;

pub use blink::blink_on;
pub use contrast::{adjust_fg, ContrastCache};
pub use decorations::SOLID_MASK_GLYPH_ID;
pub use palette::{ColorOverrides, Palette};
pub use tab_bar::{
    build_inactive_hover_chrome, build_segment_chrome, build_tab_track, compute_layout,
    format_tab_label, push_close_scrub, segment_close_left_px, segment_title_label,
    shorten_tab_title, tab_bar_height_px, tab_bar_inner_width, tab_chrome_reserve_px, tab_close_at,
    tab_index_at, TabBarLayout, TabBarMouseState, TabSegment, TAB_BAR_HEIGHT_ROWS,
    TAB_CLOSE_WIDTH_CELLS, TAB_CONTENT_GAP_PX, TAB_LABEL_PAD_CELLS,
};
pub(crate) use terminal_fallback::create_font_system_with_fallback;

/// Base de ids reservados para box/block glyphs programaticos (sobre ids de cache).
/// El `GlyphCache` de texto asigna desde ids bajos; estos rangos altos quedan
/// reservados para builtins geometricos (sin solape con SOLID_MASK=0).
pub const BOX_GLYPH_ID_BASE: u16 = 0xF000;
/// Slots reservados: cubre U+2500..=U+259F (box-drawing + block elements).
pub const BOX_GLYPH_ID_COUNT: u16 = 0xA0;
/// Base de ids para separadores Powerline U+E0B0..=U+E0B3 (tras el rango box/block).
pub const POWERLINE_GLYPH_ID_BASE: u16 = BOX_GLYPH_ID_BASE + BOX_GLYPH_ID_COUNT;
/// Cuatro slots: E0B0..E0B3.
pub const POWERLINE_GLYPH_ID_COUNT: u16 = 4;

/// Id de CustomGlyph para un builtin geometrico, o None si no aplica.
pub fn builtin_custom_glyph_id(ch: char) -> Option<u16> {
    let cp = ch as u32;
    if (0x2500..=0x259F).contains(&cp) {
        Some(BOX_GLYPH_ID_BASE + (cp - 0x2500) as u16)
    } else if (0xE0B0..=0xE0B3).contains(&cp) {
        Some(POWERLINE_GLYPH_ID_BASE + (cp - 0xE0B0) as u16)
    } else {
        None
    }
}

/// Codepoint asociado a un id de builtin geometrico.
pub fn char_from_builtin_glyph_id(id: u16) -> Option<char> {
    if (BOX_GLYPH_ID_BASE..BOX_GLYPH_ID_BASE + BOX_GLYPH_ID_COUNT).contains(&id) {
        char::from_u32(0x2500 + u32::from(id - BOX_GLYPH_ID_BASE))
    } else if (POWERLINE_GLYPH_ID_BASE..POWERLINE_GLYPH_ID_BASE + POWERLINE_GLYPH_ID_COUNT)
        .contains(&id)
    {
        char::from_u32(0xE0B0 + u32::from(id - POWERLINE_GLYPH_ID_BASE))
    } else {
        None
    }
}

use limits::{custom_pixels, MAX_CUSTOM_GLYPH_PIXELS};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

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

/// Estado del preedit IME para dibujar el overlay sobre el cursor.
#[derive(Debug)]
pub struct PreeditState {
    pub text: String,
    pub row: usize,
    pub col: usize,
}

/// Un pane del layout activo a dibujar en un frame.
pub struct PaneRender {
    pub session_id: crate::session::SessionId,
    pub term: Arc<Mutex<Term>>,
    pub rect: crate::layout::Rect,
    pub focused: bool,
    /// Si false, reutiliza display list cacheada salvo que falte cache.
    pub rebuild: bool,
}

use crate::ansi::{Color, Term};
use crate::config::{parse_hex, FontConfig, GlyphOffset, StatusConfig, ThemeConfig};
use crate::grid::{Cell, DamageSnapshot};
use crate::session::SessionId;
use crate::theme_picker::ThemePickerState;
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
    /// Buffer para la barra inferior de busqueda.
    search_bar_buffer: glyphon::Buffer,
    /// Buffers del theme picker overlay.
    picker_list_buffer: glyphon::Buffer,
    picker_detail_buffer: glyphon::Buffer,
    picker_footer_buffer: glyphon::Buffer,
    /// Buffer vacio solo para custom_glyphs de fondo (evita doble dibujo de fila 0).
    bg_buffer: glyphon::Buffer,
    /// Buffer para el overlay de consentimiento de primer arranque.
    consent_buffer: glyphon::Buffer,
    /// Modal de consentimiento activo (bloquea el terminal hasta Sí/No).
    consent_active: bool,
    // ponytail: cell_w y cell_h se calculan en new() y se actualizan en resize().
    // El renderer los usa para posicionar cada TextArea.
    pub cell_w: f32,
    pub cell_h: f32,
    // ponytail: flag del overlay. Se activa con set_status(), se desactiva
    // con texto vacio o tras duration_ms en render().
    status_active: bool,
    /// Instant en que se activo el status overlay, para auto-desaparicion.
    status_start: Option<Instant>,
    /// Duracion del overlay en ms (`0` = sin auto-dismiss).
    duration_ms: u64,
    /// Colores y geometria del pill de status del frame actual.
    status_bg: glyphon::Color,
    status_fg: glyphon::Color,
    status_pill_start_col: usize,
    status_pill_cols: usize,
    /// Texto e icono activos para rellenar el buffer tras resize.
    status_message: Option<String>,
    status_icon: String,
    /// True si el overlay_buffer debe re-shapearse (mensaje/icono/resize).
    status_overlay_dirty: bool,
    /// True si hace falta un frame para mostrar/ocultar el status (no continuamente).
    status_needs_present: bool,
    frame_count: u64,
    /// Rango normalizado de seleccion del frame anterior (start_row, start_col,
    /// end_row, end_col). Cuando cambia, invalida damage en filas afectadas.
    prev_selection_bounds: Option<(usize, usize, usize, usize)>,
    /// Offset de scrollback del frame anterior (invalida cache si cambia con seleccion).
    prev_scrollback_offset: isize,
    /// Fila/columna visible del cursor en el frame anterior. Movimientos de
    /// cursor via CSI (CUU/CUD/CUF/CUB/CUP) no marcan damage de grid por si
    /// solos, asi que esta comparacion es la unica forma de invalidar la fila
    /// vieja y la nueva cuando el cursor se mueve sin reescribir celdas.
    prev_cursor_pos: Option<(usize, usize)>,
    /// Familia tipográfica desde la configuracion.
    font_family: String,
    /// Fallbacks de fuente configurados por el usuario.
    font_fallback: Vec<String>,
    /// Tamaño de fuente desde la configuracion (en puntos).
    font_size: f32,
    /// Metricas de celda (ancho, alto, offsets).
    cell_metrics: CellMetrics,
    /// Cache de glifos para el renderer celda-determinista.
    glyph_cache: GlyphCache,
    /// Display list por sesion, reutilizada entre frames para damage parcial.
    pane_display_lists: HashMap<SessionId, DisplayList>,
    line_height: f32,
    glyph_offset: GlyphOffset,
    builtin_box_drawing: bool,
    ligatures: bool,
    /// Cache de ajuste de contraste por frame.
    contrast_cache: ContrastCache,
    /// Desplazamiento vertical extra del grid (p. ej. fila de tabs).
    grid_top_offset: f32,
    /// Buffer de texto para la barra de tabs.
    tab_bar_buffer: glyphon::Buffer,
    /// Segundo buffer para indicador de scroll derecho.
    tab_scroll_buffer: glyphon::Buffer,
    /// Boton × de la tab en hover (una sola visible por frame).
    tab_close_buffer: glyphon::Buffer,
    /// Buffers por tab visible.
    tab_segment_buffers: Vec<glyphon::Buffer>,
    /// Chrome por segmento (slice estable por frame).
    tab_bar_seg_glyphs: Vec<Vec<glyphon::CustomGlyph>>,
    /// Pista de fondo de la barra de tabs.
    tab_bar_track_glyphs: Vec<glyphon::CustomGlyph>,
    /// Chrome del boton × (scrub acorde a tab activa/inactiva).
    tab_close_glyphs: Vec<glyphon::CustomGlyph>,
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

    fn configure_tab_buffer(
        font_system: &mut glyphon::FontSystem,
        buffer: &mut glyphon::Buffer,
        cell_w: f32,
    ) {
        Self::configure_buffer(font_system, buffer, cell_w);
        buffer.set_wrap(font_system, glyphon::cosmic_text::Wrap::None);
    }

    fn reset_aux_buffers(&mut self) {
        let metrics = glyphon::Metrics::new(self.font_size, self.cell_h);
        self.overlay_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.overlay_buffer, self.cell_w);
        self.search_bar_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(
            &mut self.font_system,
            &mut self.search_bar_buffer,
            self.cell_w,
        );

        let picker_m =
            crate::theme_picker::picker_cell_metrics(&mut self.font_system, &self.font_family);
        let picker_metrics = glyphon::Metrics::new(picker_m.font_size, picker_m.cell_h);
        self.picker_list_buffer = glyphon::Buffer::new(&mut self.font_system, picker_metrics);
        Self::configure_buffer(
            &mut self.font_system,
            &mut self.picker_list_buffer,
            picker_m.cell_w,
        );
        self.picker_detail_buffer = glyphon::Buffer::new(&mut self.font_system, picker_metrics);
        Self::configure_buffer(
            &mut self.font_system,
            &mut self.picker_detail_buffer,
            picker_m.cell_w,
        );
        self.picker_footer_buffer = glyphon::Buffer::new(&mut self.font_system, picker_metrics);
        Self::configure_buffer(
            &mut self.font_system,
            &mut self.picker_footer_buffer,
            picker_m.cell_w,
        );

        self.bg_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.bg_buffer, self.cell_w);
        self.tab_bar_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.tab_bar_buffer, self.cell_w);
        self.tab_scroll_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_tab_buffer(
            &mut self.font_system,
            &mut self.tab_scroll_buffer,
            self.cell_w,
        );
        self.tab_close_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        Self::configure_tab_buffer(
            &mut self.font_system,
            &mut self.tab_close_buffer,
            self.cell_w,
        );
        if self.status_active {
            self.status_overlay_dirty = true;
        }
    }

    pub fn cell_w(&self) -> f32 {
        self.cell_w
    }

    pub fn cell_h(&self) -> f32 {
        self.cell_h
    }

    /// Cuenta de frames realmente presentados (no incrementa en los early-return
    /// de `render()` que no llegan a dibujar: Timeout/Occluded/Outdated/Lost).
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Inicializa wgpu, glyphon y la surface configuration.
    ///
    /// `font_system` llega pre-construido: el caller lo arma en paralelo con la
    /// negociacion de adapter/device de wgpu (ver `resumed()` en `window.rs`),
    /// ya que el escaneo de fuentes del sistema no depende de la GPU.
    pub fn new(
        _window: Arc<Window>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        font_config: &FontConfig,
        mut font_system: glyphon::FontSystem,
    ) -> Self {
        // Cache necesario para glyphon 0.11
        let t_glyphon_cache = Instant::now();
        let wgpu_cache = glyphon::Cache::new(&device);
        let mut atlas = glyphon::TextAtlas::new(&device, &queue, &wgpu_cache, config.format);
        tracing::info!(
            "startup: glyphon cache + atlas listos en {}ms",
            t_glyphon_cache.elapsed().as_millis()
        );
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
        let t_text_renderer = Instant::now();
        let text_renderer = glyphon::TextRenderer::new(
            &mut atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        tracing::info!(
            "startup: text renderer listo en {}ms",
            t_text_renderer.elapsed().as_millis()
        );
        let swash_cache = glyphon::SwashCache::new();

        let font_size = font_config.size as f32;
        let font_family = font_config.family.clone();
        let line_height = font_config.line_height;
        let glyph_offset = font_config.glyph_offset;
        let builtin_box_drawing = font_config.builtin_box_drawing;
        let ligatures = font_config.ligatures;

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
        let mut search_bar_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut search_bar_buffer, cell_w);

        let picker_m = crate::theme_picker::picker_cell_metrics(&mut font_system, &font_family);
        let picker_metrics = glyphon::Metrics::new(picker_m.font_size, picker_m.cell_h);
        let mut picker_list_buffer = glyphon::Buffer::new(&mut font_system, picker_metrics);
        Self::configure_buffer(&mut font_system, &mut picker_list_buffer, picker_m.cell_w);
        let mut picker_detail_buffer = glyphon::Buffer::new(&mut font_system, picker_metrics);
        Self::configure_buffer(&mut font_system, &mut picker_detail_buffer, picker_m.cell_w);
        let mut picker_footer_buffer = glyphon::Buffer::new(&mut font_system, picker_metrics);
        Self::configure_buffer(&mut font_system, &mut picker_footer_buffer, picker_m.cell_w);
        let mut bg_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut bg_buffer, cell_w);
        let mut tab_bar_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut tab_bar_buffer, cell_w);
        let mut tab_scroll_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_tab_buffer(&mut font_system, &mut tab_scroll_buffer, cell_w);
        let mut tab_close_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_tab_buffer(&mut font_system, &mut tab_close_buffer, cell_w);

        let mut consent_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        Self::configure_buffer(&mut font_system, &mut consent_buffer, cell_w);

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
            search_bar_buffer,
            picker_list_buffer,
            picker_detail_buffer,
            picker_footer_buffer,
            bg_buffer,
            consent_buffer,
            consent_active: false,
            cell_w,
            cell_h,
            status_active: false,
            status_start: None,
            duration_ms: 2000,
            status_bg: glyphon::Color::rgb(0xc4, 0x70, 0x4a),
            status_fg: glyphon::Color::rgb(0x0a, 0x0a, 0x0a),
            status_pill_start_col: 0,
            status_pill_cols: 0,
            status_message: None,
            status_icon: String::new(),
            status_overlay_dirty: false,
            status_needs_present: false,
            frame_count: 0,
            prev_selection_bounds: None,
            prev_scrollback_offset: 0,
            prev_cursor_pos: None,
            font_family,
            font_fallback: font_config.fallback.clone(),
            font_size,
            cell_metrics,
            glyph_cache: GlyphCache::new(),
            pane_display_lists: HashMap::new(),
            line_height,
            glyph_offset,
            builtin_box_drawing,
            ligatures,
            contrast_cache: ContrastCache::default(),
            grid_top_offset: 0.0,
            tab_bar_buffer,
            tab_scroll_buffer,
            tab_close_buffer,
            tab_segment_buffers: Vec::new(),
            tab_bar_seg_glyphs: Vec::new(),
            tab_bar_track_glyphs: Vec::new(),
            tab_close_glyphs: Vec::new(),
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

    /// Padding del area de celdas del terminal (incluye offset de barra de tabs).
    pub fn grid_padding(&self) -> (f32, f32) {
        (
            self.cell_metrics.padding_x,
            self.cell_metrics.padding_y + self.grid_top_offset,
        )
    }

    /// Reserva espacio vertical encima del grid para la barra de tabs.
    pub fn set_grid_top_offset(&mut self, offset: f32) {
        self.grid_top_offset = offset.max(0.0);
    }

    pub fn grid_top_offset(&self) -> f32 {
        self.grid_top_offset
    }

    /// Aplica un nuevo tamano de fuente y recalcula metricas de celda.
    pub fn set_font_size(&mut self, size: u16) -> (f32, f32) {
        self.font_size = size as f32;
        self.refresh_cell_metrics();
        self.reset_glyph_pipeline();
        self.reset_aux_buffers();
        (self.cell_w, self.cell_h)
    }

    /// Aplica cambios de fuente desde config (familia, metricas o fallback).
    pub fn apply_font_config(&mut self, font: &FontConfig, effective_size: u16) {
        let font_changed = self.font_family != font.family
            || self.font_fallback != font.fallback
            || self.ligatures != font.ligatures
            || self.line_height != font.line_height
            || self.glyph_offset != font.glyph_offset
            || self.builtin_box_drawing != font.builtin_box_drawing;

        if font_changed {
            self.font_system = terminal_fallback::create_font_system_with_fallback(&font.fallback);
            self.font_family = font.family.clone();
            self.font_fallback = font.fallback.clone();
            self.ligatures = font.ligatures;
            self.line_height = font.line_height;
            self.glyph_offset = font.glyph_offset;
            self.builtin_box_drawing = font.builtin_box_drawing;
        }

        self.font_size = effective_size as f32;
        self.refresh_cell_metrics();
        self.reset_glyph_pipeline();
        self.reset_aux_buffers();
    }

    /// Invalida caches GPU tras cambio de metricas (resize).
    fn reset_glyph_pipeline(&mut self) {
        self.glyph_cache.clear();
        builtin::clear_cache();
        self.reset_text_atlas();
        self.swash_cache = glyphon::SwashCache::new();
        self.pane_display_lists.clear();
    }

    /// Recrea atlas y text renderer (p. ej. al alternar métricas terminal/picker).
    fn reset_text_atlas(&mut self) {
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

    /// Sincroniza métricas del cache de glifos y resetea el atlas si cambiaron.
    fn prepare_glyph_metrics(&mut self, metrics: &CellMetrics) {
        if self.glyph_cache.metrics_changed(metrics) {
            self.reset_text_atlas();
        }
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
        self.reset_aux_buffers();
    }

    /// Renderiza los panes del layout activo en la surface.
    #[tracing::instrument(skip(self, panes, preedit))]
    #[expect(
        clippy::too_many_arguments,
        reason = "render frame needs term, theme, overlays"
    )]
    pub fn render(
        &mut self,
        panes: &[PaneRender],
        terminal_area: crate::layout::Rect,
        layout: &crate::layout::Layout,
        theme: &ThemeConfig,
        bold_is_bright: bool,
        window_opacity: f32,
        picker: Option<&ThemePickerState>,
        preedit: Option<PreeditState>,
        tabs: Option<&TabBarLayout>,
    ) -> Result<Vec<SessionId>, String> {
        let t0 = Instant::now();

        let t_frame_start = Instant::now();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(Vec::new());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(Vec::new());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("error: validacion de surface fallo".to_string());
            }
        };
        let get_frame_us = t_frame_start.elapsed().as_secs_f64() * 1_000_000.0;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        if self.consent_active {
            self.render_consent_only(theme, frame, &view, encoder, t0, get_frame_us)?;
            return Ok(Vec::new());
        }

        if let Some(picker_state) = picker {
            self.render_picker_only(
                picker_state,
                theme,
                bold_is_bright,
                frame,
                &view,
                encoder,
                t0,
                get_frame_us,
            )?;
            return Ok(Vec::new());
        }

        let focused = panes
            .iter()
            .find(|p| p.focused)
            .or_else(|| panes.first())
            .expect("render requiere al menos un pane");

        let overrides = focused
            .term
            .try_lock()
            .map(|guard| ColorOverrides::from_term(&guard))
            .unwrap_or_default();
        let palette = Palette {
            theme,
            overrides: &overrides,
            bold_is_bright: bold_is_bright || theme.bold_is_bright,
        };
        let (fg_r, fg_g, fg_b) = palette.rgb(Color::Default, false);
        let default_fg_color = glyphon::Color::rgb(fg_r, fg_g, fg_b);

        let mut base_metrics = self.cell_metrics;
        base_metrics.padding_y += self.grid_top_offset;
        self.prepare_glyph_metrics(&base_metrics);

        let t_build = Instant::now();
        let mut all_custom_glyphs = Vec::new();
        let mut updated_panes = Vec::with_capacity(panes.len());
        let mut rebuild_count = 0usize;
        for pane in panes {
            let mut pane_metrics = base_metrics;
            pane_metrics.padding_x += pane.rect.x as f32 * self.cell_w;
            pane_metrics.padding_y += pane.rect.y as f32 * self.cell_h;

            if !pane.rebuild
                && self.emit_cached_pane_glyphs(
                    pane.session_id,
                    &pane_metrics,
                    &palette,
                    theme,
                    &mut all_custom_glyphs,
                )?
            {
                updated_panes.push(pane.session_id);
                continue;
            }

            rebuild_count += 1;
            match pane.term.try_lock() {
                Ok(mut term) => {
                    self.append_pane_glyphs(
                        pane.session_id,
                        &mut term,
                        pane.focused,
                        &pane_metrics,
                        pane.rect.cols,
                        pane.rect.rows,
                        theme,
                        bold_is_bright,
                        &mut all_custom_glyphs,
                    )?;
                    updated_panes.push(pane.session_id);
                }
                Err(_) => {
                    if self.emit_cached_pane_glyphs(
                        pane.session_id,
                        &pane_metrics,
                        &palette,
                        theme,
                        &mut all_custom_glyphs,
                    )? {
                        updated_panes.push(pane.session_id);
                    }
                }
            }
        }

        push_pane_chrome(
            terminal_area,
            layout,
            panes,
            &base_metrics,
            &palette,
            &mut all_custom_glyphs,
        );

        let build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;

        if self.duration_ms > 0 {
            if let Some(start) = self.status_start {
                if start.elapsed() > std::time::Duration::from_millis(self.duration_ms) {
                    self.status_active = false;
                    self.status_start = None;
                    self.status_message = None;
                    self.status_icon.clear();
                    self.status_overlay_dirty = false;
                    self.status_needs_present = true;
                }
            }
        }

        let cell_w = self.cell_w;
        let cell_h = self.cell_h;
        if self.status_active && self.status_overlay_dirty {
            self.refill_status_overlay_buffer();
            self.status_overlay_dirty = false;
        }
        let mut extra_areas: Vec<glyphon::TextArea<'_>> = Vec::with_capacity(8);
        if let Some(tab_layout) = tabs.filter(|l| !l.segments.is_empty()) {
            push_tab_bar(
                tab_layout,
                theme,
                &self.cell_metrics,
                self.config.width,
                self.config.height,
                self.font_size,
                cell_w,
                cell_h,
                &self.font_family,
                &mut self.font_system,
                &self.bg_buffer,
                &mut self.tab_bar_buffer,
                &mut self.tab_scroll_buffer,
                &mut self.tab_close_buffer,
                &mut self.tab_close_glyphs,
                &mut self.tab_segment_buffers,
                &mut self.tab_bar_seg_glyphs,
                &mut self.tab_bar_track_glyphs,
                &mut extra_areas,
            );
        }
        if let Some(pre) = preedit.as_ref().filter(|p| !p.text.is_empty()) {
            let focused_metrics = panes
                .iter()
                .find(|p| p.focused)
                .map(|p| {
                    let mut m = base_metrics;
                    m.padding_x += p.rect.x as f32 * cell_w;
                    m.padding_y += p.rect.y as f32 * cell_h;
                    m
                })
                .unwrap_or(base_metrics);
            push_preedit_overlay(
                pre,
                &palette,
                &focused_metrics,
                self.config.width,
                self.cell_w,
                self.cell_h,
                &self.font_family,
                &mut self.font_system,
                &mut self.overlay_buffer,
                &mut extra_areas,
                &mut all_custom_glyphs,
            );
        } else if self.status_active {
            let (pad_x, pad_y) = (self.cell_metrics.padding_x, self.cell_metrics.padding_y);
            let panel_h = self.config.height as f32;
            let panel_w = self.config.width as f32;
            let status_top = panel_h - pad_y - cell_h;
            let pill_left = pad_x + self.status_pill_start_col as f32 * cell_w;
            let pill_width = self.status_pill_cols as f32 * cell_w;
            push_solid_quad(
                pill_left,
                status_top,
                pill_width,
                cell_h,
                self.status_bg,
                &mut all_custom_glyphs,
            );
            let bounds = glyphon::TextBounds {
                left: 0,
                top: 0,
                right: panel_w as i32,
                bottom: panel_h as i32,
            };
            extra_areas.push(glyphon::TextArea {
                buffer: &self.overlay_buffer,
                left: pill_left,
                top: status_top + cell_h * 0.15,
                scale: 1.0,
                bounds,
                default_color: self.status_fg,
                custom_glyphs: &[],
            });
        }

        if let Some(focused_pane) = panes.iter().find(|p| p.focused) {
            if let Ok(guard) = focused_pane.term.try_lock() {
                if let Some(ref search_state) = guard.search {
                    crate::search_overlay::fill_bar_buffer(
                        search_state,
                        &mut self.font_system,
                        &self.font_family,
                        &mut self.search_bar_buffer,
                        cell_w,
                        self.config.width as f32,
                        self.cell_h,
                        theme,
                        &mut self.contrast_cache,
                    );
                    crate::search_overlay::push_bar_overlay(
                        &self.search_bar_buffer,
                        &mut extra_areas,
                        &mut all_custom_glyphs,
                        self.config.width,
                        self.config.height,
                        self.cell_h,
                        theme,
                        &mut self.contrast_cache,
                    );
                }
            }
        }

        debug_assert_custom_glyphs_bounded(&all_custom_glyphs);

        let t_prepare = Instant::now();
        CellRenderer::prepare(
            &all_custom_glyphs,
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
                    view: &view,
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
        self.atlas.trim();
        let gpu_us = t_gpu.elapsed().as_secs_f64() * 1_000_000.0;

        let total_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(30) {
            tracing::info!(
                "[RENDER_PERF] frame={} mode=cell total={:.0}us get_frame={:.0}us build={:.0}us prepare={:.0}us gpu={:.0}us panes={} rebuild={}",
                self.frame_count,
                total_us,
                get_frame_us,
                build_us,
                prepare_us,
                gpu_us,
                panes.len(),
                rebuild_count,
            );
        }

        Ok(updated_panes)
    }

    pub fn has_pane_cache(&self, id: SessionId) -> bool {
        self.pane_display_lists
            .get(&id)
            .is_some_and(DisplayList::is_populated)
    }

    fn emit_cached_pane_glyphs(
        &mut self,
        session_id: SessionId,
        metrics: &CellMetrics,
        palette: &Palette<'_>,
        theme: &ThemeConfig,
        out: &mut Vec<glyphon::CustomGlyph>,
    ) -> Result<bool, String> {
        let Some(list) = self.pane_display_lists.get(&session_id) else {
            return Ok(false);
        };
        if !list.is_populated() {
            return Ok(false);
        }
        let mut pane_glyphs = Vec::new();
        CellRenderer::build_custom_glyphs(
            list,
            metrics,
            palette,
            theme.dim_alpha,
            &self.font_family,
            &mut self.glyph_cache,
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.contrast_cache,
            &mut pane_glyphs,
        )?;
        out.extend(pane_glyphs);
        Ok(true)
    }

    /// Construye custom glyphs de un pane y los agrega a `out`.
    #[allow(clippy::too_many_arguments)]
    fn append_pane_glyphs(
        &mut self,
        session_id: SessionId,
        term: &mut Term,
        track_selection: bool,
        metrics: &CellMetrics,
        cols_count: usize,
        rows_count: usize,
        theme: &ThemeConfig,
        bold_is_bright: bool,
        out: &mut Vec<glyphon::CustomGlyph>,
    ) -> Result<(), String> {
        term.ensure_search_cache();
        let overrides = ColorOverrides::from_term(term);
        let palette = Palette {
            theme,
            overrides: &overrides,
            bold_is_bright: bold_is_bright || theme.bold_is_bright,
        };
        let mut damage = term.take_active_grid_damage();
        let show_scrollback = term.scrollback_offset > 0;
        if show_scrollback {
            damage = DamageSnapshot::Full;
        }

        // Diffing de seleccion/scrollback/cursor contra el frame anterior:
        // debe correr ANTES del guard de "sin damage -> reusar cache" de abajo.
        // Ninguno de estos cambios escribe celdas (mark_cell_written), asi que
        // el damage de grid queda vacio aunque el cursor se haya movido o la
        // seleccion haya cambiado; si este bloque corriera despues del guard,
        // el early-return con glyphs cacheados se dispararia primero y esos
        // cambios nunca se pintarian.
        if track_selection {
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

            if term.scrollback_offset != self.prev_scrollback_offset {
                damage = DamageSnapshot::Full;
            }
            self.prev_scrollback_offset = term.scrollback_offset;

            // Movimientos de cursor via CSI (CUU/CUD/CUF/CUB/CUP) actualizan
            // term.cursor pero no marcan ninguna celda como escrita, asi que
            // el damage incremental por fila no los detecta por si solo.
            // Comparar contra la posicion del frame anterior invalida tanto
            // la fila vieja como la nueva cuando el cursor se movio sin
            // reescribir celdas (p.ej. Space/Delete que la app resuelve con
            // un simple avance de cursor).
            if show_scrollback {
                self.prev_cursor_pos = None;
            } else {
                let new_cursor = term
                    .cursor_visible
                    .then_some((term.cursor.row, term.cursor.col));
                if self.prev_cursor_pos != new_cursor {
                    for pos in [self.prev_cursor_pos, new_cursor].into_iter().flatten() {
                        if pos.0 < rows_count {
                            damage.mark_row_dirty(pos.0, cols_count);
                        }
                    }
                }
                self.prev_cursor_pos = new_cursor;
            }
        }

        if !damage.is_full() && !damage.has_any_dirty() {
            let needs_blink = track_selection && term.has_blink_stuff();
            if !needs_blink
                && self.emit_cached_pane_glyphs(session_id, metrics, &palette, theme, out)?
            {
                return Ok(());
            }
            damage = DamageSnapshot::Full;
        }

        let cols_count = limits::clamp_grid_dimension(cols_count);
        let rows_count = limits::clamp_grid_dimension(rows_count);

        let active = term.active_grid();
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

        self.prepare_glyph_metrics(metrics);
        let blink_on = if track_selection {
            crate::renderer::blink_on(
                term.last_blink_reset.elapsed(),
                std::time::Duration::from_millis(term.blink_interval_ms),
            )
        } else {
            true
        };

        let list = self.pane_display_lists.entry(session_id).or_default();
        if damage.is_full() {
            list.clear();
        }
        let bg_cap = list.bg_quads.capacity();
        let line_cap = list.line_quads.capacity();
        let glyph_cap = list.text_glyphs.capacity();
        list.bg_quads
            .reserve(bg_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));
        list.line_quads
            .reserve(line_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));
        list.text_glyphs
            .reserve(glyph_cap.min(limits::MAX_GRID_DIM * limits::MAX_GRID_DIM));

        let mut font_system = if self.ligatures {
            Some(&mut self.font_system)
        } else {
            None
        };
        let mut swash_cache = if self.ligatures {
            Some(&mut self.swash_cache)
        } else {
            None
        };
        DisplayListBuilder::build(
            list,
            term,
            metrics,
            &palette,
            theme.dim_alpha,
            &row_sources,
            cols_count,
            rows_count,
            &self.font_family,
            &damage,
            show_scrollback,
            self.builtin_box_drawing,
            blink_on,
            self.ligatures,
            &mut font_system,
            &mut swash_cache,
            &mut self.contrast_cache,
        );

        let mut pane_glyphs = Vec::new();
        CellRenderer::build_custom_glyphs(
            list,
            metrics,
            &palette,
            theme.dim_alpha,
            &self.font_family,
            &mut self.glyph_cache,
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.contrast_cache,
            &mut pane_glyphs,
        )?;
        out.extend(pane_glyphs);
        Ok(())
    }

    /// Render exclusivo del theme picker (sin grid del terminal).
    #[expect(
        clippy::too_many_arguments,
        reason = "render pass needs frame, timing and picker state"
    )]
    fn render_picker_only(
        &mut self,
        picker_state: &ThemePickerState,
        theme: &ThemeConfig,
        bold_is_bright: bool,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        mut encoder: wgpu::CommandEncoder,
        t0: Instant,
        get_frame_us: f64,
    ) -> Result<(), String> {
        let t_build = Instant::now();
        self.contrast_cache.clear();
        let picker_m =
            crate::theme_picker::picker_cell_metrics(&mut self.font_system, &self.font_family);
        self.prepare_glyph_metrics(&picker_m);
        crate::theme_picker::configure_picker_buffers(
            &mut self.font_system,
            &self.font_family,
            &mut self.picker_list_buffer,
            &mut self.picker_detail_buffer,
            &mut self.picker_footer_buffer,
        );

        let cell_w = picker_m.cell_w;
        let cell_h = picker_m.cell_h;
        let list_w = (self.config.width as f32 * 0.30).max(cell_w * 12.0);

        crate::theme_picker::fill_buffers(
            picker_state,
            &mut self.font_system,
            &self.font_family,
            cell_w,
            cell_h,
            self.config.width,
            self.config.height,
            &mut self.picker_list_buffer,
            &mut self.picker_detail_buffer,
            &mut self.picker_footer_buffer,
            &mut self.contrast_cache,
        );

        let preview_theme = picker_state.preview_theme();
        let layout = crate::theme_picker::palette_layout(cell_h);
        let samples_x = list_w + cell_h;

        let mut custom_glyphs = crate::theme_picker::build_custom_glyphs(
            picker_state,
            &preview_theme,
            cell_w,
            cell_h,
            self.config.width,
            self.config.height,
        );

        let sample_glyphs = crate::theme_picker::build_sample_custom_glyphs(
            &preview_theme,
            bold_is_bright || preview_theme.bold_is_bright,
            &picker_m,
            &self.font_family,
            samples_x,
            layout.samples_y,
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.glyph_cache,
            &mut self.contrast_cache,
        )?;
        custom_glyphs.extend(sample_glyphs);

        let (fr, fg, fb) = crate::config::parse_hex(&theme.foreground);
        let default_fg = glyphon::Color::rgb(fr, fg, fb);

        let mut extra_areas: Vec<glyphon::TextArea<'_>> = Vec::with_capacity(3);
        crate::theme_picker::push_text_areas(
            &self.picker_list_buffer,
            &self.picker_detail_buffer,
            &self.picker_footer_buffer,
            &mut extra_areas,
            list_w,
            self.config.width,
            self.config.height,
            cell_h,
            default_fg,
        );

        let build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;

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
            default_fg,
            &extra_areas,
        )?;
        let prepare_us = t_prepare.elapsed().as_secs_f64() * 1_000_000.0;

        let t_gpu = Instant::now();
        let (bg_r, bg_g, bg_b) = crate::config::parse_hex(&theme.background);
        let clear_color = frame_clear_color((bg_r, bg_g, bg_b), 1.0);
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("theme picker pass"),
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
                .map_err(|e| format!("error al renderizar theme picker: {e}"))?;
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        self.atlas.trim();
        let gpu_us = t_gpu.elapsed().as_secs_f64() * 1_000_000.0;

        let total_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(30) {
            tracing::info!(
                "[RENDER_PERF] frame={} mode=picker total={:.0}us get_frame={:.0}us build={:.0}us prepare={:.0}us gpu={:.0}us",
                self.frame_count,
                total_us,
                get_frame_us,
                build_us,
                prepare_us,
                gpu_us,
            );
        }

        Ok(())
    }

    /// Render exclusivo del modal de consentimiento (bloquea el terminal).
    fn render_consent_only(
        &mut self,
        theme: &ThemeConfig,
        frame: wgpu::SurfaceTexture,
        view: &wgpu::TextureView,
        mut encoder: wgpu::CommandEncoder,
        t0: Instant,
        get_frame_us: f64,
    ) -> Result<(), String> {
        let t_build = Instant::now();
        self.contrast_cache.clear();
        let metrics = glyphon::Metrics::new(self.font_size, self.cell_h);
        self.consent_buffer
            .set_metrics(&mut self.font_system, metrics);
        Self::configure_buffer(&mut self.font_system, &mut self.consent_buffer, self.cell_w);

        crate::diagnostics::consent_overlay::fill_consent_buffer(
            &mut self.consent_buffer,
            &mut self.font_system,
            &self.font_family,
            self.config.width as f32,
            self.config.height as f32,
        );

        let (fr, fg, fb) = crate::config::parse_hex(&theme.foreground);
        let default_fg = glyphon::Color::rgb(fr, fg, fb);

        let consent_area = glyphon::TextArea {
            buffer: &self.consent_buffer,
            left: 40.0,
            top: 100.0,
            scale: 1.0,
            bounds: glyphon::TextBounds {
                left: 0,
                top: 0,
                right: self.config.width as i32,
                bottom: self.config.height as i32,
            },
            default_color: default_fg,
            custom_glyphs: &[],
        };

        let build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;

        let t_prepare = Instant::now();
        CellRenderer::prepare(
            &[],
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
            default_fg,
            &[consent_area],
        )?;
        let prepare_us = t_prepare.elapsed().as_secs_f64() * 1_000_000.0;

        let t_gpu = Instant::now();
        let (bg_r, bg_g, bg_b) = crate::config::parse_hex(&theme.background);
        let clear_color = frame_clear_color((bg_r, bg_g, bg_b), 1.0);
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("consent pass"),
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
                .map_err(|e| format!("error rendering consent: {e}"))?;
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        self.atlas.trim();
        let gpu_us = t_gpu.elapsed().as_secs_f64() * 1_000_000.0;

        let total_us = t0.elapsed().as_secs_f64() * 1_000_000.0;
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(30) {
            tracing::info!(
                "[RENDER_PERF] frame={} mode=consent total={:.0}us get_frame={:.0}us build={:.0}us prepare={:.0}us gpu={:.0}us",
                self.frame_count,
                total_us,
                get_frame_us,
                build_us,
                prepare_us,
                gpu_us,
            );
        }

        Ok(())
    }

    /// El overlay de status esta activo (se compone si hay frame por otra razón).
    pub fn status_overlay_active(&self) -> bool {
        self.status_active
    }

    /// Hay que presentar un frame para mostrar u ocultar el status (one-shot).
    pub fn status_needs_present(&self) -> bool {
        self.status_needs_present
    }

    /// Tras pintar un frame, el present forzado del status ya no es necesario.
    pub fn clear_status_present(&mut self) {
        self.status_needs_present = false;
    }

    /// Instant en que el status debe ocultarse (`None` si inactivo o sin auto-dismiss).
    pub fn status_expiry(&self) -> Option<Instant> {
        if !self.status_active || self.duration_ms == 0 {
            return None;
        }
        self.status_start
            .map(|start| start + std::time::Duration::from_millis(self.duration_ms))
    }

    /// Requiere redraw continuo mientras el theme picker esta activo.
    pub fn theme_picker_active(&self, picker: Option<&ThemePickerState>) -> bool {
        picker.is_some()
    }

    /// Activa o desactiva el modal de consentimiento.
    pub fn set_consent_active(&mut self, active: bool) {
        self.consent_active = active;
    }

    /// Modal de consentimiento activo (requiere redraw continuo).
    pub fn is_consent_active(&self) -> bool {
        self.consent_active
    }

    pub fn search_overlay_active(&self, term: &Term) -> bool {
        term.search.is_some()
    }

    /// Establece el texto del overlay de status con apariencia configurable.
    pub fn set_status_with_config(
        &mut self,
        text: &str,
        icon: &str,
        theme: &ThemeConfig,
        status_cfg: &StatusConfig,
    ) {
        if text.is_empty() {
            let was_active = self.status_active;
            self.status_active = false;
            self.status_start = None;
            self.status_message = None;
            self.status_icon.clear();
            self.status_overlay_dirty = false;
            // Un frame para quitar el pill de pantalla.
            self.status_needs_present = was_active;
            return;
        }

        self.status_message = Some(text.to_string());
        self.status_icon = icon.to_string();
        let (bg, fg) = resolve_status_colors(theme, status_cfg, &mut self.contrast_cache);
        self.status_bg = bg;
        self.status_fg = fg;
        self.duration_ms = status_cfg.duration_ms;
        self.status_start = Some(Instant::now());
        self.status_active = true;
        // Shape en el próximo render, no en el hilo de input (copy/paste).
        self.status_overlay_dirty = true;
        self.status_needs_present = true;
    }

    fn refill_status_overlay_buffer(&mut self) {
        let Some(ref message) = self.status_message else {
            return;
        };
        let cols = (self.config.width / self.cell_w as u32).max(1) as usize;
        let formatted = format_status_pill(&self.status_icon, message, cols);
        self.status_pill_start_col = formatted.pill_start_col;
        self.status_pill_cols = formatted.pill_cols;

        let pill_text = formatted.pill_text;

        let family = resolve_family(&self.font_family);
        let default_attrs = glyphon::Attrs::new().family(family);
        let fg_attrs = glyphon::Attrs::new().family(family).color(self.status_fg);

        self.overlay_buffer.set_rich_text(
            &mut self.font_system,
            [(pill_text.as_str(), fg_attrs)],
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        self.overlay_buffer.set_size(
            &mut self.font_system,
            Some(self.config.width as f32),
            Some(self.cell_h),
        );
        Self::apply_monospace_grid(&mut self.font_system, &mut self.overlay_buffer, self.cell_w);
        self.overlay_buffer
            .shape_until_scroll(&mut self.font_system, false);
    }

    /// Establece el texto del overlay de status.
    ///
    /// Si `text` esta vacio, desactiva el overlay. Si no, llena el
    /// overlay_buffer con el texto y activa el flag `status_active`.
    /// El overlay se renderiza encima del grid en el proximo render().
    pub fn set_status(&mut self, text: &str) {
        let theme = ThemeConfig::default();
        let status_cfg = StatusConfig::default();
        self.set_status_with_config(text, "", &theme, &status_cfg);
    }
}

/// Pill de status centrado en una fila de terminal.
pub(crate) struct FormattedStatusPill {
    /// Texto interior del pill (`" {icon} mensaje "`).
    pub pill_text: String,
    pub pill_start_col: usize,
    pub pill_cols: usize,
}

fn truncate_to_display_width(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    use unicode_width::UnicodeWidthStr;

    if UnicodeWidthStr::width(s) <= max_cols {
        return s.to_string();
    }
    if max_cols == 0 {
        return String::new();
    }
    if max_cols == 1 {
        return "…".to_string();
    }
    let budget = max_cols - 1;
    let mut end = 0;
    let mut width = 0;
    for (i, ch) in s.char_indices() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > budget {
            break;
        }
        width += cw;
        end = i + ch.len_utf8();
    }
    format!("{}…", &s[..end])
}

/// Formatea un mensaje de status como pill centrado en `cols` columnas.
fn format_status_pill(icon: &str, message: &str, cols: usize) -> FormattedStatusPill {
    use unicode_width::UnicodeWidthStr;

    let prefix = if icon.is_empty() {
        String::new()
    } else {
        format!("{icon} ")
    };
    let max_content = cols.saturating_sub(4);
    let full = format!("{prefix}{message}");
    let content = if UnicodeWidthStr::width(full.as_str()) > max_content {
        truncate_to_display_width(&full, max_content)
    } else {
        full
    };
    let content_width = UnicodeWidthStr::width(content.as_str());
    let pill_cols = content_width + 2;
    let pill_text = format!(" {content} ");
    let left_pad = (cols.saturating_sub(pill_cols)) / 2;
    FormattedStatusPill {
        pill_text,
        pill_start_col: left_pad,
        pill_cols,
    }
}

/// Colores del pill de status derivados del tema activo (o overrides de config).
fn resolve_status_colors(
    theme: &ThemeConfig,
    status_cfg: &StatusConfig,
    contrast_cache: &mut ContrastCache,
) -> (glyphon::Color, glyphon::Color) {
    const BG_ALPHA: u8 = 235;
    const MIN_TOAST_CONTRAST: f64 = 4.5;

    if let Some(bg_hex) = status_cfg.bg_color.as_deref() {
        let (br, bg, bb) = parse_hex(bg_hex);
        let bg_color = glyphon::Color::rgb(br, bg, bb);
        let fg_rgb = status_cfg
            .fg_color
            .as_deref()
            .map(parse_hex)
            .unwrap_or_else(|| parse_hex(&theme.foreground));
        let (fr, fg, fb) = contrast_cache.adjust(
            fg_rgb,
            (br, bg, bb),
            theme.minimum_contrast.max(MIN_TOAST_CONTRAST),
        );
        return (bg_color, glyphon::Color::rgb(fr, fg, fb));
    }

    let (br, bg, bb) = parse_hex(&theme.black);
    let bg_color = glyphon::Color::rgba(br, bg, bb, BG_ALPHA);
    let fg_rgb = status_cfg
        .fg_color
        .as_deref()
        .map(parse_hex)
        .unwrap_or_else(|| parse_hex(&theme.foreground));
    let (fr, fg, fb) = contrast_cache.adjust(
        fg_rgb,
        (br, bg, bb),
        theme.minimum_contrast.max(MIN_TOAST_CONTRAST),
    );
    (bg_color, glyphon::Color::rgb(fr, fg, fb))
}

/// Tamano maximo de losa para `CustomGlyph` solido (limite de `safe_mask_len`).
const SOLID_QUAD_MAX_TILE: f32 = 512.0;

/// Particiona un rectangulo en losas `(x, y, w, h)` relativas al origen.
fn solid_quad_tiles(width: f32, height: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut tiles = Vec::new();
    if width <= 0.0 || height <= 0.0 {
        return tiles;
    }
    let mut y0 = 0.0;
    while y0 < height - f32::EPSILON {
        let th = (height - y0).clamp(1.0, SOLID_QUAD_MAX_TILE);
        let mut x0 = 0.0;
        while x0 < width - f32::EPSILON {
            let tw = (width - x0).clamp(1.0, SOLID_QUAD_MAX_TILE);
            tiles.push((x0, y0, tw, th));
            x0 += tw;
        }
        y0 += th;
    }
    tiles
}

fn push_solid_quad(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: glyphon::Color,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    for (x0, y0, tw, th) in solid_quad_tiles(width, height) {
        if limits::custom_pixels(tw, th) > limits::MAX_CUSTOM_GLYPH_PIXELS {
            continue;
        }
        out.push(glyphon::CustomGlyph {
            id: SOLID_MASK_GLYPH_ID,
            left: left + x0,
            top: top + y0,
            width: tw,
            height: th,
            color: Some(color),
            snap_to_physical_pixel: true,
            metadata: 1,
        });
    }
}

/// Chrome de panes: divisores finos (estilo wezterm/kitty), borde inset del foco e
/// overlay sutil en panes inactivos (inspirado en wezterm inactive_pane_brightness).
fn push_pane_chrome(
    area: crate::layout::Rect,
    layout: &crate::layout::Layout,
    panes: &[PaneRender],
    metrics: &CellMetrics,
    palette: &Palette<'_>,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    if panes.len() <= 1 {
        return;
    }

    const DIVIDER_ALPHA: u8 = 55;
    const INACTIVE_OVERLAY_ALPHA: u8 = 48;
    const FOCUS_INSET_ALPHA: u8 = 100;
    const FOCUS_INSET_PX: f32 = 1.0;

    let (ir, ig, ib) = palette.rgb(Color::BrightBlack, false);
    let divider = glyphon::Color::rgba(ir, ig, ib, DIVIDER_ALPHA);

    let (wr, wg, wb) = palette.rgb(Color::BrightWhite, false);
    let focus_border = glyphon::Color::rgba(wr, wg, wb, FOCUS_INSET_ALPHA);

    for pane in panes {
        if !pane.focused {
            push_inactive_overlay(&pane.rect, metrics, INACTIVE_OVERLAY_ALPHA, out);
        }
    }

    for div in layout.divider_rects(area) {
        push_thin_divider(&div, metrics, divider, out);
    }

    for pane in panes {
        if pane.focused {
            push_focus_inset(&pane.rect, metrics, focus_border, FOCUS_INSET_PX, out);
        }
    }
}

fn push_thin_divider(
    div: &crate::layout::Rect,
    metrics: &CellMetrics,
    color: glyphon::Color,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    // Usar cell_w/h flotantes (mismo origen que el grid de terminales), no geometry
    // entera: en panes grandes el redondeo por celda dejaba overlays/bordes cortos.
    let gw = metrics.cell_w;
    let gh = metrics.cell_h;
    if div.cols == 1 && div.rows > 0 {
        let height = gh * div.rows as f32;
        let left = div.x as f32 * gw + metrics.padding_x + (gw * 0.5 - 0.5).max(0.0);
        let top = div.y as f32 * gh + metrics.padding_y;
        push_solid_quad(left, top, 1.0, height, color, out);
    } else if div.rows == 1 && div.cols > 0 {
        let width = gw * div.cols as f32;
        let left = div.x as f32 * gw + metrics.padding_x;
        let top = div.y as f32 * gh + metrics.padding_y + (gh * 0.5 - 0.5).max(0.0);
        push_solid_quad(left, top, width, 1.0, color, out);
    }
}

fn push_focus_inset(
    rect: &crate::layout::Rect,
    metrics: &CellMetrics,
    color: glyphon::Color,
    inset: f32,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    let gw = metrics.cell_w;
    let gh = metrics.cell_h;
    let left = rect.x as f32 * gw + metrics.padding_x + inset;
    let top = rect.y as f32 * gh + metrics.padding_y + inset;
    let w = rect.cols as f32 * gw - inset * 2.0;
    let h = rect.rows as f32 * gh - inset * 2.0;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    push_solid_quad(left, top, w, inset, color, out);
    push_solid_quad(left, top + h - inset, w, inset, color, out);
    push_solid_quad(left, top, inset, h, color, out);
    push_solid_quad(left + w - inset, top, inset, h, color, out);
}

fn push_inactive_overlay(
    rect: &crate::layout::Rect,
    metrics: &CellMetrics,
    alpha: u8,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    let gw = metrics.cell_w;
    let gh = metrics.cell_h;
    let left = rect.x as f32 * gw + metrics.padding_x;
    let top = rect.y as f32 * gh + metrics.padding_y;
    let w = rect.cols as f32 * gw;
    let h = rect.rows as f32 * gh;
    push_solid_quad(left, top, w, h, glyphon::Color::rgba(0, 0, 0, alpha), out);
}

#[expect(
    clippy::too_many_arguments,
    reason = "tab bar shares overlay layout metrics with status bar"
)]
fn push_tab_bar<'a>(
    layout: &TabBarLayout,
    theme: &ThemeConfig,
    cell_metrics: &CellMetrics,
    surface_w: u32,
    surface_h: u32,
    font_size: f32,
    cell_w: f32,
    cell_h: f32,
    font_family: &str,
    font_system: &mut glyphon::FontSystem,
    empty_buffer: &'a glyphon::Buffer,
    scroll_left_buffer: &'a mut glyphon::Buffer,
    scroll_right_buffer: &'a mut glyphon::Buffer,
    close_buffer: &'a mut glyphon::Buffer,
    close_glyphs: &'a mut Vec<glyphon::CustomGlyph>,
    segment_buffers: &'a mut Vec<glyphon::Buffer>,
    seg_glyphs: &'a mut Vec<Vec<glyphon::CustomGlyph>>,
    track_glyphs: &'a mut Vec<glyphon::CustomGlyph>,
    extra_areas: &mut Vec<glyphon::TextArea<'a>>,
) {
    let (pad_x, pad_y) = (cell_metrics.padding_x, cell_metrics.padding_y);
    let bar_h = crate::renderer::tab_bar::tab_bar_height_px(cell_h);
    let inner_w = crate::renderer::tab_bar::tab_bar_inner_width(surface_w as f32, pad_x);
    let full_bounds = glyphon::TextBounds {
        left: 0,
        top: 0,
        right: surface_w as i32,
        bottom: surface_h as i32,
    };

    crate::renderer::build_tab_track(inner_w, bar_h, theme, track_glyphs);
    extra_areas.push(glyphon::TextArea {
        buffer: empty_buffer,
        left: pad_x,
        top: pad_y,
        scale: 1.0,
        bounds: full_bounds,
        default_color: glyphon::Color::rgb(0xff, 0xff, 0xff),
        custom_glyphs: track_glyphs,
    });

    let (fr, fg, fb) = parse_hex(&theme.foreground);
    let inactive_fg = glyphon::Color::rgba(fr, fg, fb, 120);
    let active_fg = glyphon::Color::rgb(fr, fg, fb);
    let family = resolve_family(font_family);
    let default_attrs = glyphon::Attrs::new().family(family);
    let metrics = glyphon::Metrics::new(font_size, cell_h);

    while segment_buffers.len() < layout.segments.len() {
        let mut buf = glyphon::Buffer::new(font_system, metrics);
        Renderer::configure_tab_buffer(font_system, &mut buf, cell_w);
        segment_buffers.push(buf);
    }
    while seg_glyphs.len() < layout.segments.len() {
        seg_glyphs.push(Vec::new());
    }

    let close_cell_w = cell_w * TAB_CLOSE_WIDTH_CELLS as f32;
    let mut close_target: Option<&TabSegment> = None;
    let close_alpha = layout.mouse.close_alpha;

    for (seg, (buf, chrome)) in layout
        .segments
        .iter()
        .zip(segment_buffers.iter_mut().zip(seg_glyphs.iter_mut()))
    {
        let show_close = layout.mouse.close_tab == Some(seg.index)
            && close_alpha > 0.02
            && seg.width_cells > TAB_CLOSE_WIDTH_CELLS;
        if show_close {
            close_target = Some(seg);
        }

        chrome.clear();
        if seg.active {
            crate::renderer::build_segment_chrome(seg.width_px, bar_h, true, theme, chrome);
        } else if show_close {
            crate::renderer::build_inactive_hover_chrome(
                seg.width_px,
                bar_h,
                close_alpha,
                theme,
                chrome,
            );
        }
        if !chrome.is_empty() {
            extra_areas.push(glyphon::TextArea {
                buffer: empty_buffer,
                left: seg.x_px,
                top: pad_y,
                scale: 1.0,
                bounds: full_bounds,
                default_color: glyphon::Color::rgb(0xff, 0xff, 0xff),
                custom_glyphs: chrome,
            });
        }

        let label = crate::renderer::segment_title_label(
            seg.index + 1,
            &seg.title_short,
            seg.width_cells,
            show_close,
        );
        let label_pad_px = cell_w * TAB_LABEL_PAD_CELLS as f32;
        let text_w_px = if show_close {
            seg.width_px - label_pad_px * 2.0 - close_cell_w
        } else {
            seg.width_px - label_pad_px * 2.0
        };
        let body_attrs = if seg.active {
            glyphon::Attrs::new().family(family).color(active_fg)
        } else {
            glyphon::Attrs::new().family(family).color(inactive_fg)
        };
        buf.set_rich_text(
            font_system,
            vec![(label.as_str(), body_attrs)],
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        buf.set_size(font_system, Some(text_w_px.max(cell_w)), Some(cell_h));
        buf.set_monospace_width(font_system, Some(cell_w));
        buf.set_hinting(font_system, Hinting::Enabled);
        buf.shape_until_scroll(font_system, false);

        extra_areas.push(glyphon::TextArea {
            buffer: buf,
            left: seg.x_px + label_pad_px,
            top: pad_y,
            scale: 1.0,
            bounds: full_bounds,
            default_color: if seg.active { active_fg } else { inactive_fg },
            custom_glyphs: &[],
        });
    }

    if let Some(seg) = close_target {
        let close_a = (close_alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let close_attrs = glyphon::Attrs::new()
            .family(family)
            .color(glyphon::Color::rgba(fr, fg, fb, close_a.max(140)));
        close_glyphs.clear();
        crate::renderer::push_close_scrub(
            bar_h,
            cell_w,
            close_alpha,
            seg.active,
            theme,
            close_glyphs,
        );
        close_buffer.set_rich_text(
            font_system,
            vec![("×", close_attrs)],
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        close_buffer.set_size(font_system, Some(close_cell_w), Some(cell_h));
        close_buffer.set_monospace_width(font_system, Some(cell_w));
        close_buffer.set_hinting(font_system, Hinting::Enabled);
        close_buffer.shape_until_scroll(font_system, false);
        extra_areas.push(glyphon::TextArea {
            buffer: close_buffer,
            left: crate::renderer::segment_close_left_px(seg, cell_w),
            top: pad_y,
            scale: 1.0,
            bounds: full_bounds,
            default_color: glyphon::Color::rgba(fr, fg, fb, close_a.max(140)),
            custom_glyphs: close_glyphs,
        });
    }

    let ind_cells = crate::renderer::tab_bar::SCROLL_INDICATOR_CELLS;
    let ind_w = ind_cells as f32 * cell_w;
    let dim = glyphon::Attrs::new()
        .family(family)
        .color(glyphon::Color::rgba(fr, fg, fb, 100));

    if layout.show_scroll_left {
        scroll_left_buffer.set_rich_text(
            font_system,
            vec![("‹", dim.clone())],
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        scroll_left_buffer.set_size(font_system, Some(ind_w), Some(cell_h));
        scroll_left_buffer.set_monospace_width(font_system, Some(cell_w));
        scroll_left_buffer.shape_until_scroll(font_system, false);
        extra_areas.push(glyphon::TextArea {
            buffer: scroll_left_buffer,
            left: pad_x,
            top: pad_y,
            scale: 1.0,
            bounds: full_bounds,
            default_color: inactive_fg,
            custom_glyphs: &[],
        });
    }
    if layout.show_scroll_right {
        scroll_right_buffer.set_rich_text(
            font_system,
            vec![("›", dim)],
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        scroll_right_buffer.set_size(font_system, Some(ind_w), Some(cell_h));
        scroll_right_buffer.set_monospace_width(font_system, Some(cell_w));
        scroll_right_buffer.shape_until_scroll(font_system, false);
        let right_x = (surface_w as f32 - pad_x - ind_w).max(pad_x);
        extra_areas.push(glyphon::TextArea {
            buffer: scroll_right_buffer,
            left: right_x,
            top: pad_y,
            scale: 1.0,
            bounds: full_bounds,
            default_color: inactive_fg,
            custom_glyphs: &[],
        });
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "overlay push shares layout metrics with search bar pattern"
)]
fn push_preedit_overlay<'a>(
    preedit: &PreeditState,
    palette: &Palette<'_>,
    cell_metrics: &CellMetrics,
    surface_w: u32,
    cell_w: f32,
    cell_h: f32,
    font_family: &str,
    font_system: &mut glyphon::FontSystem,
    overlay_buffer: &'a mut glyphon::Buffer,
    extra_areas: &mut Vec<glyphon::TextArea<'a>>,
    custom_glyphs: &mut Vec<glyphon::CustomGlyph>,
) {
    let (pad_x, pad_y) = (cell_metrics.padding_x, cell_metrics.padding_y);
    let left = pad_x + preedit.col as f32 * cell_w;
    let top = pad_y + preedit.row as f32 * cell_h;
    let (fg_r, fg_g, fg_b) = palette.rgb(Color::Default, false);
    let fg = glyphon::Color::rgb(fg_r, fg_g, fg_b);
    let default_attrs = glyphon::Attrs::new().family(resolve_family(font_family));
    let mut attrs = glyphon::Attrs::new().family(resolve_family(font_family));
    attrs = attrs.color(fg);
    let spans = [(preedit.text.as_str(), attrs)];

    overlay_buffer.set_rich_text(
        font_system,
        spans,
        &default_attrs,
        glyphon::Shaping::Advanced,
        None,
    );
    // ponytail: el preedit provisional puede extenderse mas alla de la fila visible
    let overlay_w = (surface_w as f32 - left).max(cell_w);
    overlay_buffer.set_size(font_system, Some(overlay_w), Some(cell_h));
    overlay_buffer.set_monospace_width(font_system, Some(cell_w));
    overlay_buffer.set_hinting(font_system, Hinting::Enabled);
    overlay_buffer.shape_until_scroll(font_system, false);

    extra_areas.push(glyphon::TextArea {
        buffer: overlay_buffer,
        left,
        top,
        scale: 1.0,
        bounds: glyphon::TextBounds {
            left: 0,
            top: 0,
            right: overlay_w as i32,
            bottom: cell_h as i32,
        },
        default_color: fg,
        custom_glyphs: &[],
    });

    let width_cells =
        unicode_width::UnicodeWidthStr::width(preedit.text.as_str()).clamp(1, 255) as u8;
    custom_glyphs.push(decorations::underline_quad(
        preedit.row,
        preedit.col,
        width_cells,
        cell_metrics,
        fg,
    ));
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
        assert_eq!(fc.family, "FiraCode Nerd Font Mono");
        assert_eq!(fc.size, 12);
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

    #[test]
    fn solid_quad_tiles_linea_ancha_no_se_recorta_a_512() {
        let tiles = solid_quad_tiles(1200.0, 1.0);
        assert_eq!(tiles.len(), 3);
        let total_w: f32 = tiles.iter().map(|(_, _, w, _)| *w).sum();
        assert!((total_w - 1200.0).abs() < 0.01);
    }

    #[test]
    fn solid_quad_tiles_cubren_overlay_grande() {
        let tiles = solid_quad_tiles(960.0, 540.0);
        let total_area: f32 = tiles.iter().map(|(_, _, w, h)| w * h).sum();
        assert!((total_area - 960.0 * 540.0).abs() < 1.0);
        assert!(tiles.iter().all(|(_, _, w, h)| *w <= 512.0 && *h <= 512.0));
    }

    #[test]
    fn test_status_text_formato_pill() {
        use unicode_width::UnicodeWidthStr;

        let pill = format_status_pill("✓", "Copiado al clipboard", 80);
        assert!(pill.pill_text.contains('✓'));
        assert!(pill.pill_text.contains("Copiado al clipboard"));
        assert_eq!(
            UnicodeWidthStr::width(pill.pill_text.as_str()) + (80 - pill.pill_cols),
            80
        );
        let inner = " ✓ Copiado al clipboard ";
        assert_eq!(
            pill.pill_cols,
            UnicodeWidthStr::width(inner),
            "pill_cols usa ancho de visualizacion, no bytes"
        );
    }

    #[test]
    fn test_status_text_truncado() {
        use unicode_width::UnicodeWidthStr;

        let pill = format_status_pill(
            "✗",
            "mensaje muy largo que excede el ancho maximo del pill y debe truncarse con puntos suspensivos en un terminal de 40 columnas",
            40,
        );
        assert!(UnicodeWidthStr::width(pill.pill_text.as_str()) <= 40 - pill.pill_start_col);
        assert!(pill.pill_text.contains('…'));
    }

    #[test]
    fn truncate_to_display_width_respeta_limite_unicode() {
        let s = truncate_to_display_width("✓ mensaje largo", 5);
        assert!(unicode_width::UnicodeWidthStr::width(s.as_str()) <= 5);
        assert!(s.ends_with('…'));
    }
}
