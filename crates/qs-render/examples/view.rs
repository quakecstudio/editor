// SPDX-License-Identifier: GPL-3.0-or-later

//! A map viewer: reads a map from a Quake install through the game filesystem and draws its
//! world with `qs-render` inside an egui window (P5-05). It is a development tool, not the app.
//!
//! ```text
//! cargo run --release -p qs-render --example view -- -basedir <game root> [-game <dir>] [map]
//! ```
//!
//! The map defaults to `e1m1`. You play it: W, A, S and D move, Space jumps, Q and E swim
//! down and up, Shift runs, and dragging with the mouse looks around. `-fly` gives a free
//! camera instead, Space and C rising and sinking. `-gamma <g>` is GLQuake's, 0.7 by default, and
//! `-show water` or `-show sky` starts at the map's first water or sky.

use std::io::Write as _;
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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `-fly`: a free camera instead of the player.
    let fly = args.iter().any(|a| a == "-fly");
    args.retain(|a| a != "-fly");
    let common::Loaded {
        map,
        mesh,
        models,
        camera,
        mut sim,
    } = common::load(&args)?;
    if !fly {
        sim.connect()?;
    }
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
                &models.skins,
            );
            state.renderer.write().callback_resources.insert(renderer);
            Ok(Box::new(Viewer {
                map,
                camera,
                triangles,
                models,
                sim,
                fly,
                behind: 0.0,
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
    models: common::Models,
    sim: common::Sim,
    fly: bool,
    behind: f64,
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
        if !self.fly {
            self.play(ui, f64::from(seconds));
        } else {
            self.fly_around(ui, seconds);
            if let Err(error) = self.sim.advance(f64::from(seconds), &mut self.behind) {
                report_server_error(&error);
            }
        }
        let aspect = rect.width() / rect.height().max(1.0);
        let time = now.duration_since(self.started);

        self.models.update(&self.sim);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            MapCallback {
                view: View {
                    clip_from_world: self.camera.clip_from_world(aspect),
                    eye: self.camera.origin,
                    time: time.as_secs_f32(),
                },
                models: self.models.frame(self.sim.time()),
                placements: self.models.placements(),
            },
        ));
        // egui's fonts are not bundled (their licenses are outside deny.toml's), so the stats
        // go in the title bar.
        let [x, y, z] = self.camera.origin;
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "qs-render: {}  {} triangles  {} models  ({x:.0} {y:.0} {z:.0})  {:.0} fps",
                self.map,
                self.triangles,
                self.models.len(),
                1.0 / seconds.max(1e-3)
            )));
        ui.ctx().request_repaint();
    }
}

impl Viewer {
    /// The player: the keys and the view as `CL_BaseMove` makes the command, the server's
    /// frame, then the eye where the player stands.
    fn play(&mut self, ui: &egui::Ui, seconds: f64) {
        // CL_AdjustAngles keeps the pitch within -70 and 80.
        self.camera.pitch = self.camera.pitch.clamp(-70.0, 80.0);
        let cmd = ui.input(|input| {
            let key = |k: egui::Key| f32::from(u8::from(input.key_down(k)));
            // cl_movespeedkey
            let speed = if input.modifiers.shift { 2.0 } else { 1.0 };
            qs_sim::UserCmd {
                angles: [self.camera.pitch, self.camera.yaw, 0.0],
                // cl_forwardspeed, cl_backspeed, cl_sidespeed, cl_upspeed
                forward: (200.0 * key(egui::Key::W) - 200.0 * key(egui::Key::S)) * speed,
                side: (350.0 * key(egui::Key::D) - 350.0 * key(egui::Key::A)) * speed,
                up: (200.0 * key(egui::Key::E) - 200.0 * key(egui::Key::Q)) * speed,
                buttons: u8::from(input.key_down(egui::Key::Space)) << 1,
                impulse: 0,
            }
        });
        self.sim.command(cmd);
        if let Err(error) = self.sim.advance(seconds, &mut self.behind) {
            report_server_error(&error);
        }
        if let Some([pitch, yaw, _]) = self.sim.set_angle() {
            self.camera.pitch = pitch;
            self.camera.yaw = yaw;
        }
        if let Some(eye) = self.sim.eye() {
            self.camera.origin = eye;
        }
    }

    /// The free camera of `-fly`.
    fn fly_around(&mut self, ui: &egui::Ui, seconds: f32) {
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
    }
}

/// A server error stops the frame but not the viewer: it is printed.
fn report_server_error(error: &str) {
    let mut err = std::io::stderr().lock();
    writeln!(err, "server: {error}").ok();
}
