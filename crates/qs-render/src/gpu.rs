// SPDX-License-Identifier: GPL-3.0-or-later

//! A [`MapMesh`] on the GPU, drawn inside egui's render pass through a paint callback
//! (ADR-0003): the map view and the preview host share it.

use egui_wgpu::wgpu::util::{BufferInitDescriptor, DeviceExt, TextureDataOrder};
use egui_wgpu::wgpu::{self, BindGroup, Buffer, Device, Queue, RenderPipeline, TextureFormat};
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

use crate::alias::AliasVertex;
use crate::mesh::{Animation, Batch, Image, MapMesh, Placement, Surface, frame, mip_levels};

/// The bytes of a [`crate::mesh::Vertex`] on the GPU: position, texture and light coordinates.
const VERTEX_BYTES: u64 = 7 * 4;
/// The bytes of an [`AliasVertex`] on the GPU: position, skin coordinates and shade.
const ALIAS_VERTEX_BYTES: u64 = 6 * 4;
/// The bytes of the view's uniform, one 4 by 4 matrix.
/// The bytes of the view's uniform: a 4 by 4 matrix, then the eye's position and the time.
const VIEW_BYTES: u64 = 20 * 4;

/// A map's buffers, textures and pipeline, kept in egui's callback resources.
pub struct MapRenderer {
    /// The pipelines of lit, turbulent and sky surfaces.
    pipelines: [RenderPipeline; 3],
    vertices: Buffer,
    indices: Buffer,
    view: Buffer,
    shared: BindGroup,
    textures: Vec<BindGroup>,
    batches: Vec<Batch>,
    animations: Vec<Option<Animation>>,
    /// The pipeline of alias models (REND-4).
    alias_pipeline: RenderPipeline,
    /// Each alias skin image, bound as a surface texture.
    skins: Vec<BindGroup>,
    /// This frame's alias triangles, and how many vertices the buffer holds.
    alias: Option<(Buffer, u64)>,
    alias_draws: Vec<AliasDraw>,
    /// Each model's matrix, one every [`MODEL_STRIDE`] bytes, the world's first (REND-1.9).
    models: Buffer,
    model_group: BindGroup,
    model_count: usize,
}

/// The bytes between two models' matrices: the dynamic offset alignment every device allows.
const MODEL_STRIDE: u64 = 256;
/// The bytes of one model's matrix.
const MATRIX_BYTES: u64 = 16 * 4;

/// One run of a frame's alias vertices, drawn with one skin image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AliasDraw {
    /// The index of its image among the skins the renderer was made with.
    pub skin: usize,
    /// Its first vertex.
    pub first: u32,
    /// Its vertices, three a triangle.
    pub count: u32,
}

/// The alias models of one frame: their vertices and the runs drawn with each skin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelFrame {
    /// Every model's vertices, three a triangle.
    pub vertices: Vec<AliasVertex>,
    /// The runs of `vertices`.
    pub draws: Vec<AliasDraw>,
}

/// The pipeline of each kind of surface, an index into [`MapRenderer`]'s pipelines.
const fn pipeline_of(surface: Surface) -> usize {
    match surface {
        Surface::Lit => 0,
        Surface::Turbulent => 1,
        Surface::Sky => 2,
    }
}

