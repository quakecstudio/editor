// SPDX-License-Identifier: GPL-3.0-or-later

//! `--headless` (P3-05): the game with no window and no sound, for scenario runs, CI and a
//! bridge client's tests (P3-06). Each frame is one of `Host_FilterTime`'s shortest, 1/72 of a
//! second, run when that much time has passed; the bridge is served between two server frames
//! (BRIDGE-4.3), the client reads the server's messages so `pick` looks from the player's eye,
//! and level changes load the next map as the window does. The server's and the client's
//! random sources start at the engine's fixed state (MOVE-6.1), so a run is the same each
//! time. With a GPU device, `screenshot` draws the client's view offscreen at the window's
//! first size; with none, the host does not offer it (BRIDGE-2.10).

use std::time::{Duration, Instant};

use eframe::egui_wgpu::wgpu;
use std::path::Path;

use qs_client::{
    Client, Connection, Console, ConsoleHold, Hud, KeyDest, Menu, MenuFrame, MenuSettings,
};
use qs_render::{Look, MapMesh, MapRenderer, Offscreen, Play};
use qs_sim::ClientCommand;
use qs_sim::input::{Input, MoveSettings};

use crate::Error;
use crate::args::Args;
use crate::audio::Audio;
use crate::bridge::Bridge;
use crate::contents::contents_at;
use crate::demos::{DemoHost, Demos};
use crate::game::{Eye, Game, PendingSwap, Screen};
use crate::keys::focus;
use crate::load::{Acted, Describer, Loaded, Rand, map_mesh};
use crate::map_bsp::MapBsp;
use crate::screenshot::{Drawer, Frame};
use crate::settings::{DrawingSettings, HostSettings};
use crate::view_script::{Step, ViewScript};
use crate::window::{CONWIDTH, FRAME, frametime_f32, screen_height};

/// The bytes of an RGBA pixel.
const RGBA_BYTES: usize = 4;
/// A full turn, in degrees.
const FULL_TURN: f32 = 360.0;

/// The view's width per unit of height `pick` assumes: the window's first size, 1280 by 800.
const ASPECT: f32 = 1.6;
/// The size an offscreen frame is drawn at: the window's first size.
const SIZE: [u32; 2] = [1280, 800];
/// The depth buffer the map's renderer draws with, as the window's (`depth_buffer: 32`).
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

