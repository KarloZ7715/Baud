//! Pase de quads texturizados para placements de gráficos.

use std::collections::HashMap;

use crate::graphics::{DecodedImage, ImageId, VisiblePlacement};
use crate::renderer::geometry::{cell_origin, CellGeometry};

const VERTEX_SIZE: u64 = 16;
const VERTEX_ATTRS: &[wgpu::VertexAttribute] = &[
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 8,
        shader_location: 1,
    },
];

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

struct CachedImage {
    bind_group: wgpu::BindGroup,
    generation: u64,
    width: u32,
    height: u32,
}

/// Pipeline y caché de texturas. Se crea en el primer frame con imágenes.
pub struct ImagePass {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cache: HashMap<ImageId, CachedImage>,
    vbo: wgpu::Buffer,
    vbo_capacity: u64,
    prepared: Vec<PreparedDraw>,
    below_end: usize,
}

struct PreparedDraw {
    id: ImageId,
    vertex_start: u32,
    scissor: (u32, u32, u32, u32),
}

#[derive(Clone, Copy)]
pub struct ImageDraw {
    pub placement: VisiblePlacement,
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub scissor: (u32, u32, u32, u32),
}

impl ImagePass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graphics images"),
            source: wgpu::ShaderSource::Wgsl(include_str!("images.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graphics images bind"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graphics images pipeline layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graphics images pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_SIZE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: VERTEX_ATTRS,
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("graphics images sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graphics images vbo"),
            size: VERTEX_SIZE * 6,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            bind_layout,
            sampler,
            cache: HashMap::new(),
            vbo,
            vbo_capacity: VERTEX_SIZE * 6,
            prepared: Vec::new(),
            below_end: 0,
        }
    }

    pub fn sync_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: ImageId,
        img: &DecodedImage,
    ) {
        if self
            .cache
            .get(&id)
            .is_some_and(|c| c.generation == img.generation)
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("graphics image"),
            size: wgpu::Extent3d {
                width: img.width.max(1),
                height: img.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut premul = img.rgba.clone();
        for px in premul.chunks_exact_mut(4) {
            let a = f32::from(px[3]) / 255.0;
            px[0] = (f32::from(px[0]) * a).round() as u8;
            px[1] = (f32::from(px[1]) * a).round() as u8;
            px[2] = (f32::from(px[2]) * a).round() as u8;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &premul,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width.max(1) * 4),
                rows_per_image: Some(img.height.max(1)),
            },
            wgpu::Extent3d {
                width: img.width.max(1),
                height: img.height.max(1),
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graphics image bind"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.cache.insert(
            id,
            CachedImage {
                bind_group,
                generation: img.generation,
                width: img.width,
                height: img.height,
            },
        );
    }

    pub fn drop_ids(&mut self, ids: &[ImageId]) {
        for id in ids {
            self.cache.remove(id);
        }
    }

    /// Empaqueta vértices de ambos grupos z. Llamar antes del render pass.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[ImageDraw],
        screen_w: f32,
        screen_h: f32,
    ) {
        self.prepared.clear();
        let mut verts: Vec<Vertex> = Vec::new();
        for draw in draws.iter().filter(|d| d.placement.z < 0) {
            self.push_prepared(draw, screen_w, screen_h, &mut verts);
        }
        self.below_end = self.prepared.len();
        for draw in draws.iter().filter(|d| d.placement.z >= 0) {
            self.push_prepared(draw, screen_w, screen_h, &mut verts);
        }
        if verts.is_empty() {
            return;
        }
        let bytes = vertices_as_bytes(&verts);
        let size = bytes.len() as u64;
        if size > self.vbo_capacity {
            self.vbo = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("graphics images vbo"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vbo_capacity = size;
        }
        queue.write_buffer(&self.vbo, 0, bytes);
    }

    fn push_prepared(
        &mut self,
        draw: &ImageDraw,
        screen_w: f32,
        screen_h: f32,
        verts: &mut Vec<Vertex>,
    ) {
        let Some(cached) = self.cache.get(&draw.placement.image_id) else {
            return;
        };
        let (sx, sy, sw, sh) = draw.scissor;
        if sw == 0 || sh == 0 {
            return;
        }
        let start = verts.len() as u32;
        verts.extend_from_slice(&quad_vertices(
            draw,
            cached.width,
            cached.height,
            screen_w,
            screen_h,
        ));
        self.prepared.push(PreparedDraw {
            id: draw.placement.image_id,
            vertex_start: start,
            scissor: (sx, sy, sw, sh),
        });
    }

    pub fn draw_below(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_range(pass, 0, self.below_end);
    }

    pub fn draw_above(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.draw_range(pass, self.below_end, self.prepared.len());
    }

    fn draw_range(&self, pass: &mut wgpu::RenderPass<'_>, start: usize, end: usize) {
        if start >= end {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vbo.slice(..self.vbo_capacity));
        for item in &self.prepared[start..end] {
            let Some(cached) = self.cache.get(&item.id) else {
                continue;
            };
            let (sx, sy, sw, sh) = item.scissor;
            pass.set_scissor_rect(sx, sy, sw, sh);
            pass.set_bind_group(0, &cached.bind_group, &[]);
            pass.draw(item.vertex_start..item.vertex_start + 6, 0..1);
        }
    }
}

