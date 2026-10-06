//! 2D sprite renderer on wgpu: premultiplied-alpha textured quads, mip-mapped textures loaded from PNG.
use bytemuck::{Pod, Zeroable};
use std::path::Path;

/// One quad. `rect` = x, y, w, h in target pixels (y down); `uv` = u0, v0, u1, v1; `tint` is PREMULTIPLIED rgba.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub tint: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextureId(pub usize);

#[derive(Clone, Copy)]
pub struct Draw {
    pub tex: TextureId,
    pub inst: Instance,
}

struct GpuTexture {
    sdf: bool,
    bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

pub struct SpriteRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    sdf_pipeline: wgpu::RenderPipeline,
    frame_buf: wgpu::Buffer,
    frame_bg: wgpu::BindGroup,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: Vec<GpuTexture>,
    inst_buf: wgpu::Buffer,
    inst_cap: u64,
}

fn premultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in 0..3 {
            px[c] = ((px[c] as u32 * a + 127) / 255) as u8;
        }
    }
}

/// 2x2 box filter on premultiplied RGBA8.
fn downsample(src: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    let i = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = ((acc[c] + 2) / 4) as u8;
            }
        }
    }
    (nw, nh, out)
}

fn premul_blend() -> wgpu::BlendState {
    let c = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState { color: c, alpha: c }
}

const INSTANCE_STRIDE: u64 = std::mem::size_of::<Instance>() as u64;

impl SpriteRenderer {
    /// `target_format` must be non-sRGB: all compositing happens in gamma space, like the original game.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat) -> Self {
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame ubo"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bg"),
            layout: &frame_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame_buf.as_entire_binding() }],
        });
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture bgl"),
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
            label: Some("sprite layout"),
            bind_group_layouts: &[Some(&frame_bgl), Some(&tex_bgl)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });
        let attr = |location: u32, offset: u64| wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset,
            shader_location: location,
        };
        let attrs = [attr(0, 0), attr(1, 16), attr(2, 32)];
        let make_pipeline = |label: &str, fs: &str| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(premul_blend()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline = make_pipeline("sprite pipeline", "fs_main");
        let sdf_pipeline = make_pipeline("sdf pipeline", "fs_sdf");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let inst_cap = 256;
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite instances"),
            size: inst_cap * INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        SpriteRenderer { device, queue, pipeline, sdf_pipeline, frame_buf, frame_bg, tex_bgl, sampler, textures: Vec::new(), inst_buf, inst_cap }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Loads a PNG (straight alpha) and uploads it premultiplied with a full mip chain. Returns the id and the pixel size.
    pub fn load_png(&mut self, path: &Path) -> Result<(TextureId, u32, u32), String> {
        self.load_png_as(path, false)
    }

    /// Loads a signed-distance-field glyph sheet (distance in alpha); drawn with the SDF pipeline.
    pub fn load_sdf_png(&mut self, path: &Path) -> Result<(TextureId, u32, u32), String> {
        self.load_png_as(path, true)
    }

    fn load_png_as(&mut self, path: &Path, sdf: bool) -> Result<(TextureId, u32, u32), String> {
        let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
        let (w, h) = img.dimensions();
        let id = self.upload(&path.to_string_lossy(), w, h, img.into_raw(), sdf);
        Ok((id, w, h))
    }

    /// A 1x1 opaque white texture; tint it to draw solid rectangles.
    pub fn white_pixel(&mut self) -> TextureId {
        self.upload("white pixel", 1, 1, vec![255, 255, 255, 255], false)
    }

    fn upload(&mut self, label: &str, w: u32, h: u32, mut rgba: Vec<u8>, sdf: bool) -> TextureId {
        if !sdf {
            premultiply(&mut rgba);
        }
        let mips = 32 - w.max(h).leading_zeros();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (mut lw, mut lh, mut level_data) = (w, h, rgba);
        for level in 0..mips {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &level_data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(lh) },
                wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
            );
            if level + 1 < mips {
                let (nw, nh, next) = downsample(&level_data, lw, lh);
                lw = nw;
                lh = nh;
                level_data = next;
            }
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.tex_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.textures.push(GpuTexture { sdf, bind_group, _texture: texture });
        TextureId(self.textures.len() - 1)
    }

    /// Draws `draws` in order into `target` (`w` x `h` pixels). `clear` = opaque colour to clear to first;
    /// `None` keeps what is already there (the HUD pass over a 3D scene).
    pub fn render(&mut self, draws: &[Draw], target: &wgpu::TextureView, w: u32, h: u32, clear: Option<[f64; 3]>) {
        self.queue.write_buffer(&self.frame_buf, 0, bytemuck::cast_slice(&[w as f32, h as f32, 0.0f32, 0.0f32]));

        if draws.len() as u64 > self.inst_cap {
            self.inst_cap = (draws.len() as u64).next_power_of_two();
            self.inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sprite instances"),
                size: self.inst_cap * INSTANCE_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let instances: Vec<Instance> = draws.iter().map(|d| d.inst).collect();
        if !instances.is_empty() {
            self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&instances));
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sprites") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprite pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match clear {
                            Some(c) => wgpu::LoadOp::Clear(wgpu::Color { r: c[0], g: c[1], b: c[2], a: 1.0 }),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bg, &[]);
            pass.set_vertex_buffer(0, self.inst_buf.slice(..));
            let mut first = 0usize;
            let mut sdf_bound = false;
            while first < draws.len() {
                let tex = draws[first].tex;
                let mut end = first + 1;
                while end < draws.len() && draws[end].tex == tex {
                    end += 1;
                }
                if self.textures[tex.0].sdf != sdf_bound {
                    sdf_bound = !sdf_bound;
                    pass.set_pipeline(if sdf_bound { &self.sdf_pipeline } else { &self.pipeline });
                }
                pass.set_bind_group(1, &self.textures[tex.0].bind_group, &[]);
                pass.draw(0..6, first as u32..end as u32);
                first = end;
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Renders offscreen and returns RGBA8 pixels (row 0 = top). The target format must be `Rgba8Unorm`.
    pub fn render_to_rgba(&mut self, draws: &[Draw], w: u32, h: u32, clear: [f64; 3]) -> Vec<u8> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render(draws, &view, w, h, Some(clear));
        self.read_back(&texture, w, h)
    }

    /// Copies an `Rgba8Unorm` texture to the CPU (row 0 = top).
    pub fn read_back(&self, texture: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
        let unpadded = w * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("gpu poll");
        rx.recv().expect("map callback").expect("buffer map");
        let data = slice.get_mapped_range().expect("mapped range");
        let mut out = Vec::with_capacity((unpadded * h) as usize);
        for row in 0..h {
            let start = (row * padded) as usize;
            out.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        buffer.unmap();
        out
    }
}