impl Args {
    /// Runs the game of `loaded` until the bridge's client stops the host or goes away.
    pub(crate) fn run_headless(&self, loaded: Loaded, mut bridge: Bridge) -> Result<(), Error> {
        let mut offscreen = match Offscreen::new(DEPTH) {
            Ok(offscreen) => Some(Drawer::new(offscreen)),
            Err(error) => {
                tracing::warn!(%error, "no screenshots");
                None
            }
        };
        let mut world = Headless::new(loaded, offscreen.as_ref());
        let frame = Duration::from_secs_f64(FRAME);
        let started = Instant::now();
        let mut next = started;
        loop {
            if !bridge.serve(&mut world.game) {
                return Ok(());
            }
            if let Some(swap) = world.game.take_swap() {
                world.draw_swap(offscreen.as_ref(), swap);
            }
            // Paused, no time passes for the keys that turn the view; the move is still made,
            // so an impulse typed while paused is in the frame a step runs (PHYS-9.8).
            world.steer(if world.game.paused() {
                0.0
            } else {
                frametime_f32(FRAME)
            });
            // CLIENT-10.6: while a demo plays the server stands still.
            let ran = world.demos.playing() || world.game.frame(FRAME);
            // The progs' `localcmd` and `stuffcmd` reach the client's commands too.
            world.client_commands(self);
            if let Some(action) = world.game.host_action() {
                let demos = (&mut world.demos, &world.client);
                match self.act(&mut world.game, action, demos) {
                    Acted::Load(loaded) => {
                        let next = Headless::new(*loaded, offscreen.as_ref());
                        let old = core::mem::replace(&mut world, next);
                        world.game.carry_from(old.game);
                        // A button held stays held into the next level, as the window's,
                        // and so does a demo being recorded or played.
                        world.input = old.input;
                        world.demos = old.demos;
                        continue;
                    }
                    Acted::Quit => return Ok(()),
                    Acted::Stay => {}
                }
            }
            // The client's clock stands while the server's does, as the window's.
            world.read_messages(
                if ran { FRAME } else { 0.0 },
                started.elapsed().as_secs_f64(),
            );
            if world.game.wants_screenshot() {
                match offscreen.as_mut().map(|drawer| world.draw(drawer)) {
                    Some(Ok(frame)) => world.game.screenshot_taken(&frame),
                    Some(Err(why)) => world.game.screenshot_failed(&why.to_string()),
                    None => world.game.screenshot_failed("no GPU device"),
                }
            }
            next = next.checked_add(frame).unwrap_or(next);
            if let Some(wait) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    }
}

/// The frames of id's `timerefresh` (`R_TimeRefresh_f`, WinQuake/r_misc.c at
/// id-Software/Quake@bf4ac424): "for (i=0 ; i<128 ; i++) { r_refdef.viewangles[1] =
/// i/128.0*360.0; ... R_RenderView (); ... }", timed together and printed as
/// "%f seconds (%f fps)\n".
const TIMEREFRESH_FRAMES: u16 = 128;
/// The server frames run before a `timerefresh`, so the client has the map's entities, and
/// timed for the server-frame and QuakeC benchmarks: one second.
const SETTLE_FRAMES: u16 = 72;
/// How many times the 128 frames of a `timerefresh` are drawn back to back.
const PASSES: u16 = 16;

impl Loaded {
    /// `--timerefresh <x> <y> <z>` (P3-29, ENG-17): the loaded map, which took `load` to load,
    /// run for a second with each server frame and its QuakeC timed, then id's `timerefresh` from
    /// `eye`, the 128 frames drawn offscreen at the window's first size with no server frame
    /// between them and finished on the GPU, [`PASSES`] times; then id's line for the first pass
    /// and a JSON line of every measurement, in seconds, for `cargo xtask bench`, on stdout. A
    /// pass's frame time is its mean, as QuakeSpasm's `timerefresh` reports it: offscreen nothing
    /// holds the CPU back to the GPU, so a frame's own submit time leaves the GPU's work out.
    ///
    /// # Errors
    ///
    /// Why the GPU device or a frame failed, or that the map asked the host to change level, start
    /// a game or quit in its first second, which would leave the frames measured on a server the
    /// host would not have kept.
    pub(crate) fn timerefresh(
        self,
        load: std::time::Duration,
        eye: [f32; 3],
    ) -> Result<String, Error> {
        let offscreen = Offscreen::new(DEPTH).map_err(|error| Error::Draw(error.to_string()))?;
        let mut drawer = Drawer::new(offscreen);
        let mut world = Headless::new(self, Some(&drawer));
        let mut server = Vec::with_capacity(usize::from(SETTLE_FRAMES));
        let mut quakec = Vec::with_capacity(usize::from(SETTLE_FRAMES));
        for _ in 0..SETTLE_FRAMES {
            let before = world.game.world().qc_time();
            let start = std::time::Instant::now();
            let ran = world.game.frame(FRAME);
            server.push(start.elapsed().as_secs_f64());
            quakec.push(
                world
                    .game
                    .world()
                    .qc_time()
                    .saturating_sub(before)
                    .as_secs_f64(),
            );
            if world.game.host_action().is_some() {
                return Err(Error::Load(
                    "the map changed level, started a game or quit in its first second".to_owned(),
                ));
            }
            // No demo is played here, so the clock a `timedemo` reads stands.
            world.read_messages(if ran { FRAME } else { 0.0 }, 0.0);
        }
        let map = world.game.world().map().to_owned();
        let yaw = |i: u16| f32::from(i) / f32::from(TIMEREFRESH_FRAMES) * FULL_TURN;
        let count = f64::from(TIMEREFRESH_FRAMES);
        let mut passes = Vec::with_capacity(usize::from(PASSES));
        for _ in 0..PASSES {
            // As `R_TimeRefresh_f` runs in QuakeSpasm (Quake/gl_rmisc.c at
            // sezero/quakespasm@c9c8ee98): the frames drawn back to back, then `glFinish ();`
            // before the clock stops.
            let start = std::time::Instant::now();
            for i in 0..TIMEREFRESH_FRAMES {
                let callback = world.callback(Some((eye, yaw(i))));
                drawer.submit(callback, SIZE);
            }
            drawer.finish()?;
            passes.push(start.elapsed().as_secs_f64());
        }
        let first = passes.first().copied().unwrap_or_default();
        let frames: Vec<f64> = passes.iter().map(|pass| pass / count).collect();
        let json = serde_json::json!({
            "map": map,
            "load": load.as_secs_f64(),
            "server_frame": server,
            "quakec": quakec,
            "frame": frames,
        });
        let fps = count / first.max(f64::MIN_POSITIVE);
        Ok(format!("{first:.6} seconds ({fps:.6} fps)\n{json}"))
    }

