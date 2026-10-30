// SPDX-License-Identifier: GPL-3.0-or-later

//! The map view drawn in the app's window through its paint callback, compared with a golden
//! image (P5-05's acceptance), by a test renderer that gives egui's render pass the depth
//! buffer the app's window has.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test code: a failed unwrap or a mismatched snapshot is a failed test; sizes are \
              small whole pixels"
)]

use std::path::{Path, PathBuf};

use eframe::egui::{self, TexturesDelta};
use eframe::egui_wgpu::{self, RenderState, ScreenDescriptor, wgpu};
use egui_kittest::{Harness, OsThreshold, SnapshotOptions, TestRenderer};
use qs_render::{Batch, Camera, Image, MeshBuilder, Palette, Placement, Surface, Vertex};

use super::DEPTH_FORMAT;
use crate::App;
use crate::shell::CentreTab;

/// egui-kittest's wgpu test renderer, with a depth buffer in its render pass as the app's
/// window has (`depth_buffer: 32`).
struct DepthRenderer {
    state: RenderState,
}

impl TestRenderer for DepthRenderer {
    fn handle_delta(&mut self, delta: &mut TexturesDelta) {
        let mut renderer = self.state.renderer.write();
        // A hash map's order does not matter: each texture is set or freed on its own.
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each texture is handled on its own"
        )]
        for (id, images) in delta.set.drain() {
            for image in images {
                renderer.update_texture(&self.state.device, &self.state.queue, id, &image);
            }
        }
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each texture is handled on its own"
        )]
        for id in delta.free.drain() {
            renderer.free_texture(&id);
        }
    }

    fn render(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
    ) -> Result<image::RgbaImage, String> {
        let state = &self.state;
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        let [width, height] = [size.x.round(), size.y.round()].map(|v| v as u32);
        let screen = ScreenDescriptor {
            pixels_per_point: ctx.pixels_per_point(),
            size_in_pixels: [width, height],
        };
        let tessellated = ctx.tessellate(output.shapes.clone(), ctx.pixels_per_point());
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let mut renderer = state.renderer.write();
        let user = renderer.update_buffers(
            &state.device,
            &state.queue,
            &mut encoder,
            &tessellated,
            &screen,
        );
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let target = state.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = state.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &tessellated, &screen);
        }
        drop(renderer);
        state
            .queue
            .submit(user.into_iter().chain([encoder.finish()]));
        read_back(state, &target, width, height)
    }
}

/// `texture`'s pixels, read back from the GPU.
fn read_back(
    state: &RenderState,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<image::RgbaImage, String> {
    let unpadded = width * 4;
    let padded =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = state.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    state.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    state
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    let data = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| e.to_string())?;
    let pixels: Vec<u8> = data
        .chunks(padded as usize)
        .flat_map(|row| row.iter().take(unpadded as usize).copied())
        .collect();
    image::RgbaImage::from_raw(width, height, pixels).ok_or_else(|| "no image".to_owned())
}

/// A floor of squares in a checker of two colours, 256 units across at z = 0, full
/// bright: the corner texel of the lightmap is always 255 (REND-1.6).
fn checker_floor() -> qs_render::MapMesh {
    let palette = Palette::read(&[0; 768]).unwrap();
    let mut mesh = MeshBuilder::new(&palette).finish();
    let corner = |x: f32, y: f32, s: f32, t: f32| Vertex {
        position: [x, y, 0.0],
        texture: [s, t],
        light: [0.0, 0.0],
    };
    mesh.vertices = vec![
        corner(-128.0, -128.0, 0.0, 0.0),
        corner(128.0, -128.0, 2.0, 0.0),
        corner(128.0, 128.0, 2.0, 2.0),
        corner(-128.0, 128.0, 0.0, 2.0),
    ];
    // Quake winds a face clockwise seen from its front (REND-1.10).
    mesh.indices = vec![0, 2, 1, 0, 3, 2];
    mesh.batches = vec![Batch {
        texture: 0,
        surface: Surface::Lit,
        first: 0,
        count: 6,
        model: 0,
    }];
    let ember = [238, 139, 69, 255];
    let stone = [70, 64, 60, 255];
    // 16 by 16 texels in blocks of 8, so filtering touches only the blocks' edges.
    let rgba: Vec<u8> = (0..16_u32)
        .flat_map(|t| {
            (0..16_u32).map(move |s| {
                if (s / 8 + t / 8) % 2 == 0 {
                    ember
                } else {
                    stone
                }
            })
        })
        .flatten()
        .collect();
    mesh.textures = vec![Image {
        width: 16,
        height: 16,
        rgba,
    }];
    mesh.skies = vec![None];
    mesh.animations = vec![None];
    mesh.models = vec![Placement::WORLD];
    mesh.mins = [-128.0, -128.0, 0.0];
    mesh.maxs = [128.0, 128.0, 0.0];
    mesh
}

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots")
}

// MAP-17: the map view draws the map in the window, through its paint callback on the window's
// device, as the golden image shows
#[test]
fn the_map_view_draws_in_the_window() {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.0)
        .renderer(DepthRenderer {
            state: state.clone(),
        })
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::new(&harness.ctx, None).unwrap();
    let mesh = checker_floor();
    let mut camera = Camera::new([-300.0, 0.0, 200.0], 0.0);
    camera.turn(0.0, 30.0);
    assert!(
        !app.shell_mut().show_map(&mesh, camera),
        "no device until the window gives one"
    );
    app.shell_mut().set_render_state(Some(state));
    assert!(app.shell_mut().show_map(&mesh, camera));
    assert_eq!(app.shell().centre_tab(), CentreTab::Map);
    *harness.state_mut() = Some(app);
    harness.run();
    let options = SnapshotOptions::new()
        .output_path(goldens())
        .max_failed_pixels(OsThreshold::new(0).linux(400).windows(400));
    if let Err(error) = harness.try_snapshot_options("map-view", &options) {
        panic!("{error}");
    }
    let shown = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_view()
        .unwrap();
    assert_eq!(shown.camera().origin, [-300.0, 0.0, 200.0]);
}
