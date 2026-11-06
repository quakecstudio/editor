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
use qs_client::Client;
use qs_render::{MapMesh, MapRenderer, Offscreen, Play};

use crate::args::Args;
use crate::bridge::{Bridge, serve};
use crate::contents::contents_at;
use crate::game::{Eye, Game, Screen};
use crate::load::{Acted, Describer, Loaded, Rand, act};
use crate::screenshot::{Drawer, Frame};
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
        }
    }

    /// The client's view drawn at [`SIZE`], as the window draws it: the 3D scene through the
    /// map's paint callback, and the 2D layer of GLQuake's 640 wide console.
    fn draw(&mut self, drawer: &mut Drawer) -> Result<Frame, String> {
        let view = self.client.scene().view;
        // `V_SetContentsColor`: the leaf the eye is in tints the view (VIEW-4.3).
        self.client
            .set_contents(contents_at(&self.bsp, view.origin));
        let scene = self.client.scene();
        let hud = self.client.hud();
        self.play.screen = [CONWIDTH, screen_height(ASPECT)];
        self.client
            .set_screen(self.play.screen[0], self.play.screen[1]);
        let callback = self
            .play
            .frame(&scene, &hud, &self.bsp, &mut self.mesh, ASPECT);
        drawer.draw(callback, SIZE)
    }

    /// The client's frame: it reads what the server wrote, and the eye `pick` looks from is
    /// the client's view.
    fn read_messages(&mut self, frametime: f64) {
        let packets = self.game.world_mut().take_messages().packets;
        let seconds = frametime_f32(frametime);
        if let Err(error) =
            self.client
                .frame(seconds, &packets, &mut self.describer, &mut self.rand)
        {
            tracing::warn!(%error, "client");
        }
        let view = self.client.scene().view;
        self.game.set_eye(Eye {
            origin: view.origin,
            angles: view.angles,
            fov_x: self.fov_x,
            aspect: ASPECT,
        });
    }
}
