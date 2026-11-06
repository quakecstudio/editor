// SPDX-License-Identifier: GPL-3.0-or-later

//! `--headless` (P3-05): the game with no window and no sound, for scenario runs, CI and a
//! bridge client's tests (P3-06). Each frame is one of `Host_FilterTime`'s shortest, 1/72 of a
//! second, run when that much time has passed; the bridge is served between two server frames
//! (BRIDGE-4.3), the client reads the server's messages so `pick` looks from the player's eye,
//! and level changes load the next map as the window does. The server's and the client's
//! random sources start at the engine's fixed state (MOVE-6.1), so a run is the same each
//! time. With a GPU device, `screenshot` draws the client's view offscreen at the window's
//! first size; with none, the host does not offer it (BRIDGE-2.6).

use std::time::{Duration, Instant};

use eframe::egui_wgpu::wgpu;
use qs_bsp::Bsp;
use std::path::Path;

use qs_client::{Client, Hud};
use qs_render::{MapMesh, MapRenderer, Offscreen, Play};

use crate::args::Args;
use crate::audio::Audio;
use crate::bridge::{Bridge, serve};
use crate::contents::contents_at;
use crate::game::{Eye, Game, Screen};
use crate::load::{Acted, Describer, Loaded, Rand, act};
use crate::screenshot::{Drawer, Frame};
use crate::view_script::{Step, ViewScript};
use crate::window::{CONWIDTH, FRAME, frametime_f32, screen_height};

/// The view's width per unit of height `pick` assumes: the window's first size, 1280 by 800.
const ASPECT: f32 = 1.6;
/// The size an offscreen frame is drawn at: the window's first size.
const SIZE: [u32; 2] = [1280, 800];
/// The depth buffer the map's renderer draws with, as the window's (`depth_buffer: 32`).
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Runs the game of `loaded` until the bridge's client stops the host or goes away.
pub(crate) fn run(args: &Args, loaded: Loaded, mut bridge: Bridge) -> Result<(), String> {
    let mut offscreen = match Offscreen::new(DEPTH) {
        Ok(offscreen) => Some(Drawer::new(offscreen)),
        Err(error) => {
            tracing::warn!(%error, "no screenshots");
            None
        }
    };
    let mut world = Headless::new(loaded, offscreen.as_ref());
    let frame = Duration::from_secs_f64(FRAME);
    let mut next = Instant::now();
    loop {
        if !serve(&mut bridge, &mut world.game) {
            return Ok(());
        }
        let ran = world.game.frame(FRAME);
        if let Some(action) = world.game.host_action() {
            match act(args, &mut world.game, action) {
                Acted::Load(loaded) => {
                    let next = Headless::new(*loaded, offscreen.as_ref());
                    let old = core::mem::replace(&mut world, next);
                    world.game.carry_from(old.game);
                    continue;
                }
                Acted::Quit => return Ok(()),
                Acted::Stay => {}
            }
        }
        // The client's clock stands while the server's does, as the window's.
        world.read_messages(if ran { FRAME } else { 0.0 });
        if world.game.wants_screenshot() {
            match offscreen.as_mut().map(|drawer| world.draw(drawer)) {
                Some(Ok(frame)) => world.game.screenshot_taken(&frame),
                Some(Err(why)) => world.game.screenshot_failed(&why),
                None => world.game.screenshot_failed("no GPU device"),
            }
        }
        next = next.checked_add(frame).unwrap_or(next);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
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

/// `--timerefresh <x> <y> <z>` (P3-29, ENG-17): the map of `loaded`, which took `load` to load,
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
    loaded: Loaded,
    load: std::time::Duration,
    eye: [f32; 3],
) -> Result<String, String> {
    let offscreen = Offscreen::new(DEPTH).map_err(|error| error.to_string())?;
    let mut drawer = Drawer::new(offscreen);
    let mut world = Headless::new(loaded, Some(&drawer));
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
            return Err("the map changed level, started a game or quit in its first second".into());
        }
        world.read_messages(if ran { FRAME } else { 0.0 });
    }
    let map = world.game.world().map().to_owned();
    let yaw = |i: u16| f32::from(i) / f32::from(TIMEREFRESH_FRAMES) * 360.0;
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

/// The size the view oracle draws at: 320 by 240, square pixels at 4:3 (VIEWDIFF-2.1).
const VIEW_SIZE: [u32; 2] = [320, 240];

