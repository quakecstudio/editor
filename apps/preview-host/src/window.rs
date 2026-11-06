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
use qs_client::{
    Client, Console, ConsoleKey, Event, KeyDest, Menu, MenuEvent, MenuKey, Random as _, Typed,
};
use qs_render::{Camera, MapMesh, MapRenderer, Offscreen, Play, View};
use raw_window_handle::{HasDisplayHandle as _, RawDisplayHandle};

use crate::args::Args;
use crate::audio::Audio;
use crate::bridge::{Bridge, serve};
use crate::contents::contents_at;
use crate::game::{Eye, Game, Placement, Screen};
use crate::load::{Acted, Describer, Loaded, Rand, act, load};
use crate::screenshot::Drawer;

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
pub(crate) const FRAME: f64 = 1.0 / 72.0;
/// `Host_FilterTime`'s longest frame.
const LONGEST: f64 = 0.1;

/// Opens the window on the map `loaded` holds and runs the game until it closes or the
/// bridge's client stops it.
pub(crate) fn run(args: &Args, loaded: Loaded, bridge: Option<Bridge>) -> Result<(), String> {
    let args = args.clone();
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
        printed,
    } = loaded;
    // The console's first lines: the host's, as `Host_Init` prints the engine's, then what the
    // map's first frame printed (CLIENT-1.7).
    let mut console = Console::new();
    let banner = format!("QuakeC Studio preview host {}\n", env!("CARGO_PKG_VERSION"));
    console.print(banner.as_bytes(), 0.0);
    for text in &printed {
        console.print(text, 0.0);
    }
    let size = args
        .size
        .as_deref()
        .and_then(|values| <[f32; 2]>::try_from(values).ok())
        .unwrap_or([1280.0, 800.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("QuakeC Studio preview: {map}"))
        .with_inner_size(size);
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
    let camera_origin = camera.origin;
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
            let mut game = game;
            // BRIDGE-4.10: Wayland lets a client neither move nor raise its window.
            let wayland = cc
                .display_handle()
                .is_ok_and(|handle| matches!(handle.as_raw(), RawDisplayHandle::Wayland(_)));
            if wayland {
                game.set_screen(Screen::FixedWindow);
            }
            // Screenshots are drawn offscreen on the window's own device (BRIDGE-4.9).
            let drawer = Some(Drawer::new(Offscreen::from_state(
                state.clone(),
                wgpu::TextureFormat::Depth32Float,
            )));
            Ok(Box::new(Viewer {
                drawer,
                args,
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
                placed: None,
                eye: camera_origin,
                last_set: [f32::NAN; 2],
                behind: 0.0,
                started: Instant::now(),
                last: Instant::now(),
                benchmark,
                frames: 0,
                slowest: 0.0,
                worked: 0.0,
                longest_work: 0.0,
                console,
                dest: KeyDest::Game,
                menu: Menu::new(),
                asked: Vec::new(),
            }))
        }),
    )
    .map_err(|e| e.to_string())
}

struct Viewer {
    /// The command line, for the next level's load.
    args: Args,
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
    /// Draws `screenshot`'s frames on the window's device.
    drawer: Option<Drawer>,
    /// Whether the window holds the mouse.
    pointer: Pointer,
    /// The placement applied last frame, whose result is logged this one.
    placed: Option<Placement>,
    /// Where the eye was last frame, for the contents shift.
    eye: [f32; 3],
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
    /// The console on the screen and where keys go (CON).
    console: Console,
    dest: KeyDest,
    /// The menu (MENU), and what it asked of the host this frame: a new game or quitting.
    menu: Menu,
    asked: Vec<MenuEvent>,
}

