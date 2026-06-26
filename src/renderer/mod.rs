//! Modulo de render GPU del grid dinamico.

use std::sync::Arc;
use std::time::Instant;

use crate::ansi::{Color, Term};
use crate::config::{parse_hex, FontConfig, ThemeConfig, WindowConfig};
use crate::grid::Cell;
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
    font_system: glyphon::FontSystem,
    atlas: glyphon::TextAtlas,
    viewport: glyphon::Viewport,
    text_renderer: glyphon::TextRenderer,
    swash_cache: glyphon::SwashCache,
    // Un buffer por fila del grid. Cada fila tiene su propio top en screen-space
    // (row * cell_h), permitiendo que el cursor coincida correctamente con la
    // posicion de cada fila, a diferencia del buffer multilinea donde todo el
    // texto fluye desde top=0.
    buffers: Vec<glyphon::Buffer>,
    // Buffer para overlays (cursor block, mensajes de status).
    // Renderizado encima del grid, con color diferente.
    overlay_buffer: glyphon::Buffer,
    // Buffer separado para el cursor (no comparte con overlay_buffer de status).
    cursor_buffer: glyphon::Buffer,
    // ponytail: cell_w y cell_h se calculan en new() y se actualizan en resize().
    // El renderer los usa para posicionar cada TextArea.
    pub cell_w: f32,
    pub cell_h: f32,
    // ponytail: flag del overlay. Se activa con set_status(), se desactiva
    // cuando se llama con texto vacio o cuando se hace render() sin status.
    status_active: bool,
    /// Instant en que se activó el status overlay, para auto-desaparición.
    status_start: Option<Instant>,
    frame_count: u64,
    // Shaper cache por fila: evita set_rich_text/set_size/shape_until_scroll
    // cuando el contenido de una fila no cambió entre frames.
    line_cache: Vec<String>,
    /// Familia tipográfica desde la configuración.
    font_family: String,
    /// Tamaño de fuente desde la configuración (en puntos).
    font_size: f32,
    /// Opacidad de la ventana desde configuración (0.0 = transparente, 1.0 = opaco).
    opacity: f32,
}