    /// `--view-script <script> <audio>` (P3-28): the loaded map played as `script` says, its
    /// shots written as binary PPMs and the sound mixed meanwhile written to `audio` as 16-bit
    /// little-endian stereo at 11025 Hz, as the view oracle writes them (VIEWDIFF-1, VIEWDIFF-2).
    ///
    /// # Errors
    ///
    /// Why the GPU device, a frame, the player's place or a file failed, or that the map asked
    /// the host to change level, start a game or quit.
    pub(crate) fn view_script(
        self,
        script: &ViewScript,
        audio: &Path,
        size: [u32; 2],
    ) -> Result<(), Error> {
        let offscreen = Offscreen::new(DEPTH).map_err(|error| Error::Draw(error.to_string()))?;
        let mut drawer = Drawer::new(offscreen);
        let mut world = Headless::new(self, Some(&drawer));
        let mut sound = world.audio.take_captured();
        let [width, height] = size;
        let aspect = f32::from(u16::try_from(width).unwrap_or(1))
            / f32::from(u16::try_from(height).unwrap_or(1).max(1));
        // REND-13.7: QuakeSpasm's 2D layer is the image's own size, GLQuake's 640 wide.
        let screen = if world.mesh.look.native_2d() {
            [
                i32::try_from(width).unwrap_or(0),
                i32::try_from(height).unwrap_or(0),
            ]
        } else {
            [CONWIDTH, screen_height(aspect)]
        };
        let mut overlay = Overlay::default();
        for text in core::mem::take(&mut world.printed) {
            overlay.console.print(&text, 0.0);
        }
        for step in &script.steps {
            match step {
                Step::Frames(count) => {
                    for _ in 0..*count {
                        // PHYS-11.9: the menu or the console up pauses the game.
                        world.game.world_mut().set_focus(focus(overlay.dest));
                        let ran = world.game.frame(FRAME);
                        if world.game.host_action().is_some() {
                            return Err(Error::Script(
                                "the map changed level, started a game or quit".to_owned(),
                            ));
                        }
                        // No demo is played here, so the clock a `timedemo` reads stands.
                        world.read_messages(if ran { FRAME } else { 0.0 }, 0.0);
                        overlay.frame(screen);
                    }
                }
                Step::Place { origin, angles } => {
                    world
                        .game
                        .place_player(*origin, [angles[0], angles[1], 0.0])?;
                }
                Step::ToggleConsole => {
                    overlay.dest = overlay.console.toggle(overlay.dest, Connection::Connected)
                }
                Step::ToggleMenu => {
                    overlay.dest = match overlay.dest {
                        KeyDest::Console => {
                            overlay.console.toggle(overlay.dest, Connection::Connected)
                        }
                        dest => overlay.menu.toggle(dest),
                    };
                }
                Step::Fog(density) => {
                    let now = world.client.scene().time;
                    world.play.fog.command_density(*density, now);
                }
                Step::Shot(path) => {
                    world.play.screen = screen;
                    let game = world.game.world();
                    let settings = MenuSettings::read(&|name| game.setting(name).unwrap_or(0.0));
                    let hud = overlay.hud(screen, world.mesh.look, settings);
                    let callback = world.view_callback(aspect, &hud);
                    let frame = drawer.draw(callback, size)?;
                    std::fs::write(path, frame.ppm())
                        .map_err(|error| Error::Script(format!("{}: {error}", path.display())))?;
                }
            }
            sound.extend(world.audio.take_captured());
        }
        let bytes: Vec<u8> = sound.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(audio, bytes)
            .map_err(|error| Error::Script(format!("{}: {error}", audio.display())))
    }
}

/// The size the view oracle draws at: 320 by 240, square pixels at 4:3 (VIEWDIFF-2.1).
pub(crate) const VIEW_SIZE: [u32; 2] = [320, 240];

/// The console and the menu a view script opens over the view (CON, MENU), and the time and
/// height the console has slid to.
#[derive(Default)]
struct Overlay {
    console: Console,
    menu: Menu,
    dest: KeyDest,
    realtime: f64,
    lines: i32,
}

impl Overlay {
    /// One frame of 1/72 of a second: the console slides (CON-3.1).
    fn frame(&mut self, screen: [i32; 2]) {
        self.realtime += FRAME;
        self.console.resize(screen[0]);
        self.lines = self.console.slide(
            screen[1],
            ConsoleHold::Sliding,
            self.dest,
            frametime_f32(FRAME),
        );
    }