/// `--view-script <script> <audio>` (P3-28): the map of `loaded` played as `script` says, its
/// shots written as binary PPMs and the sound mixed meanwhile written to `audio` as 16-bit
/// little-endian stereo at 11025 Hz, as the view oracle writes them (VIEWDIFF-1, VIEWDIFF-2).
///
/// # Errors
///
/// Why the GPU device, a frame, the player's place or a file failed, or that the map asked
/// the host to change level, start a game or quit.
pub(crate) fn view_script(loaded: Loaded, script: &ViewScript, audio: &Path) -> Result<(), String> {
    let offscreen = Offscreen::new(DEPTH).map_err(|error| error.to_string())?;
    let mut drawer = Drawer::new(offscreen);
    let mut world = Headless::new(loaded, Some(&drawer));
    let mut sound = world.audio.take_captured();
    for step in &script.steps {
        match step {
            Step::Frames(count) => {
                for _ in 0..*count {
                    let ran = world.game.frame(FRAME);
                    if world.game.host_action().is_some() {
                        return Err("the map changed level, started a game or quit".into());
                    }
                    world.read_messages(if ran { FRAME } else { 0.0 });
                }
            }
            Step::Place { origin, angles } => {
                world
                    .game
                    .place_player(*origin, [angles[0], angles[1], 0.0])?;
            }
            Step::Shot(path) => {
                let [width, height] = VIEW_SIZE;
                let aspect = f32_of_u32(width) / f32_of_u32(height);
                let callback = world.view_callback(aspect);
                let frame = drawer.draw(callback, VIEW_SIZE)?;
                std::fs::write(path, ppm(&frame))
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        sound.extend(world.audio.take_captured());
    }
    let bytes: Vec<u8> = sound.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(audio, bytes).map_err(|error| format!("{}: {error}", audio.display()))
}

/// `frame` as a binary PPM, `P6`, its alpha dropped (VIEWDIFF-1.2).
fn ppm(frame: &Frame) -> Vec<u8> {
    let mut bytes = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
    for pixel in frame.rgba.chunks_exact(4) {
        bytes.extend(pixel.iter().take(3));
    }
    bytes
}

/// A small pixel count as a float, exactly.
fn f32_of_u32(n: u32) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// What a headless host keeps of a loaded map.
struct Headless {
    game: Game,
    client: Client,
    describer: Describer,
    rand: Rand,
    fov_x: f32,
    /// What a frame is drawn from: the map's mesh, its renderer's assets and the BSP.
    mesh: MapMesh,
    play: Play,
    bsp: Bsp<'static>,
    /// The sound, mixed and dropped, or kept for `--view-script`.
    audio: Audio,
}

impl Headless {
    /// The game of `loaded`, its map's renderer given to `drawer`'s device when there is one.
    fn new(loaded: Loaded, drawer: Option<&Drawer>) -> Self {
        let Loaded {
            mut game,
            client,
            describer,
            rand,
            camera,
            mesh,
            play,
            bsp,
            audio,
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
            client,
            describer,
            rand,
            fov_x: camera.fov_x,
            mesh,
            play,
            bsp,
            audio,
        }
    }

    /// The client's view drawn at [`SIZE`], as the window draws it: the 3D scene through the
    /// map's paint callback, and the 2D layer of GLQuake's 640 wide console.
    fn draw(&mut self, drawer: &mut Drawer) -> Result<Frame, String> {
        let callback = self.callback(None);
        drawer.draw(callback, SIZE)
    }

    /// The paint callback of the client's view, from `eye` at its yaw when one is given, as
    /// `timerefresh` turns the view, else from the client's own eye.
    fn callback(&mut self, eye: Option<([f32; 3], f32)>) -> qs_render::MapCallback {
        let mut scene = self.client.scene();
        if let Some((origin, yaw)) = eye {
            scene.view.origin = origin;
            scene.view.angles = [0.0, yaw, 0.0];
        }
        // `V_SetContentsColor`: the leaf the eye is in tints the view (VIEW-4.3).
        self.client
            .set_contents(contents_at(&self.bsp, scene.view.origin));
        let hud = self.client.hud();
        self.play.screen = [CONWIDTH, screen_height(ASPECT)];
        self.client
            .set_screen(self.play.screen[0], self.play.screen[1]);
        self.play
            .frame(&scene, &hud, &self.bsp, &mut self.mesh, ASPECT)
    }

    /// The client's view alone, on an image `aspect` wide per unit of height: no status bar,
    /// centre print or weapon, as the view oracle draws it with `viewsize 120` and
    /// `r_drawviewmodel 0` (P3-28).
    fn view_callback(&mut self, aspect: f32) -> qs_render::MapCallback {
        let mut scene = self.client.scene();
        scene.view.weapon.model = 0;
        // `V_SetContentsColor`: the leaf the eye is in tints the view (VIEW-4.3).
        self.client
            .set_contents(contents_at(&self.bsp, scene.view.origin));
        self.play
            .frame(&scene, &Hud::default(), &self.bsp, &mut self.mesh, aspect)
    }

    /// The client's frame: it reads what the server wrote, and the eye `pick` looks from is
    /// the client's view.
    fn read_messages(&mut self, frametime: f64) {
        let packets = self.game.world_mut().take_messages().packets;
        let seconds = frametime_f32(frametime);
        match self
            .client
            .frame(seconds, &packets, &mut self.describer, &mut self.rand)
        {
            Ok(events) => self.audio.handle(&events, &mut self.rand),
            Err(error) => tracing::warn!(%error, "client"),
        }
        let view = self.client.scene().view;
        // `S_Update (r_origin, vpn, vright, vup)`, once a host frame, silent without
        // `--view-script`.
        self.audio
            .update(&self.bsp, view.origin, view.angles, frametime);
        self.game.set_eye(Eye {
            origin: view.origin,
            angles: view.angles,
            // The view's width as it is drawn, widened by the look (REND-13.6).
            fov_x: self.mesh.look.fov_x(self.fov_x, ASPECT),
            aspect: ASPECT,
        });
    }
}
