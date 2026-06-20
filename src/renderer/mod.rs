//! Modulo de render GPU del grid 24x80.

use std::sync::Arc;

use crate::ansi::{Color, Term};
use crate::grid::{COLS, ROWS};
use winit::window::Window;

/// Renderer GPU del terminal virtual.
///
/// Mantiene los recursos wgpu y glyphon necesarios para pintar el grid 24x80.
/// Los campos son privados: se inicializa via `Renderer::new` y se consume
/// via `render` y `resize`.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    font_system: glyphon::FontSystem,
    atlas: glyphon::TextAtlas,
    viewport: glyphon::Viewport,
    text_renderer: glyphon::TextRenderer,
    swash_cache: glyphon::SwashCache,
    // ponytail: 1 buffer por fila del grid. 24 buffers en lugar de 1 buffer
    // compartido permite que cada fila tenga su propio top y que el resize
    // solo reconfigure los buffers afectados, no todo el buffer.
    buffers: Vec<glyphon::Buffer>,
    // ponytail: 1 buffer para overlays (cursor block, mensajes de status).
    // Renderizado encima del grid, con color diferente.
    overlay_buffer: glyphon::Buffer,
    // ponytail: cell_w y cell_h se calculan en new() y se actualizan en resize().
    // NO son #[expect(dead_code)] en este sprint; el renderer los usa para
    // posicionar cada TextArea.
    cell_w: f32,
    cell_h: f32,
    // ponytail: flag del overlay. Se activa con set_status(), se desactiva
    // cuando se llama con texto vacio o cuando se hace render() sin status.
    status_active: bool,
}

impl Renderer {
    /// Inicializa wgpu, glyphon y la surface configuration.
    ///
    /// Recibe la ventana (`Arc<Window>` para lifetime `'static` de la surface),
    /// el device, queue, surface y configuracion de surface pre-creados.
    ///
    /// Calcula cell_w, cell_h y font_size a partir del tamano de la ventana
    /// y crea 24 buffers (uno por fila) + 1 overlay buffer.
    pub fn new(
        _window: Arc<Window>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        let mut font_system = glyphon::FontSystem::new();
        font_system.db_mut().load_system_fonts();

        let cache = glyphon::Cache::new(&device);
        let mut atlas = glyphon::TextAtlas::new(&device, &queue, &cache, config.format);
        let viewport = glyphon::Viewport::new(&device, &cache);
        let text_renderer = glyphon::TextRenderer::new(
            &mut atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );
        let swash_cache = glyphon::SwashCache::new();

        // Calcular tamano de celda y font size para el grid 24x80.
        let (cell_w, cell_h) = Self::cell_size_for_window(config.width, config.height);
        let font_size = Self::font_size_for_cells(cell_w, cell_h);

        // Crear 24 buffers (uno por fila del grid).
        let metrics = glyphon::Metrics::new(font_size, font_size * 1.4);
        let mut buffers = Vec::with_capacity(ROWS);
        for _ in 0..ROWS {
            buffers.push(glyphon::Buffer::new(&mut font_system, metrics));
        }

        // Crear 1 buffer para overlays (cursor, status).
        let overlay_buffer = glyphon::Buffer::new(&mut font_system, metrics);

        Self {
            device,
            queue,
            surface,
            config,
            font_system,
            atlas,
            viewport,
            text_renderer,
            swash_cache,
            buffers,
            overlay_buffer,
            cell_w,
            cell_h,
            status_active: false,
        }
    }

    /// Calcula cell_w y cell_h para un tamano de ventana dado.
    fn cell_size_for_window(width: u32, height: u32) -> (f32, f32) {
        (width as f32 / COLS as f32, height as f32 / ROWS as f32)
    }

    /// Calcula el font size optimo para las celdas.
    /// Usa GLYPH_RATIO (0.6) para el ancho y LINE_RATIO (1.4) para el alto,
    /// tomando el minimo y aplicando un piso MIN_SIZE (6.0).
    fn font_size_for_cells(cell_w: f32, cell_h: f32) -> f32 {
        const GLYPH_RATIO: f32 = 0.6;
        const LINE_RATIO: f32 = 1.4;
        const MIN_SIZE: f32 = 6.0;
        let from_w = cell_w / GLYPH_RATIO;
        let from_h = cell_h / LINE_RATIO;
        from_w.min(from_h).max(MIN_SIZE)
    }

    /// Actualiza el tamano de la surface, el viewport, cell_w, cell_h y
    /// recrea los 24 buffers + 1 overlay buffer con el nuevo font size.
    ///
    /// ponytail: 25 buffers es barato (cada buffer es 0 costo si no se usa).
    /// Si en Sprint 6+ medimos que esto es lento, refactorizamos a un solo
    /// buffer con multiples TextArea o a lazy allocation.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.viewport
            .update(&self.queue, glyphon::Resolution { width, height });