fn vertices_as_bytes(verts: &[Vertex]) -> &[u8] {
    let ptr = verts.as_ptr().cast::<u8>();
    // SAFETY: Vertex es repr(C) de 4 f32; el slice es denso y alineado.
    unsafe { std::slice::from_raw_parts(ptr, std::mem::size_of_val(verts)) }
}

fn quad_vertices(
    draw: &ImageDraw,
    img_w: u32,
    img_h: u32,
    screen_w: f32,
    screen_h: f32,
) -> [Vertex; 6] {
    let (row, col, rows, cols) = draw.placement.dst_cells;
    let geom = CellGeometry::new(draw.cell_w, draw.cell_h);
    let (x0, y0) = cell_origin(
        row as usize,
        col as usize,
        geom,
        draw.origin_x,
        draw.origin_y,
    );
    let x1 = x0 + f32::from(cols) * draw.cell_w;
    let y1 = y0 + f32::from(rows) * draw.cell_h;
    let (sx, sy, sw, sh) = draw.placement.src_rect_px;
    let iw = img_w.max(1) as f32;
    let ih = img_h.max(1) as f32;
    let u0 = sx as f32 / iw;
    let v0 = sy as f32 / ih;
    let u1 = (sx + sw.max(1)) as f32 / iw;
    let v1 = (sy + sh.max(1)) as f32 / ih;
    let tl = Vertex {
        pos: ndc(x0, y0, screen_w, screen_h),
        uv: [u0, v0],
    };
    let tr = Vertex {
        pos: ndc(x1, y0, screen_w, screen_h),
        uv: [u1, v0],
    };
    let bl = Vertex {
        pos: ndc(x0, y1, screen_w, screen_h),
        uv: [u0, v1],
    };
    let br = Vertex {
        pos: ndc(x1, y1, screen_w, screen_h),
        uv: [u1, v1],
    };
    [tl, tr, bl, bl, tr, br]
}

fn ndc(x: f32, y: f32, w: f32, h: f32) -> [f32; 2] {
    let w = w.max(1.0);
    let h = h.max(1.0);
    [(x / w) * 2.0 - 1.0, 1.0 - (y / h) * 2.0]
}

/// Convierte placements visibles a draws en píxeles de pane.
pub fn draws_for_placements(
    placements: &[VisiblePlacement],
    origin_x: f32,
    origin_y: f32,
    cell_w: f32,
    cell_h: f32,
    pane_px: (u32, u32, u32, u32),
) -> Vec<ImageDraw> {
    placements
        .iter()
        .copied()
        .map(|placement| ImageDraw {
            placement,
            origin_x,
            origin_y,
            cell_w,
            cell_h,
            scissor: pane_px,
        })
        .collect()
}