impl eframe::App for Viewer {
    #[expect(
        clippy::too_many_lines,
        reason = "one host frame in Host_Frame's order: input, the server, the client, the sound and the draw, read as one"
    )]
    fn ui(&mut self, ui: &mut egui::Ui, window: &mut eframe::Frame) {
        let now = Instant::now();
        let frame = now.duration_since(self.last).as_secs_f32();
        let seconds = frame.min(0.1);
        self.last = now;
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click());
        self.type_keys(ui);
        self.hold_pointer(ui, response.clicked() && self.dest == KeyDest::Game);
        // The bridge's commands run between two server frames (BRIDGE-4.3).
        self.serve_bridge(ui);
        if self.fly {
            self.fly_around(ui, seconds);
        } else {
            self.steer(ui);
        }
        let frametime = self.advance(f64::from(seconds));
        // MENU-3.2, MENU-5.2: the menu's new game, `map start` from nothing carried, and quit.
        for asked in core::mem::take(&mut self.asked) {
            match asked {
                MenuEvent::NewGame => match load(&self.args, "start", None, None, None) {
                    Ok(loaded) => {
                        self.change_level(window, loaded);
                        return;
                    }
                    Err(error) => tracing::warn!(%error, "the new game did not load"),
                },
                MenuEvent::Quit => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                MenuEvent::Sound(_) => {}
            }
        }
        // PHYS-13.6: a level with what the player carries, a save, a load or quit.
        if let Some(action) = self.game.host_action() {
            match act(&self.args, &mut self.game, action) {
                Acted::Load(loaded) => {
                    self.change_level(window, *loaded);
                    return;
                }
                Acted::Quit => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                Acted::Stay => {}
            }
        }
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
            Ok(events) => {
                self.print_events(&events);
                self.audio.handle(&events, &mut self.rand);
            }
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

        // `R_SetupFrame`'s `V_SetContentsColor (r_viewleaf->contents)`: the leaf the eye was
        // in last frame tints this one (VIEW-4.3).
        self.client.set_contents(contents_at(&self.bsp, self.eye));
        let scene = self.client.scene();
        let (origin, angles) = if self.fly {
            (
                self.camera.origin,
                [self.camera.pitch, self.camera.yaw, 0.0],
            )
        } else {
            (scene.view.origin, scene.view.angles)
        };
        self.eye = origin;
        self.game.set_eye(Eye {
            origin,
            angles,
            // The view's width as it is drawn: the free camera's own, else widened by the look
            // (REND-13.6).
            fov_x: if self.fly {
                self.camera.fov_x
            } else {
                self.mesh.look.fov_x(self.camera.fov_x, aspect)
            },
            aspect,
        });
        // `S_Update (r_origin, vpn, vright, vup)`, once a host frame.
        self.audio.update(
            &self.bsp,
            scene.view.origin,
            scene.view.angles,
            f64::from(seconds),
        );
        let mut hud = self.client.hud();
        // REND-13.7: QuakeSpasm's 2D layer is the window's own size in points; GLQuake's keeps
        // its 640 wide console, as tall as the window's shape.
        self.play.screen = if self.mesh.look.native_2d() {
            [points(rect.width()), points(rect.height())]
        } else {
            [CONWIDTH, screen_height(aspect)]
        };
        self.client
            .set_screen(self.play.screen[0], self.play.screen[1]);
        hud.console = self.console_items(seconds);
        let console_down = !hud.console.is_empty() && self.dest != KeyDest::Game;
        let (menu, sound) = self.menu.draw(
            self.dest,
            self.started.elapsed().as_secs_f64(),
            console_down,
            self.play.screen[1],
        );
        hud.menu = menu;
        if let Some(sound) = sound {
            self.audio.local(sound, &mut self.rand);
        }
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
        if self.game.wants_screenshot() {
            // BRIDGE-4.9: the frame drawn again offscreen at the window's size, from the same
            // callback, on the window's device.
            let pixels = ui.ctx().pixels_per_point();
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a window's size in pixels, rounded, a few thousand at most"
            )]
            let size = [rect.width(), rect.height()].map(|n| (n * pixels).round().max(1.0) as u32);
            match self
                .drawer
                .as_mut()
                .map(|drawer| drawer.draw(callback.clone(), size))
            {
                Some(Ok(frame)) => self.game.screenshot_taken(&frame),
                Some(Err(why)) => self.game.screenshot_failed(&why),
                None => self.game.screenshot_failed("no GPU device"),
            }
        }
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        // egui's fonts are not bundled (their licenses are outside deny.toml's), so the stats
        // go in the title bar.
        // The eye: the client's in play, the free camera's with `--fly`.
        let [x, y, z] = self.eye;
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
    /// Loads `map` in place of this one, the player carrying `carried`, and gives the
    /// renderer its world (PHYS-10.4); a map that does not load leaves this one running.
    fn change_level(&mut self, window: &eframe::Frame, loaded: Loaded) {
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
            printed,
        } = loaded;
        let realtime = self.started.elapsed().as_secs_f64();
        for text in &printed {
            self.console.print(text, realtime);
        }
        if let Some(state) = window.wgpu_render_state() {
            let renderer = MapRenderer::new(
                &state.device,
                &state.queue,
                (state.target_format, wgpu::TextureFormat::Depth32Float),
                &mesh,
                &play.assets(),
            );
            state.renderer.write().callback_resources.insert(renderer);
        }
        self.triangles = mesh.indices.len().checked_div(3).unwrap_or(0);
        self.map = map;
        self.mesh = mesh;
        self.play = play;
        self.eye = camera.origin;
        self.camera = camera;
        let old = core::mem::replace(&mut self.game, game);
        self.game.carry_from(old);
        self.audio = audio;
        self.client = client;
        self.bsp = bsp;
        self.describer = describer;
        self.rand = rand;
        self.last_set = [f32::NAN; 2];
        self.behind = 0.0;
    }

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
        let running = serve(bridge, &mut self.game);
        if let Some(asked) = self.placed.take() {
            // The frame the windowing system gave, a frame after it was asked for.
            let outer = ui.input(|input| input.viewport().outer_rect);
            tracing::info!(?asked, ?outer, "window placed");
        }
        if let Some(placement) = self.game.take_placement() {
            place(ui.ctx(), placement);
            self.placed = Some(placement);
        }
        if !running {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
        // The keys are the console's while it is open (CON-4).
        let playing = self.dest == KeyDest::Game;
        let cmd = ui.input(|input| {
            let key = |k: egui::Key| f32::from(u8::from(playing && input.key_down(k)));
            // cl_movespeedkey
            let speed = if input.modifiers.shift { 2.0 } else { 1.0 };
            let held = self.pointer == Pointer::Captured;
            let attack = u8::from(playing && held && input.pointer.primary_down());
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
                    playing && input.key_pressed(key)
                })
                .unwrap_or(0);
            qs_sim::UserCmd {
                angles: [self.camera.pitch, self.camera.yaw, 0.0],
                // cl_forwardspeed, cl_backspeed, cl_sidespeed, cl_upspeed
                forward: (200.0 * key(egui::Key::W) - 200.0 * key(egui::Key::S)) * speed,
                side: (350.0 * key(egui::Key::D) - 350.0 * key(egui::Key::A)) * speed,
                up: (200.0 * key(egui::Key::E) - 200.0 * key(egui::Key::Q)) * speed,
                buttons: attack | (u8::from(playing && input.key_down(egui::Key::Space)) << 1),
                impulse,
            }
        });
        self.game.world_mut().set_command(cmd);
    }

    /// The keys the console takes (CON-4): the backquote toggles it, as id1's `default.cfg`
    /// binds it to `toggleconsole` (CON-4.8); while it is open the keys and text go to its line,
    /// Escape leaves it as `M_ToggleMenu_f` does from the console, and an entered line runs
    /// through qs-sim's console path, what it prints printed in the console.
    fn type_keys(&mut self, ui: &egui::Ui) {
        let events = ui.input(|input| input.events.clone());
        let realtime = self.started.elapsed().as_secs_f64();
        let height = self.play.screen[1];
        for event in events {
            match event {
                egui::Event::Key {
                    key: egui::Key::Backtick,
                    pressed: true,
                    ..
                } => self.dest = self.console.toggle(self.dest, true),
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    // `Key_Event`'s Escape: the menu's own key in the menu, else
                    // `M_ToggleMenu_f`, which from the console toggles the console (MENU-1.1).
                    match self.dest {
                        KeyDest::Console => self.dest = self.console.toggle(self.dest, true),
                        KeyDest::Menu => self.menu_key(MenuKey::Escape),
                        KeyDest::Game => self.dest = self.menu.toggle(self.dest),
                    }
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } if self.dest == KeyDest::Menu => {
                    if let Some(key) = menu_key(key) {
                        self.menu_key(key);
                    }
                }
                egui::Event::Text(text) if self.dest == KeyDest::Menu => {
                    for c in text.bytes() {
                        self.menu_key(MenuKey::Char(c));
                    }
                }
                egui::Event::Key {
                    key, pressed: true, ..
                } if self.dest == KeyDest::Console => {
                    if let Some(key) = console_key(key) {
                        self.console_key(key, height, realtime);
                    }
                }
                egui::Event::Text(text) if self.dest == KeyDest::Console => {
                    // The backquote and `~` are never typed (CON-4.8).
                    for c in text.bytes().filter(|&c| c != b'`' && c != b'~') {
                        self.console_key(ConsoleKey::Char(c), height, realtime);
                    }
                }
                _ => {}
            }
        }
    }

    /// One key into the menu, what it asks of the host done: a sound, a new game, or quitting
    /// (MENU).
    fn menu_key(&mut self, key: MenuKey) {
        let random = self.rand.rand();
        let (dest, events) = self.menu.key(key, random);
        self.dest = dest;
        for event in events {
            match event {
                MenuEvent::Sound(name) => self.audio.local(name, &mut self.rand),
                MenuEvent::NewGame | MenuEvent::Quit => self.asked.push(event),
            }
        }
    }

    /// `svc_print`: the server's text for the console (CON-1).
    fn print_events(&mut self, events: &[Event]) {
        let realtime = self.started.elapsed().as_secs_f64();
        for event in events {
            if let Event::Print(text) = event {
                self.console.print(text, realtime);
            }
        }
    }

    /// `SCR_SetUpToDrawConsole` and `SCR_DrawConsole` for a frame of `seconds`: the console
    /// as high as it has slid, else the notify lines in the game (CON-2, CON-3).
    fn console_items(&mut self, seconds: f32) -> Vec<qs_client::Placed> {
        let realtime = self.started.elapsed().as_secs_f64();
        let [width, height] = self.play.screen;
        self.console.resize(width);
        let lines = self.console.slide(height, false, self.dest, seconds);
        if lines > 0 && self.mesh.look.native_2d() {
            // CON-5: QuakeSpasm's layout, with this host's name for the version.
            let version = format!("QuakeC Studio {}", env!("CARGO_PKG_VERSION"));
            self.console
                .draw_quakespasm(lines, height, realtime, version.as_bytes())
        } else if lines > 0 {
            self.console.draw(lines, height, realtime, true)
        } else if self.dest == KeyDest::Game {
            self.console.notify(realtime)
        } else {
            Vec::new()
        }
    }

    /// One key into the console's line; an entered line runs (CON-4.2).
    fn console_key(&mut self, key: ConsoleKey, height: i32, realtime: f64) {
        let game = &self.game;
        let typed = self
            .console
            .key(key, height, realtime, &|partial| game.complete(partial));
        if let Typed::Run(line) = typed {
            for printed in self.game.type_console(&line) {
                let mut text = printed;
                text.push(b'\n');
                self.console.print(&text, realtime);
            }
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

/// The menu key an egui key is, if it is one the menu reads (MENU-1.2); printable keys come as
/// text instead.
const fn menu_key(key: egui::Key) -> Option<MenuKey> {
    Some(match key {
        egui::Key::ArrowUp => MenuKey::Up,
        egui::Key::ArrowDown => MenuKey::Down,
        egui::Key::ArrowLeft => MenuKey::Left,
        egui::Key::ArrowRight => MenuKey::Right,
        egui::Key::Enter => MenuKey::Enter,
        _ => return None,
    })
}

/// The console key an egui key is, if it is one the console reads (CON-4); printable keys
/// come as text instead.
const fn console_key(key: egui::Key) -> Option<ConsoleKey> {
    Some(match key {
        egui::Key::Enter => ConsoleKey::Enter,
        egui::Key::Tab => ConsoleKey::Tab,
        egui::Key::Backspace | egui::Key::ArrowLeft => ConsoleKey::Back,
        egui::Key::ArrowUp => ConsoleKey::Up,
        egui::Key::ArrowDown => ConsoleKey::Down,
        egui::Key::PageUp => ConsoleKey::PageUp,
        egui::Key::PageDown => ConsoleKey::PageDown,
        egui::Key::Home => ConsoleKey::Home,
        egui::Key::End => ConsoleKey::End,
        _ => return None,
    })
}

/// Moves and sizes the window to `placement`'s outer frame and sets whether it stays above
/// other windows (BRIDGE-4.10). egui sizes a window by its inside, so the frame's own size, what
/// the title bar and borders take, is taken off.
fn place(ctx: &egui::Context, placement: Placement) {
    let (outer, inner) = ctx.input(|input| {
        let viewport = input.viewport();
        (viewport.outer_rect, viewport.inner_rect)
    });
    let [frame_x, frame_y] = match (outer, inner) {
        (Some(outer), Some(inner)) => [
            (outer.width() - inner.width()).max(0.0),
            (outer.height() - inner.height()).max(0.0),
        ],
        _ => [0.0, 0.0],
    };
    let inside = egui::vec2(
        (placement.width - frame_x).max(1.0),
        (placement.height - frame_y).max(1.0),
    );
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
        placement.x,
        placement.y,
    )));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inside));
    let level = if placement.above {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
}

/// A window length in points as the 2D layer's whole units, toward zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a window's size in points, far inside i32, toward zero as vid.width is an int"
)]
fn points(length: f32) -> i32 {
    length.max(0.0) as i32
}

/// GLQuake's console width, `vid.conwidth` (HUD-1).
pub(crate) const CONWIDTH: i32 = 640;

/// The 2D layer's height for a window `aspect` wide per unit of height.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a rounded height of a few hundred units"
)]
pub(crate) fn screen_height(aspect: f32) -> i32 {
    (f32::from(u16::try_from(CONWIDTH).unwrap_or(640)) / aspect.max(0.1)).round() as i32
}

/// The server's frame as the client's `host_frametime`, in seconds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a frame of at most 0.1 seconds, exact enough in f32"
)]
pub(crate) fn frametime_f32(frametime: f64) -> f32 {
    frametime as f32
}