    /// The console and the menu as the window draws them on `screen`, in QuakeSpasm's layout
    /// under `look`'s native 2D layer (CON-5), the options showing `settings`.
    fn hud(&mut self, screen: [i32; 2], look: Look, settings: MenuSettings) -> Hud {
        let native = look.native_2d();
        let [_, height] = screen;
        let console = if self.lines > 0 && native {
            let version = format!("QuakeC Studio {}", env!("CARGO_PKG_VERSION"));
            let typing = self.dest.input_line();
            self.console.draw_quakespasm(
                self.lines,
                height,
                self.realtime,
                typing,
                version.as_bytes(),
            )
        } else if self.lines > 0 {
            let typing = self.dest.input_line();
            self.console.draw(self.lines, height, self.realtime, typing)
        } else {
            self.console.notify(self.realtime)
        };
        let down = self.lines > 0 && self.dest != KeyDest::Game;
        let (menu, _) = self.menu.draw(&MenuFrame {
            dest: self.dest,
            host_time: self.realtime,
            realtime: self.realtime,
            console_down: down,
            console_forced: ConsoleHold::Sliding,
            screen_height: height,
            settings,
            bound: Vec::new(),
        });
        Hud {
            console,
            menu,
            ..Hud::default()
        }
    }
}

impl Frame {
    /// The frame as a binary PPM, `P6`, its alpha dropped (VIEWDIFF-1.4).
    fn ppm(&self) -> Vec<u8> {
        let mut bytes = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        for &[r, g, b, _] in self.rgba.as_chunks::<RGBA_BYTES>().0 {
            bytes.extend([r, g, b]);
        }
        bytes
    }
}

/// What a headless host keeps of a loaded map.
struct Headless {
    game: Game,
    /// The map's name, `maps/<map>.bsp`'s.
    map: String,
    client: Client,
    describer: Describer,
    rand: Rand,
    fov_x: f32,
    /// What a frame is drawn from: the map's mesh, its renderer's assets and the BSP.
    mesh: MapMesh,
    play: Play,
    bsp: MapBsp,
    /// The sound, mixed and dropped, or kept for `--view-script`.
    audio: Audio,
    /// What the map's first client frame printed (CLIENT-1.7).
    printed: Vec<Vec<u8>>,
    /// The buttons and the impulse the console's commands hold (INPUT-2.1): with no window
    /// there is no key and no mouse, so `+forward`, `+attack` and `impulse` typed at the
    /// console, or sent over the bridge, are all that moves the player.
    input: Input,
    /// The demo being recorded and the one being played (CLIENT-10).
    demos: Demos,
}

impl Headless {
    /// The game of `loaded`, its map's renderer given to `drawer`'s device when there is one.
    fn new(loaded: Loaded, drawer: Option<&Drawer>) -> Self {
        let Loaded {
            mut game,
            map,
            client,
            describer,
            rand,
            camera,
            mesh,
            play,
            bsp,
            audio,
            printed,
            ..
        } = loaded;
        match drawer {
            Some(drawer) => {
                let state = drawer.offscreen().state();
                let renderer = MapRenderer::new(
                    &state.device,
                    &state.queue,
                    (state.target_format, DEPTH),
                    &mesh,
                    &play.assets(),
                );
                state.renderer.write().callback_resources.insert(renderer);
                game.set_screen(Screen::Offscreen);
            }
            None => game.set_screen(Screen::Headless),
        }
        Self {
            game,
            map,
            client,
            describer,
            rand,
            fov_x: camera.fov_x.get(),
            mesh,
            play,
            bsp,
            audio,
            printed,
            input: Input::new(),
            demos: Demos::new(),
        }
    }

