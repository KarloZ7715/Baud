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
    buffer: glyphon::Buffer,
    // ponytail: celdas 8x16 hardcoded, configurables en Sprint 5 con SIGWINCH
    #[expect(dead_code)]
    cell_w: f32,
    #[expect(dead_code)]
    cell_h: f32,
}

impl Renderer {
    /// Inicializa wgpu, glyphon y la surface configuration.
    ///
    /// Recibe la ventana (`Arc<Window>` para lifetime `'static` de la surface),
    /// el device, queue, surface y configuracion de surface pre-creados.
    ///
    /// La surface `'static` se logra porque wgpu internamente retiene el
    /// `Arc<Window>` que se pasa al crear la surface.
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

        // ponytail: el font size se calcula proporcional al tamano del grid
        // (24x80) y de la ventana. Si el font es fijo, una ventana grande
        // muestra texto microscopico en la esquina. Si escala, el texto
        // ocupa la ventana de forma natural.
        let initial_font_size = font_size_for_window(config.width, config.height);
        let metrics = glyphon::Metrics::new(initial_font_size, initial_font_size * 1.4);
        let buffer = glyphon::Buffer::new(&mut font_system, metrics);

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
            buffer,
            cell_w: 8.0,
            cell_h: 16.0,
        }
    }

    /// Actualiza el tamano de la surface, el viewport y el font size.
    /// El Buffer se recrea con metrics que escalan el texto al nuevo tamano.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.viewport
            .update(&self.queue, glyphon::Resolution { width, height });
        // Recalcular font size para que el texto escale a la nueva ventana.
        let new_size = font_size_for_window(width, height);
        self.buffer.set_metrics(
            &mut self.font_system,
            glyphon::Metrics::new(new_size, new_size * 1.4),
        );
    }

    /// Renderiza el grid 24x80 del terminal en la surface GPU.
    ///
    /// Construye el contenido de texto a partir del estado de `Term`, lo
    /// prepara con glyphon y lo dibuja en un render pass con fondo negro.
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
                return Err("error: validacion de surface falló".to_string());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // 2. Construir contenido de texto con estilo por celda usando
        //    set_rich_text (cosmic-text 0.18.2) que acepta spans con color
        let mut text = String::with_capacity(ROWS * COLS);
        // Almacenar info de estilo por caracter: (byte_start, byte_end, bold, fg)
        struct CharStyle {
            start: usize,
            end: usize,
            bold: bool,
            fg: Color,
        }
        let mut styles: Vec<CharStyle> = Vec::with_capacity(ROWS * COLS);

        for row in 0..ROWS {
            for col in 0..COLS {
                let cell = &term.active_grid().rows[row][col];
                let start = text.len();
                text.push(cell.ch);
                let end = text.len();
                styles.push(CharStyle {
                    start,
                    end,
                    bold: cell.attrs.bold,
                    fg: cell.attrs.fg,
                });
            }
        }

        // 3. Configurar buffer de cosmic-text via set_rich_text
        let default_attrs = glyphon::Attrs::new().family(glyphon::Family::Monospace);
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
            (&text[s.start..s.end], attrs)
        });

        self.buffer.set_rich_text(
            &mut self.font_system,
            spans,
            &default_attrs,
            glyphon::Shaping::Advanced,
            None,
        );
        self.buffer.set_size(
            &mut self.font_system,
            Some(self.config.width as f32),
            Some(self.config.height as f32),
        );
        self.buffer.shape_until_scroll(&mut self.font_system, false);

        // 4. Preparar area de texto para glyphon
        // ponytail: bg no soportado nativamente por glyphon; se limpia con
        // negro. Renderizado de bg por celda requiere wgpu::RenderPass
        // separado (Sprint 4).
        let text_area = glyphon::TextArea {
            buffer: &self.buffer,
            left: 0.0,
            top: 0.0,
            scale: 1.0,
            bounds: glyphon::TextBounds::default(),
            default_color: glyphon::Color::rgb(0xcd, 0xd6, 0xf4),
            custom_glyphs: &[],
        };

        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                [text_area],
                &mut self.swash_cache,
            )
            .map_err(|e| format!("error al preparar texto: {e}"))?;

        // 5. Renderizar en el render pass
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

        // 6. Enviar comandos y presentar
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
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

/// Calcula el font size optimo para que el grid 24x80 llene la ventana
/// sin recorte. Usa el minimo entre el ancho de celda (con ratio monospace
/// 0.6) y el alto de celda (con line-height ratio 1.4). Asi, una ventana
/// 1920x1080 produce ~32px, una ventana 941x1030 produce ~20px, una
/// ventana pequena produce ~10px. El texto escala, no queda fijo.
///
/// ponytail: ratios hardcoded; refinables en Sprint 5 con SIGWINCH
/// cuando midamos el ancho real de un glyph monospace del sistema.
fn font_size_for_window(width: u32, height: u32) -> f32 {
    const GLYPH_RATIO: f32 = 0.6; // glyph width / font size para monospace
    const LINE_RATIO: f32 = 1.4; // line height / font size
    const MIN_SIZE: f32 = 6.0; // piso para que el texto sea legible
    let cell_w = width as f32 / COLS as f32;
    let cell_h = height as f32 / ROWS as f32;
    let from_w = cell_w / GLYPH_RATIO;
    let from_h = cell_h / LINE_RATIO;
    from_w.min(from_h).max(MIN_SIZE)
}

// ---------------------------------------------------------------------------
// Suite de tests unitarios del Renderer y pipeline de render
// ---------------------------------------------------------------------------
//
// Tests de color mapping: verifican que color_to_glyphon mapea correctamente
// los 9 colores ANSI a los valores Catppuccin Mocha hardcoded.
//
// Tests de propagacion SGR: verifican que el parser ANSI alimenta correctamente
// los attrs que el Renderer consume para construir los rich text spans.
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
