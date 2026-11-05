// SPDX-License-Identifier: GPL-3.0-or-later

//! The game's window (ADR-0004): the player's keys and mouse, the host loop at
//! `Host_FilterTime`'s pace, the client reading the server's messages, `qs-render` drawing the
//! scene, and the bridge answered between two server frames (BRIDGE-4.3).
//!
//! W, A, S and D move, Space jumps, Q and E swim down and up, Shift runs, the mouse button
//! fires, the number keys pick a weapon and the mouse looks around once a click in the window
//! has captured it; Escape frees it. `--fly` gives a free camera instead, Space and C rising
//! and sinking.

use std::io::Write as _;
use std::time::Instant;

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use qs_bsp::Bsp;
use qs_client::Client;
use qs_render::{Camera, MapMesh, MapRenderer, Play, View};

use crate::args::Args;
use crate::audio::Audio;
use crate::bridge::Bridge;
use crate::game::{Eye, Game};
use crate::load::{Describer, Loaded, Rand};
use crate::screenshot::Frame;

/// Map units a second walking, and running with Shift.
const SPEEDS: [f32; 2] = [320.0, 960.0];
/// Degrees turned per unit the mouse moves: `sensitivity` 3 times `m_yaw` and `m_pitch`
/// 0.022, id's defaults (`WinQuake/cl_main.c`), as `IN_MouseMove` turns the view
/// (`WinQuake/in_win.c`).
const TURN: f32 = 3.0 * 0.022;

/// Whether the window holds the mouse, as an engine does in play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pointer {
    /// The pointer is the system's; a click in the window captures it.
    Free,
    /// The pointer is hidden and locked: moving the mouse turns the view, and its button
    /// fires. Escape, or the window losing focus, frees it.
    Captured,
}

/// `Host_FilterTime`: no frame comes sooner than 1/72 of a second after the last.
const FRAME: f64 = 1.0 / 72.0;
/// `Host_FilterTime`'s longest frame.
const LONGEST: f64 = 0.1;