impl MapRenderer {
    /// Uploads `mesh` and the alias model skins `skins`, and builds the pipelines for a target
    /// of `format` with a depth buffer of `depth` (the format egui's render pass was made
    /// with).
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "one pipeline's layouts, buffers and bind groups read best in the order wgpu needs them"
    )]
    pub fn new(
        device: &Device,
        queue: &Queue,
        (format, depth): (TextureFormat, TextureFormat),
        mesh: &MapMesh,
        skins: &[Image],
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("qs-render world"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let shared_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render view and lightmap"),
            entries: &[
                // The matrix is read by the vertex stage, the eye and the time by the water's and
                // the sky's fragments (REND-2.2, REND-2.3).
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render surface texture"),
            // A surface's texture and its sampler; for the sky, its solid layer, and its alpha
            // layer at 2 (REND-2.3).
            entries: &[texture_entry(0), sampler_entry(1), texture_entry(2)],
        });
        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("qs-render model"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(MATRIX_BYTES),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("qs-render world"),
            bind_group_layouts: &[
                Some(&shared_layout),
                Some(&texture_layout),
                Some(&model_layout),
            ],
            immediate_size: 0,
        });
        let model_count = mesh.models.len().max(1);
        let models = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("qs-render models"),
            size: MODEL_STRIDE.saturating_mul(u64::try_from(model_count).unwrap_or(1)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qs-render model"),
            layout: &model_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &models,
                    offset: 0,
                    size: wgpu::BufferSize::new(MATRIX_BYTES),
                }),
            }],
        });
        let placed: Vec<[[f32; 4]; 4]> = mesh.models.iter().map(Placement::matrix).collect();
        queue.write_buffer(&models, 0, &model_bytes(&placed));
        let world_vertex = wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2],
        };
        let alias_vertex = wgpu::VertexBufferLayout {
            array_stride: ALIAS_VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32],
        };
        let pipeline = |(vertex, layout_of): (&str, &wgpu::VertexBufferLayout<'_>), entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("qs-render world"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(layout_of.clone())],
                },
                primitive: primitive(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(DEPTH_COMPARE),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let world = ("vertex", &world_vertex);
        let pipelines = [
            pipeline(world, "fragment"),
            pipeline(world, "turbulent"),
            pipeline(world, "sky"),
        ];
        let alias_pipeline = pipeline(("alias_vertex", &alias_vertex), "alias_fragment");
        let vertex_bytes: Vec<u8> = mesh
            .vertices
            .iter()
            .flat_map(|v| {
                [
                    v.position[0],
                    v.position[1],
                    v.position[2],
                    v.texture[0],
                    v.texture[1],
                    v.light[0],
                    v.light[1],
                ]
            })
            .flat_map(f32::to_ne_bytes)
            .collect();
        let index_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
        let vertices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("qs-render vertices"),
            contents: &vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("qs-render indices"),
            contents: &index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("qs-render view"),
            size: VIEW_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Textures keep Quake's sharp texels; the lightmap is smoothed, as GLQuake draws it.
        let mipmapped = device.create_sampler(&world_sampler());
        let linear_repeat = sampler(device, wgpu::FilterMode::Linear, wgpu::AddressMode::Repeat);
        let linear = sampler(
            device,
            wgpu::FilterMode::Linear,
            wgpu::AddressMode::ClampToEdge,
        );
        let lightmap = upload(device, queue, &mesh.lightmap);
        let shared = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("qs-render view and lightmap"),
            layout: &shared_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&lightmap),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
            ],
        });
        let bind =
            |texture: &wgpu::TextureView, filter: &wgpu::Sampler, second: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("qs-render surface texture"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(texture),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(filter),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(second),
                        },
                    ],
                })
            };
        // REND-4.3: a skin is mipmapped and filtered as a world texture is.
        let skins = skins
            .iter()
            .map(|image| {
                let texture = upload_levels(device, queue, &mip_levels(image));
                bind(&texture, &mipmapped, &texture)
            })
            .collect();
        let textures = mesh
            .textures
            .iter()
            .enumerate()
            .map(|(slot, image)| {
                let sky = mesh.skies.get(slot).and_then(Option::as_ref);
                // A sky's two layers, each repeating on its own, filtered linearly as GLQuake
                // filters them (`R_InitSky`); any other texture is its own second binding.
                let (texture, second, filter) = match sky {
                    Some([solid, alpha]) => (
                        upload(device, queue, solid),
                        upload(device, queue, alpha),
                        &linear_repeat,
                    ),
                    None => {
                        let texture = upload_levels(device, queue, &mip_levels(image));
                        (texture.clone(), texture, &mipmapped)
                    }
                };
                bind(&texture, filter, &second)
            })
            .collect();
        Self {
            pipelines,
            vertices,
            indices,
            view,
            shared,
            textures,
            batches: mesh.batches.clone(),
            animations: mesh.animations.clone(),
            alias_pipeline,
            skins,
            alias: None,
            alias_draws: Vec::new(),
            models,
            model_group,
            model_count,
        }
    }

    /// Moves the models to `matrices`, the world's first, as the entities stand now
    /// (REND-1.9); a matrix past the mesh's models is left out.
    pub fn set_placements(&self, queue: &Queue, matrices: &[[[f32; 4]; 4]]) {
        let take = matrices.len().min(self.model_count);
        queue.write_buffer(
            &self.models,
            0,
            &model_bytes(matrices.get(..take).unwrap_or(&[])),
        );
    }

    /// Sets the alias models drawn next, growing their buffer when they need more room.
    pub fn set_models(&mut self, device: &Device, queue: &Queue, models: &ModelFrame) {
        let bytes = alias_bytes(&models.vertices);
        let needed = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if let Some(size) = grown(self.alias.as_ref().map(|(_, size)| *size), needed) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("qs-render alias vertices"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.alias = Some((buffer, size));
        }
        if let Some((buffer, _)) = &self.alias {
            queue.write_buffer(buffer, 0, &bytes);
        }
        self.alias_draws.clone_from(&models.draws);
    }

    /// Sets the view for the next draw: the matrix from map coordinates to clip space, the
    /// eye's position (REND-2.3) and the time in seconds (REND-2.2, REND-2.3).
    pub fn set_view(&self, queue: &Queue, view: &View) {
        let eye_time = [view.eye[0], view.eye[1], view.eye[2], view.time];
        let bytes: Vec<u8> = view
            .clip_from_world
            .iter()
            .flatten()
            .chain(&eye_time)
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        queue.write_buffer(&self.view, 0, &bytes);
    }

    /// Draws every batch into `pass`, each animated texture at its frame for `time` seconds
    /// (REND-2.1).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, time: f32) {
        pass.set_bind_group(0, &self.shared, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in &self.batches {
            let frame = frame(&self.animations, batch.texture, time);
            if let (Some(texture), Some(pipeline)) = (
                self.textures.get(frame),
                self.pipelines.get(pipeline_of(batch.surface)),
            ) {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, texture, &[]);
                pass.set_bind_group(2, &self.model_group, &[model_offset(batch.model)]);
                pass.draw_indexed(
                    batch.first..batch.first.saturating_add(batch.count),
                    0,
                    0..1,
                );
            }
        }
        let Some((buffer, _)) = &self.alias else {
            return;
        };
        pass.set_pipeline(&self.alias_pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        // Alias vertices are already where they are drawn: the world's matrix.
        pass.set_bind_group(2, &self.model_group, &[0]);
        for draw in &self.alias_draws {
            if let Some(skin) = self.skins.get(draw.skin) {
                pass.set_bind_group(1, skin, &[]);
                pass.draw(draw.first..draw.first.saturating_add(draw.count), 0..1);
            }
        }
    }
}