        // Recalcular cell_w, cell_h y font size.
        let (cell_w, cell_h) = Self::cell_size_for_window(width, height);
        self.cell_w = cell_w;
        self.cell_h = cell_h;
        let font_size = Self::font_size_for_cells(cell_w, cell_h);

        // Recrear todos los buffers con el nuevo font size.
        let metrics = glyphon::Metrics::new(font_size, font_size * 1.4);
        self.buffers.clear();
        for _ in 0..ROWS {
            self.buffers
                .push(glyphon::Buffer::new(&mut self.font_system, metrics));
        }
        self.overlay_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);

        // ponytail: status_active se mantiene a traves de resize.
        // Si hay un status activo antes del resize, seguira activo.
    }

    /// Renderiza el grid 24x80 del terminal en la surface GPU.
    ///
    /// Construye 24 TextArea (uno por fila del grid activo), cada uno con
    /// left=0, top=row*cell_h, y bounds que recortan a la celda. Si
    /// status_active esta activo, agrega un TextArea extra para el overlay.
    ///
    /// # Errors
    /// Retorna un `String` si la GPU no puede presentar el frame, preparar
    /// el texto, o renderizar.
    pub fn render(&mut self, term: &Term) -> Result<(), String> {
        // 1. Obtener frame de la surface
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
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // 2. Construir 24 TextArea, uno por fila del grid activo.
        let default_attrs = glyphon::Attrs::new().family(glyphon::Family::Monospace);
        let active = term.active_grid();
        let mut text_areas = Vec::with_capacity(ROWS + 1);

        // Fase A: llenar los 24 buffers (borrow mutable). Fase B abajo (borrow inmutable).
        for row in 0..ROWS {
            // 2a. Construir el string de la fila (80 chars).
            let mut line = String::with_capacity(COLS);
            // Almacenar informacion de estilo por celda para esta fila.
            struct CellStyle {
                start: usize,
                end: usize,
                bold: bool,
                fg: Color,
            }
            let mut styles: Vec<CellStyle> = Vec::with_capacity(COLS);

            for col in 0..COLS {
                let cell = &active.rows[row][col];
                let start = line.len();
                line.push(cell.ch);
                let end = line.len();
                styles.push(CellStyle {
                    start,
                    end,
                    bold: cell.attrs.bold,
                    fg: cell.attrs.fg,
                });
            }

            // 2b. set_rich_text en el buffer de esta fila con spans por celda.
            // ponytail: colores SGR individuales via color_to_glyphon.
            let spans = styles.iter().map(|s| {
                let fg = color_to_glyphon(s.fg);
                let color = glyphon::Color::rgba(fg.r(), fg.g(), fg.b(), 255);
                let mut attrs = glyphon::Attrs::new().family(glyphon::Family::Monospace);
                if s.bold {
                    attrs = attrs.weight(glyphon::Weight::BOLD);
                }
                // ponytail: underline no soportado nativamente por glyphon
                // 0.11. Pendiente para Sprint 4 via wgpu::RenderPass separado.
                attrs = attrs.color(color);
                (&line[s.start..s.end], attrs)
            });

            self.buffers[row].set_rich_text(
                &mut self.font_system,
                spans,
                &default_attrs,
                glyphon::Shaping::Advanced,
                None,
            );
            self.buffers[row].set_size(
                &mut self.font_system,
                Some(self.config.width as f32),
                Some(self.config.height as f32),
            );
            self.buffers[row].shape_until_scroll(&mut self.font_system, false);
        }

        // Fase B: referencias inmutables como TextArea, una por fila con top = row * cell_h.
        for row in 0..ROWS {
            let top = row as f32 * self.cell_h;
            text_areas.push(glyphon::TextArea {
                buffer: &self.buffers[row],
                left: 0.0,
                top,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: top as i32,
                    right: self.config.width as i32,
                    bottom: (top + self.cell_h) as i32,
                },
                default_color: glyphon::Color::rgb(0xcd, 0xd6, 0xf4),
                custom_glyphs: &[],
            });
        }

        // 2d. Si hay overlay activo (status), agregar TextArea extra.
        if self.status_active {
            text_areas.push(glyphon::TextArea {
                buffer: &self.overlay_buffer,
                left: 0.0,
                top: 0.0,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: self.config.width as i32,
                    bottom: self.config.height as i32,
                },
                // ponytail: rojo para status (Catppuccin Mocha Red).
                default_color: glyphon::Color::rgb(0xf3, 0x8b, 0xa8),
                custom_glyphs: &[],
            });
        }

        // 3. Preparar todos los TextArea para glyphon
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| format!("error al preparar texto: {e}"))?;

        // 4. Renderizar en el render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glyphon render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut render_pass)
                .map_err(|e| format!("error al renderizar texto: {e}"))?;
        }

        // 5. Enviar comandos y presentar
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }

    /// Establece el texto del overlay de status.
    ///
    /// Si `text` esta vacio, desactiva el overlay. Si no, llena el
    /// overlay_buffer con el texto y activa el flag `status_active`.
    /// El overlay se renderiza encima del grid en el proximo render().
    pub fn set_status(&mut self, text: &str) {
        if text.is_empty() {
            self.status_active = false;
            return;
        }

        let default_attrs = glyphon::Attrs::new().family(glyphon::Family::Monospace);
        let mut attrs = glyphon::Attrs::new().family(glyphon::Family::Monospace);
        // ponytail: color rojo para status. Refinable en Sprint 8 con theme.
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
        self.overlay_buffer
            .shape_until_scroll(&mut self.font_system, false);

        self.status_active = true;
    }
}