/// Opens the window on the map `loaded` holds and runs the game until it closes or the
/// bridge's client stops it.
pub(crate) fn run(args: &Args, loaded: Loaded, bridge: Option<Bridge>) -> Result<(), String> {
    let benchmark = args.benchmark;
    if benchmark.is_some_and(|seconds| !seconds.is_finite() || seconds <= 0.0) {
        return Err("--benchmark needs a positive number of seconds".to_owned());
    }
    let fly = args.fly || benchmark.is_some();
    let position = args
        .position
        .as_deref()
        .and_then(|values| <[f32; 2]>::try_from(values).ok());
    let Loaded {
        map,
        mesh,
        play,
        camera,
        game,
        audio,
        client,
        bsp,
        describer,
        rand,
    } = loaded;
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("QuakeC Studio preview: {map}"))
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
        "preview-host",
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
                &play.assets(),
            );
            state.renderer.write().callback_resources.insert(renderer);
            Ok(Box::new(Viewer {
                map,
                mesh,
                camera,
                triangles,
                play,
                game,
                audio,
                bridge,
                client,
                bsp,
                describer,
                rand,
                fly,
                pointer: Pointer::Free,
                last_set: [f32::NAN; 2],
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

struct Viewer {
    map: String,
    mesh: MapMesh,
    camera: Camera,
    triangles: usize,
    play: Play,
    game: Game,
    audio: Audio,
    /// The IDE's connection, when the host was started with `-bridge`.
    bridge: Option<Bridge>,
    client: Client,
    bsp: Bsp<'static>,
    describer: Describer,
    rand: Rand,
    fly: bool,
    /// Whether the window holds the mouse.
    pointer: Pointer,
    /// The view angles last given to the client, to tell a `svc_setangle` from them.
    last_set: [f32; 2],
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
        let response = ui.allocate_rect(rect, egui::Sense::click());
        self.hold_pointer(ui, response.clicked());
        self.take_screenshot(ui);
        // The bridge's commands run between two server frames (BRIDGE-4.3).
        self.serve_bridge(ui);
        if self.fly {
            self.fly_around(ui, seconds);
        } else {
            self.steer(ui);
        }
        let frametime = self.advance(f64::from(seconds));
        self.serve_bridge(ui);
        // The client reads what the server wrote and advances its clock by the server's
        // frame, as the host does when both run in one process.
        let packets = self.game.world_mut().take_messages().packets;
        match self.client.frame(
            frametime_f32(frametime),
            &packets,
            &mut self.describer,
            &mut self.rand,
        ) {
            Ok(events) => self.audio.handle(&events, &mut self.rand),
            Err(error) => tracing::warn!(%error, "client"),
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

        let scene = self.client.scene();
        let (origin, angles) = if self.fly {
            (
                self.camera.origin,
                [self.camera.pitch, self.camera.yaw, 0.0],
            )
        } else {
            (scene.view.origin, scene.view.angles)
        };
        self.game.set_eye(Eye {
            origin,
            angles,
            fov_x: self.camera.fov_x,
            aspect,
        });
        // `S_Update (r_origin, vpn, vright, vup)`, once a host frame.
        self.audio.update(
            &self.bsp,
            scene.view.origin,
            scene.view.angles,
            f64::from(seconds),
        );
        let hud = self.client.hud();
        // The 2D layer keeps GLQuake's 640 wide console, as tall as the window's shape.
        self.play.screen = [CONWIDTH, screen_height(aspect)];
        self.client
            .set_screen(self.play.screen[0], self.play.screen[1]);
        let mut callback = self
            .play
            .frame(&scene, &hud, &self.bsp, &mut self.mesh, aspect);
        if self.fly {
            let mut view = View::new(
                self.camera.clip_from_world(aspect),
                self.camera.origin,
                time.as_secs_f32(),
            );
            view.fog = callback.view.fog;
            view.screen = callback.view.screen;
            callback.view = view;
            callback.weapon = Default::default();
        }
        let entities = callback
            .models
            .draws
            .len()
            .saturating_add(callback.sprites.draws.len());
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        // egui's fonts are not bundled (their licenses are outside deny.toml's), so the stats
        // go in the title bar.
        let [x, y, z] = self.camera.origin;
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "QuakeC Studio preview: {}  {} triangles  {entities} entities  ({x:.0} {y:.0} {z:.0})  {:.0} fps",
                self.map,
                self.triangles,
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
    /// `Host_FilterTime`: when 1/72 of a second has passed since the last frame, one server
    /// frame of the time that passed, at most 0.1; the frame's length, 0 when none ran.
    fn advance(&mut self, seconds: f64) -> f64 {
        self.behind += seconds;
        if self.behind < FRAME {
            return 0.0;
        }
        let frametime = self.behind.min(LONGEST);
        self.behind = 0.0;
        if self.game.frame(frametime) {
            frametime
        } else {
            0.0
        }
    }

    /// Answers the bridge's messages and sends the game's events; closes the window when the
    /// client asked the host to stop or went away (BRIDGE-1.1, BRIDGE-2.5).
    fn serve_bridge(&mut self, ui: &egui::Ui) {
        let Some(bridge) = &mut self.bridge else {
            // No one hears the events of a game played on its own.
            drop(self.game.take_events());
            return;
        };
        let open = bridge.poll(&mut self.game);
        for (id, outcome) in self.game.take_responses() {
            bridge.respond(id, outcome);
        }
        // The events wait for `initialize`, so the client hears the progs and the map load
        // however late it initializes (BRIDGE-2.2).
        if bridge.is_initialized() {
            for (method, params) in self.game.take_events() {
                bridge.event(method, params);
            }
        }
        if self.game.wants_screenshot() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if !open || self.game.stopped() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Answers the waiting `screenshot`s with the frame drawn last, once a frame, before
    /// any new request is read.
    fn take_screenshot(&mut self, ui: &egui::Ui) {
        let shot = ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Frame {
                    width: u32::try_from(image.size[0]).unwrap_or(0),
                    height: u32::try_from(image.size[1]).unwrap_or(0),
                    rgba: image.as_raw().to_vec(),
                }),
                _ => None,
            })
        });
        if let Some(frame) = shot {
            self.game.screenshot_taken(&frame);
        }
    }

    /// Captures the mouse on a click and frees it on Escape or when the window loses focus;
    /// while it is held, its motion turns the view (`IN_MouseMove`).
    fn hold_pointer(&mut self, ui: &egui::Ui, clicked: bool) {
        let (escape, focused, motion) = ui.input(|input| {
            let mut motion = [0.0_f32; 2];
            for event in &input.events {
                if let egui::Event::MouseMoved(delta) = event {
                    motion[0] += delta.x;
                    motion[1] += delta.y;
                }
            }
            (input.key_pressed(egui::Key::Escape), input.focused, motion)
        });
        let next = match self.pointer {
            Pointer::Free if clicked => Pointer::Captured,
            Pointer::Captured if escape || !focused => Pointer::Free,
            same => same,
        };
        if next != self.pointer {
            let (grab, visible) = match next {
                Pointer::Captured => (egui::CursorGrab::Locked, false),
                Pointer::Free => (egui::CursorGrab::None, true),
            };
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorGrab(grab));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(visible));
            self.pointer = next;
        } else if self.pointer == Pointer::Captured {
            // `cl.viewangles[YAW] -= m_yaw.value * mouse_x`,
            // `cl.viewangles[PITCH] += m_pitch.value * mouse_y`.
            let [x, y] = motion;
            self.camera.turn(-x * TURN, y * TURN);
        }
    }

    /// The player: the keys and the view as `CL_BaseMove` makes the command for the next
    /// server frame; the eye is the client's.
    fn steer(&mut self, ui: &egui::Ui) {
        // The server's `svc_setangle` turned the client's view: follow it.
        let [pitch, yaw, _] = self.client.viewangles();
        if pitch.to_bits() != self.last_set[0].to_bits()
            || yaw.to_bits() != self.last_set[1].to_bits()
        {
            self.camera.pitch = pitch;
            self.camera.yaw = yaw;
        }
        // CL_AdjustAngles keeps the pitch within -70 and 80.
        self.camera.pitch = self.camera.pitch.clamp(-70.0, 80.0);
        self.client
            .set_viewangles([self.camera.pitch, self.camera.yaw, 0.0]);
        self.last_set = [self.camera.pitch, self.camera.yaw];
        let cmd = ui.input(|input| {
            let key = |k: egui::Key| f32::from(u8::from(input.key_down(k)));
            // cl_movespeedkey
            let speed = if input.modifiers.shift { 2.0 } else { 1.0 };
            let held = self.pointer == Pointer::Captured;
            let attack = u8::from(held && input.pointer.primary_down());
            let impulse = (1..=8_u8)
                .find(|n| {
                    let key = match n {
                        1 => egui::Key::Num1,
                        2 => egui::Key::Num2,
                        3 => egui::Key::Num3,
                        4 => egui::Key::Num4,
                        5 => egui::Key::Num5,
                        6 => egui::Key::Num6,
                        7 => egui::Key::Num7,
                        _ => egui::Key::Num8,
                    };
                    input.key_pressed(key)
                })
                .unwrap_or(0);
            qs_sim::UserCmd {
                angles: [self.camera.pitch, self.camera.yaw, 0.0],
                // cl_forwardspeed, cl_backspeed, cl_sidespeed, cl_upspeed
                forward: (200.0 * key(egui::Key::W) - 200.0 * key(egui::Key::S)) * speed,
                side: (350.0 * key(egui::Key::D) - 350.0 * key(egui::Key::A)) * speed,
                up: (200.0 * key(egui::Key::E) - 200.0 * key(egui::Key::Q)) * speed,
                buttons: attack | (u8::from(input.key_down(egui::Key::Space)) << 1),
                impulse,
            }
        });
        self.game.world_mut().set_command(cmd);
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

/// GLQuake's console width, `vid.conwidth` (HUD-1).
const CONWIDTH: i32 = 640;

/// The 2D layer's height for a window `aspect` wide per unit of height.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a rounded height of a few hundred units"
)]
fn screen_height(aspect: f32) -> i32 {
    (f32::from(u16::try_from(CONWIDTH).unwrap_or(640)) / aspect.max(0.1)).round() as i32
}

/// The server's frame as the client's `host_frametime`, in seconds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a frame of at most 0.1 seconds, exact enough in f32"
)]
fn frametime_f32(frametime: f64) -> f32 {
    frametime as f32
}