/// Each matrix at its [`MODEL_STRIDE`], column by column.
fn model_bytes(matrices: &[[[f32; 4]; 4]]) -> Vec<u8> {
    let stride = usize::try_from(MODEL_STRIDE).unwrap_or(256);
    let mut bytes = vec![0; matrices.len().saturating_mul(stride)];
    for (chunk, matrix) in bytes.chunks_mut(stride).zip(matrices) {
        let floats: Vec<u8> = matrix
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        for (b, f) in chunk.iter_mut().zip(floats) {
            *b = f;
        }
    }
    bytes
}

/// The dynamic offset of model `model`'s matrix.
fn model_offset(model: usize) -> u32 {
    u32::try_from(model)
        .ok()
        .and_then(|m| m.checked_mul(u32::try_from(MODEL_STRIDE).unwrap_or(256)))
        .unwrap_or(0)
}

/// `vertices` as the alias pipeline reads them: position, skin coordinates and shade.
fn alias_bytes(vertices: &[AliasVertex]) -> Vec<u8> {
    vertices
        .iter()
        .flat_map(|v| {
            let [x, y, z] = v.position;
            let [s, t] = v.texture;
            [x, y, z, s, t, v.shade]
        })
        .flat_map(f32::to_ne_bytes)
        .collect()
}

/// The size a buffer of `size` bytes, or none yet, grows to for `needed` bytes; `None` when it
/// holds them already. A buffer is never empty.
fn grown(size: Option<u64>, needed: u64) -> Option<u64> {
    let needed = needed.max(ALIAS_VERTEX_BYTES);
    size.is_none_or(|size| size < needed).then_some(needed)
}

/// REND-1.11: `glDepthFunc (GL_LEQUAL)`, so of two surfaces at one depth the later drawn,
/// an entity's over the world's, is seen.
const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::LessEqual;