    /// The player for a frame of `seconds` (INPUT-2), as the window steers it with no key
    /// down and the mouse still: the buttons the console holds turn the client's view and
    /// make the command for the next server frame.
    fn steer(&mut self, seconds: f32) {
        let world = self.game.world();
        let settings = MoveSettings::read(&|name| world.setting(name).unwrap_or(0.0));
        let [pitch, yaw, _] = self.client.viewangles();
        let mut angles = [pitch, yaw, 0.0];
        let (cmd, drift) = self.input.frame(&settings, seconds, &mut angles, [0.0; 2]);
        self.client.set_viewangles(angles);
        if drift.stop {
            self.client.stop_pitch_drift();
        }
        if drift.start {
            self.client.start_pitch_drift();
        }
        self.game.world_mut().set_command(cmd);
    }

    /// The client's commands the console met, run in order (CON-6.1): the buttons and the
    /// impulse. The console, the menus and the bindings are the window's: with no screen and
    /// no key there is nothing for them to change.
    fn client_commands(&mut self, args: &Args) {
        for command in self.game.world_mut().take_client_commands() {
            match command {
                ClientCommand::Record(_)
                | ClientCommand::StopDemo
                | ClientCommand::PlayDemo(_)
                | ClientCommand::TimeDemo(_) => {
                    let describer = &self.describer;
                    let mut host = DemoHost {
                        args,
                        game: &mut self.game,
                        client: &self.client,
                        read: &|name| describer.read(name),
                    };
                    self.demos.command(&command, &mut host);
                }
                ClientCommand::Impulse(n) => self.input.set_impulse(n),
                ClientCommand::Press(button, key) => {
                    // What `KeyDown` prints goes to the console on the screen, which is the
                    // window's.
                    let _printed = self.input.press(button, key);
                }
                ClientCommand::Release(button, key) => {
                    let lookspring = self.game.world().setting("lookspring").unwrap_or(0.0);
                    if self.input.release(button, key, lookspring).start {
                        self.client.start_pitch_drift();
                    }
                }
                ClientCommand::ToggleConsole
                | ClientCommand::Clear
                | ClientCommand::ToggleMenu
                | ClientCommand::MenuMain
                | ClientCommand::MenuSinglePlayer
                | ClientCommand::MenuLoad
                | ClientCommand::MenuSave
                | ClientCommand::MenuOptions
                | ClientCommand::Help
                | ClientCommand::MenuQuit
                | ClientCommand::Bind(_)
                | ClientCommand::Unbind(_)
                | ClientCommand::UnbindAll => {}
            }
        }
    }

    /// The client's view drawn at [`SIZE`], as the window draws it: the 3D scene through the
    /// map's paint callback, and the 2D layer of GLQuake's 640 wide console.
    /// The swap waiting for its BSP to be drawn (BRIDGE-4.17), as the window draws it: the mesh
    /// made from the new build and given to `drawer`'s device, then the swap put in place; a
    /// build that does not draw is refused, and the old one stays drawn.
    fn draw_swap(&mut self, drawer: Option<&Drawer>, swap: PendingSwap) {
        let describer = &self.describer;
        let read = |name: &str| {
            describer
                .read(name)
                .ok_or_else(|| Error::Load(format!("{name} is not on the search path")))
        };
        let palette = self.play.palette().clone();
        let mesh = map_mesh(
            (&self.map, swap.bsp().bsp()),
            self.game.world(),
            &read,
            (&palette, self.mesh.look),
        );
        let (mesh, instances) = match mesh {
            Ok(mesh) => mesh,
            Err(why) => {
                let _refused = self.game.finish_swap(swap, Err(why));
                return;
            }
        };
        let Some(bsp) = self.game.finish_swap(swap, Ok(())) else {
            return;
        };
        self.play.set_instances(instances);
        if let Some(drawer) = drawer {
            let state = drawer.offscreen().state();
            let renderer = MapRenderer::new(
                &state.device,
                &state.queue,
                (state.target_format, DEPTH),
                &mesh,
                &self.play.assets(),
            );
            state.renderer.write().callback_resources.insert(renderer);
        }
        self.mesh = mesh;
        self.bsp = bsp;
    }

