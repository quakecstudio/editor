// SPDX-License-Identifier: GPL-3.0-or-later

//! A map viewer: reads a map from a Quake install through the game filesystem and draws its
//! world with `qs-render` inside an egui window (P5-05). It is a development tool, not the app.
//!
//! ```text
//! cargo run --release -p qs-render --example view -- -basedir <game root> [-game <dir>] [map]
//! ```
//!
//! The map defaults to `e1m1`. W, A, S and D move, Space and C rise and sink, Shift is fast,
//! and dragging with the mouse looks around. `-gamma <g>` is GLQuake's, 0.7 by default, and
//! `-show water` or `-show sky` starts at the map's first water or sky.

use std::time::Instant;

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use qs_render::{Camera, MapCallback, MapRenderer, View};

mod common;

/// Map units a second walking, and running with Shift.
const SPEEDS: [f32; 2] = [320.0, 960.0];
/// Degrees turned per point the mouse drags.
const TURN: f32 = 0.25;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let common::Loaded { map, mesh, camera } = common::load(&args)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("qs-render: {map}"))
            .with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        ..eframe::NativeOptions::default()
    };
    let triangles = mesh.indices.len().checked_div(3).unwrap_or(0);
    eframe::run_native(
        "qs-render view",
        options,
        Box::new(move |cc| {
            let state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("eframe started without wgpu")?;
            let renderer = MapRenderer::new(
                &state.device,
                &state.queue,
                (state.target_format, wgpu::TextureFormat::Depth32Float),
                &mesh,
            );
            state.renderer.write().callback_resources.insert(renderer);
            Ok(Box::new(Viewer {
                map,
                camera,
                triangles,
                started: Instant::now(),
                last: Instant::now(),
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

struct Viewer {
    map: String,
    camera: Camera,
    triangles: usize,
    started: Instant,
    last: Instant,
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let seconds = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::drag());
        let drag = response.drag_delta();
        self.camera.turn(-drag.x * TURN, drag.y * TURN);
        ui.input(|input| {
            let speed = SPEEDS
                .get(usize::from(input.modifiers.shift))
                .copied()
                .unwrap_or(0.0)
                * seconds;
            let axis = |plus: egui::Key, minus: egui::Key| {
                f32::from(u8::from(input.key_down(plus)))
                    - f32::from(u8::from(input.key_down(minus)))
            };
            self.camera.travel(
                axis(egui::Key::W, egui::Key::S) * speed,
                axis(egui::Key::D, egui::Key::A) * speed,
                axis(egui::Key::Space, egui::Key::C) * speed,
            );
        });
        let aspect = rect.width() / rect.height().max(1.0);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            MapCallback {
                view: View {
                    clip_from_world: self.camera.clip_from_world(aspect),
                    eye: self.camera.origin,
                    time: now.duration_since(self.started).as_secs_f32(),
                },
            },
        ));
        // egui's fonts are not bundled (their licenses are outside deny.toml's), so the stats
        // go in the title bar.
        let [x, y, z] = self.camera.origin;
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "qs-render: {}  {} triangles  ({x:.0} {y:.0} {z:.0})  {:.0} fps",
                self.map,
                self.triangles,
                1.0 / seconds.max(1e-3)
            )));
        ui.ctx().request_repaint();
    }
}