/// REND-1.10: GLQuake's `glCullFace (GL_FRONT)` with `GL_CULL_FACE` on and GL's
/// counter-clockwise front face: only the side a face is seen from the front is drawn.
fn primitive() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: Some(wgpu::Face::Front),
        ..wgpu::PrimitiveState::default()
    }
}

/// REND-3.3: world textures are filtered `GL_LINEAR_MIPMAP_NEAREST` when smaller and
/// `GL_LINEAR` when larger, and repeat.
fn world_sampler() -> wgpu::SamplerDescriptor<'static> {
    wgpu::SamplerDescriptor {
        label: Some("qs-render world texture"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    }
}

/// A sampler filtering with `filter`, its coordinates addressed by `address`.
fn sampler(device: &Device, filter: wgpu::FilterMode, address: wgpu::AddressMode) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("qs-render"),
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: filter,
        min_filter: filter,
        ..wgpu::SamplerDescriptor::default()
    })
}

/// `image` as a texture's view, with no mipmaps.
fn upload(device: &Device, queue: &Queue, image: &Image) -> wgpu::TextureView {
    upload_levels(device, queue, core::slice::from_ref(image))
}

/// The mip levels `levels`, largest first, as one texture's view.
fn upload_levels(device: &Device, queue: &Queue, levels: &[Image]) -> wgpu::TextureView {
    let first = levels.first();
    let data: Vec<u8> = levels.iter().flat_map(|l| l.rgba.iter().copied()).collect();
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("qs-render"),
                size: wgpu::Extent3d {
                    width: first.map_or(1, |l| l.width),
                    height: first.map_or(1, |l| l.height),
                    depth_or_array_layers: 1,
                },
                mip_level_count: u32::try_from(levels.len()).unwrap_or(1).max(1),
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &data,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// What one frame of the map view is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The matrix from map coordinates to clip space.
    pub clip_from_world: [[f32; 4]; 4],
    /// The eye's position, which the sky is seen from (REND-2.3).
    pub eye: [f32; 3],
    /// Seconds since the map started, which moving textures follow (REND-2).
    pub time: f32,
}

/// One frame of the map view, set before egui's render pass and drawn inside it from the
/// [`MapRenderer`] in egui's callback resources.
pub struct MapCallback {
    /// What it is drawn with.
    pub view: View,
    /// The alias models it shows.
    pub models: ModelFrame,
    /// Each of the mesh's models' matrices, the world's first.
    pub placements: Vec<[[f32; 4]; 4]>,
}