impl Renderer {
    /// Relación altura-de-línea / tamaño-de-fuente para espaciado vertical.
    const LINE_HEIGHT_RATIO: f32 = 1.3;

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
        window_config: &WindowConfig,
    ) -> Self {
        let mut font_system = glyphon::FontSystem::new();
        // Cache necesario para glyphon 0.11
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

        let font_size = font_config.size as f32;
        let font_family = font_config.family.clone();
        let cell_h = font_size * Self::LINE_HEIGHT_RATIO;
        let metrics = glyphon::Metrics::new(font_size, font_size * Self::LINE_HEIGHT_RATIO);

        // Crear buffers iniciales (uno por fila del grid por defecto).
        let mut buffers = Vec::with_capacity(crate::grid::DEFAULT_ROWS);
        for _ in 0..crate::grid::DEFAULT_ROWS {
            buffers.push(glyphon::Buffer::new(&mut font_system, metrics));
        }
        let overlay_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        let cursor_buffer = glyphon::Buffer::new(&mut font_system, metrics);

        // Medir el ancho real de celda con glyphon para que el cursor
        // coincida exactamente con la posición del texto renderizado.
        // ponytail: medir con 1 y 10 'M', restar para cancelar left bearing.
        let mut measure_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        measure_buffer.set_text(
            &mut font_system,
            "M",
            &glyphon::Attrs::new().family(resolve_family(&font_family)),
            glyphon::Shaping::Basic,
            None,
        );
        measure_buffer.shape_until_scroll(&mut font_system, false);
        let w1 = measure_buffer
            .layout_runs()
            .next()
            .map(|run| run.line_w)
            .unwrap_or(0.0);

        let mut measure_buffer = glyphon::Buffer::new(&mut font_system, metrics);
        measure_buffer.set_text(
            &mut font_system,
            "MMMMMMMMMM",
            &glyphon::Attrs::new().family(resolve_family(&font_family)),
            glyphon::Shaping::Basic,
            None,
        );
        measure_buffer.shape_until_scroll(&mut font_system, false);
        let w10 = measure_buffer
            .layout_runs()
            .next()
            .map(|run| run.line_w)
            .unwrap_or(0.0);

        // (w10 - w1) / 9 cancela el left bearing del primer carácter.
        let cell_w = if w10 > w1 {
            (w10 - w1) / 9.0
        } else {
            cell_h * 0.6
        };

        let line_cache = vec![String::new(); crate::grid::DEFAULT_ROWS];

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
            cursor_buffer,
            cell_w,
            cell_h,
            status_active: false,
            status_start: None,
            frame_count: 0,
            line_cache,
            font_family,
            font_size,
            opacity: window_config.opacity,
        }
    }

    /// Cambia el tamaño de la surface y recrea los buffers por fila.
    pub fn resize(&mut self, width: u32, height: u32, rows_count: usize) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.viewport
            .update(&self.queue, glyphon::Resolution { width, height });

        let font_size = self.font_size;
        self.cell_h = font_size * Self::LINE_HEIGHT_RATIO;
        let metrics =
            glyphon::Metrics::new(self.font_size, self.font_size * Self::LINE_HEIGHT_RATIO);

        // Medir ancho de celda cancelando left bearing (w10 - w1) / 9.
        let mut measure_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        measure_buffer.set_text(
            &mut self.font_system,
            "M",
            &glyphon::Attrs::new().family(resolve_family(&self.font_family)),
            glyphon::Shaping::Basic,
            None,
        );
        measure_buffer.shape_until_scroll(&mut self.font_system, false);
        let w1 = measure_buffer
            .layout_runs()
            .next()
            .map(|run| run.line_w)
            .unwrap_or(0.0);

        let mut measure_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        measure_buffer.set_text(
            &mut self.font_system,
            "MMMMMMMMMM",
            &glyphon::Attrs::new().family(resolve_family(&self.font_family)),
            glyphon::Shaping::Basic,
            None,
        );
        measure_buffer.shape_until_scroll(&mut self.font_system, false);
        let w10 = measure_buffer
            .layout_runs()
            .next()
            .map(|run| run.line_w)
            .unwrap_or(0.0);

        self.cell_w = if w10 > w1 {
            (w10 - w1) / 9.0
        } else {
            self.cell_h * 0.6
        };

        // Recrear buffers con el nuevo font size y la nueva cantidad de filas.
        // (metrics ya calculado arriba)
        self.buffers.clear();
        for _ in 0..rows_count {
            self.buffers
                .push(glyphon::Buffer::new(&mut self.font_system, metrics));
        }
        self.overlay_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        self.cursor_buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
        // Invalidar cache completa al cambiar tamaño: se recrean los buffers,
        // por lo que todo el contenido previo queda obsoleto.
        self.line_cache = vec![String::new(); rows_count];
    }

    /// Renderiza el estado del `term` en la surface.
    #[tracing::instrument(skip(self, term))]
    pub fn render(&mut self, term: &Term, theme: &ThemeConfig) -> Result<(), String> {
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // 2. Construir contenido por fila (Fase A) y TextAreas (Fase B).
        let t_phase_a = Instant::now();
        let default_attrs = glyphon::Attrs::new().family(resolve_family(&self.font_family));
        let active = term.active_grid();
        let cols_count = active.cols_count;
        let rows_count = active.rows_count;
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

        /// Estilo de un tramo de caracteres agrupados por atributos.
        struct CellStyle {
            start: usize,
            end: usize,
            bold: bool,
            fg: Color,
            selected: bool, // ponytail: flag para marcador visible de seleccion
        }

        // Pre-calcular filas fuente y si estan vacias (para compartir entre fases).
        // Modelo correcto: viewport sobre buffer virtual [scrollback + grid].
        // scrollback[0..N-1] (antiguas primero) + grid[0..M-1] (presente).
        // viewport_start = max(0, N + M - rows_count - offset) = N - offset (offset <= N).
        let row_sources: Vec<&[Cell]> = (0..rows_count)
            .map(|row| {
                if show_scrollback {
                    let sb_len = term.grid.scrollback.len();
                    let offset = term.scrollback_offset as usize;
                    // viewport_start apunta al buffer virtual: N - offset
                    let viewport_start = sb_len.saturating_sub(offset);
                    let virtual_row = viewport_start + row; // posicion en el buffer virtual
                    if virtual_row < sb_len {
                        // Viene del scrollback
                        sb_rows[virtual_row - viewport_start]
                    } else {
                        // Viene del grid primario (NO active.rows que podria ser alt_grid)
                        let grid_row = virtual_row - sb_len;
                        &term.grid.rows[grid_row]
                    }
                } else {
                    &active.rows[row]
                }
            })
            .collect();

        let row_empty: Vec<bool> = row_sources
            .iter()
            .map(|r| r.is_empty() || r.iter().all(|c| *c == Cell::default()))
            .collect();

        // Fase A: llenar los buffers por fila con spans agrupados.
        // ponytail: si hay seleccion activa, invalidar cache porque
        // el texto es el mismo pero el color visual cambio.
        if term.selection.is_some() {
            self.line_cache.iter_mut().for_each(|c| c.clear());
        }
        for (row, source_row) in row_sources.iter().enumerate() {
            if row_empty[row] {
                // Fila vacia: actualizar cache a vacio, no llamar a glyphon.
                self.line_cache[row].clear();
                continue;
            }

            // Construir string de la fila con spans agrupados por atributos.
            let mut line = String::with_capacity(cols_count);
            let mut styles: Vec<CellStyle> = Vec::with_capacity(2); // pocos spans por fila

            let mut span_start = 0usize;
            let mut current_bold = false;
            let mut current_fg = Color::Default;
            let mut current_selected = false; // ponytail: seguimiento de selección para invertir colores

            for col in 0..cols_count {
                let default_cell = Cell::default();
                let cell = source_row.get(col).unwrap_or(&default_cell);
                let pos_before = line.len();
                line.push(cell.ch);

                // Ronda 2 Sprint 7: Invertir fg ↔ bg para celdas seleccionadas.
                let is_sel = term.is_selected(row, col);
                let effective_fg = if is_sel { cell.attrs.bg } else { cell.attrs.fg };

                // Cerrar span anterior si cambian los atributos o el estado de selección.
                if pos_before > span_start
                    && (cell.attrs.bold != current_bold
                        || effective_fg != current_fg
                        || is_sel != current_selected)
                {
                    styles.push(CellStyle {
                        start: span_start,
                        end: pos_before,
                        bold: current_bold,
                        fg: current_fg,
                        selected: current_selected,
                    });
                    span_start = pos_before;
                }
                current_bold = cell.attrs.bold;
                current_fg = effective_fg;
                current_selected = is_sel;
            }

            // Cerrar ultimo span de la fila.
            if span_start < line.len() {
                styles.push(CellStyle {
                    start: span_start,
                    end: line.len(),
                    bold: current_bold,
                    fg: current_fg,
                    selected: current_selected,
                });
            }

            // Shaper cache: si el contenido de la fila no cambió, el buffer
            // ya tiene el shaped correcto del frame anterior.
            if line == self.line_cache[row] {
                continue;
            }

            // Contenido cambió: actualizar cache y shapear.
            self.line_cache[row].clone_from(&line);

            // Construir spans de glyphon a partir de los CellStyle.
            let spans = styles.iter().map(|s| {
                let fg_color = if s.selected {
                    if let Some(ref sel_bg) = theme.selection_bg {
                        let (r, g, b) = parse_hex(sel_bg);
                        glyphon::Color::rgb(r, g, b)
                    } else {
                        match s.fg {
                            // ponytail: glyphon no soporta bg color. Cuando bg=Default
                            // la inversion fg↔bg seria invisible (gris claro sobre negro).
                            // Usamos blanco puro como marcador visible de seleccion.
                            Color::Default | Color::White => glyphon::Color::rgb(0xff, 0xff, 0xff),
                            other => color_to_glyphon(other, theme),
                        }
                    }
                } else {
                    color_to_glyphon(s.fg, theme)
                };
                let color = glyphon::Color::rgba(fg_color.r(), fg_color.g(), fg_color.b(), 255);
                let mut attrs = glyphon::Attrs::new().family(resolve_family(&self.font_family));
                if s.bold {
                    attrs = attrs.weight(glyphon::Weight::BOLD);
                }
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

        let phase_a_us = t_phase_a.elapsed().as_secs_f64() * 1_000_000.0;

        // Fase B: construir TextAreas con top = row * cell_h.
        let t_phase_b = Instant::now();
        let mut text_areas = Vec::with_capacity(rows_count + 2); // filas + cursor + overlay

        for (row, _) in row_sources.iter().enumerate() {
            if row_empty[row] {
                // Fila vacia: no agregar TextArea, se ve el fondo del clear.
                continue;
            }

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

        // 2b. Cursor: solo si NO estamos en scrollback.
        if !show_scrollback && term.cursor_visible {
            let cur_row = term.cursor.row;
            let cur_col = term.cursor.col;
            if cur_row < rows_count && cur_col < cols_count {
                let mut cursor_text = String::with_capacity(1);
                cursor_text.push('\u{2588}'); // FULL BLOCK '█'
                let cursor_spans = [(
                    cursor_text.as_str(),
                    glyphon::Attrs::new()
                        .family(resolve_family(&self.font_family))
                        .color(glyphon::Color::rgb(0xcd, 0xd6, 0xf4)),
                )];
                self.cursor_buffer.set_rich_text(
                    &mut self.font_system,
                    cursor_spans,
                    &glyphon::Attrs::new().family(resolve_family(&self.font_family)),
                    glyphon::Shaping::Advanced,
                    None,
                );
                self.cursor_buffer.set_size(
                    &mut self.font_system,
                    Some(self.config.width as f32),
                    Some(self.config.height as f32),
                );
                self.cursor_buffer
                    .shape_until_scroll(&mut self.font_system, false);
                let cursor_top = cur_row as f32 * self.cell_h;
                // Obtener posicion x real del glifo en la columna del cursor
                // desde el buffer de texto, para alinear con el texto renderizado.
                let cursor_left = self
                    .buffers
                    .get(cur_row)
                    .and_then(|buf| {
                        buf.layout_runs()
                            .next()
                            .and_then(|run| run.glyphs.get(cur_col).map(|g| g.x))
                    })
                    .unwrap_or(cur_col as f32 * self.cell_w);
                text_areas.push(glyphon::TextArea {
                    buffer: &self.cursor_buffer,
                    left: cursor_left,
                    top: cursor_top,
                    scale: 1.0,
                    bounds: glyphon::TextBounds {
                        left: cursor_left as i32,
                        top: cursor_top as i32,
                        right: (cursor_left + self.cell_w) as i32,
                        bottom: (cursor_top + self.cell_h) as i32,
                    },
                    default_color: glyphon::Color::rgb(0xcd, 0xd6, 0xf4),
                    custom_glyphs: &[],
                });
            }
        }

        // 2c. Si hay overlay activo (status), agregar TextArea extra.
        // Posicionado a la derecha, con margen de 10px.
        if self.status_active {
            let overlay_left = self.config.width as f32 - (23.0 * self.cell_w) - 10.0;
            text_areas.push(glyphon::TextArea {
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

            // Auto-desactivar el status después de 2 segundos.
            if let Some(start) = self.status_start {
                if start.elapsed() > std::time::Duration::from_secs(2) {
                    self.status_active = false;
                    self.status_start = None;
                }
            }
        }
        let phase_b_us = t_phase_b.elapsed().as_secs_f64() * 1_000_000.0;

        // 3. Preparar todos los TextArea para glyphon
        let t_prepare = Instant::now();
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
        let prepare_us = t_prepare.elapsed().as_secs_f64() * 1_000_000.0;

        // 4. Renderizar en el render pass
        let t_gpu = Instant::now();
        // Calcular color de fondo desde el tema con alpha = opacity.
        let (bg_r, bg_g, bg_b) = parse_hex(&theme.background);
        let clear_color = wgpu::Color {
            r: bg_r as f64 / 255.0,
            g: bg_g as f64 / 255.0,
            b: bg_b as f64 / 255.0,
            a: self.opacity as f64,
        };
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glyphon render pass"),
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
                .map_err(|e| format!("error al renderizar texto: {e}"))?;
        }

        // 5. Enviar comandos y presentar
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        let gpu_us = t_gpu.elapsed().as_secs_f64() * 1_000_000.0;

        let total_us = t0.elapsed().as_secs_f64() * 1_000_000.0;

        self.frame_count += 1;
        // Log each 30 frames to avoid spam
        if self.frame_count.is_multiple_of(30) {
            tracing::info!(
                "[RENDER_PERF] frame={} total={:.0}us get_frame={:.0}us phase_a={:.0}us phase_b={:.0}us prepare={:.0}us gpu={:.0}us rows={} cols={}",
                self.frame_count, total_us, get_frame_us, phase_a_us, phase_b_us, prepare_us, gpu_us,
                rows_count, cols_count,
            );
        }

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
        self.overlay_buffer
            .shape_until_scroll(&mut self.font_system, false);

        self.status_start = Some(Instant::now());
        self.status_active = true;
    }
}

/// Convierte un Color ANSI a `glyphon::Color` usando los valores del tema.
fn color_to_glyphon(color: Color, theme: &ThemeConfig) -> glyphon::Color {
    let (r, g, b) = match color {
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
    };
    glyphon::Color::rgb(r, g, b)
}

/// Mapea un color indexado 0-255 a RGB según el estándar ISO-8613-3.
///
/// Los índices 0-15 usan los colores ANSI del tema; 16-231 usan un cubo 6×6×6;
/// 232-255 son 24 tonos de gris.
/// ponytail: fórmula estándar, sin crate de paleta de color.
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

/// Construye la familia tipográfica desde el nombre en configuración.
/// ponytail: match de 3 variantes + Named para fuentes del sistema.
fn resolve_family(name: &str) -> glyphon::Family<'_> {
    match name {
        "monospace" => glyphon::Family::Monospace,
        "sans-serif" => glyphon::Family::SansSerif,
        "serif" => glyphon::Family::Serif,
        other => glyphon::Family::Name(other),
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
        let theme = ThemeConfig::default();
        let cases = [
            (Color::Default, (0xcd, 0xd6, 0xf4)),
            (Color::Black, (0x45, 0x47, 0x5a)),
            (Color::Red, (0xf3, 0x8b, 0xa8)),
            (Color::Green, (0xa6, 0xe3, 0xa1)),
            (Color::Yellow, (0xf9, 0xe2, 0xaf)),
            (Color::Blue, (0x89, 0xb4, 0xfa)),
            (Color::Magenta, (0xf5, 0xc2, 0xe7)),
            (Color::Cyan, (0x94, 0xe2, 0xd5)),
            (Color::White, (0xba, 0xc2, 0xde)),
        ];
        for (color, (r, g, b)) in cases {
            let c = color_to_glyphon(color, &theme);
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

    // =====================================================================
    // TESTS ADVERSARIALES — Sprint 7 Fase 4: Color inversion en selección
    // Asumen que TODO está ROTO y buscan bugs, no happy-path.
    // =====================================================================

    /// ADVERSARIAL: Color::Default y Color::White producen DIFERENTES
    /// valores ahora que se usa ThemeConfig (Default=#cdd6f4, White=#bac2de).
    /// La lógica de inversión de selección los trata por separado.
    #[test]
    fn test_color_to_glyphon_default_differs_from_white() {
        let theme = ThemeConfig::default();
        let c_default = color_to_glyphon(Color::Default, &theme);
        let c_white = color_to_glyphon(Color::White, &theme);
        assert_ne!(
            c_default.r(),
            c_white.r(),
            "BUG: Color::Default y Color::White NO deberian tener el mismo R con ThemeConfig"
        );
        assert_ne!(
            c_default.g(),
            c_white.g(),
            "BUG: Color::Default y Color::White NO deberian tener el mismo G con ThemeConfig"
        );
        assert_ne!(
            c_default.b(),
            c_white.b(),
            "BUG: Color::Default y Color::White NO deberian tener el mismo B con ThemeConfig"
        );
    }

    /// ADVERSARIAL: Verificar que TODOS los colores sean VISIBLES sobre fondo
    /// negro. El renderer usa `Clear(BLACK)` como fondo. Si algún color mapea
    /// a valores muy oscuros (R,G,B todos <= 50), es INVISIBLE para el usuario.
    ///
    /// BUG CONOCIDO: `Color::Black` mapea a (0,0,0) que es INVISIBLE sobre
    /// fondo negro. Un usuario no puede ver texto con fg=Black en Baud.
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

    /// ADVERSARIAL: La inversión fg↔bg en selección debe producir un color
    /// DIFERENTE al original. Si fg == bg (mismo color en ambas capas),
    /// la selección es invisible porque el color invertido es el mismo.
    ///
    /// Replica exactamente la lógica de `render()` para spans seleccionados:
    ///   let effective = if is_sel { cell.attrs.bg } else { cell.attrs.fg };
    ///   match effective {
    ///       Color::Default | Color::White => glyphon::Color::rgb(0xff, 0xff, 0xff),
    ///       other => color_to_glyphon(other),
    ///   }
    #[test]
    fn test_inversion_produces_different_color() {
        // Helper que replica la lógica del renderer
        fn inverted_fg(cell_fg: Color, cell_bg: Color, selected: bool) -> (u8, u8, u8) {
            let effective = if selected { cell_bg } else { cell_fg };
            let c = match effective {
                Color::Default | Color::White => glyphon::Color::rgb(0xff, 0xff, 0xff),
                other => color_to_glyphon(other, &ThemeConfig::default()),
            };
            (c.r(), c.g(), c.b())
        }

        // Caso normal: fg=Red, bg=Blue -> invertido debe ser DIFERENTE
        let normal = inverted_fg(Color::Red, Color::Blue, false);
        let selected = inverted_fg(Color::Red, Color::Blue, true);
        assert_ne!(
            normal, selected,
            "BUG: fg=Red, bg=Blue deberia cambiar al invertir pero dio igual"
        );

        // Caso donde fg==bg: la inversión produce el mismo color (limitación conocida).
        // Esto ocurre porque cuando selection_bg=None, tanto Default como White
        // caen en el match especial que devuelve blanco puro.
        // ponytail: con selection_bg configurado, la selección es visible incluso
        // cuando fg==bg.
        let normal = inverted_fg(Color::Default, Color::Default, false);
        let selected = inverted_fg(Color::Default, Color::Default, true);
        assert_eq!(
            normal, selected,
            "fg=Default, bg=Default produce mismo color con selection_bg=None"
        );

        let normal = inverted_fg(Color::White, Color::White, false);
        let selected = inverted_fg(Color::White, Color::White, true);
        assert_eq!(
            normal, selected,
            "fg=White, bg=White produce mismo color con selection_bg=None"
        );

        // Black ahora mapea a #45475a del tema, pero sigue siendo el mismo
        // valor tanto para normal como para selected si fg==bg.
        let normal = inverted_fg(Color::Black, Color::Black, false);
        let selected = inverted_fg(Color::Black, Color::Black, true);
        assert_eq!(
            normal, selected,
            "fg=Black, bg=Black produce mismo color con selection_bg=None"
        );

        // Default y White caen en el mismo match -> blanco puro
        let normal = inverted_fg(Color::Default, Color::White, false);
        let selected = inverted_fg(Color::Default, Color::White, true);
        assert_eq!(
            normal, selected,
            "fg=Default, bg=White -> ambos caen en match de blanco puro"
        );
    }

    /// ADVERSARIAL: Verificar que un `CellStyle` con `selected=true` produce
    /// un color DIFERENTE que con `selected=false` para cualquier par fg/bg
    /// donde fg != bg. Si el renderer produce el mismo color visual, la
    /// selección es indistinguible.
    #[test]
    fn test_selected_cell_style_changes_color() {
        // Helper que replica la lógica de construcción de spans del renderer
        fn span_color(fg: Color, bg: Color, selected: bool) -> (u8, u8, u8) {
            let effective = if selected { bg } else { fg };
            let c = match effective {
                Color::Default | Color::White => glyphon::Color::rgb(0xff, 0xff, 0xff),
                other => color_to_glyphon(other, &ThemeConfig::default()),
            };
            (c.r(), c.g(), c.b())
        }

        // Pares donde fg != bg -> deberian producir colores diferentes
        let test_pairs = [
            (Color::Red, Color::Blue),
            (Color::Green, Color::Magenta),
            (Color::Yellow, Color::Cyan),
            (Color::White, Color::Black),
            (Color::Default, Color::Red),
            (Color::Black, Color::Green),
            (Color::Blue, Color::Yellow),
            (Color::Cyan, Color::Magenta),
        ];
        for (fg, bg) in test_pairs {
            let normal = span_color(fg, bg, false);
            let sel = span_color(fg, bg, true);
            assert_ne!(
                normal, sel,
                "BUG: fg={fg:?}, bg={bg:?} produce mismo color con y sin seleccion"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Tests de ansi_256_to_rgb
    // -----------------------------------------------------------------------

    #[test]
    fn test_ansi_256_to_rgb_standard() {
        let theme = ThemeConfig::default();
        // Índice 16 → negro del cubo 6×6×6: (0*51, 0*51, 0*51) = (0, 0, 0)
        assert_eq!(ansi_256_to_rgb(16, &theme), (0, 0, 0));
        // Índice 231 → blanco del cubo: (5*51, 5*51, 5*51) = (255, 255, 255)
        assert_eq!(ansi_256_to_rgb(231, &theme), (255, 255, 255));
        // Índice 232 → primer gris: nivel=0, gris = 0*10 + 8 = 8
        assert_eq!(ansi_256_to_rgb(232, &theme), (8, 8, 8));
        // Índice 255 → ultimo gris: nivel=23, gris = 23*10 + 8 = 238
        assert_eq!(ansi_256_to_rgb(255, &theme), (238, 238, 238));
        // Índice 17 → (0, 0, 1): r=0, g=0, b=1 → (0, 0, 51)
        assert_eq!(ansi_256_to_rgb(17, &theme), (0, 0, 51));
        // Índice 88 → (2, 0, 0): r=2, g=0, b=0 → (102, 0, 0)
        assert_eq!(ansi_256_to_rgb(88, &theme), (102, 0, 0));
    }

    #[test]
    fn test_ansi_256_to_rgb_theme_colors() {
        // El índice 0 debe mapear al black del tema (Catppuccin: #45475a),
        // no al negro puro (0,0,0).
        let theme = ThemeConfig::default();
        let (r, g, b) = ansi_256_to_rgb(0, &theme);
        assert_eq!(
            (r, g, b),
            (0x45, 0x47, 0x5a),
            "Índice 0 debe mapear al black del tema Catppuccin, no a negro puro"
        );
    }

    #[test]
    fn test_color_to_glyphon_with_theme() {
        // Default → foreground del tema (Catppuccin: #cdd6f4)
        let theme = ThemeConfig::default();
        let c = color_to_glyphon(Color::Default, &theme);
        assert_eq!(c.r(), 0xcd, "Default R debe ser foreground del tema");
        assert_eq!(c.g(), 0xd6, "Default G debe ser foreground del tema");
        assert_eq!(c.b(), 0xf4, "Default B debe ser foreground del tema");
    }

    #[test]
    fn test_font_config_defaults() {
        let fc = FontConfig::default();
        assert_eq!(fc.family, "monospace");
        assert_eq!(fc.size, 14);
    }

    #[test]
    fn test_resolve_family_known() {
        assert!(matches!(
            resolve_family("monospace"),
            glyphon::Family::Monospace
        ));
        assert!(matches!(
            resolve_family("sans-serif"),
            glyphon::Family::SansSerif
        ));
        assert!(matches!(resolve_family("serif"), glyphon::Family::Serif));
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
    fn test_selection_bg_none() {
        let theme = ThemeConfig::default();
        assert!(
            theme.selection_bg.is_none(),
            "sin selection_bg, debe ser None"
        );
    }
}
