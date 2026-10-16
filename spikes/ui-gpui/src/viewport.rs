// SPDX-License-Identifier: GPL-3.0-or-later
//! The 3D viewport for gpui: wgpu renders offscreen on its own device and the pixels are read
//! back into a gpui `RenderImage`.
//!
//! gpui draws with Metal on macOS, DirectX 11 on Windows and its own wgpu device on Linux and
//! the web, and exposes none of them, so there is no way to share a texture without `unsafe`
//! platform interop (on macOS, an IOSurface shown through gpui's `surface()` element). The
//! readback is the safe path, and its cost is part of what the spike measures. It is pipelined
//! over two buffers so the UI thread never waits for the GPU: each frame shows the pixels of
//! the frame before, one frame of latency.

use std::sync::Arc;
use std::sync::mpsc;

use gpui::RenderImage;
use ui_common::{scene, theme};

pub struct Viewport {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    target: wgpu::Texture,
    readback: [wgpu::Buffer; 2],
    in_flight: [Option<mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>; 2],
    frame: usize,
    width: u32,
    height: u32,
    padded_row: u32,
    count: u32,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

impl Viewport {
    /// Creates the device and resources for a `width` x `height` pixel target.
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| format!("no wgpu adapter: {e}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("no wgpu device: {e}"))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cube"),
            source: wgpu::ShaderSource::Wgsl(scene::SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cube"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..wgpu::PrimitiveState::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let cube = scene::cube();
        let vertices = wgpu::util::DeviceExt::create_buffer_init(
            &device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("cube"),
                contents: &scene::vertex_bytes(&cube),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cube"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = [0, 1].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport readback"),
                size: u64::from(padded_row) * u64::from(height),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            vertices,
            uniforms,
            bind_group,
            target,
            readback,
            in_flight: [None, None],
            frame: 0,
            width,
            height,
            padded_row,
            count: u32::try_from(cube.len()).unwrap_or(0),
        })
    }

    /// Renders the scene at time `t` into this frame's buffer and returns the previous frame's
    /// pixels, waiting only if the GPU has not finished them yet.
    pub fn render(&mut self, t: f32) -> Option<Arc<RenderImage>> {
        let slot = self.frame % 2;
        let previous = 1 - slot;
        self.frame = self.frame.wrapping_add(1);
        #[expect(clippy::cast_precision_loss, reason = "small sizes")]
        let aspect = self.width as f32 / self.height.max(1) as f32;
        self.queue
            .write_buffer(&self.uniforms, 0, &scene::mat_bytes(&scene::mvp(t, aspect)));
        let view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport"),
            });
        {
            let [r, g, b] = theme::rgb_f32(theme::SURFACE_EDITOR);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.count, 0..1);
        }
        let buffer = self.readback.get(slot)?;
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = tx.send(result);
        });
        if let Some(pending) = self.in_flight.get_mut(slot) {
            *pending = Some(rx);
        }
        self.device.poll(wgpu::PollType::Poll).ok()?;
        let rx = self.in_flight.get_mut(previous)?.take()?;
        let ready = match rx.try_recv() {
            Ok(result) => result,
            Err(_) => {
                self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
                rx.recv().ok()?
            }
        };
        ready.ok()?;
        let buffer = self.readback.get(previous)?;
        let row = usize::try_from(self.width * 4).ok()?;
        let padded = usize::try_from(self.padded_row).ok()?;
        let mut pixels = Vec::with_capacity(row * usize::try_from(self.height).ok()?);
        {
            let mapped = buffer.get_mapped_range(..);
            for chunk in mapped.chunks(padded) {
                pixels.extend_from_slice(chunk.get(..row)?);
            }
        }
        buffer.unmap();
        let image = image::RgbaImage::from_raw(self.width, self.height, pixels)?;
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(image)])))
    }
}