impl CallbackTrait for MapCallback {
    fn prepare(
        &self,
        device: &Device,
        queue: &Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<MapRenderer>() {
            renderer.set_view(queue, &self.view);
            renderer.set_models(device, queue, &self.models);
            if !self.placements.is_empty() {
                renderer.set_placements(queue, &self.placements);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: epaint::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<MapRenderer>() {
            renderer.draw(pass, self.view.time);
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use egui_wgpu::wgpu;
    use egui_wgpu::wgpu::naga::front::wgsl;
    use egui_wgpu::wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};

    use super::{ALIAS_VERTEX_BYTES, DEPTH_COMPARE, alias_bytes, grown, primitive};

    // spec REND-1.11: a fragment at the depth already there passes, GL_LEQUAL
    #[test]
    fn depth_ties_go_to_the_later_surface() {
        assert_eq!(DEPTH_COMPARE, wgpu::CompareFunction::LessEqual);
    }

    // spec REND-1.10: only a face's front is drawn: GL's counter-clockwise front, front culled
    #[test]
    fn faces_are_culled_as_glquake_culls_them() {
        let state = primitive();
        assert_eq!(state.front_face, wgpu::FrontFace::Ccw);
        assert_eq!(state.cull_mode, Some(wgpu::Face::Front));
        assert_eq!(state.topology, wgpu::PrimitiveTopology::TriangleList);
    }
    use crate::alias::AliasVertex;

    #[test]
    fn alias_vertices_are_six_floats_in_order() {
        let vertex = AliasVertex {
            position: [1.0, 2.0, 3.0],
            texture: [4.0, 5.0],
            shade: 6.0,
        };
        let bytes = alias_bytes(&[vertex, vertex]);
        assert_eq!(u64::try_from(bytes.len()).unwrap(), 2 * ALIAS_VERTEX_BYTES);
        let floats: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .take(6)
            .collect();
        assert_eq!(floats, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn the_alias_buffer_grows_only_when_it_must() {
        assert_eq!(grown(None, 0), Some(ALIAS_VERTEX_BYTES));
        assert_eq!(grown(None, 100), Some(100));
        assert_eq!(grown(Some(100), 100), None);
        assert_eq!(grown(Some(100), 50), None);
        assert_eq!(grown(Some(100), 101), Some(101));
    }

    const SHADER: &str = include_str!("shader.wgsl");

    // spec REND-1.7: the surface is its texture's colour times the lightmap's light, in a
    // shader every backend accepts
    #[test]
    fn the_shader_validates_and_darkens_the_texture_by_the_light() {
        let module = wgsl::parse_str(SHADER).unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap();
        assert!(
            SHADER
                .contains("let light = textureSample(lightmap, lightmap_sampler, input.light).r;")
        );
        assert!(SHADER.contains("return vec4<f32>(colour.rgb * light, OPAQUE);"));
    }

    // spec REND-4.8: an alias model's skin is multiplied by its vertex's shade
    #[test]
    fn the_shader_modulates_a_skin_by_its_shade() {
        let module = wgsl::parse_str(SHADER).unwrap();
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap();
        assert!(SHADER.contains("return vec4<f32>(colour.rgb * input.shade, OPAQUE);"));
    }

    // spec REND-2.2: water ripples by 8 sin of `t*0.125 + realtime`, over 64
    // spec REND-2.3: the solid layer scrolls at 8 a second in the right half, the alpha layer
    // at 16 in the left half over it
    // spec REND-2.4: both are computed for each pixel
    #[test]
    fn the_shader_moves_water_and_sky() {
        let module = wgsl::parse_str(SHADER).unwrap();
        let entries: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(
            entries,
            [
                "vertex",
                "fragment",
                "turbulent",
                "sky",
                "alias_vertex",
                "alias_fragment"
            ]
        );
        for constant in [
            "const TURB_AMPLITUDE: f32 = 8.0;",
            "const TURB_FREQUENCY: f32 = 0.125;",
            "const TURB_TILE: f32 = 64.0;",
            "const SKY_FLATTEN: f32 = 3.0;",
            "const SKY_RADIUS: f32 = 378.0;",
            "const SKY_TILE: f32 = 128.0;",
            "const SKY_SOLID_SPEED: f32 = 8.0;",
            "const SKY_ALPHA_SPEED: f32 = 16.0;",
            "const SKY_SCROLL_WRAP: f32 = 128.0;",
        ] {
            assert!(SHADER.contains(constant), "{constant}");
        }
        assert!(
            SHADER.contains(
                "(st.x + TURB_AMPLITUDE * sin(st.y * TURB_FREQUENCY + time)) / TURB_TILE"
            )
        );
        assert!(SHADER.contains("mix(solid.rgb, alpha.rgb, alpha.a)"));
        assert!(SHADER.contains("sky_st(flat, time * SKY_SOLID_SPEED)"));
        assert!(SHADER.contains(
            "textureSample(sky_alpha, surface_sampler, sky_st(flat, time * SKY_ALPHA_SPEED))"
        ));
    }

    // spec REND-3.3: world textures are linear within the nearest mip level, repeating
    #[test]
    fn world_textures_filter_as_gl_linear_mipmap_nearest() {
        use egui_wgpu::wgpu::{AddressMode, FilterMode, MipmapFilterMode};
        let world = super::world_sampler();
        assert_eq!(
            (world.min_filter, world.mag_filter, world.mipmap_filter),
            (
                FilterMode::Linear,
                FilterMode::Linear,
                MipmapFilterMode::Nearest
            )
        );
        assert_eq!(
            (world.address_mode_u, world.address_mode_v),
            (AddressMode::Repeat, AddressMode::Repeat)
        );
    }

    // Every number in the shader's code is a named constant: none is written after the
    // constants but in a comment.
    #[test]
    fn the_shader_names_its_numbers() {
        let code = SHADER.split("struct View").nth(1).unwrap();
        for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
            assert!(
                !line.chars().any(|c| c.is_ascii_digit())
                    || line.contains("@location")
                    || line.contains("@binding")
                    || line.contains("@group")
                    || line.contains("vec2<f32>")
                    || line.contains("vec3<f32>")
                    || line.contains("vec4<f32>")
                    || line.contains("mat4x4<f32>"),
                "a number in: {line}"
            );
        }
    }
}
