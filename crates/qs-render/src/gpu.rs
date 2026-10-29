// SPDX-License-Identifier: GPL-3.0-or-later

//! A [`MapMesh`] on the GPU, drawn inside egui's render pass through a paint callback
//! (ADR-0003): the map view and the preview host share it.

use egui_wgpu::wgpu::util::{BufferInitDescriptor, DeviceExt, TextureDataOrder};
use egui_wgpu::wgpu::{self, BindGroup, Buffer, Device, Queue, RenderPipeline, TextureFormat};
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

use crate::mesh::{Batch, Image, MapMesh};

/// The bytes of a [`crate::mesh::Vertex`] on the GPU: position, texture and light coordinates.
const VERTEX_BYTES: u64 = 7 * 4;
/// The bytes of the view's uniform, one 4 by 4 matrix.
const VIEW_BYTES: u64 = 16 * 4;

/// A map's buffers, textures and pipeline, kept in egui's callback resources.
pub struct MapRenderer {
    pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
    view: Buffer,
    shared: BindGroup,
    textures: Vec<BindGroup>,
    batches: Vec<Batch>,
}

impl MapRenderer {
    /// Uploads `mesh` and builds the pipeline for a target of `format` with a depth buffer of
    /// `depth` (the format egui's render pass was made with).
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
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("qs-render world"),
            bind_group_layouts: &[Some(&shared_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("qs-render world"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
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
        let nearest = sampler(device, wgpu::FilterMode::Nearest, wgpu::AddressMode::Repeat);
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
        let textures = mesh
            .textures
            .iter()
            .map(|image| {
                let texture = upload(device, queue, image);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("qs-render surface texture"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&nearest),
                        },
                    ],
                })
            })
            .collect();
        Self {
            pipeline,
            vertices,
            indices,
            view,
            shared,
            textures,
            batches: mesh.batches.clone(),
        }
    }

    /// Sets the matrix from map coordinates to clip space for the next draw.
    pub fn set_view(&self, queue: &Queue, clip_from_world: &[[f32; 4]; 4]) {
        let bytes: Vec<u8> = clip_from_world
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        queue.write_buffer(&self.view, 0, &bytes);
    }

    /// Draws every batch into `pass`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.shared, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in &self.batches {
            if let Some(texture) = self.textures.get(batch.texture) {
                pass.set_bind_group(1, texture, &[]);
                pass.draw_indexed(
                    batch.first..batch.first.saturating_add(batch.count),
                    0,
                    0..1,
                );
            }
        }
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

/// `image` as a texture's view.
fn upload(device: &Device, queue: &Queue, image: &Image) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("qs-render"),
                size: wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &image.rgba,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// One frame of the map view: the matrix it is drawn with, set before egui's render pass and
/// drawn inside it from the [`MapRenderer`] in egui's callback resources.
pub struct MapCallback {
    /// The matrix from map coordinates to clip space.
    pub clip_from_world: [[f32; 4]; 4],
}

impl CallbackTrait for MapCallback {
    fn prepare(
        &self,
        _device: &Device,
        queue: &Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get::<MapRenderer>() {
            renderer.set_view(queue, &self.clip_from_world);
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
            renderer.draw(pass);
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use egui_wgpu::wgpu::naga;

    const SHADER: &str = include_str!("shader.wgsl");

    // spec REND-1.7: the surface is its texture's colour times the lightmap's light, in a
    // shader every backend accepts
    #[test]
    fn the_shader_validates_and_darkens_the_texture_by_the_light() {
        let module = naga::front::wgsl::parse_str(SHADER).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        assert!(SHADER.contains("textureSample(lightmap, lightmap_sampler, input.light).r"));
        assert!(SHADER.contains("return vec4<f32>(colour.rgb * light, 1.0);"));
    }
}
