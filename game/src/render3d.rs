//! 3D mesh renderer: instanced, vertex-coloured, hemisphere + sun lit, distance fog, depth buffered.
//! Draws before the 2D sprite pass, which then puts the HUD on top.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex3 {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Default, Clone)]
pub struct MeshData {
    pub vertices: Vec<Vertex3>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Appends `other` transformed by `m` (positions as points, normals as directions).
    pub fn append(&mut self, other: &MeshData, m: Mat4) {
        let base = self.vertices.len() as u32;
        for v in &other.vertices {
            let p = m.transform_point3(Vec3::from(v.pos));
            let n = m.transform_vector3(Vec3::from(v.normal)).normalize_or_zero();
            self.vertices.push(Vertex3 { pos: p.to_array(), normal: n.to_array(), color: v.color });
        }
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

/// Textured vertex: the plain vertex plus texture coordinates.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct TVertex3 {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}

#[derive(Default, Clone)]
pub struct TexMeshData {
    pub vertices: Vec<TVertex3>,
    pub indices: Vec<u32>,
}

/// A texture uploaded to the 3D renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TexId(pub usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MeshId(pub usize);

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
}

#[derive(Clone, Copy)]
pub struct Draw3d {
    pub mesh: MeshId,
    pub model: Mat4,
    pub tint: [f32; 4],
}

pub struct Camera {
    pub view_proj: Mat4,
    pub eye: Vec3,
}

pub struct World {
    pub camera: Camera,
    pub draws: Vec<Draw3d>,
    pub sky: [f32; 3],
    pub fog_density: f32,
    pub sun_dir: Vec3,
    /// Draw over what is already in the target (a 3D view inside a 2D screen) instead of clearing to the sky.
    pub overlay: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraRaw {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    fog: [f32; 4],
    sun: [f32; 4],
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Some = drawn with the textured pipeline and this texture.
    texture: Option<usize>,
}

struct DepthTarget {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    width: u32,
    height: u32,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INSTANCE_STRIDE: u64 = std::mem::size_of::<InstanceRaw>() as u64;

pub struct Scene3d {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    tex_pipeline: wgpu::RenderPipeline,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: Vec<wgpu::BindGroup>,
    camera_buf: wgpu::Buffer,
    camera_bg: wgpu::BindGroup,
    meshes: Vec<GpuMesh>,
    inst_buf: wgpu::Buffer,
    inst_cap: u64,
    depth: Option<DepthTarget>,
}

impl Scene3d {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat) -> Self {
        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera ubo"),
            size: std::mem::size_of::<CameraRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bg"),
            layout: &camera_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buf.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh layout"),
            bind_group_layouts: &[Some(&camera_bgl)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        let vertex_attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 24, shader_location: 2 },
        ];
        let inst_attrs: Vec<wgpu::VertexAttribute> = (0..5)
            .map(|k| wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16 * k as u64, shader_location: 3 + k })
            .collect();
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh texture bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let tex_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("mesh tex layout"), bind_group_layouts: &[Some(&camera_bgl), Some(&tex_bgl)], immediate_size: 0 });
        let tvertex_attrs = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 24, shader_location: 2 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 40, shader_location: 8 },
        ];
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mesh sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex3>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attrs,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE_STRIDE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &inst_attrs,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let tex_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh tex pipeline"),
            layout: Some(&tex_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_tex"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<TVertex3>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &tvertex_attrs }),
                    Some(wgpu::VertexBufferLayout { array_stride: INSTANCE_STRIDE, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs }),
                ],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_tex"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let inst_cap = 256;
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh instances"),
            size: inst_cap * INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Scene3d { device, queue, pipeline, tex_pipeline, tex_bgl, sampler, textures: Vec::new(), camera_buf, camera_bg, meshes: Vec::new(), inst_buf, inst_cap, depth: None }
    }

    pub fn add_mesh(&mut self, data: &MeshData) -> MeshId {
        use wgpu::util::DeviceExt;
        let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&data.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.push(GpuMesh { vertices, indices, index_count: data.indices.len() as u32, texture: None });
        MeshId(self.meshes.len() - 1)
    }

    /// Uploads an RGBA8 image (straight alpha) with a CPU-built mip chain.
    pub fn add_texture(&mut self, w: u32, h: u32, rgba: Vec<u8>) -> TexId {
        let mips = 32 - w.max(h).leading_zeros();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("track texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (mut lw, mut lh, mut data) = (w, h, rgba);
        for level in 0..mips {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(lh) },
                wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
            );
            if level + 1 < mips {
                let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
                let mut next = vec![0u8; (nw * nh * 4) as usize];
                for y in 0..nh {
                    for x in 0..nw {
                        let mut acc = [0u32; 4];
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let (sx, sy) = (((x * 2 + dx).min(lw - 1)), ((y * 2 + dy).min(lh - 1)));
                            let i = ((sy * lw + sx) * 4) as usize;
                            for c in 0..4 {
                                acc[c] += data[i + c] as u32;
                            }
                        }
                        let o = ((y * nw + x) * 4) as usize;
                        for c in 0..4 {
                            next[o + c] = ((acc[c] + 2) / 4) as u8;
                        }
                    }
                }
                lw = nw;
                lh = nh;
                data = next;
            }
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("track texture bg"),
            layout: &self.tex_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.textures.push(bind);
        TexId(self.textures.len() - 1)
    }

    /// A mesh drawn with `texture` (the textured pipeline).
    pub fn add_tex_mesh(&mut self, data: &TexMeshData, texture: TexId) -> MeshId {
        use wgpu::util::DeviceExt;
        let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("tex mesh vertices"), contents: bytemuck::cast_slice(&data.vertices), usage: wgpu::BufferUsages::VERTEX });
        let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("tex mesh indices"), contents: bytemuck::cast_slice(&data.indices), usage: wgpu::BufferUsages::INDEX });
        self.meshes.push(GpuMesh { vertices, indices, index_count: data.indices.len() as u32, texture: Some(texture.0) });
        MeshId(self.meshes.len() - 1)
    }

    fn depth_view(&mut self, w: u32, h: u32) -> &wgpu::TextureView {
        let stale = self.depth.as_ref().map_or(true, |d| d.width != w || d.height != h);
        if stale {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth = Some(DepthTarget { view, _texture: texture, width: w, height: h });
        }
        &self.depth.as_ref().unwrap().view
    }

    /// Draws `world` into `target`, clearing colour to the sky colour and depth to far.
    pub fn render(&mut self, world: &World, target: &wgpu::TextureView, w: u32, h: u32) {
        let camera = CameraRaw {
            view_proj: world.camera.view_proj.to_cols_array_2d(),
            eye: [world.camera.eye.x, world.camera.eye.y, world.camera.eye.z, 1.0],
            fog: [world.sky[0], world.sky[1], world.sky[2], world.fog_density],
            sun: [world.sun_dir.x, world.sun_dir.y, world.sun_dir.z, 0.0],
        };
        self.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&camera));

        if world.draws.len() as u64 > self.inst_cap {
            self.inst_cap = (world.draws.len() as u64).next_power_of_two();
            self.inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh instances"),
                size: self.inst_cap * INSTANCE_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        let instances: Vec<InstanceRaw> =
            world.draws.iter().map(|d| InstanceRaw { model: d.model.to_cols_array_2d(), tint: d.tint }).collect();
        if !instances.is_empty() {
            self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&instances));
        }

        // borrow dance: the depth view is created lazily but must outlive the pass
        self.depth_view(w, h);
        let depth_view = &self.depth.as_ref().unwrap().view;

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mesh") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mesh pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if world.overlay {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color { r: world.sky[0] as f64, g: world.sky[1] as f64, b: world.sky[2] as f64, a: 1.0 })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bg, &[]);
            pass.set_vertex_buffer(1, self.inst_buf.slice(..));
            let mut textured = false;
            let mut first = 0usize;
            while first < world.draws.len() {
                let mesh = world.draws[first].mesh;
                let mut end = first + 1;
                while end < world.draws.len() && world.draws[end].mesh == mesh {
                    end += 1;
                }
                let gpu = &self.meshes[mesh.0];
                if gpu.texture.is_some() != textured {
                    textured = !textured;
                    pass.set_pipeline(if textured { &self.tex_pipeline } else { &self.pipeline });
                    pass.set_bind_group(0, &self.camera_bg, &[]);
                    pass.set_vertex_buffer(1, self.inst_buf.slice(..));
                }
                if let Some(t) = gpu.texture {
                    pass.set_bind_group(1, &self.textures[t], &[]);
                }
                pass.set_vertex_buffer(0, gpu.vertices.slice(..));
                pass.set_index_buffer(gpu.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu.index_count, 0, first as u32..end as u32);
                first = end;
            }
        }
        self.queue.submit([encoder.finish()]);
    }
}