    fn draw(&mut self, drawer: &mut Drawer) -> Result<Frame, Error> {
        let callback = self.callback(None);
        drawer.draw(callback, SIZE)
    }

    /// The paint callback of the client's view, from `eye` at its yaw when one is given, as
    /// `timerefresh` turns the view, else from the client's own eye.
    fn callback(&mut self, eye: Option<([f32; 3], f32)>) -> qs_render::MapCallback {
        // The view size and gamma first: the scene places the weapon and blends by them.
        self.apply_settings();
        let mut scene = self.client.scene();
        if let Some((origin, yaw)) = eye {
            scene.view.origin = origin;
            scene.view.angles = [0.0, yaw, 0.0];
        }
        // `V_SetContentsColor`: the leaf the eye is in tints the view (VIEW-4.3).
        self.client
            .set_contents(contents_at(self.bsp.bsp(), scene.view.origin));
        let hud = self.client.hud();
        self.play.screen = [CONWIDTH, screen_height(ASPECT)];
        self.client
            .set_screen(self.play.screen[0], self.play.screen[1]);
        self.play
            .frame(&scene, &hud, self.bsp.bsp(), &mut self.mesh, ASPECT)
    }

    /// The client's settings as the console left them (CON-7.4), as the window reads them each
    /// frame: the view size and gamma for the view and the frame. Nothing is played, so the
    /// volumes are left.
    fn apply_settings(&mut self) {
        let world = self.game.world();
        let read = HostSettings::read(
            |name| world.setting(name),
            *self.client.view_settings(),
            self.mesh.look,
        );
        self.client.set_view_settings(read.view);
        self.play.gamma = read.frame_gamma;
        read.drawing.apply(&mut self.play);
    }

    /// The later engines' drawing settings alone (CON-7.6), for the view oracle's frames, whose
    /// view size and gamma are the script's.
    fn apply_drawing_settings(&mut self) {
        let world = self.game.world();
        DrawingSettings::read(|name| world.setting(name)).apply(&mut self.play);
    }

    /// The client's view alone, on an image `aspect` wide per unit of height: no status bar,
    /// centre print or weapon, as the view oracle draws it with `viewsize 120` and
    /// `r_drawviewmodel 0` (P3-28).
    fn view_callback(&mut self, aspect: f32, hud: &Hud) -> qs_render::MapCallback {
        // `V_SetContentsColor`: the leaf the eye is in tints the view (VIEW-4.3), before the
        // scene that carries the tint is taken.
        let origin = self.client.scene().view.origin;
        self.client
            .set_contents(contents_at(self.bsp.bsp(), origin));
        let mut scene = self.client.scene();
        scene.view.weapon.model = qs_client::ModelIndex::NONE;
        self.apply_drawing_settings();
        self.play
            .frame(&scene, hud, self.bsp.bsp(), &mut self.mesh, aspect)
    }

    /// The client's frame: it reads what the server wrote, and the eye `pick` looks from is
    /// the client's view.
    fn read_messages(&mut self, frametime: f64, realtime: f64) {
        // The console on the screen is the window's; with none, what it would show goes.
        self.game.take_printed();
        let packets = self.game.world_mut().take_messages().packets;
        let seconds = frametime_f32(frametime);
        // CLIENT-10.2, CLIENT-10.6: a demo being recorded takes each packet, and one being
        // played is read in their place.
        match self.demos.frame(
            &mut self.client,
            (seconds, realtime, &packets),
            (&mut self.describer, &mut self.rand),
            &mut self.game,
        ) {
            // The sound is mixed only when it is kept, for `--view-script`.
            Ok(events) if self.audio.captures() => self.audio.handle(&events, &mut self.rand),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "client"),
        }
        let view = self.client.scene().view;
        // `S_Update (r_origin, vpn, vright, vup)`, once a host frame.
        if self.audio.captures() {
            self.audio
                .update(self.bsp.bsp(), view.origin, view.angles, frametime);
        }
        self.game.set_eye(Eye {
            origin: view.origin,
            angles: view.angles,
            // The view's width as it is drawn, widened by the look (REND-13.6).
            fov_x: self.mesh.look.fov_x(self.fov_x, ASPECT),
            aspect: ASPECT,
        });
    }
}
