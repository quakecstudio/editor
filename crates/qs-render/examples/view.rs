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
//! `-show water` or `-show sky` starts at the map's first water or sky. `-benchmark <seconds>`
//! presents without vsync, turns a free camera slowly and, after that many seconds, prints the
//! average frame rate, the slowest frame and the time the frames' work took on the CPU, then
//! exits (P2-152); `-position <x> <y>` opens the window at that point, on a chosen display.

use std::io::Write as _;
use std::time::Instant;

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use qs_render::{Camera, MapCallback, MapMesh, MapRenderer, View};

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
    // `-benchmark <seconds>`: no vsync, a free camera turning slowly, and after that many
    // seconds the average frame rate on stdout and exit (P2-152).
    let benchmark = take_numbers(&mut args, "-benchmark", 1)?.and_then(|v| match v.as_slice() {
        [seconds] => Some(*seconds),
        _ => None,
    });
    if benchmark.is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0) {
        return Err("-benchmark needs a positive number of seconds".to_owned());
    }
    let fly = fly || benchmark.is_some();
    // `-position <x> <y>`: where the window opens, to put it on a chosen display.
    let position = take_numbers(&mut args, "-position", 2)?.and_then(|v| match v.as_slice() {
        [x, y] => Some([*x, *y]),
        _ => None,
    });
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("qs-render: {map}"))
        .with_inner_size([1280.0, 800.0]);
    if let Some(at) = position {
        viewport = viewport.with_position(at);
    }
    let mut wgpu_options = egui_wgpu::WgpuConfiguration::default();
    if benchmark.is_some() {
        wgpu_options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        wgpu_options,
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
                mesh,
                camera,
                triangles,
                models,
                sim,
                fly,
                behind: 0.0,
                started: Instant::now(),
                last: Instant::now(),
                benchmark,
                frames: 0,
                slowest: 0.0,
                worked: 0.0,
                longest_work: 0.0,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

/// Takes the flag `name` and the `count` numbers after it out of `args`, when it is there.
fn take_numbers(
    args: &mut Vec<String>,
    name: &str,
    count: usize,
) -> Result<Option<Vec<f32>>, String> {
    let Some(i) = args.iter().position(|a| a == name) else {
        return Ok(None);
    };
    let end = i.saturating_add(count);
    let values: Vec<f32> = args
        .get(i.saturating_add(1)..=end)
        .ok_or_else(|| format!("{name} needs {count} number(s)"))?
        .iter()
        .map(|s| s.parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{name}: {e}"))?;
    args.drain(i..=end);
    Ok(Some(values))
}

struct Viewer {
    map: String,
    mesh: MapMesh,
    camera: Camera,
    triangles: usize,
    models: common::Models,
    sim: common::Sim,
    fly: bool,
    behind: f64,
    started: Instant,
    last: Instant,
    /// The seconds a benchmark runs, when it is one.
    benchmark: Option<f32>,
    /// The frames drawn so far.
    frames: u32,
    /// The longest frame so far, in seconds.
    slowest: f32,
    /// The seconds the frames' work on the CPU took so far: the simulation, the relighting
    /// and the paint callback's recording, without the wait for the display.
    worked: f32,
    /// The longest such work, in seconds.
    longest_work: f32,
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let frame = now.duration_since(self.last).as_secs_f32();
        let seconds = frame.min(0.1);
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
        if self.benchmark.is_some() {
            // A slow turn, so the whole view around the start is drawn. The first frame
            // starts the clock: the window's own appearance is not the renderer's.
            self.camera.turn(seconds * 30.0, 0.0);
            if self.frames == 0 {
                self.started = now;
            } else {
                self.slowest = self.slowest.max(frame);
            }
            self.frames = self.frames.saturating_add(1);
        }
        let time = now.duration_since(self.started);

        self.models.update(&self.sim);
        let lightmap = self
            .mesh
            .relight(&self.sim.light_styles())
            .then(|| self.mesh.lightmap.clone());
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
                lightmap,
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
        if let Some(limit) = self.benchmark {
            let work = now.elapsed().as_secs_f32();
            self.worked += work;
            self.longest_work = self.longest_work.max(work);
            let elapsed = time.as_secs_f32();
            if elapsed >= limit && self.frames > 1 {
                let frames = f64::from(self.frames.saturating_sub(1));
                let mut out = std::io::stdout().lock();
                writeln!(
                    out,
                    "benchmark: {} {:.0} fps average over {} frames in {elapsed:.1} s, slowest \
                     frame {:.1} ms, work per frame {:.2} ms average and {:.2} ms longest",
                    self.map,
                    frames / f64::from(elapsed),
                    self.frames.saturating_sub(1),
                    self.slowest * 1000.0,
                    f64::from(self.worked) / frames * 1000.0,
                    self.longest_work * 1000.0
                )
                .ok();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                self.benchmark = None;
            }
        }
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