/// Convierte un Color ANSI a `glyphon::Color` (Catppuccin Mocha hardcoded).
// ponytail: colores hardcoded, theme configurable en Sprint 8.
fn color_to_glyphon(color: Color) -> glyphon::Color {
    match color {
        Color::Default => glyphon::Color::rgb(0xcd, 0xd6, 0xf4),
        Color::Black => glyphon::Color::rgb(0, 0, 0),
        Color::Red => glyphon::Color::rgb(0xf3, 0x8b, 0xa8),
        Color::Green => glyphon::Color::rgb(0xa6, 0xe3, 0xa1),
        Color::Yellow => glyphon::Color::rgb(0xf9, 0xe2, 0xaf),
        Color::Blue => glyphon::Color::rgb(0x89, 0xb4, 0xfa),
        Color::Magenta => glyphon::Color::rgb(0xc6, 0x9b, 0x6d),
        Color::Cyan => glyphon::Color::rgb(0x94, 0xe2, 0xd5),
        Color::White => glyphon::Color::rgb(0xcd, 0xd6, 0xf4),
    }
}

// ---------------------------------------------------------------------------
// Tests unitarios del Renderer y pipeline de render
// ---------------------------------------------------------------------------
//
// Tests de color mapping: verifican que color_to_glyphon mapea correctamente
// los 9 colores ANSI a los valores Catppuccin Mocha hardcoded.
//
// Tests de propagacion SGR: verifican que el parser ANSI alimenta
// correctamente los attrs que el Renderer consume para los rich text spans.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::{Color, Term};

    // -----------------------------------------------------------------------
    // Helper: alimenta bytes crudos al parser vte con Term como performer.
    // -----------------------------------------------------------------------
    fn feed(term: &mut Term, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, data);
    }

    // -----------------------------------------------------------------------
    // Tests de color_to_glyphon (helper puro, sin GPU)
    // -----------------------------------------------------------------------

    #[test]
    fn test_color_mapping_all_nine() {
        // Verifica los 9 colores del enum Color: Default, Black, Red, Green,
        // Yellow, Blue, Magenta, Cyan, White. Catppuccin Mocha hardcoded.
        let cases = [
            (Color::Default, (0xcd, 0xd6, 0xf4)),
            (Color::Black, (0x00, 0x00, 0x00)),
            (Color::Red, (0xf3, 0x8b, 0xa8)),
            (Color::Green, (0xa6, 0xe3, 0xa1)),
            (Color::Yellow, (0xf9, 0xe2, 0xaf)),
            (Color::Blue, (0x89, 0xb4, 0xfa)),
            (Color::Magenta, (0xc6, 0x9b, 0x6d)),
            (Color::Cyan, (0x94, 0xe2, 0xd5)),
            (Color::White, (0xcd, 0xd6, 0xf4)),
        ];
        for (color, (r, g, b)) in cases {
            let c = color_to_glyphon(color);
            assert_eq!(c.r(), r, "r para {color:?}");
            assert_eq!(c.g(), g, "g para {color:?}");
            assert_eq!(c.b(), b, "b para {color:?}");
        }
    }

    // -----------------------------------------------------------------------
    // Tests de propagacion SGR: el parser ANSI alimenta los attrs que el
    // Renderer consume para construir rich text spans con color_to_glyphon.
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
        // ponytail: 1;31 = bold + red, ambos consumidos por Renderer
        feed(&mut term, b"\x1b[1;31mB");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.ch, 'B', "caracter en (0,0)");
        assert!(cell.attrs.bold, "bold activo");
        assert_eq!(cell.attrs.fg, Color::Red, "fg = Red");
    }
}
